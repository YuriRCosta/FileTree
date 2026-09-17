use super::*;

pub(super) fn home_trash() -> PathBuf {
    if let Some(value) = std::env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return normalize_path(&path).join("Trash");
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/nonexistent"));
    normalize_path(&home).join(".local/share/Trash")
}

pub(super) fn mounted_top_directories() -> Vec<PathBuf> {
    let path = PathBuf::from(format!("/proc/{}/mountinfo", std::process::id()));
    let data = secure::read_bounded_nofollow(&path, MAX_MOUNTINFO_BYTES)
        .ok()
        .flatten()
        .unwrap_or_default();
    let mut values = Vec::new();
    let mut seen = HashSet::new();
    for line in data.split(|byte| *byte == b'\n').take(MAX_MOUNT_LINES) {
        if line.len() > MAX_MOUNT_LINE_BYTES || values.len() >= MAX_MOUNTS {
            continue;
        }
        let fields = line
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|field| !field.is_empty())
            .collect::<Vec<_>>();
        let Some(separator) = fields.iter().position(|field| *field == b"-") else {
            continue;
        };
        if separator < 6
            || separator + 1 >= fields.len()
            || skipped_filesystem(fields[separator + 1])
        {
            continue;
        }
        let Some(decoded) = decode_mount_field(fields[4]) else {
            continue;
        };
        let mount = PathBuf::from(OsString::from_vec(decoded));
        let Some(mount) = normalized_absolute(&mount) else {
            continue;
        };
        if seen.insert(mount.clone()) {
            values.push(mount);
        }
    }
    if seen.insert(PathBuf::from("/")) {
        values.push(PathBuf::from("/"));
    }
    values.sort_by(|left, right| {
        right
            .components()
            .count()
            .cmp(&left.components().count())
            .then_with(|| left.cmp(right))
    });
    values.truncate(MAX_MOUNTS);
    values
}

pub(super) fn skipped_filesystem(value: &[u8]) -> bool {
    matches!(
        value,
        b"autofs"
            | b"bpf"
            | b"cgroup"
            | b"cgroup2"
            | b"configfs"
            | b"debugfs"
            | b"devpts"
            | b"devtmpfs"
            | b"efivarfs"
            | b"fusectl"
            | b"hugetlbfs"
            | b"mqueue"
            | b"nfs"
            | b"nfs4"
            | b"nsfs"
            | b"proc"
            | b"pstore"
            | b"ramfs"
            | b"securityfs"
            | b"sysfs"
            | b"tracefs"
            | b"cifs"
            | b"smb3"
            | b"fuse.sshfs"
    )
}

pub(super) fn decode_mount_field(value: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if value[index] != b'\\' {
            if value[index] == 0 {
                return None;
            }
            output.push(value[index]);
            index += 1;
            continue;
        }
        if index + 3 >= value.len() {
            return None;
        }
        let digits = &value[index + 1..index + 4];
        if !digits.iter().all(|digit| matches!(digit, b'0'..=b'7')) {
            return None;
        }
        let decoded = (digits[0] - b'0') * 64 + (digits[1] - b'0') * 8 + digits[2] - b'0';
        if decoded == 0 {
            return None;
        }
        output.push(decoded);
        index += 4;
    }
    Some(output)
}

pub(super) fn discover_stores(context: &TrashContext, errors: &mut Vec<String>) -> Vec<TrashStore> {
    let mut stores = Vec::new();
    let mut seen = HashSet::new();
    if context.home_trash.is_absolute() && path_present(&context.home_trash) {
        let base = context
            .home_trash
            .parent()
            .unwrap_or(Path::new("/"))
            .to_path_buf();
        let source_mount = source_mount_for(&context.home_trash, &context.mount_tops);
        add_store(
            TrashStore {
                path: context.home_trash.clone(),
                source_mount,
                kind: StoreKind::Home { base },
            },
            &mut stores,
            &mut seen,
            errors,
        );
    }
    let uid = getuid().as_raw().to_string();
    for top in context.mount_tops.iter().take(MAX_MOUNTS) {
        if stores.len() >= MAX_STORES || secure::open_directory_nofollow(top).is_err() {
            continue;
        }
        let shared = top.join(".Trash");
        if path_present(&shared) {
            match secure::entry_stat(&shared) {
                Ok(stat)
                    if stat.kind == EntryKind::Directory
                        && stat.mode & 0o1000 != 0
                        && secure::open_directory_nofollow(&shared).is_ok() =>
                {
                    let user_store = shared.join(&uid);
                    if path_present(&user_store) {
                        add_store(
                            TrashStore {
                                path: user_store,
                                source_mount: top.clone(),
                                kind: StoreKind::Mount { top: top.clone() },
                            },
                            &mut stores,
                            &mut seen,
                            errors,
                        );
                    }
                }
                _ => push_error(
                    errors,
                    &format!(
                        "Ignoring unsafe mounted Trash directory {}",
                        shared.display()
                    ),
                ),
            }
        }
        let private = top.join(format!(".Trash-{uid}"));
        if path_present(&private) {
            add_store(
                TrashStore {
                    path: private,
                    source_mount: top.clone(),
                    kind: StoreKind::Mount { top: top.clone() },
                },
                &mut stores,
                &mut seen,
                errors,
            );
        }
    }
    stores
}

pub(super) fn add_store(
    store: TrashStore,
    stores: &mut Vec<TrashStore>,
    seen: &mut HashSet<PathBuf>,
    errors: &mut Vec<String>,
) {
    if stores.len() >= MAX_STORES || !seen.insert(store.path.clone()) {
        return;
    }
    match validate_store(&store.path) {
        Ok(()) => stores.push(store),
        Err(error) => push_error(
            errors,
            &format!(
                "Ignoring unsafe Trash store {}: {error}",
                store.path.display()
            ),
        ),
    }
}

pub(super) fn validate_store(path: &Path) -> io::Result<()> {
    validate_owned_directory(path)?;
    validate_owned_directory(&path.join("info"))?;
    validate_owned_directory(&path.join("files"))
}

pub(super) fn validate_owned_directory(path: &Path) -> io::Result<()> {
    secure::open_directory_nofollow(path)?;
    let stat = secure::entry_stat(path)?;
    if stat.kind != EntryKind::Directory || stat.uid != geteuid().as_raw() || stat.mode & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "directory is not private and owned by this user",
        ));
    }
    Ok(())
}

pub(super) fn source_mount_for(path: &Path, mounts: &[PathBuf]) -> PathBuf {
    mounts
        .iter()
        .filter(|mount| path.starts_with(mount))
        .max_by_key(|mount| mount.components().count())
        .cloned()
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub(super) fn secure_directory_names(
    path: &Path,
    limit: usize,
) -> io::Result<(Vec<OsString>, bool)> {
    let directory = secure::open_directory_nofollow(path)?;
    secure::directory_names_bounded_from(&directory, limit)
}

pub(super) fn path_present(path: &Path) -> bool {
    match secure::entry_stat(path) {
        Ok(_) => true,
        Err(error) => !matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
        ),
    }
}

pub(super) fn normalized_absolute(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() || path.as_os_str().as_bytes().contains(&0) {
        return None;
    }
    Some(normalize_path(path))
}

pub(super) fn valid_stored_name(value: &[u8]) -> bool {
    !value.is_empty()
        && value != b"."
        && value != b".."
        && !value.contains(&0)
        && !value.contains(&b'/')
}

pub(super) fn optional_stat(path: &Path) -> Option<EntryStat> {
    secure::entry_stat(path).ok()
}

pub(super) fn trashinfo_path(store: &TrashStore, name: &OsStr) -> PathBuf {
    let mut value = name.as_bytes().to_vec();
    value.extend_from_slice(b".trashinfo");
    store.path.join("info").join(OsString::from_vec(value))
}
