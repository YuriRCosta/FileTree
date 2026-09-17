use super::{
    GitStatusCounts, add_repository_identity, branch_name_is_safe, display_git_status,
    git_repository_cancellable, run_git,
};
use crate::common::path_text;
use crate::filesystem::read_regular_prefix;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const MAX_PLACE_BRANCHES: usize = 2000;
const MAX_PLACE_WORKTREES: usize = 32;
const PLACE_TIMEOUT: Duration = Duration::from_secs(3);
const PLACE_BYTES: usize = 1024 * 1024;
const BRANCH_FORMAT: &str = "--format=%(refname)%00%(refname:short)%00%(objectname)%00%(upstream:short)%00%(upstream:track)%00%(committerdate:unix)%00%(authorname)%00%(subject)%00%(HEAD)";

struct Worktree {
    path: PathBuf,
    branch: String,
    head: String,
    main: bool,
    current: bool,
    locked: bool,
    prunable: bool,
    counts: [usize; 4],
    changes: GitStatusCounts,
    at: i64,
}

pub fn git_places(raw_path: &str, cancelled: &AtomicBool) -> Value {
    let Some(mut repository) = git_repository_cancellable(raw_path, cancelled) else {
        return json!({"ok": false, "error": "no repository here"});
    };
    add_repository_identity(&mut repository);
    let root = repository.root.clone();
    let Some(refs) = git_lines(
        &root,
        [
            "for-each-ref",
            "--sort=-committerdate",
            "--count=2001",
            BRANCH_FORMAT,
            "refs/heads",
            "refs/remotes",
        ],
        cancelled,
    ) else {
        return json!({"ok": false, "error": "branch list is unavailable"});
    };
    let mut worktrees = worktree_rows(&root, cancelled);
    let checked_out: HashMap<String, String> = worktrees
        .iter()
        .filter(|row| !row.branch.is_empty())
        .map(|row| (row.branch.clone(), path_text(&row.path)))
        .collect();
    let rows: Vec<Vec<&str>> = refs
        .iter()
        .map(|line| line.split('\0').collect::<Vec<_>>())
        .filter(|fields| fields.len() >= 9 && !fields[0].ends_with("/HEAD"))
        .collect();
    let truncated = rows.len() > MAX_PLACE_BRANCHES;
    let locals: HashSet<&str> = rows
        .iter()
        .filter_map(|fields| fields[0].strip_prefix("refs/heads/"))
        .collect();
    let mut remote_names: HashMap<&str, &str> = HashMap::new();
    for fields in &rows {
        if let Some((remote, name)) = remote_ref(fields[0]) {
            remote_names.entry(name).or_insert(remote);
        }
    }
    let mut branches: Vec<Value> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut tips: HashMap<&str, i64> = HashMap::new();
    for fields in rows.iter().take(MAX_PLACE_BRANCHES) {
        let (name, kind) = match remote_ref(fields[0]) {
            Some((_, name)) if locals.contains(name) => continue,
            Some((_, name)) => (name, "remote"),
            None => match fields[0].strip_prefix("refs/heads/") {
                Some(name) if remote_names.contains_key(name) => (name, "both"),
                Some(name) => (name, "local"),
                None => continue,
            },
        };
        if !branch_name_is_safe(name) || !seen.insert(name) {
            continue;
        }
        let at = fields[5].parse::<i64>().unwrap_or_default();
        tips.insert(fields[2], at);
        let (ahead, behind, gone) = parse_track(fields[4]);
        branches.push(json!({
            "name": name,
            "kind": kind,
            "remote": remote_names.get(name).copied().unwrap_or_default(),
            "upstream": fields[3],
            "ahead": ahead,
            "behind": behind,
            "gone": gone,
            "current": fields[8] == "*",
            "worktree": checked_out.get(name).cloned().unwrap_or_default(),
            "at": at,
            "author": fields[6],
            "subject": fields[7]
        }));
    }
    for row in &mut worktrees {
        row.at = match tips.get(row.head.as_str()) {
            Some(at) => *at,
            None => git_lines(&root, ["log", "-1", "--format=%ct", &row.head], cancelled)
                .and_then(|lines| lines.first()?.trim().parse().ok())
                .unwrap_or_default(),
        };
    }
    let detached = read_regular_prefix(&repository.git_dir.join("HEAD"), 4096)
        .map(|data| !data.starts_with(b"ref: "))
        .unwrap_or(false);
    json!({
        "ok": true,
        "root": path_text(&root),
        "current": repository.branch,
        "detached": detached,
        "truncated": truncated,
        "branches": branches,
        "worktrees": worktrees.iter().map(worktree_document).collect::<Vec<_>>()
    })
}

fn remote_ref(refname: &str) -> Option<(&str, &str)> {
    refname.strip_prefix("refs/remotes/")?.split_once('/')
}

fn git_lines<const N: usize>(
    root: &Path,
    arguments: [&str; N],
    cancelled: &AtomicBool,
) -> Option<Vec<String>> {
    let output = run_git(
        [OsString::from("-C"), root.as_os_str().to_owned()]
            .into_iter()
            .chain(arguments.into_iter().map(OsString::from)),
        PLACE_TIMEOUT,
        PLACE_BYTES,
        cancelled,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().map(ToOwned::to_owned).collect())
}

fn parse_track(value: &str) -> (u64, u64, bool) {
    let mut ahead = 0;
    let mut behind = 0;
    let mut words = value.trim_matches(['[', ']']).split(' ');
    while let Some(word) = words.next() {
        let count = words
            .next()
            .and_then(|count| count.trim_end_matches(',').parse::<u64>().ok())
            .unwrap_or_default();
        match word {
            "ahead" => ahead = count,
            "behind" => behind = count,
            _ => {}
        }
    }
    (ahead, behind, value.contains("gone"))
}

fn worktree_rows(root: &Path, cancelled: &AtomicBool) -> Vec<Worktree> {
    let output = run_git(
        [OsString::from("-C"), root.as_os_str().to_owned()]
            .into_iter()
            .chain(["worktree", "list", "--porcelain", "-z"].map(OsString::from)),
        PLACE_TIMEOUT,
        PLACE_BYTES,
        cancelled,
    );
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let real_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut rows = Vec::new();
    for block in text.split("\0\0").filter(|block| !block.is_empty()) {
        if rows.len() >= MAX_PLACE_WORKTREES {
            break;
        }
        let fields: HashMap<&str, &str> = block
            .split('\0')
            .map(|line| line.split_once(' ').unwrap_or((line, "")))
            .collect();
        if fields.contains_key("bare") {
            continue;
        }
        let path = PathBuf::from(fields.get("worktree").copied().unwrap_or_default());
        let current = std::fs::canonicalize(&path).is_ok_and(|real| real == real_root);
        let (counts, changes) = status_counts(&path, cancelled);
        rows.push(Worktree {
            branch: fields
                .get("branch")
                .and_then(|value| value.strip_prefix("refs/heads/"))
                .unwrap_or_default()
                .to_string(),
            head: fields.get("HEAD").copied().unwrap_or_default().to_string(),
            main: rows.is_empty(),
            current,
            locked: fields.contains_key("locked"),
            prunable: fields.contains_key("prunable"),
            counts,
            changes,
            at: 0,
            path,
        });
    }
    rows
}

fn status_counts(path: &Path, cancelled: &AtomicBool) -> ([usize; 4], GitStatusCounts) {
    let mut counts = [0; 4];
    let mut changes = GitStatusCounts::default();
    let output = run_git(
        [OsString::from("-C"), path.as_os_str().to_owned()]
            .into_iter()
            .chain(["status", "--porcelain=v2", "-z", "--untracked-files=all"].map(OsString::from)),
        PLACE_TIMEOUT,
        PLACE_BYTES,
        cancelled,
    );
    let Ok(output) = output else {
        return (counts, changes);
    };
    let mut records = output.stdout.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        let xy = match record {
            [b'?', b' ', ..] => {
                counts[2] += 1;
                *b"??"
            }
            [b'u', b' ', x, y, b' ', ..] => {
                counts[3] += 1;
                [*x, *y]
            }
            [kind @ (b'1' | b'2'), b' ', index, worktree, b' ', ..] => {
                counts[0] += usize::from(*index != b'.');
                counts[1] += usize::from(*worktree != b'.');
                if *kind == b'2' {
                    records.next();
                }
                [*index, *worktree]
            }
            _ => continue,
        };
        let letter = |byte: u8| if byte == b'.' { ' ' } else { byte as char };
        changes.add(&display_git_status(letter(xy[0]), letter(xy[1])));
    }
    (counts, changes)
}

fn worktree_document(row: &Worktree) -> Value {
    let [staged, unstaged, untracked, conflicted] = row.counts;
    json!({
        "path": path_text(&row.path),
        "branch": row.branch,
        "head": row.head.get(..7).unwrap_or(&row.head),
        "main": row.main,
        "current": row.current,
        "locked": row.locked,
        "prunable": row.prunable,
        "dirty": row.counts.iter().any(|count| *count > 0),
        "staged": staged,
        "unstaged": unstaged,
        "untracked": untracked,
        "conflicted": conflicted,
        "modified": row.changes.modified,
        "added": row.changes.added,
        "deleted": row.changes.deleted,
        "renamed": row.changes.renamed,
        "copied": row.changes.copied,
        "type_changed": row.changes.type_changed,
        "at": row.at
    })
}

pub(super) fn remote_only_upstream(
    root: &Path,
    branch: &str,
    cancelled: &AtomicBool,
) -> Option<String> {
    let refs = git_lines(
        root,
        [
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
        cancelled,
    )?;
    if refs
        .iter()
        .any(|name| name == &format!("refs/heads/{branch}"))
    {
        return None;
    }
    let mut candidates = refs.iter().filter_map(|name| {
        let rest = name.strip_prefix("refs/remotes/")?;
        let (remote, short) = rest.split_once('/')?;
        (short == branch).then(|| format!("{remote}/{branch}"))
    });
    let upstream = candidates.next()?;
    candidates.next().is_none().then_some(upstream)
}
