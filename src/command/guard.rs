use std::io;
use std::os::fd::RawFd;

pub(super) const NORMAL: i32 = 0;
pub(super) const TIMED_OUT: i32 = 1;
pub(super) const CANCELLED: i32 = 2;
pub(super) const FAILED: i32 = 3;

unsafe extern "C" {
    fn _Fork() -> libc::pid_t;
}

#[derive(Clone, Copy)]
pub(super) struct Controls {
    pub file_limit: Option<u64>,
    pub memory_limit: Option<u64>,
    pub cancel: RawFd,
    pub result: RawFd,
    pub owner: RawFd,
    pub deadline: i64,
}

pub(super) fn monotonic_ms() -> i64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
    time.tv_sec
        .saturating_mul(1000)
        .saturating_add(time.tv_nsec / 1_000_000)
}

pub(super) unsafe fn enter(controls: Controls) -> io::Result<()> {
    unsafe {
        let mut signals: libc::sigaction = std::mem::zeroed();
        signals.sa_sigaction = libc::SIG_DFL;
        libc::sigemptyset(&mut signals.sa_mask);
        if libc::sigaction(libc::SIGCHLD, &signals, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::syscall(libc::SYS_prctl, libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0 {
            return Err(io::Error::last_os_error());
        }
        let child = _Fork();
        if child < 0 {
            return Err(io::Error::last_os_error());
        }
        if child == 0 {
            for (resource, value) in [
                (libc::RLIMIT_FSIZE, controls.file_limit),
                (libc::RLIMIT_AS, controls.memory_limit),
            ] {
                if let Some(value) = value {
                    let limit = libc::rlimit {
                        rlim_cur: value as _,
                        rlim_max: value as _,
                    };
                    if libc::setrlimit(resource, &limit) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
            }
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            libc::close(controls.cancel);
            libc::close(controls.result);
            libc::close(controls.owner);
            return Ok(());
        }
        libc::setpgid(child, child);
        let setup = close_inherited(controls);
        let reason = if setup {
            await_exit(child, controls)
        } else {
            FAILED
        };
        let (status, clean) = clean_group(child);
        report(controls.result, status, if clean { reason } else { FAILED });
        libc::_exit(0);
    }
}

fn close_inherited(controls: Controls) -> bool {
    let mut keep = [controls.cancel, controls.result, controls.owner];
    keep.sort_unstable();
    let mut first = 0_u32;
    for fd in keep {
        if first < fd as u32
            && unsafe { libc::syscall(libc::SYS_close_range, first, fd as u32 - 1, 0_u32) } < 0
        {
            return false;
        }
        first = fd as u32 + 1;
    }
    unsafe { libc::syscall(libc::SYS_close_range, first, u32::MAX, 0_u32) == 0 }
}

fn await_exit(child: i32, controls: Controls) -> i32 {
    let child_fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child, 0_u32) as i32 };
    if child_fd < 0 {
        return FAILED;
    }
    let mut fds = [
        pollfd(controls.cancel),
        pollfd(controls.owner),
        pollfd(child_fd),
    ];
    let reason = loop {
        let remaining = controls.deadline.saturating_sub(monotonic_ms());
        if remaining <= 0 {
            break TIMED_OUT;
        }
        let ready = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as _,
                remaining.min(i32::MAX as i64) as i32,
            )
        };
        if ready < 0 {
            if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break FAILED;
        }
        if fds[0].revents != 0 || fds[1].revents != 0 {
            break CANCELLED;
        }
        if fds[2].revents != 0 {
            break NORMAL;
        }
    };
    unsafe { libc::close(child_fd) };
    reason
}

fn pollfd(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

fn exited(pid: i32) -> bool {
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        libc::syscall(
            libc::SYS_waitid,
            libc::P_PID,
            pid as libc::id_t,
            &mut info as *mut libc::siginfo_t,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            std::ptr::null_mut::<libc::rusage>(),
        ) == 0
            && info.si_pid() == pid
    }
}

fn pause() {
    unsafe { libc::poll(std::ptr::null_mut(), 0, 5) };
}

fn clean_group(leader: i32) -> (i32, bool) {
    unsafe { libc::kill(-leader, libc::SIGTERM) };
    let grace = monotonic_ms().saturating_add(150);
    while monotonic_ms() < grace {
        if exited(leader) && children(leader, false) == Some(false) {
            break;
        }
        pause();
    }
    unsafe { libc::kill(-leader, libc::SIGKILL) };
    let deadline = monotonic_ms().saturating_add(1000);
    while monotonic_ms() < deadline {
        if exited(leader) && children(leader, true) == Some(false) {
            let mut status = 0;
            let reaped = unsafe { libc::waitpid(leader, &mut status, libc::WNOHANG) };
            return (status, reaped == leader);
        }
        pause();
    }
    (0, false)
}

fn children(leader: i32, reap: bool) -> Option<bool> {
    let mut bytes = [0_u8; 256 * 1024];
    let fd = unsafe {
        libc::open(
            c"/proc/thread-self/children".as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return None;
    }
    let mut length = 0;
    loop {
        let count = unsafe {
            libc::read(
                fd,
                bytes.as_mut_ptr().add(length).cast(),
                bytes.len() - length,
            )
        };
        if count <= 0 || length + count as usize == bytes.len() {
            unsafe { libc::close(fd) };
            if count != 0 {
                return None;
            }
            break;
        }
        length += count as usize;
    }
    let mut pending = false;
    let mut pid = 0_i32;
    for byte in bytes[..length].iter().chain(std::iter::once(&b' ')) {
        if byte.is_ascii_digit() {
            pid = pid.checked_mul(10)?.checked_add((byte - b'0') as i32)?;
        } else if pid != 0 {
            if pid != leader {
                let owned =
                    unsafe { libc::syscall(libc::SYS_getpgid, pid) } == leader as libc::c_long;
                if reap || !owned {
                    unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
                }
                pending |= owned && (reap || !exited(pid));
            }
            pid = 0;
        }
    }
    Some(pending)
}

fn report(fd: RawFd, status: i32, reason: i32) {
    let message = [status.to_ne_bytes(), reason.to_ne_bytes()];
    let mut offset = 0;
    while offset < 8 {
        let written = unsafe {
            libc::write(
                fd,
                message.as_ptr().cast::<u8>().add(offset).cast(),
                8 - offset,
            )
        };
        if written > 0 {
            offset += written as usize;
        } else if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            break;
        }
    }
}
