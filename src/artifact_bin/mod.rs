use crate::common::{display_path, expanded_path, parse_path, path_text};
use crate::secure::{self, EntryKind, PRIVATE_DIRECTORY_MODE, PRIVATE_FILE_MODE};
use crate::{AppError, AppResult};
use chrono::{Duration as ChronoDuration, Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use uuid::Uuid;
use xattr::FileExt;

mod listing;
mod metadata;
mod restore;
mod store;
mod transaction;
pub use listing::*;
use metadata::*;
pub use restore::*;
pub use store::*;
pub use transaction::{remove_with_helper, restore_with_helper};
const SCHEMA_VERSION: u32 = 1;
const BIN_PREFIX: &str = "bin:";
const MANIFEST_NAME: &str = "manifest.json";
const ITEMS_DIRECTORY: &str = "items";
const MAX_ENTRIES: usize = 500;
const MAX_MODULES: usize = 256;
const MAX_ALL_ENTRIES: usize = 10_000;
const MAX_ITEM_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TREE_ENTRIES: usize = 2_000;
const MAX_TREE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TREE_DEPTH: usize = 12;
const MAX_ITEM_JSON: usize = 64 * 1024;
const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_HEAD_BYTES: usize = 8 * 1024;
const COPY_CHUNK: usize = 64 * 1024;
const MAX_LIST_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_LIST_ITEMS: usize = 10_000;
const MAX_LIST_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_LIST_TIME: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, Serialize)]
struct ResultRow {
    path: String,
    ok: bool,
    changed: bool,
    message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredItem {
    #[serde(rename = "from")]
    source: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target_bytes: Option<Vec<u8>>,
    mode: u32,
    stored: String,
    size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dev: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ino: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    atime_ns: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mtime_ns: Option<i64>,
}

impl StoredItem {
    fn link_target(&self) -> PathBuf {
        use std::os::unix::ffi::OsStringExt;
        self.target_bytes
            .as_ref()
            .map(|bytes| PathBuf::from(OsString::from_vec(bytes.clone())))
            .unwrap_or_else(|| PathBuf::from(&self.target))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    module: String,
    id: String,
    name: String,
    kind: String,
    scope: String,
    detail: String,
    path: String,
    realpath: String,
    #[serde(rename = "deletedAt")]
    deleted_at: String,
    #[serde(
        default,
        rename = "deletedAtEpoch",
        skip_serializing_if = "Option::is_none"
    )]
    deleted_at_epoch: Option<i64>,
    #[serde(default)]
    payload: Value,
    #[serde(
        default,
        rename = "restoreHelper",
        skip_serializing_if = "Option::is_none"
    )]
    restore_helper: Option<crate::module_helpers::Route>,
    #[serde(
        default,
        rename = "helperRecordId",
        skip_serializing_if = "Option::is_none"
    )]
    helper_record_id: Option<String>,
    #[serde(default, rename = "helperRestored")]
    helper_restored: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    position: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    groups: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    metrics: BTreeMap<String, Value>,
    items: Vec<StoredItem>,
    #[serde(
        default,
        rename = "restoreCompleted",
        skip_serializing_if = "Vec::is_empty"
    )]
    restore_completed: Vec<String>,
}

struct ParsedItem {
    id: String,
    name: String,
    kind: String,
    scope: String,
    detail: String,
    path: String,
    realpath: String,
    paths: Vec<String>,
    payload: Value,
    position: Option<u32>,
    groups: Vec<String>,
    metrics: BTreeMap<String, Value>,
}

struct Inventory {
    items: Vec<StoredItem>,
    bytes: u64,
}

fn bin_root(module: &str) -> AppResult<PathBuf> {
    Ok(bin_base()?.join(module))
}

fn bin_base() -> AppResult<PathBuf> {
    if let Some(root) = crate::lease::selected_root()? {
        return Ok(root.join("artifact-bin"));
    }
    let base = match std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        Some(value) => {
            let path = PathBuf::from(value);
            if !path.is_absolute() || path.as_os_str().as_encoded_bytes().contains(&0) {
                return Err(AppError::invalid("XDG_DATA_HOME must be an absolute path"));
            }
            path
        }
        None => expanded_path("~/.local/share"),
    };
    Ok(base.join("fileblade/bin"))
}

fn valid_module(module: &str) -> bool {
    let bytes = module.as_bytes();
    (1..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || *value == b'-')
}

fn valid_entry_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=121).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..].iter().all(|value| {
            value.is_ascii_alphanumeric() || matches!(*value, b'.' | b'_' | b':' | b'-')
        })
        && !name.contains("..")
}

fn nanoseconds(seconds: i64, nanoseconds: i64) -> i64 {
    seconds
        .saturating_mul(1_000_000_000)
        .saturating_add(nanoseconds)
}

fn split_nanoseconds(value: i64) -> (i64, i64) {
    (
        value.div_euclid(1_000_000_000),
        value.rem_euclid(1_000_000_000),
    )
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

fn mutation_lease() -> Result<secure::LockedFile, String> {
    let path = bin_base()
        .map_err(|error| error.to_string())?
        .join(".mutation.lock");
    secure::try_open_private_lock(&path)
        .map_err(|error| format!("cannot lock recovery storage: {error}"))?
        .ok_or_else(|| "Another recovery operation is running; retry when it finishes".to_string())
}
