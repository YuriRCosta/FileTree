use super::Roots;
use crate::command::{CommandSpec, which};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LegacyWriter {
    Absent,
    Stopped {
        evidence: Vec<String>,
    },
    Active {
        pid: Option<u32>,
        evidence: Vec<String>,
    },
    Unknown {
        reason: String,
    },
}

pub fn legacy_writer(roots: &Roots) -> LegacyWriter {
    inspect(roots).unwrap_or_else(|reason| LegacyWriter::Unknown { reason })
}

fn inspect(roots: &Roots) -> Result<LegacyWriter, String> {
    let mut evidence = Vec::new();
    let mut present = false;
    let mut directories = Vec::new();
    let mut absent = Vec::new();
    for root in [&roots.state, &roots.config, &roots.recovery] {
        if !root.is_absolute() {
            return Err("legacy roots must be absolute".into());
        }
        match crate::secure::open_directory_nofollow(root) {
            Ok(fd) => {
                let file = File::from(fd);
                let metadata = file.metadata().map_err(|error| error.to_string())?;
                if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
                    return Err(format!("unsafe legacy root: {}", root.display()));
                }
                directories.push((root, file));
                present = true;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => absent.push(root),
            Err(error) => return Err(format!("legacy root {}: {error}", root.display())),
        }
    }
    let lock_path = roots.state.join("journal.json.lock");
    let lock = match directories.iter().find(|(path, _)| **path == roots.state) {
        Some((_, directory)) => {
            use rustix::fs::{Mode, OFlags, openat};
            match openat(
                directory,
                "journal.json.lock",
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                Mode::empty(),
            ) {
                Ok(fd) => Some(File::from(fd)),
                Err(rustix::io::Errno::NOENT) => None,
                Err(error) => return Err(format!("legacy journal lock: {error}")),
            }
        }
        None => None,
    };
    if let Some(file) = &lock {
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o022 != 0
        {
            return Err("legacy journal lock is not a safe owned regular file".into());
        }
        let mut range: libc::flock = unsafe { std::mem::zeroed() };
        range.l_type = libc::F_RDLCK as _;
        range.l_whence = libc::SEEK_SET as _;
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_SETLK, &range) } != 0 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(libc::EAGAIN | libc::EACCES)) {
                return Ok(LegacyWriter::Active {
                    pid: None,
                    evidence: vec!["journal.json.lock has a conflicting OFD/POSIX writer".into()],
                });
            }
            return Err(format!("legacy OFD lock probe failed: {error}"));
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Ok(LegacyWriter::Active {
                    pid: None,
                    evidence: vec![
                        "journal.json.lock has a conflicting legacy flock writer".into(),
                    ],
                });
            }
            return Err(format!("legacy flock probe failed: {error}"));
        }
        evidence.push("journal.json.lock permits shared OFD and flock probes".into());
    } else {
        evidence.push("journal.json.lock is absent".into());
    }
    let activation = crate::plugin_catalog::legacy_activation(&roots.config);
    let ipc = ipc_status();
    if let Ok(enabled) = &activation
        && !enabled.is_empty()
    {
        evidence.push(format!(
            "legacy shell activation is enabled: {}",
            enabled.join(", ")
        ));
        return Ok(LegacyWriter::Active {
            pid: None,
            evidence,
        });
    }
    match ipc {
        Ok(true) => {
            evidence.push("legacy FileBlade IPC target responds".into());
            return Ok(LegacyWriter::Active {
                pid: None,
                evidence,
            });
        }
        Ok(false) => {
            evidence.push("running shell reports legacy FileBlade IPC target absent".into())
        }
        Err(reason) => return Err(reason),
    }
    activation?;
    evidence.push("no legacy FileBlade or absorbed companion activation is enabled".into());
    for path in absent {
        match crate::secure::open_directory_nofollow(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err("legacy root appeared during writer detection".into()),
        }
    }
    for (path, file) in directories {
        let held = file.metadata().map_err(|error| error.to_string())?;
        let current = File::from(
            crate::secure::open_directory_nofollow(path).map_err(|error| error.to_string())?,
        )
        .metadata()
        .map_err(|error| error.to_string())?;
        if current.uid() != unsafe { libc::geteuid() }
            || current.mode() & 0o022 != 0
            || (held.dev(), held.ino()) != (current.dev(), current.ino())
        {
            return Err("legacy root identity changed during writer detection".into());
        }
    }
    if let Some(file) = lock {
        let held = file.metadata().map_err(|error| error.to_string())?;
        let current = std::fs::symlink_metadata(lock_path).map_err(|error| error.to_string())?;
        if !current.is_file() || (held.dev(), held.ino()) != (current.dev(), current.ino()) {
            return Err("legacy journal lock identity changed during writer detection".into());
        }
    } else {
        match std::fs::symlink_metadata(lock_path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err("legacy journal lock appeared during writer detection".into()),
        }
    }
    Ok(if present {
        LegacyWriter::Stopped { evidence }
    } else {
        LegacyWriter::Absent
    })
}

fn ipc_status() -> Result<bool, String> {
    let config = std::env::var_os("OMARCHY_PATH")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("legacy IPC configuration is unknown")?
        .join("shell");
    if !config.join("shell.qml").is_file() {
        return Err("legacy IPC shell configuration is unavailable".into());
    }
    let program = which("qs")
        .or_else(|| which("quickshell"))
        .ok_or("legacy IPC executable is unavailable")?;
    let output = CommandSpec::new(program)
        .args(["ipc", "-n", "-p"])
        .args([config])
        .args(["call", "--", "data-goblin.fileblade", "status"])
        .env("LC_ALL", "C")
        .timeout(Duration::from_secs(2))
        .limits(64 * 1024, 4096)
        .run()
        .map_err(|error| format!("legacy IPC probe failed: {error}"))?;
    if !output.status.success() || output.stdout_truncated || output.stderr_truncated {
        return Err("legacy IPC probe failed or exceeded its output bound".into());
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| "legacy IPC probe returned non-UTF-8 output")?
        .trim();
    if text == "Target not found." {
        return Ok(false);
    }
    let document: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| "legacy IPC probe returned an unrecognized response")?;
    if !document.is_object() {
        return Err("legacy IPC probe did not return a status object".into());
    }
    Ok(true)
}
