use std::path::{Path, PathBuf};

use crate::common::expanded_path;
use crate::error::{AppError, AppResult};

const CURRENT: &str = "omarchy/fileblade";
const LEGACY: &str = "omarchy/filetree";
const APP_ROOT_VARIABLE: &str = "FILEBLADE_APP_ROOT";
const APP_ROOT_MARKERS: [&str; 2] = ["manifest.json", "python"];
const APP_ROOT_SEARCH_DEPTH: usize = 4;

pub fn app_root() -> AppResult<PathBuf> {
    if let Some(value) = std::env::var_os(APP_ROOT_VARIABLE).filter(|value| !value.is_empty()) {
        let declared = PathBuf::from(value);
        if declared.is_absolute() && is_app_root(&declared) {
            return Ok(declared);
        }
        return Err(AppError::command(format!(
            "{APP_ROOT_VARIABLE} does not name an app root: {}",
            declared.display()
        )));
    }
    let executable = std::env::current_exe()
        .map_err(|error| AppError::command(format!("could not locate the executable: {error}")))?;
    let mut candidate = executable.parent();
    for _ in 0..APP_ROOT_SEARCH_DEPTH {
        let Some(directory) = candidate else { break };
        if is_app_root(directory) {
            return Ok(directory.to_path_buf());
        }
        candidate = directory.parent();
    }
    Err(AppError::command(format!(
        "no app root above {} within {APP_ROOT_SEARCH_DEPTH} levels; set {APP_ROOT_VARIABLE}",
        executable.display()
    )))
}

fn is_app_root(directory: &Path) -> bool {
    directory.is_dir()
        && APP_ROOT_MARKERS
            .iter()
            .all(|marker| directory.join(marker).symlink_metadata().is_ok())
}

pub fn xdg_home(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| expanded_path(fallback))
}

pub fn state_dir() -> PathBuf {
    migrated(xdg_home("XDG_STATE_HOME", "~/.local/state"))
}

pub fn config_dir() -> PathBuf {
    migrated(xdg_home("XDG_CONFIG_HOME", "~/.config"))
}

fn migrated(home: PathBuf) -> PathBuf {
    let current = home.join(CURRENT);
    if current.symlink_metadata().is_ok() {
        return current;
    }
    let legacy = home.join(LEGACY);
    let legacy_is_plain_dir = legacy
        .symlink_metadata()
        .map(|meta| meta.is_dir())
        .unwrap_or(false);
    if legacy_is_plain_dir {
        let _ = std::fs::rename(&legacy, &current);
    }
    current
}
