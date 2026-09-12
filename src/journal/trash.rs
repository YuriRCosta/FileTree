use super::*;

pub fn trash_snapshot(paths: &[PathBuf]) -> TrashSnapshot {
    let mut snapshot = BTreeMap::new();
    for path in paths {
        for directory in trash_directories(path) {
            snapshot
                .entry(directory.clone())
                .or_insert_with(|| info_names(&directory));
        }
    }
    snapshot
}

pub fn trashed_items(paths: &[PathBuf], before: &TrashSnapshot) -> Vec<JournalItem> {
    let mut origins = BTreeMap::new();
    for (trash_dir, previous) in before {
        for name in info_names(trash_dir).difference(previous) {
            let origin = trashinfo_origin(trash_dir, name);
            if origin.is_absolute() {
                origins.insert(normalize_path(&origin), (trash_dir.clone(), name.clone()));
            }
        }
    }
    paths
        .iter()
        .filter_map(|path| {
            let absolute = normalize_path(path);
            let (trash_dir, name) = origins.get(&absolute)?;
            let stored = trash_dir.join("files").join(name);
            let name_text = name
                .to_str()
                .map(str::to_string)
                .unwrap_or_else(|| path_text(&stored));
            Some(JournalItem {
                source: path_text(path),
                target: String::new(),
                is_dir: false,
                trash_dir: path_text(trash_dir),
                trash_name: name_text,
                before_value: String::new(),
                after_value: String::new(),
                fingerprint: fingerprint(&stored),
                unverified: false,
            })
        })
        .collect()
}

pub(super) fn home_trash() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| expanded_path("~/.local/share"))
        .join("Trash")
}

pub(super) fn trash_directories(path: &Path) -> Vec<PathBuf> {
    let top = mount_top(path.parent().unwrap_or(path));
    let uid = rustix::process::getuid().as_raw().to_string();
    let mut values = vec![
        home_trash(),
        top.join(".Trash").join(&uid),
        top.join(format!(".Trash-{uid}")),
    ];
    values.dedup();
    values
}

pub(super) fn mount_top(path: &Path) -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let mut current = normalize_path(path);
    let device = match std::fs::symlink_metadata(&current) {
        Ok(metadata) => metadata.dev(),
        Err(_) => return PathBuf::from("/"),
    };
    while current != Path::new("/") {
        let parent = current.parent().unwrap_or(Path::new("/")).to_path_buf();
        match std::fs::symlink_metadata(&parent) {
            Ok(metadata) if metadata.dev() == device => current = parent,
            _ => return current,
        }
    }
    current
}

pub(super) fn info_names(trash_dir: &Path) -> BTreeSet<OsString> {
    secure::directory_names(&trash_dir.join("info"))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            let bytes = name.as_bytes();
            bytes
                .strip_suffix(b".trashinfo")
                .map(|stem| OsString::from_vec(stem.to_vec()))
        })
        .collect()
}

pub(super) fn trash_top_directory(trash_dir: &Path) -> PathBuf {
    let parent = trash_dir.parent().unwrap_or(Path::new("/"));
    if parent.file_name() == Some(OsStr::new(".Trash")) {
        parent.parent().unwrap_or(Path::new("/")).to_path_buf()
    } else {
        parent.to_path_buf()
    }
}

pub(super) fn trashinfo_origin(trash_dir: &Path, name: &OsStr) -> PathBuf {
    let mut info_name = name.as_bytes().to_vec();
    info_name.extend_from_slice(b".trashinfo");
    let path = trash_dir.join("info").join(OsString::from_vec(info_name));
    let Some(data) = secure::read_bounded_nofollow(&path, TRASHINFO_BYTES_LIMIT)
        .ok()
        .flatten()
    else {
        return PathBuf::new();
    };
    for line in data.split(|byte| *byte == b'\n') {
        if let Some(value) = line.strip_prefix(b"Path=") {
            let decoded = percent_decode(trim_ascii(value));
            let path = PathBuf::from(OsString::from_vec(decoded));
            return if path.is_absolute() {
                path
            } else {
                trash_top_directory(trash_dir).join(path)
            };
        }
    }
    PathBuf::new()
}

pub(super) fn percent_decode(value: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if value[index] == b'%'
            && index + 2 < value.len()
            && let (Some(high), Some(low)) = (hex(value[index + 1]), hex(value[index + 2]))
        {
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(value[index]);
            index += 1;
        }
    }
    output
}

pub(super) fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub(super) fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

pub(super) fn restore_from_trash(
    item: &mut JournalItem,
    destination: &Path,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    let destination = secure::resolved_parent(destination)?;
    restore_from_trash_resolved(item, &destination, cancelled)
}

pub(super) fn restore_from_trash_resolved(
    item: &mut JournalItem,
    destination: &secure::ResolvedParent,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    let stored = stored_trash_path(item)?;
    let info = trash_info_path(item)?;
    let mut ignored: fn(&Path) = ignore_path;
    let expected = item
        .fingerprint
        .as_ref()
        .and_then(fingerprint_identity)
        .ok_or_else(|| io::Error::other("Trash identity is missing"))?;
    let source = secure::resolved_parent(&stored)?;
    secure::relocate_noreplace_matching_resolved(
        source,
        destination,
        expected,
        cancelled,
        &mut ignored,
    )?;
    let _ = secure::remove_path(&info);
    item.trash_dir.clear();
    item.trash_name.clear();
    item.fingerprint = fingerprint(&destination.full_path());
    Ok(())
}

pub(super) fn trash_item(
    item: &mut JournalItem,
    path: &Path,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    let trashed = send_to_trash(path, item.fingerprint.as_ref(), cancelled)?;
    item.trash_dir = trashed.trash_dir;
    item.trash_name = trashed.trash_name;
    item.fingerprint = trashed.fingerprint;
    Ok(())
}

pub fn trash_path(path: &Path, cancelled: &AtomicBool) -> io::Result<JournalItem> {
    let expected = fingerprint(path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path_text(path)))?;
    send_to_trash(path, Some(&expected), cancelled)
}

pub(super) fn send_to_trash(
    path: &Path,
    expected: Option<&Fingerprint>,
    cancelled: &AtomicBool,
) -> io::Result<JournalItem> {
    check_cancelled(cancelled)?;
    let paths = vec![path.to_path_buf()];
    let before = trash_snapshot(&paths);
    let quarantine = secure::quarantine_path(path)?;
    if expected.is_some_and(|value| !same_fingerprint_identity(value, quarantine.stat())) {
        let restored = quarantine.restore();
        return Err(match restored {
            Ok(()) => io::Error::other(format!("{} was replaced by another item", path.display())),
            Err(error) => io::Error::other(format!(
                "{} was replaced by another item; data remains at {} because rollback failed: {error}",
                path.display(),
                quarantine.path().display()
            )),
        });
    }
    send_quarantined_to_trash(path, quarantine, &before, cancelled)
}

pub fn trash_matching_resolved(
    source: secure::ResolvedParent,
    expected: secure::EntryStat,
    empty_directory: bool,
    cancelled: &AtomicBool,
) -> io::Result<JournalItem> {
    check_cancelled(cancelled)?;
    let path = source.full_path();
    let current = secure::entry_stat_resolved(&source)?;
    if current.identity() != expected.identity()
        || current.size != expected.size
        || current.mtime != expected.mtime
        || current.mtime_nsec != expected.mtime_nsec
        || current.ctime != expected.ctime
        || current.ctime_nsec != expected.ctime_nsec
    {
        return Err(io::Error::other("item changed since transfer preflight"));
    }
    let before = trash_snapshot(std::slice::from_ref(&path));
    let quarantine = secure::quarantine_resolved(source)?;
    let stat = quarantine.stat();
    let unchanged = stat.identity() == expected.identity()
        && stat.size == expected.size
        && stat.mode == expected.mode
        && stat.mtime == expected.mtime
        && stat.mtime_nsec == expected.mtime_nsec;
    let empty = !empty_directory
        || (stat.kind == EntryKind::Directory
            && secure::directory_names(&quarantine.path()).is_ok_and(|names| names.is_empty()));
    if !unchanged || !empty {
        let restored = quarantine.restore();
        return Err(trash_failure(
            "item changed before it could be secured".into(),
            restored,
            &quarantine,
        ));
    }
    send_quarantined_to_trash(&path, quarantine, &before, cancelled)
}

fn send_quarantined_to_trash(
    path: &Path,
    quarantine: secure::QuarantinedEntry,
    before: &TrashSnapshot,
    cancelled: &AtomicBool,
) -> io::Result<JournalItem> {
    let program = which("gio").unwrap_or_else(|| PathBuf::from("gio"));
    let output = CommandSpec::new(program)
        .args([
            "trash".to_string(),
            "--".to_string(),
            quarantine.command_path(),
        ])
        .cwd(quarantine.command_directory())
        .timeout(CONTROL_TIMEOUT)
        .limits(64 * 1024, 64 * 1024)
        .run_cancellable(cancelled)
        .map_err(|error| io::Error::other(error.to_string()));
    if let Some(mut trashed) = quarantined_trash_item(path, quarantine.name(), before) {
        quarantine.finish_or_restore()?;
        if let Err(error) = publish_trash_origin(&mut trashed, path, cancelled) {
            let mut rollback_item = trashed.clone();
            let rollback = restore_from_trash(&mut rollback_item, path, cancelled);
            return Err(match rollback {
                Ok(()) => error,
                Err(rollback_error) => io::Error::other(format!(
                    "{error}; the item remains in Trash because rollback failed: {rollback_error}"
                )),
            });
        }
        trashed.source = path_text(path);
        return Ok(trashed);
    }
    let message = match output {
        Ok(output) if output.status.success() => format!(
            "Trash moved {} but did not report its secured entry",
            path.display()
        ),
        Ok(output) => {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if message.is_empty() {
                "Trash failed".to_string()
            } else {
                message
            }
        }
        Err(error) => error.to_string(),
    };
    let restored = quarantine.restore();
    Err(trash_failure(message, restored, &quarantine))
}

pub(super) fn trash_failure(
    message: String,
    restored: io::Result<()>,
    quarantine: &secure::QuarantinedEntry,
) -> io::Error {
    match restored {
        Ok(()) => io::Error::other(message),
        Err(error) => io::Error::other(format!(
            "{message}; data remains at {} because rollback failed: {error}",
            quarantine.path().display()
        )),
    }
}

pub(super) fn quarantined_trash_item(
    original: &Path,
    quarantine_name: &OsStr,
    before: &TrashSnapshot,
) -> Option<JournalItem> {
    for (trash_dir, previous) in before {
        for name in info_names(trash_dir).difference(previous) {
            if trashinfo_origin(trash_dir, name).file_name() != Some(quarantine_name) {
                continue;
            }
            let stored = trash_dir.join("files").join(name);
            let name_text = name
                .to_str()
                .map(str::to_string)
                .unwrap_or_else(|| path_text(&stored));
            return Some(JournalItem {
                source: path_text(original),
                target: String::new(),
                is_dir: false,
                trash_dir: path_text(trash_dir),
                trash_name: name_text,
                before_value: String::new(),
                after_value: String::new(),
                fingerprint: fingerprint(&stored),
                unverified: false,
            });
        }
    }
    None
}

fn publish_trash_origin(
    item: &mut JournalItem,
    original: &Path,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;

    let trash_dir = parse_path(&item.trash_dir)?;
    let info = trash_info_path(item)?;
    let data = secure::read_bounded_nofollow(&info, TRASHINFO_BYTES_LIMIT)?
        .ok_or_else(|| io::Error::other("Trash metadata disappeared"))?;
    let stored_path = if trash_dir == home_trash() {
        original.to_path_buf()
    } else {
        original
            .strip_prefix(trash_top_directory(&trash_dir))
            .map(Path::to_path_buf)
            .map_err(|_| io::Error::other("Trash metadata cannot represent the original path"))?
    };
    let encoded = percent_encode(stored_path.as_os_str().as_bytes());
    let mut rewritten = Vec::with_capacity(data.len().saturating_add(encoded.len()));
    let mut found = false;
    for line in data.split_inclusive(|byte| *byte == b'\n') {
        if trim_ascii(line).starts_with(b"Path=") {
            rewritten.extend_from_slice(b"Path=");
            rewritten.extend_from_slice(&encoded);
            rewritten.push(b'\n');
            found = true;
        } else {
            rewritten.extend_from_slice(line);
        }
    }
    if !found {
        return Err(io::Error::other("Trash metadata has no Path field"));
    }
    if rewritten.len() > TRASHINFO_BYTES_LIMIT {
        return Err(io::Error::other("Trash metadata exceeds its size limit"));
    }

    let basename: String = original
        .file_name()
        .unwrap_or(OsStr::new("item"))
        .to_string_lossy()
        .chars()
        .take(40)
        .collect();
    let name = format!("{basename}-{}", Uuid::new_v4().simple());
    let info_directory = trash_dir.join("info");
    let directory = secure::ensure_private_directory(&info_directory)?;
    let new_info = info_directory.join(format!("{name}.trashinfo"));
    let target =
        secure::resolved_child(&directory, &info_directory, new_info.file_name().unwrap())?;
    let old_info_identity = secure::entry_stat(&info)?.identity();
    let old_stored = stored_trash_path(item)?;
    let new_stored = trash_dir.join("files").join(&name);
    let expected = item
        .fingerprint
        .as_ref()
        .and_then(fingerprint_identity)
        .ok_or_else(|| io::Error::other("Trash identity is missing"))?;
    let mut metadata = secure::create_file_noreplace_resolved(&target, 0o600)?;
    let stat = metadata.metadata()?;
    let metadata_identity = secure::EntryIdentity {
        dev: stat.dev(),
        ino: stat.ino(),
        kind: EntryKind::File,
    };
    let written = metadata
        .write_all(&rewritten)
        .and_then(|()| metadata.sync_all());
    let moved = written.and_then(|()| {
        secure::relocate_noreplace_matching(
            &old_stored,
            &new_stored,
            expected,
            cancelled,
            &mut ignore_path,
        )
    });
    let published = secure::entry_stat(&new_stored).is_ok_and(|stat| stat.identity() == expected);
    if published {
        item.trash_name = name;
        item.fingerprint = fingerprint(&new_stored);
        secure::remove_nondirectory_matching(&info, old_info_identity)?;
    } else {
        let _ = secure::remove_nondirectory_matching(&new_info, metadata_identity);
        return moved.and_then(|()| Err(io::Error::other("Trash publication disappeared")));
    }
    moved
}

pub(super) fn percent_encode(value: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = Vec::with_capacity(value.len());
    for byte in value {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte);
        } else {
            encoded.push(b'%');
            encoded.push(HEX[(byte >> 4) as usize]);
            encoded.push(HEX[(byte & 0x0f) as usize]);
        }
    }
    encoded
}
