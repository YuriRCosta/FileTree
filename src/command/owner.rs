use super::{CommandSpec, supervisor::Running};
use crate::{AppError, AppResult};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct OwnerHandle {
    stop: Option<io::PipeWriter>,
    worker: Option<JoinHandle<AppResult<()>>>,
}

impl OwnerHandle {
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for OwnerHandle {
    fn drop(&mut self) {
        if let Some(mut stop) = self.stop.take() {
            let _ = stop.write_all(&[1]);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Output<R> {
    pipe: Option<R>,
    bytes: Vec<u8>,
    limit: usize,
}

impl<R: Read + AsFd> Output<R> {
    fn new(pipe: Option<R>, limit: usize) -> AppResult<Self> {
        let pipe = pipe.ok_or_else(|| AppError::command("owner output pipe unavailable"))?;
        nonblocking(&pipe)?;
        Ok(Self {
            pipe: Some(pipe),
            bytes: Vec::new(),
            limit,
        })
    }

    fn fd(&self) -> RawFd {
        self.pipe
            .as_ref()
            .map_or(-1, |pipe| pipe.as_fd().as_raw_fd())
    }

    fn read(&mut self) -> AppResult<()> {
        let Some(pipe) = self.pipe.as_mut() else {
            return Ok(());
        };
        let mut bytes = [0; 8192];
        match pipe.read(&mut bytes) {
            Ok(0) => self.pipe = None,
            Ok(count) => {
                if count > self.limit.saturating_sub(self.bytes.len()) {
                    return Err(AppError::command(
                        "clipboard owner exceeded its output limit",
                    ));
                }
                self.bytes.extend_from_slice(&bytes[..count]);
            }
            Err(error) if retryable(&error) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
}

impl CommandSpec {
    pub fn spawn_owner(&self, payload: Vec<u8>) -> AppResult<OwnerHandle> {
        self.spawn_owner_cancellable(payload, &AtomicBool::new(false))
    }

    pub fn spawn_owner_cancellable(
        &self,
        payload: Vec<u8>,
        cancelled: &AtomicBool,
    ) -> AppResult<OwnerHandle> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        if payload.len() > 1024 * 1024 {
            return Err(AppError::invalid("clipboard owner input exceeds 1 MiB"));
        }
        let deadline = Instant::now() + self.timeout;
        let configuration = self.clone().stdin(Vec::new()).timeout(Duration::MAX);
        let mut running = Running::spawn(&configuration)?;
        let mut input = running
            .child
            .stdin
            .take()
            .ok_or_else(|| AppError::command("owner input pipe unavailable"))?;
        nonblocking(&input)?;
        let mut stdout = Output::new(running.child.stdout.take(), self.stdout_limit)?;
        let mut stderr = Output::new(running.child.stderr.take(), self.stderr_limit)?;
        let mut written = 0;
        while written < payload.len() {
            check_startup(cancelled, deadline)?;
            stdout.read()?;
            stderr.read()?;
            match input.write(&payload[written..payload.len().min(written + 8192)]) {
                Ok(0) => return Err(AppError::command("clipboard owner closed its input")),
                Ok(count) => written += count,
                Err(error) if retryable(&error) => {
                    poll(&mut [descriptor(input.as_raw_fd(), libc::POLLOUT)], 10)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        drop(input);
        let accepted_at = Instant::now() + Duration::from_millis(200);
        loop {
            check_startup(cancelled, deadline)?;
            stdout.read()?;
            stderr.read()?;
            let mut descriptors = [descriptor(running.result.as_raw_fd(), libc::POLLIN)];
            poll(&mut descriptors, 0)?;
            if descriptors[0].revents != 0 || (stdout.pipe.is_none() && stderr.pipe.is_none()) {
                let detail = String::from_utf8_lossy(&stderr.bytes).trim().to_string();
                return Err(AppError::command(if detail.is_empty() {
                    "clipboard owner exited before accepting the selection".into()
                } else {
                    detail
                }));
            }
            if Instant::now() >= accepted_at {
                break;
            }
            poll(&mut descriptors, 10)?;
        }
        let (monitor, stop) = io::pipe()?;
        let worker = thread::Builder::new()
            .name("fileblade-clipboard".into())
            .spawn(move || {
                loop {
                    let mut descriptors = [
                        descriptor(monitor.as_raw_fd(), libc::POLLIN),
                        descriptor(running.result.as_raw_fd(), libc::POLLIN),
                        descriptor(stdout.fd(), libc::POLLIN),
                        descriptor(stderr.fd(), libc::POLLIN),
                    ];
                    poll(&mut descriptors, -1)?;
                    if descriptors[0].revents != 0 {
                        running.cancel();
                    }
                    if descriptors[2].revents != 0 {
                        stdout.read()?;
                    }
                    if descriptors[3].revents != 0 {
                        stderr.read()?;
                    }
                    if descriptors[0].revents != 0 || descriptors[1].revents != 0 {
                        return running.finish();
                    }
                }
            })?;
        Ok(OwnerHandle {
            stop: Some(stop),
            worker: Some(worker),
        })
    }
}

fn check_startup(cancelled: &AtomicBool, deadline: Instant) -> AppResult<()> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(AppError::command("clipboard owner startup timed out"));
    }
    Ok(())
}

fn nonblocking(fd: impl AsFd) -> io::Result<()> {
    let flags = rustix::fs::fcntl_getfl(&fd)?;
    Ok(rustix::fs::fcntl_setfl(
        fd,
        flags | rustix::fs::OFlags::NONBLOCK,
    )?)
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

fn descriptor(fd: RawFd, events: i16) -> libc::pollfd {
    libc::pollfd {
        fd,
        events,
        revents: 0,
    }
}

fn poll(descriptors: &mut [libc::pollfd], timeout: i32) -> io::Result<()> {
    if unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, timeout) } < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    Ok(())
}
