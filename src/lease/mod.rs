use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod authority;
mod storage;
pub use authority::{Authority, RootIdentity, WriteMode};

pub mod operations;
pub mod persistence;
pub mod transport;

pub fn native_config_root() -> PathBuf {
    crate::paths::xdg_home("XDG_CONFIG_HOME", "~/.config").join("omarchy/fileblade")
}

pub fn native_recovery_root() -> PathBuf {
    crate::paths::xdg_home("XDG_STATE_HOME", "~/.local/state").join("fileblade")
}

pub fn native_extension_root() -> PathBuf {
    crate::paths::xdg_home("XDG_CONFIG_HOME", "~/.config").join("fileblade/extensions")
}

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
