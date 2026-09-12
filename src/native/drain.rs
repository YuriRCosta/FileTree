use crate::{AppError, AppResult};
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn run(timeout_ms: u64) -> Value {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut result = json!({"schema": 1, "action": "drain", "status": "busy",
        "operation_ids": [], "dirty_note_ids": [], "error": ""});
    let mut view = None;
    if let Err(error) = drain(deadline, &mut result, &mut view) {
        result["status"] = json!(if Instant::now() >= deadline {
            "busy"
        } else {
            "error"
        });
        result["error"] = json!(error.to_string());
    }
    if result["status"] != "drained"
        && let Some((payload, token)) = view
    {
        let _ = qml(&payload, deadline, "drainAbort", &[token]);
    }
    result
}

fn drain(
    deadline: Instant,
    result: &mut Value,
    cleanup: &mut Option<(PathBuf, String)>,
) -> AppResult<()> {
    let root = crate::lease::selected_root()?
        .ok_or_else(|| AppError::command("native state root is not configured"))?;
    let payload = crate::paths::app_root()?;
    let portals = portal_processes(deadline)?;
    let lock = open_lock(&root)?;
    let mut socket = match crate::lease::transport::connect(&root) {
        Ok(socket) => socket,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            if lock
                .as_ref()
                .is_some_and(|lock| !released(lock).unwrap_or(false))
            {
                return Err(AppError::command(
                    "native authority lock is held but its socket is unavailable",
                ));
            }
            if qml(&payload, deadline, "status", &[])?.is_some() {
                return Err(AppError::command(
                    "native view is live without its authority; preserve it and retry",
                ));
            }
            wait_portals(&portals, deadline)?;
            result["status"] = json!("already_stopped");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let lock = lock.ok_or_else(|| AppError::command("native authority has no lock file"))?;
    let mut reader = BufReader::new(socket.try_clone()?);
    exchange(
        &mut socket,
        &mut reader,
        deadline,
        json!({"v":1,"type":"hello"}),
    )?
    .get("authority")
    .filter(|value| **value == true)
    .ok_or_else(|| AppError::command("native authority handshake failed"))?;
    let state = command(&mut socket, &mut reader, deadline, "status")?;
    result["operation_ids"] = state["payload"]["operation_ids"].clone();
    let token = uuid::Uuid::new_v4().to_string();
    let has_view = state["payload"]["views"].as_u64().unwrap_or(0) > 0;
    let view_status = qml(&payload, deadline, "status", &[])?;
    if state["payload"]["views"].as_u64().unwrap_or(0) > 1 || (!has_view && view_status.is_some()) {
        result["error"] =
            json!("native view attachment is changing or has additional participants; retry");
        return Ok(());
    }
    if has_view {
        let status = view_status.ok_or_else(|| {
            AppError::command("native authority has a view but its control entry is unavailable")
        })?;
        if status["sourceDir"]
            .as_str()
            .map(PathBuf::from)
            .and_then(|path| std::fs::canonicalize(path).ok())
            != Some(std::fs::canonicalize(&payload)?)
        {
            return Err(AppError::command(
                "native view belongs to a different payload",
            ));
        }
        *cleanup = Some((payload.clone(), token.clone()));
        qml(
            &payload,
            deadline,
            "drainBegin",
            &[token.clone(), remaining(deadline)?.as_millis().to_string()],
        )?
        .ok_or_else(|| AppError::command("native view did not acknowledge drain"))?;
    }
    loop {
        remaining(deadline)?;
        let state = command(&mut socket, &mut reader, deadline, "status")?;
        result["operation_ids"] = state["payload"]["operation_ids"].clone();
        let view = if has_view {
            qml(
                &payload,
                deadline,
                "drainStatus",
                std::slice::from_ref(&token),
            )?
            .ok_or_else(|| AppError::command("native view disappeared during drain"))?
        } else {
            json!({"ready":true,"dirty_note_ids":[]})
        };
        result["dirty_note_ids"] = view["dirty_note_ids"].clone();
        if view["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
        {
            result["error"] = view["error"].clone();
            return Ok(());
        }
        if view["ready"] == true
            && state["payload"]["operation_ids"]
                .as_array()
                .is_some_and(Vec::is_empty)
        {
            let paused = command(&mut socket, &mut reader, deadline, "quiesce")?;
            if paused["ok"] == true {
                if paused["payload"]["views"].as_u64() != Some(u64::from(has_view)) {
                    result["error"] = json!("native view attachment changed during drain; retry");
                    return Ok(());
                }
                if has_view {
                    let committed = qml(
                        &payload,
                        deadline,
                        "drainCommit",
                        std::slice::from_ref(&token),
                    )?;
                    let Some(committed) = committed else {
                        command(&mut socket, &mut reader, deadline, "resume")?;
                        result["error"] = json!("native view disappeared before confirming drain");
                        return Ok(());
                    };
                    if committed["ready"] != true {
                        command(&mut socket, &mut reader, deadline, "resume")?;
                        std::thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                }
                loop {
                    let exited = command(&mut socket, &mut reader, deadline, "exit")?;
                    if exited["ok"] == true {
                        break;
                    }
                    std::thread::sleep(remaining(deadline)?.min(Duration::from_millis(20)));
                }
                drop(reader);
                drop(socket);
                while !released(&lock)? {
                    std::thread::sleep(remaining(deadline)?.min(Duration::from_millis(20)));
                }
                let current_lock = open_lock(&root)?.ok_or_else(|| {
                    AppError::command("native authority lock disappeared during drain")
                })?;
                let before = lock.metadata()?;
                let after = current_lock.metadata()?;
                if before.dev() != after.dev() || before.ino() != after.ino() {
                    return Err(AppError::command(
                        "native authority lock identity changed during drain",
                    ));
                }
                wait_portals(&portals, deadline)?;
                result["status"] = json!("drained");
                return Ok(());
            }
        }
        std::thread::sleep(remaining(deadline)?.min(Duration::from_millis(20)));
    }
}

fn remaining(deadline: Instant) -> AppResult<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| {
            AppError::command("native drain deadline reached; active work was left alive")
        })
}

fn command(
    socket: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    deadline: Instant,
    action: &str,
) -> AppResult<Value> {
    exchange(
        socket,
        reader,
        deadline,
        json!({"v":1,"type":"drain","action":action,
        "timeout_ms":remaining(deadline)?.as_millis().max(1)}),
    )
}

fn exchange(
    socket: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    deadline: Instant,
    frame: Value,
) -> AppResult<Value> {
    socket.set_read_timeout(Some(remaining(deadline)?))?;
    socket.set_write_timeout(Some(remaining(deadline)?))?;
    serde_json::to_writer(&mut *socket, &frame)?;
    socket.write_all(b"\n")?;
    use std::io::BufRead;
    let mut line = Vec::new();
    reader.take(1024 * 1024 + 1).read_until(b'\n', &mut line)?;
    if line.len() > 1024 * 1024 || line.last() != Some(&b'\n') {
        return Err(AppError::command(
            "native drain received an incomplete or excessive authority frame",
        ));
    }
    let value: Value = serde_json::from_slice(&line)?;
    if value["v"] != 1 || value["type"] != frame["type"] {
        return Err(AppError::command(
            "native drain authority protocol mismatch",
        ));
    }
    Ok(value)
}

fn qml(
    payload: &Path,
    deadline: Instant,
    method: &str,
    arguments: &[String],
) -> AppResult<Option<Value>> {
    remaining(deadline)?;
    let mut child = Command::new("qs")
        .args(["ipc", "-n", "-p"])
        .arg(payload.join("app"))
        .args(["call", "--", "fileblade.native", method])
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::command(
                "native drain control query exceeded its deadline",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output()?;
    if let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) {
        return Ok(Some(value));
    }
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if diagnostic.contains("No running instances") {
        return Ok(None);
    }
    Err(AppError::command(format!(
        "native view control failed: {}",
        diagnostic.trim()
    )))
}

fn open_lock(root: &Path) -> AppResult<Option<File>> {
    let path = root.join("authority.lock");
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(AppError::command("unsafe native authority lock"));
    }
    Ok(Some(file))
}

fn released(file: &File) -> AppResult<bool> {
    let mut range: libc::flock = unsafe { std::mem::zeroed() };
    range.l_type = libc::F_WRLCK as _;
    range.l_whence = libc::SEEK_SET as _;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_GETLK, &mut range) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(i32::from(range.l_type) == libc::F_UNLCK)
}

fn portal_processes(deadline: Instant) -> AppResult<Vec<PathBuf>> {
    let executable = std::fs::metadata(std::env::current_exe()?)?;
    let mut processes = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        remaining(deadline)?;
        let entry = entry?;
        if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
            continue;
        }
        let path = entry.path();
        if let Ok(candidate) = std::fs::metadata(path.join("exe"))
            && candidate.dev() == executable.dev()
            && candidate.ino() == executable.ino()
            && let Ok(argv) = std::fs::read(path.join("cmdline"))
            && argv.split(|byte| *byte == 0).skip(1).collect::<Vec<_>>()
                == [b"native".as_slice(), b"portal".as_slice(), b""]
        {
            processes.push(path);
        }
    }
    Ok(processes)
}

fn wait_portals(processes: &[PathBuf], deadline: Instant) -> AppResult<()> {
    while !processes.is_empty()
        && portal_processes(deadline)?
            .iter()
            .any(|path| processes.contains(path))
    {
        std::thread::sleep(remaining(deadline)?.min(Duration::from_millis(20)));
    }
    Ok(())
}
