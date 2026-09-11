use crate::command::{CommandSpec, which};
use crate::common::{CONTROL_TIMEOUT, expanded_path, normalize_path, parse_path, path_text};
use crate::secure::{self, EntryKind};
use crate::{AppError, AppResult};
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

mod compound;
mod fingerprint;
mod permissions;
mod record;
mod step;
mod store;
mod trash;
pub use compound::*;
pub use fingerprint::*;
pub use permissions::*;
pub use record::*;
pub use step::*;
pub use store::*;
pub use trash::*;
const STACK_LIMIT: usize = 100;
const TREE_DIGEST_LIMIT: usize = 50_000;
const JOURNAL_BYTES_LIMIT: usize = 4 * 1024 * 1024;
const TRASHINFO_BYTES_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferMapping {
    pub source: String,
    pub destination: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeFingerprint {
    #[serde(default)]
    pub digest: String,
    pub count: u64,
    pub bytes: u64,
    pub mtime_ns: i64,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fingerprint {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub mtime_ns: i64,
    pub kind: String,
    #[serde(default)]
    pub ctime_ns: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<TreeFingerprint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalItem {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_dir: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trash_dir: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trash_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before_value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after_value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<Fingerprint>,
    #[serde(skip)]
    pub unverified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub at: String,
    pub items: Vec<JournalItem>,
}

pub type TrashSnapshot = BTreeMap<PathBuf, BTreeSet<OsString>>;

enum ApplyFailure {
    Io(io::Error),
    Partial { done: usize, error: io::Error },
}

pub fn journal_path() -> PathBuf {
    if let Some(value) = std::env::var_os("FILEBLADE_JOURNAL").filter(|value| !value.is_empty()) {
        return match value.to_str() {
            Some(text) => expanded_path(text),
            None => std::path::absolute(PathBuf::from(value))
                .unwrap_or_else(|_| crate::paths::state_dir().join("journal.json")),
        };
    }
    crate::paths::state_dir().join("journal.json")
}

pub fn new_entry_id() -> String {
    format!("op-{}", Uuid::new_v4().simple())
}

fn check_cancelled(cancelled: &AtomicBool) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "operation cancelled",
        ))
    } else {
        Ok(())
    }
}

fn ignore_path(_: &Path) {}

fn is_false(value: &bool) -> bool {
    !value
}
