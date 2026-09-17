use crate::command::{CommandSpec, which};
use crate::common::{display_path, file_name, normalize_path, parse_path, path_error, path_text};
use crate::filesystem::{entry_mime, has_git_marker, read_regular_prefix};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

mod batch;
mod cache;
mod excludes;
mod places;

pub(crate) use batch::cached_git_repositories_for_markers_bounded;
pub use batch::{
    git_metadata_batch, git_metadata_batch_cancellable, git_repositories_for_markers,
    git_repositories_for_markers_bounded, git_repositories_for_markers_cancellable,
};
pub use excludes::ignored_paths;
pub use places::git_places;

pub const MAX_GIT_STATUS_BYTES: usize = 4 * 1024 * 1024;
const MAX_GIT_STATUS_ENTRIES: usize = 100_000;
const MAX_GIT_INDEX_PATHS: usize = 100_000;
const MAX_GIT_AGGREGATE_BYTES: usize = 16 * 1024 * 1024;
const MAX_GIT_BATCH_PATHS: usize = 1000;
const MAX_GIT_MARKERS: usize = 1000;

#[derive(Clone, Debug)]
pub struct GitStatus {
    pub path: PathBuf,
    pub relative: String,
    pub status: String,
    pub label: String,
    pub index_status: String,
    pub worktree_status: String,
    pub original_path: String,
    pub deleted: bool,
}

#[derive(Clone, Debug)]
pub struct GitRepository {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub name: String,
    pub worktree: String,
    pub branch: String,
    pub upstream: String,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub linked: bool,
    pub ok: bool,
    pub error: String,
    pub dirty: bool,
    pub entries: Vec<GitStatus>,
    summary_counts: GitStatusCounts,
}

#[derive(Clone, Debug)]
pub struct GitStatusIndex {
    statuses: Vec<GitStatus>,
    exact: HashMap<PathBuf, usize>,
    descendants: HashMap<PathBuf, usize>,
    counts: HashMap<PathBuf, GitStatusCounts>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GitStatusCounts {
    pub modified: usize,
    pub deleted: usize,
    pub new: usize,
    untracked: usize,
    added: usize,
    copied: usize,
    type_changed: usize,
    renamed: usize,
    conflicted: usize,
}

pub fn git_repository(raw_path: &str) -> Option<GitRepository> {
    git_repository_cancellable(raw_path, &AtomicBool::new(false))
}

pub fn git_repository_cancellable(raw_path: &str, cancelled: &AtomicBool) -> Option<GitRepository> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let path = parse_path(raw_path).ok()?;
    let probe = if path.is_dir() {
        path
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("/"))
            .to_path_buf()
    };
    let output = run_git(
        [
            OsString::from("-C"),
            probe.into_os_string(),
            OsString::from("rev-parse"),
            OsString::from("--show-toplevel"),
            OsString::from("--absolute-git-dir"),
        ],
        Duration::from_secs(2),
        64 * 1024,
        cancelled,
    )
    .ok()?;
    if !output.status.success() || output.stdout_truncated {
        return None;
    }
    let mut lines = output
        .stdout
        .strip_suffix(b"\n")?
        .split(|byte| *byte == b'\n');
    let root = PathBuf::from(OsStr::from_bytes(lines.next()?));
    let git_dir = PathBuf::from(OsStr::from_bytes(lines.next()?));
    if lines.next().is_some() || !root.is_absolute() || !git_dir.is_absolute() {
        return None;
    }
    Some(repository_shell(root, git_dir))
}

pub fn repository_from_marker(raw_root: &str) -> Option<GitRepository> {
    let root = parse_path(raw_root).ok()?;
    let marker = root.join(".git");
    if std::fs::symlink_metadata(&marker)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
        && std::fs::symlink_metadata(marker.join("HEAD"))
            .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false)
    {
        return Some(repository_shell(root, marker));
    }
    let data = read_regular_prefix(&marker, 4096).ok()?;
    let value = data.strip_suffix(b"\n").unwrap_or(&data);
    let target = value.strip_prefix(b"gitdir: ")?;
    let git_dir = PathBuf::from(OsStr::from_bytes(target));
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        root.join(git_dir)
    };
    Some(repository_shell(root, normalize_path(&git_dir)))
}

pub fn git_worktree_status(raw_path: &str, trusted_marker: bool) -> Option<GitRepository> {
    git_worktree_status_cancellable(raw_path, trusted_marker, &AtomicBool::new(false))
}

pub fn git_worktree_status_cancellable(
    raw_path: &str,
    trusted_marker: bool,
    cancelled: &AtomicBool,
) -> Option<GitRepository> {
    git_worktree_status_with_timeout(raw_path, trusted_marker, Duration::from_secs(4), cancelled)
}

fn git_worktree_status_with_timeout(
    raw_path: &str,
    trusted_marker: bool,
    status_timeout: Duration,
    cancelled: &AtomicBool,
) -> Option<GitRepository> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let repository = if trusted_marker {
        repository_from_marker(raw_path)
    } else {
        git_repository_cancellable(raw_path, cancelled)
    }?;
    load_repository_status(repository, status_timeout, cancelled)
}

fn load_repository_status(
    mut repository: GitRepository,
    status_timeout: Duration,
    cancelled: &AtomicBool,
) -> Option<GitRepository> {
    add_repository_identity(&mut repository);
    let result = run_git(
        [
            OsString::from("-C"),
            repository.root.as_os_str().to_owned(),
            OsString::from("status"),
            OsString::from("--porcelain=v2"),
            OsString::from("--branch"),
            OsString::from("--ahead-behind"),
            OsString::from("-z"),
            OsString::from("--untracked-files=all"),
            OsString::from("--ignored=no"),
        ],
        status_timeout,
        MAX_GIT_STATUS_BYTES,
        cancelled,
    );
    let output = match result {
        Ok(output) => output,
        Err(error) => {
            repository.error = error.to_string();
            return Some(repository);
        }
    };
    if output.stdout_truncated {
        repository.error = "Git status exceeds 4 MiB".to_string();
        return Some(repository);
    }
    if !output.status.success() {
        repository.error = "Unable to read Git status".to_string();
        return Some(repository);
    }
    let (entries, aggregate_truncated, parse_cancelled) =
        parse_git_status(&output.stdout, &repository.root, cancelled);
    if parse_cancelled {
        repository.error = "operation cancelled".to_string();
        return Some(repository);
    }
    if aggregate_truncated {
        repository.error = "Git status exceeds 100000 entries".to_string();
        return Some(repository);
    }
    repository.ok = true;
    repository.dirty = !entries.is_empty();
    repository.entries = entries;
    for entry in &repository.entries {
        repository.summary_counts.add(&entry.status);
    }
    for record in output.stdout.split(|byte| *byte == 0) {
        if !record.starts_with(b"# ") {
            break;
        }
        if let Some(value) = record.strip_prefix(b"# branch.upstream ") {
            repository.upstream = String::from_utf8_lossy(value).into_owned();
        } else if let Some(value) = record.strip_prefix(b"# branch.ab ") {
            let text = String::from_utf8_lossy(value);
            if let Some((ahead, behind)) = text.split_once(' ') {
                repository.ahead = ahead.strip_prefix('+').and_then(|n| n.parse().ok());
                repository.behind = behind.strip_prefix('-').and_then(|n| n.parse().ok());
            }
        }
    }
    Some(repository)
}

pub(crate) fn cached_git_worktree_status(
    marker: &Path,
    refresh: bool,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Option<GitRepository> {
    cache::repository(marker, refresh, timeout, cancelled)
}

pub fn repository_status_index_cancellable(
    repository: &GitRepository,
    cancelled: &AtomicBool,
) -> Option<GitStatusIndex> {
    build_git_status_index(&repository.entries, &repository.root, cancelled)
}

pub fn indexed_git_status_for_path<'a>(
    index: &'a GitStatusIndex,
    path: &Path,
    is_dir: bool,
) -> Option<&'a GitStatus> {
    let exact = index
        .exact
        .get(path)
        .map(|position| &index.statuses[*position]);
    if !is_dir {
        return exact;
    }
    let descendant = index
        .descendants
        .get(path)
        .map(|position| &index.statuses[*position]);
    match (exact, descendant) {
        (Some(current), Some(candidate)) => Some(higher_priority(current, candidate)),
        (Some(current), None) => Some(current),
        (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

pub fn indexed_git_counts_for_path(
    index: &GitStatusIndex,
    path: &Path,
    is_dir: bool,
) -> GitStatusCounts {
    if is_dir {
        return index.counts.get(path).copied().unwrap_or_default();
    }
    index
        .exact
        .get(path)
        .map(|position| GitStatusCounts::for_status(&index.statuses[*position].status))
        .unwrap_or_default()
}

pub fn decorate_git_entry(
    item: &mut Value,
    repository: &GitRepository,
    status: Option<&GitStatus>,
    counts: GitStatusCounts,
) {
    item["git_repo_root"] = json!(path_text(&repository.root));
    item["git_repo_name"] = json!(repository.name);
    item["git_branch"] = json!(repository.branch);
    item["git_worktree"] = json!(repository.worktree);
    item["git_modified_count"] = json!(counts.modified);
    item["git_deleted_count"] = json!(counts.deleted);
    item["git_new_count"] = json!(counts.new);
    item["git_untracked_count"] = json!(counts.untracked);
    if item["is_git_repo"].as_bool() == Some(true) {
        item["git_summary"] = repository_summary(repository);
    }
    if let Some(status) = status {
        item["git_status"] = json!(status.status);
        let label = if item["is_dir"].as_bool().unwrap_or(false) {
            counts.label().unwrap_or_else(|| status.label.clone())
        } else {
            status.label.clone()
        };
        item["git_status_label"] = json!(label);
        item["git_index_status"] = json!(status.index_status);
        item["git_worktree_status"] = json!(status.worktree_status);
        item["git_original_path"] = json!(status.original_path);
    }
}

pub fn deleted_git_entry(status: &GitStatus, repository: &GitRepository) -> Value {
    let name = file_name(&status.path);
    json!({
        "name": name,
        "path": path_text(&status.path),
        "is_dir": false,
        "is_symlink": false,
        "is_git_repo": false,
        "is_deleted": true,
        "size": -1,
        "size_text": "—",
        "modified": "",
        "created": "",
        "stat_fingerprint": "",
        "kind": "Deleted file",
        "mime": entry_mime(&name, false),
        "git_repo_root": path_text(&repository.root),
        "git_repo_name": repository.name,
        "git_branch": repository.branch,
        "git_worktree": repository.worktree,
        "git_status": status.status,
        "git_status_label": status.label,
        "git_index_status": status.index_status,
        "git_worktree_status": status.worktree_status,
        "git_original_path": status.original_path,
        "git_modified_count": usize::from(status.status == "M"),
        "git_deleted_count": usize::from(status.status == "D"),
        "git_new_count": usize::from(matches!(status.status.as_str(), "?" | "A" | "C")),
        "git_untracked_count": usize::from(status.status == "?"),
        "git_ignored": false
    })
}

pub fn git_metadata_document(
    repository: &GitRepository,
    status: Option<&GitStatus>,
    target_is_dir: bool,
    counts: GitStatusCounts,
) -> Value {
    let status_label = status.map(|item| {
        if target_is_dir {
            counts.label().unwrap_or_else(|| item.label.clone())
        } else {
            item.label.clone()
        }
    });
    json!({
        "ok": repository.ok,
        "root": path_text(&repository.root),
        "git_dir": path_text(&repository.git_dir),
        "name": repository.name,
        "branch": repository.branch,
        "worktree": repository.worktree,
        "linked": repository.linked,
        "dirty": repository.dirty,
        "status": status.map(|item| item.status.as_str()).unwrap_or(""),
        "status_label": status_label.unwrap_or_default(),
        "index_status": status.map(|item| item.index_status.as_str()).unwrap_or(""),
        "worktree_status": status.map(|item| item.worktree_status.as_str()).unwrap_or(""),
        "original_path": status.map(|item| item.original_path.as_str()).unwrap_or(""),
        "modified_count": counts.modified,
        "deleted_count": counts.deleted,
        "new_count": counts.new,
        "untracked_count": counts.untracked,
        "summary": repository_summary(repository),
        "deleted": status.is_some_and(|item| item.deleted && !target_is_dir),
        "error": repository.error
    })
}

fn repository_summary(repository: &GitRepository) -> Value {
    let counts = repository.summary_counts;
    json!({
        "ok": repository.ok && repository.error.is_empty(),
        "branch": repository.branch, "worktree": repository.worktree,
        "modified": counts.modified, "new": counts.new, "deleted": counts.deleted,
        "untracked": counts.untracked, "added": counts.added, "copied": counts.copied,
        "type_changed": counts.type_changed,
        "renamed": counts.renamed, "conflicted": counts.conflicted,
        "upstream": repository.upstream, "ahead": repository.ahead, "behind": repository.behind
    })
}

pub fn nearest_git_marker(
    raw_path: &str,
    cache: &mut HashMap<PathBuf, Option<PathBuf>>,
) -> Option<PathBuf> {
    let path = parse_path(raw_path).ok()?;
    let mut probe = if path.is_dir()
        && std::fs::symlink_metadata(&path)
            .map(|metadata| !metadata.file_type().is_symlink())
            .unwrap_or(false)
    {
        path
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("/"))
            .to_path_buf()
    };
    let mut visited = Vec::new();
    let marker = loop {
        if let Some(cached) = cache.get(&probe) {
            break cached.clone();
        }
        visited.push(probe.clone());
        if has_git_marker(&probe) {
            break Some(probe);
        }
        if !probe.pop() {
            break None;
        }
    };
    for directory in visited {
        cache.insert(directory, marker.clone());
    }
    marker
}

fn run_git(
    arguments: impl IntoIterator<Item = OsString>,
    timeout: Duration,
    stdout_limit: usize,
    cancelled: &AtomicBool,
) -> crate::AppResult<crate::command::CommandOutput> {
    let git = which("git").ok_or_else(|| crate::AppError::command("git is not installed"))?;
    CommandSpec::new(git)
        .args(["-c", "core.fsmonitor=false"])
        .args(arguments)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .timeout(timeout)
        .limits(stdout_limit, 64 * 1024)
        .run_cancellable(cancelled)
}

fn repository_shell(root: PathBuf, git_dir: PathBuf) -> GitRepository {
    GitRepository {
        root,
        git_dir,
        name: String::new(),
        worktree: String::new(),
        branch: String::new(),
        upstream: String::new(),
        ahead: None,
        behind: None,
        linked: false,
        ok: false,
        error: String::new(),
        dirty: false,
        entries: Vec::new(),
        summary_counts: GitStatusCounts::default(),
    }
}

fn repository_weight(repository: &GitRepository) -> usize {
    repository.entries.iter().fold(0_usize, |total, status| {
        total
            .saturating_add(status.path.as_os_str().as_encoded_bytes().len())
            .saturating_add(status.relative.len())
            .saturating_add(status.original_path.len())
            .saturating_add(128)
    })
}

fn add_repository_identity(repository: &mut GitRepository) {
    repository.linked = repository
        .git_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "worktrees");
    let common_dir = if repository.linked {
        repository
            .git_dir
            .parent()
            .and_then(Path::parent)
            .unwrap_or(&repository.git_dir)
    } else {
        &repository.git_dir
    };
    let main_root = common_dir.parent().unwrap_or(common_dir);
    repository.name = file_name(main_root);
    repository.worktree = if repository.linked {
        file_name(&repository.root)
    } else {
        String::new()
    };
    repository.branch = head_branch(&repository.git_dir);
}

fn head_branch(git_dir: &Path) -> String {
    let Ok(data) = read_regular_prefix(&git_dir.join("HEAD"), 4096) else {
        return String::new();
    };
    let Ok(value) = String::from_utf8(data) else {
        return String::new();
    };
    let value = value.trim();
    value
        .strip_prefix("ref: refs/heads/")
        .or_else(|| value.strip_prefix("ref: "))
        .unwrap_or_else(|| value.get(..value.len().min(7)).unwrap_or(value))
        .to_string()
}

fn parse_git_status(
    data: &[u8],
    root: &Path,
    cancelled: &AtomicBool,
) -> (Vec<GitStatus>, bool, bool) {
    let mut fields = data.split(|byte| *byte == 0);
    let mut result = Vec::new();
    while let Some(record) = fields.next() {
        if result.len().is_multiple_of(256) && cancelled.load(Ordering::Relaxed) {
            return (result, false, true);
        }
        if record.len() < 3 || record[1] != b' ' {
            continue;
        }
        let (xy, relative_bytes) = match record[0] {
            b'?' => (&b"??"[..], &record[2..]),
            b'1' | b'2' | b'u' => {
                let count = match record[0] {
                    b'1' => 9,
                    b'2' => 10,
                    _ => 11,
                };
                let fields = record
                    .splitn(count, |byte| *byte == b' ')
                    .collect::<Vec<_>>();
                if fields.len() != count || fields[1].len() != 2 {
                    continue;
                }
                (fields[1], fields[count - 1])
            }
            _ => continue,
        };
        if result.len() >= MAX_GIT_STATUS_ENTRIES {
            return (result, true, false);
        }
        let index_status = if xy[0] == b'.' { ' ' } else { xy[0] as char };
        let worktree_status = if xy[1] == b'.' { ' ' } else { xy[1] as char };
        let relative_path = Path::new(OsStr::from_bytes(relative_bytes));
        let relative = display_path(relative_path);
        let mut original_relative = None;
        if record[0] == b'2'
            && let Some(original) = fields.next()
        {
            original_relative = Some(Path::new(OsStr::from_bytes(original)));
        }
        let status = display_git_status(index_status, worktree_status);
        if status.is_empty() {
            continue;
        }
        let path = normalize_path(&root.join(relative_path));
        let original_path = original_relative
            .map(|path| path_text(&normalize_path(&root.join(path))))
            .unwrap_or_default();
        let deleted = status == "D" && std::fs::symlink_metadata(&path).is_err();
        result.push(GitStatus {
            path,
            relative,
            label: status_label(&status).to_string(),
            status,
            index_status: index_status.to_string().trim().to_string(),
            worktree_status: worktree_status.to_string().trim().to_string(),
            original_path,
            deleted,
        });
    }
    (result, false, false)
}

fn display_git_status(index: char, worktree: char) -> String {
    if index == '?' && worktree == '?' {
        return "?".to_string();
    }
    let pair = [index, worktree];
    if matches!(
        pair,
        ['D', 'D'] | ['A', 'U'] | ['U', 'D'] | ['U', 'A'] | ['D', 'U'] | ['A', 'A'] | ['U', 'U']
    ) || pair.contains(&'U')
    {
        return "U".to_string();
    }
    for marker in ['D', 'M', 'R', 'C', 'A', 'T'] {
        if pair.contains(&marker) {
            return marker.to_string();
        }
    }
    String::new()
}

fn status_label(status: &str) -> &'static str {
    match status {
        "?" => "Untracked",
        "A" => "Added",
        "C" => "Copied",
        "R" => "Renamed",
        "M" => "Modified",
        "T" => "Type changed",
        "D" => "Deleted",
        "U" => "Conflict",
        _ => "Changed",
    }
}

fn build_git_status_index(
    statuses: &[GitStatus],
    root: &Path,
    cancelled: &AtomicBool,
) -> Option<GitStatusIndex> {
    let mut exact = HashMap::new();
    let mut descendants = HashMap::new();
    let mut counts = HashMap::new();
    for (position, status) in statuses.iter().enumerate() {
        if position.is_multiple_of(256) && cancelled.load(Ordering::Relaxed) {
            return None;
        }
        if exact.len().saturating_add(descendants.len()) >= MAX_GIT_INDEX_PATHS {
            break;
        }
        if status.path != root && !status.path.starts_with(root) {
            continue;
        }
        insert_higher(&mut exact, status.path.clone(), position, statuses);
        counts
            .entry(status.path.clone())
            .or_insert_with(GitStatusCounts::default)
            .add(&status.status);
        let mut parent = status.path.parent();
        while let Some(directory) = parent {
            if directory != root && !directory.starts_with(root) {
                break;
            }
            if exact.len().saturating_add(descendants.len()) >= MAX_GIT_INDEX_PATHS {
                break;
            }
            insert_higher(
                &mut descendants,
                directory.to_path_buf(),
                position,
                statuses,
            );
            counts
                .entry(directory.to_path_buf())
                .or_insert_with(GitStatusCounts::default)
                .add(&status.status);
            if directory == root {
                break;
            }
            parent = directory.parent();
        }
    }
    Some(GitStatusIndex {
        statuses: statuses.to_vec(),
        exact,
        descendants,
        counts,
    })
}

impl GitStatusCounts {
    fn for_status(status: &str) -> Self {
        let mut counts = Self::default();
        counts.add(status);
        counts
    }

    fn add(&mut self, status: &str) {
        if matches!(status, "?" | "A" | "C") {
            self.new += 1;
        }
        match status {
            "M" => self.modified += 1,
            "D" => self.deleted += 1,
            "?" => self.untracked += 1,
            "A" => self.added += 1,
            "C" => self.copied += 1,
            "T" => self.type_changed += 1,
            "R" => self.renamed += 1,
            "U" => self.conflicted += 1,
            _ => {}
        }
    }

    fn label(self) -> Option<String> {
        let mut parts = Vec::new();
        if self.modified > 0 {
            parts.push(format!("{} modified", self.modified));
        }
        if self.deleted > 0 {
            parts.push(format!("{} deleted", self.deleted));
        }
        if self.added > 0 {
            parts.push(format!("{} added", self.added));
        }
        if self.untracked > 0 {
            parts.push(format!("{} untracked", self.untracked));
        }
        if self.copied > 0 {
            parts.push(format!("{} copied", self.copied));
        }
        if self.type_changed > 0 {
            parts.push(format!("{} type changed", self.type_changed));
        }
        if self.renamed > 0 {
            parts.push(format!("{} renamed", self.renamed));
        }
        if self.conflicted > 0 {
            parts.push(format!(
                "{} conflict{}",
                self.conflicted,
                if self.conflicted == 1 { "" } else { "s" }
            ));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

fn insert_higher(
    target: &mut HashMap<PathBuf, usize>,
    path: PathBuf,
    position: usize,
    statuses: &[GitStatus],
) {
    match target.get(&path) {
        Some(current)
            if status_priority(&statuses[*current].status)
                >= status_priority(&statuses[position].status) => {}
        _ => {
            target.insert(path, position);
        }
    }
}

fn higher_priority<'a>(current: &'a GitStatus, candidate: &'a GitStatus) -> &'a GitStatus {
    if status_priority(&candidate.status) > status_priority(&current.status) {
        candidate
    } else {
        current
    }
}

fn status_priority(status: &str) -> u8 {
    match status {
        "?" => 1,
        "A" => 2,
        "C" => 3,
        "R" => 4,
        "M" | "T" => 5,
        "D" => 6,
        "U" => 7,
        _ => 0,
    }
}

const MAX_BRANCHES: usize = 2000;

fn branch_name_is_safe(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 255
        && !branch.starts_with('-')
        && !branch.starts_with('.')
        && !branch.contains("..")
        && !branch.ends_with(".lock")
        && branch
            .chars()
            .all(|ch| !ch.is_control() && !ch.is_whitespace() && !"~^:?*[\\\u{7f}".contains(ch))
}

pub fn git_branches(raw_path: &str, cancelled: &AtomicBool) -> Value {
    let Some(mut repository) = git_repository_cancellable(raw_path, cancelled) else {
        return json!({"ok": false, "error": "no repository here"});
    };
    add_repository_identity(&mut repository);
    let output = run_git(
        [
            OsString::from("-C"),
            repository.root.clone().into_os_string(),
            OsString::from("for-each-ref"),
            OsString::from("--format=%(refname:short)"),
            OsString::from("--sort=-committerdate"),
            OsString::from("refs/heads"),
        ],
        Duration::from_secs(3),
        1024 * 1024,
        cancelled,
    );
    let Ok(output) = output else {
        return json!({"ok": false, "error": "branch list is unavailable"});
    };
    if !output.status.success() {
        return json!({"ok": false, "error": "branch list is unavailable"});
    }
    let branches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| branch_name_is_safe(name))
        .take(MAX_BRANCHES)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    json!({
        "ok": true,
        "root": path_text(&repository.root),
        "current": repository.branch,
        "worktree": repository.worktree,
        "dirty": repository.dirty,
        "branches": branches
    })
}

pub fn git_switch(raw_path: &str, branch: &str, cancelled: &AtomicBool) -> Value {
    if !branch_name_is_safe(branch) {
        return json!({"ok": false, "error": "that branch name cannot be used"});
    }
    let Some(mut repository) = git_repository_cancellable(raw_path, cancelled) else {
        return json!({"ok": false, "error": "no repository here"});
    };
    add_repository_identity(&mut repository);
    if repository.branch == branch {
        return json!({"ok": true, "branch": branch, "root": path_text(&repository.root)});
    }
    let selection = match places::remote_only_upstream(&repository.root, branch, cancelled) {
        Some(upstream) => vec![OsString::from("--track"), OsString::from(upstream)],
        None => vec![
            OsString::from("--no-guess"),
            OsString::from("--"),
            OsString::from(branch),
        ],
    };
    let output = run_git(
        [
            OsString::from("-C"),
            repository.root.clone().into_os_string(),
            OsString::from("switch"),
        ]
        .into_iter()
        .chain(selection),
        Duration::from_secs(20),
        256 * 1024,
        cancelled,
    );
    let Ok(output) = output else {
        return json!({"ok": false, "error": "git switch could not run"});
    };
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        let first = message
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("git switch failed")
            .trim()
            .chars()
            .take(240)
            .collect::<String>();
        return json!({"ok": false, "error": first});
    }
    json!({"ok": true, "branch": branch, "root": path_text(&repository.root)})
}
