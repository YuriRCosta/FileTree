use crate::common::path_text;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub const MAX_WATCH_PATHS: usize = 512;
pub const MAX_WATCH_CANDIDATES: usize = 2048;
pub const MAX_WATCH_BYTES: usize = 64 * 1024;
pub const SCOPES: [&str; 3] = ["all", "user", "project"];
const MAX_WALK_UP: usize = 64;

#[derive(Default)]
pub struct WatchPlan {
    paths: HashSet<PathBuf>,
    candidates: HashSet<PathBuf>,
    truncated: bool,
}

impl WatchPlan {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.paths.iter().cloned().collect();
        paths.sort_by(|left, right| {
            left.as_os_str()
                .as_bytes()
                .cmp(right.as_os_str().as_bytes())
        });
        paths
    }

    pub fn directory(&mut self, path: &Path) {
        if !path.is_absolute() || self.candidates.contains(path) {
            return;
        }
        if self.paths.len() >= MAX_WATCH_PATHS || self.candidates.len() >= MAX_WATCH_CANDIDATES {
            self.truncated = true;
            return;
        }
        self.candidates.insert(path.to_path_buf());
        let mut current = path.to_path_buf();
        for _ in 0..MAX_WALK_UP {
            if current.is_dir() {
                let resolved = std::fs::canonicalize(&current).unwrap_or(current);
                self.paths.insert(resolved);
                return;
            }
            let Some(parent) = current.parent() else {
                return;
            };
            if parent == current {
                return;
            }
            current = parent.to_path_buf();
        }
    }

    pub fn watch_path(&mut self, path: &Path, directory: bool) {
        if let Some(parent) = path.parent() {
            self.directory(parent);
        }
        if directory {
            self.directory(path);
            return;
        }
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink())
            && let Ok(resolved) = std::fs::canonicalize(path)
            && let Some(parent) = resolved.parent()
        {
            self.directory(parent);
        }
    }

    pub fn finish(&mut self, document: &mut Map<String, Value>) {
        let mut encoded = Vec::new();
        let mut size = 2;
        for path in self.paths() {
            let text = path_text(&path);
            let mut wire = String::new();
            super::canonical::escape_ascii(&text, &mut wire);
            let cost = wire.len() + 1;
            if size + cost > MAX_WATCH_BYTES {
                self.truncated = true;
                break;
            }
            size += cost;
            encoded.push(Value::from(text));
        }
        document.insert("watchPaths".to_string(), Value::Array(encoded));
        document.insert("watchTruncated".to_string(), json!(self.truncated));
    }
}

pub fn lane_rows(rows: Vec<Value>, scope: &str, project_scopes: &[&str], key: &str) -> Vec<Value> {
    let selects = |row: &Value| {
        row.get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| project_scopes.contains(&value))
    };
    match scope {
        "project" => rows.into_iter().filter(selects).collect(),
        "user" => rows.into_iter().filter(|row| !selects(row)).collect(),
        _ => rows,
    }
}
