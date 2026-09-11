use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod operations;
pub mod transport;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Holder {
    pub pid: u32,
    pub since: u64,
    pub host: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    #[error("native state authority is held; diagnostic: {diagnostic:?}")]
    Held { diagnostic: Option<Holder> },
    #[error("unsafe native authority storage: {0}")]
    Unsafe(String),
    #[error("native authority I/O: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug)]
pub struct Authority {
    root: PathBuf,
    directory: File,
    lock: File,
}

impl Authority {
    pub fn acquire(state_root: impl AsRef<Path>) -> Result<Self, LeaseError> {
        let state_root = state_root.as_ref();
        if !state_root.is_absolute() {
            return Err(LeaseError::Unsafe("state root must be absolute".into()));
        }
        fs::create_dir_all(state_root)?;
        let root = fs::canonicalize(state_root)?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&root)?;
        let metadata = directory.metadata()?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
            return Err(LeaseError::Unsafe(
                "state root must be owned by this user and not writable by others".into(),
            ));
        }
        let lock_path = PathBuf::from(format!(
            "/proc/self/fd/{}/authority.lock",
            directory.as_raw_fd()
        ));
        let mut lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(lock_path)?;
        let metadata = lock.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(LeaseError::Unsafe(
                "authority.lock must be a private regular file with one link".into(),
            ));
        }
        let mut range: libc::flock = unsafe { std::mem::zeroed() };
        range.l_type = libc::F_WRLCK as _;
        range.l_whence = libc::SEEK_SET as _;
        if unsafe { libc::fcntl(lock.as_raw_fd(), libc::F_OFD_SETLK, &range) } != 0 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(libc::EAGAIN | libc::EACCES)) {
                let mut diagnostic = String::new();
                let _ = (&mut lock).take(4096).read_to_string(&mut diagnostic);
                return Err(LeaseError::Held {
                    diagnostic: serde_json::from_str(&diagnostic).ok(),
                });
            }
            return Err(error.into());
        }
        let holder = Holder {
            pid: std::process::id(),
            since: now(),
            host: fs::read_to_string("/proc/sys/kernel/hostname")
                .unwrap_or_default()
                .trim()
                .chars()
                .take(255)
                .collect(),
        };
        lock.set_len(0)?;
        lock.rewind()?;
        lock.write_all(&serde_json::to_vec(&holder).map_err(io::Error::other)?)?;
        lock.sync_data()?;
        let authority = Self {
            root,
            directory,
            lock,
        };
        authority.verify()?;
        Ok(authority)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn identity(&self) -> io::Result<(u64, u64)> {
        let metadata = self.directory.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    pub fn verify(&self) -> Result<(), LeaseError> {
        let directory = fs::symlink_metadata(&self.root)?;
        let current = fs::symlink_metadata(self.root.join("authority.lock"))?;
        let held = self.lock.metadata()?;
        if !directory.is_dir()
            || directory.uid() != unsafe { libc::geteuid() }
            || directory.mode() & 0o022 != 0
            || (directory.dev(), directory.ino()) != self.identity()?
            || !current.is_file()
            || (current.dev(), current.ino()) != (held.dev(), held.ino())
            || held.nlink() != 1
            || held.uid() != unsafe { libc::geteuid() }
            || held.mode() & 0o077 != 0
        {
            return Err(LeaseError::Unsafe(
                "authority storage identity changed".into(),
            ));
        }
        Ok(())
    }
}

pub fn selected_root() -> crate::AppResult<Option<PathBuf>> {
    let Some(root) = std::env::var_os("FILEBLADE_NATIVE_STATE_ROOT") else {
        return Ok(None);
    };
    let root = PathBuf::from(root);
    let expected =
        crate::paths::xdg_home("XDG_STATE_HOME", "~/.local/state").join("omarchy/fileblade");
    let same = root == expected
        || fs::canonicalize(&root)
            .ok()
            .zip(fs::canonicalize(&expected).ok())
            .is_some_and(|(a, b)| a == b);
    if !root.is_absolute() || !same {
        return Err(crate::AppError::command(
            "native state root must match the selected XDG state namespace",
        ));
    }
    Ok(Some(root))
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
