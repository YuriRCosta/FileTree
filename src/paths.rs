use std::path::PathBuf;

use crate::common::expanded_path;

const CURRENT: &str = "omarchy/fileblade";
const LEGACY: &str = "omarchy/filetree";

pub fn xdg_home(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| expanded_path(fallback))
}

pub fn screenshots_dir() -> PathBuf {
    if let Some(configured) = std::env::var_os("OMARCHY_SCREENSHOT_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return configured;
    }
    if let Some(pictures) = user_directory("XDG_PICTURES_DIR") {
        return pictures;
    }
    expanded_path("~/Pictures")
}

pub fn downloads_dir() -> PathBuf {
    user_directory("XDG_DOWNLOAD_DIR").unwrap_or_else(|| expanded_path("~/Downloads"))
}

fn user_directory(key: &str) -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Some(value);
    }
    let document = xdg_home("XDG_CONFIG_HOME", "~/.config").join("user-dirs.dirs");
    let text = std::fs::read_to_string(&document).ok()?;
    if text.len() > 64 * 1024 {
        return None;
    }
    for line in text.lines().take(256) {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let value = rest.trim().trim_matches('"');
        let resolved = match value.strip_prefix("$HOME/") {
            Some(tail) => expanded_path("~").join(tail),
            None => PathBuf::from(value),
        };
        if resolved.is_absolute() {
            return Some(resolved);
        }
    }
    None
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
