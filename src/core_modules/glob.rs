use super::watch::WatchPlan;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

pub const MAX_GLOB_MATCHES: usize = 64;
pub const MAX_DISCOVERY_DIRS: usize = 200;
pub const MAX_DIR_ENTRIES: usize = 512;
const MAX_PATTERN_PARTS: usize = 32;

pub fn fnmatch_case(value: &str, pattern: &str) -> bool {
    let value: Vec<char> = value.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    matches(&value, &pattern)
}

fn matches(value: &[char], pattern: &[char]) -> bool {
    let Some((first, rest)) = pattern.split_first() else {
        return value.is_empty();
    };
    match first {
        '*' => {
            let rest = match rest.iter().position(|character| *character != '*') {
                Some(offset) => &rest[offset..],
                None => return true,
            };
            (0..=value.len()).any(|index| matches(&value[index..], rest))
        }
        '?' => !value.is_empty() && matches(&value[1..], rest),
        '[' => {
            let Some((set, tail)) = bracket(rest) else {
                return !value.is_empty() && value[0] == '[' && matches(&value[1..], rest);
            };
            !value.is_empty() && in_set(value[0], set) && matches(&value[1..], tail)
        }
        character => !value.is_empty() && value[0] == *character && matches(&value[1..], rest),
    }
}

fn bracket(pattern: &[char]) -> Option<(&[char], &[char])> {
    let start = if pattern.first() == Some(&'!') { 1 } else { 0 };
    let start = if pattern.get(start) == Some(&']') {
        start + 1
    } else {
        start
    };
    let offset = pattern[start..]
        .iter()
        .position(|character| *character == ']')?;
    let close = start + offset;
    Some((&pattern[..close], &pattern[close + 1..]))
}

fn in_set(character: char, set: &[char]) -> bool {
    let (negated, set) = match set.split_first() {
        Some(('!', rest)) => (true, rest),
        _ => (false, set),
    };
    let mut index = 0;
    let mut hit = false;
    while index < set.len() {
        if index + 2 < set.len() && set[index + 1] == '-' {
            if set[index] <= character && character <= set[index + 2] {
                hit = true;
            }
            index += 3;
        } else {
            if set[index] == character {
                hit = true;
            }
            index += 1;
        }
    }
    hit != negated
}

fn realpath_of(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| crate::common::normalize_path(path))
}

fn parts_of(pattern: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for component in Path::new(pattern).components() {
        match component {
            Component::Normal(name) => parts.push(name.to_str()?.to_string()),
            Component::CurDir => continue,
            _ => return None,
        }
    }
    Some(parts)
}

pub fn bounded_glob(plan: &mut WatchPlan, base: &Path, pattern: &str) -> Vec<PathBuf> {
    plan.watch_path(base, true);
    let cleaned = pattern.trim();
    if cleaned.is_empty()
        || cleaned.starts_with('/')
        || cleaned.starts_with('~')
        || cleaned.contains("..")
    {
        return Vec::new();
    }
    let Some(parts) = parts_of(cleaned) else {
        return Vec::new();
    };
    if parts.len() > MAX_PATTERN_PARTS
        || parts.iter().any(|part| part.contains("**") && part != "**")
    {
        return Vec::new();
    }
    let mut found: HashSet<PathBuf> = HashSet::new();
    let root = realpath_of(base);
    let mut pending: VecDeque<(PathBuf, usize)> = VecDeque::new();
    pending.push_back((base.to_path_buf(), 0));
    let mut visited: HashSet<(PathBuf, usize)> = HashSet::new();
    while !pending.is_empty()
        && visited.len() < MAX_DISCOVERY_DIRS
        && found.len() < MAX_GLOB_MATCHES
    {
        let (directory, index) = pending.pop_front().expect("pending entry");
        if index >= parts.len() {
            continue;
        }
        let resolved = realpath_of(&directory);
        let key = (resolved.clone(), index);
        if (resolved != root && !resolved.starts_with(&root)) || visited.contains(&key) {
            continue;
        }
        visited.insert(key);
        plan.watch_path(&directory, true);
        let part = parts[index].as_str();
        if part == "**" {
            pending.push_back((directory.clone(), index + 1));
        }
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for (count, entry) in entries.enumerate() {
            if count >= MAX_DIR_ENTRIES || found.len() >= MAX_GLOB_MATCHES {
                break;
            }
            let Ok(entry) = entry else { continue };
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let candidate = directory.join(name);
            if part != "**" && !fnmatch_case(name, part) {
                continue;
            }
            let kind = entry.metadata().ok();
            if part != "**"
                && index == parts.len() - 1
                && kind.as_ref().is_some_and(|metadata| metadata.is_file())
            {
                let target = realpath_of(&candidate);
                if target
                    .parent()
                    .is_some_and(|parent| parent.starts_with(&root))
                {
                    found.insert(candidate);
                }
            } else if kind.as_ref().is_some_and(|metadata| metadata.is_dir())
                && pending.len() + visited.len() < MAX_DISCOVERY_DIRS
            {
                pending.push_back((candidate, if part == "**" { index } else { index + 1 }));
            }
        }
    }
    let mut result: Vec<PathBuf> = found.into_iter().collect();
    result.sort_by(|left, right| {
        left.as_os_str()
            .as_bytes()
            .cmp(right.as_os_str().as_bytes())
    });
    result
}
