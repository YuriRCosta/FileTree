use crate::common::{display_path, human_size, normalize_path, parse_path, path_text};
use crate::filesystem::entry_mime;
use crate::secure::{self, EntryKind, EntryStat};
use chrono::{Duration as ChronoDuration, Local, NaiveDateTime};
use rustix::process::{geteuid, getuid};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

mod catalog;
mod mutate;
mod stores;
use catalog::*;
use mutate::*;
use stores::*;
const MAX_LIST_ENTRIES: usize = 1000;
const MAX_CANDIDATES: usize = 10_000;
const MAX_STORE_CANDIDATES: usize = 10_000;
const MAX_STORES: usize = 256;
const MAX_MOUNTS: usize = 512;
const MAX_MOUNT_LINES: usize = 4096;
const MAX_MOUNT_LINE_BYTES: usize = 16 * 1024;
const MAX_MOUNTINFO_BYTES: usize = 1024 * 1024;
const MAX_TRASHINFO_BYTES: usize = 64 * 1024;
const MAX_DIRECTORYSIZES_BYTES: usize = 1024 * 1024;
const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_ERRORS: usize = 64;
const MAX_ERROR_BYTES: usize = 1024;
const MAX_ID_BYTES: usize = 16 * 1024;
const MAX_PATH_BYTES: usize = 64 * 1024;
const MAX_SELECTED_ENTRIES: usize = 512;
const MAX_MUTATION_RESULTS: usize = 512;
const MAX_MUTATION_RESULT_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct TrashContext {
    home_trash: PathBuf,
    mount_tops: Vec<PathBuf>,
}

#[derive(Clone)]
enum StoreKind {
    Home { base: PathBuf },
    Mount { top: PathBuf },
}

#[derive(Clone)]
struct TrashStore {
    path: PathBuf,
    source_mount: PathBuf,
    kind: StoreKind,
}

#[derive(Clone)]
struct TrashMetadata {
    original: PathBuf,
    deleted_at: String,
}

#[derive(Clone)]
struct TrashEntry {
    id: String,
    store: TrashStore,
    stored_name: OsString,
    info_path: PathBuf,
    stored_path: PathBuf,
    info_stat: Option<EntryStat>,
    stored_stat: Option<EntryStat>,
    metadata: Option<TrashMetadata>,
    size: Option<u64>,
}

struct Catalog {
    entries: Vec<TrashEntry>,
    store_count: usize,
    watch_paths: Vec<PathBuf>,
    errors: Vec<String>,
    scanned: usize,
    truncated: bool,
    cancelled: bool,
}

struct MetadataBudget {
    remaining: usize,
    exhausted: bool,
}

impl TrashContext {
    pub fn system() -> Self {
        Self {
            home_trash: home_trash(),
            mount_tops: mounted_top_directories(),
        }
    }

    pub fn for_roots(home_trash: PathBuf, mount_tops: Vec<PathBuf>) -> Self {
        Self {
            home_trash: normalized_absolute(&home_trash).unwrap_or_default(),
            mount_tops: mount_tops
                .into_iter()
                .filter_map(|path| normalized_absolute(&path))
                .take(MAX_MOUNTS)
                .collect(),
        }
    }

    pub fn list(&self, limit: usize, cancelled: &AtomicBool) -> Value {
        let maximum = limit.clamp(1, MAX_LIST_ENTRIES);
        let catalog = build_catalog(self, cancelled);
        catalog_document(&catalog, maximum)
    }

    pub fn restore(
        &self,
        id: &str,
        destination_directory: &str,
        recreate_parent: bool,
        cancelled: &AtomicBool,
    ) -> Value {
        if cancelled.load(Ordering::Relaxed) {
            return mutation_error("trash-restore", "operation cancelled");
        }
        if id.is_empty() || id.len() > MAX_ID_BYTES {
            return mutation_error("trash-restore", "Invalid Trash entry identity");
        }
        let catalog = build_catalog(self, cancelled);
        if catalog.cancelled {
            return mutation_error("trash-restore", "operation cancelled");
        }
        let mut store_paths = catalog
            .entries
            .iter()
            .map(|entry| entry.store.path.clone())
            .collect::<Vec<_>>();
        store_paths.sort();
        store_paths.dedup();
        let Some(entry) = catalog.entries.into_iter().find(|entry| entry.id == id) else {
            return mutation_error("trash-restore", "Trash entry is missing or changed");
        };
        restore_entry(
            &entry,
            destination_directory,
            recreate_parent,
            cancelled,
            &store_paths,
        )
    }

    pub fn delete_permanently(&self, ids: &[String], cancelled: &AtomicBool) -> Value {
        delete_selected(self, ids, cancelled)
    }

    pub fn empty(&self, cancelled: &AtomicBool, progress: &mut dyn FnMut(Value)) -> Value {
        empty_context(self, cancelled, progress)
    }

    pub fn prune(
        &self,
        days: u32,
        cancelled: &AtomicBool,
        progress: &mut dyn FnMut(Value),
    ) -> Value {
        if !(1..=3650).contains(&days) {
            return mutation_error(
                "trash-prune",
                "Trash retention must be between 1 and 3650 days",
            );
        }
        let cutoff = Local::now().naive_local() - ChronoDuration::days(i64::from(days));
        prune_context(self, cutoff, cancelled, progress)
    }
}
