use super::*;

pub fn put(module: &str, raw_item: &str, cancelled: &AtomicBool) -> Value {
    if matches!(module, "skills" | "memory")
        && let Err(error) = crate::preferences::require_agent_management()
    {
        return refusal(error.to_string(), "");
    }
    let _lease = match mutation_lease() {
        Ok(lease) => lease,
        Err(error) => return refusal(error, ""),
    };
    if !valid_module(module) {
        return refusal(format!("{module:?} is not a valid bin module name"), "");
    }
    let parsed = match parse_item(raw_item) {
        Ok(parsed) => parsed,
        Err(error) => return refusal(error, ""),
    };
    let inventory = if parsed.paths.is_empty() {
        Inventory {
            items: Vec::new(),
            bytes: 0,
        }
    } else {
        match describe_paths(&parsed.paths, &parsed.realpath) {
            Ok(inventory) => inventory,
            Err(error) => return refusal(error, ""),
        }
    };
    if let Err(error) = check_cancelled(cancelled) {
        return refusal(error.to_string(), "");
    }
    let root = match bin_root(module) {
        Ok(root) => root,
        Err(error) => return refusal(error.to_string(), ""),
    };
    let entry_dir = match prepare_entry(&root) {
        Ok(entry_dir) => entry_dir,
        Err(error) => return refusal(error, ""),
    };
    let manifest = manifest_for(module, &parsed, inventory.items);
    if let Err(error) = store_items(&entry_dir, &manifest, cancelled) {
        let _ = discard_entry(&entry_dir, Some(&manifest));
        return refusal(
            format!("cannot store in {}: {error}", entry_dir.display()),
            "",
        );
    }
    let entry_id = format!(
        "{BIN_PREFIX}{}",
        entry_dir
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default()
    );
    let results = if manifest.items.is_empty() {
        vec![result(
            "",
            true,
            true,
            format!("stored in {}", entry_dir.display()),
        )]
    } else {
        remove_listed(&manifest.items, cancelled)
    };
    document(results, &entry_id, manifest.payload.clone())
}

pub(super) fn prepare_entry(root: &Path) -> Result<PathBuf, String> {
    secure::ensure_private_directory(root)
        .map_err(|error| format!("cannot prepare {}: {error}", root.display()))?;
    let entry_dir = fresh_entry_dir(root)
        .map_err(|error| format!("cannot prepare {}: {error}", root.display()))?;
    secure::create_directory_noreplace(&entry_dir, PRIVATE_DIRECTORY_MODE)
        .and_then(|()| {
            secure::create_directory_noreplace(
                &entry_dir.join(ITEMS_DIRECTORY),
                PRIVATE_DIRECTORY_MODE,
            )
        })
        .map_err(|error| format!("cannot prepare {}: {error}", root.display()))?;
    Ok(entry_dir)
}

pub(super) fn fresh_entry_dir(root: &Path) -> io::Result<PathBuf> {
    for _ in 0..100 {
        let candidate = root.join(Uuid::new_v4().simple().to_string());
        if !secure::entry_exists(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(io::Error::other("no free bin entry name"))
}

pub(super) fn store_items(
    entry_dir: &Path,
    manifest: &Manifest,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    for item in &manifest.items {
        check_cancelled(cancelled)?;
        verify_inventory_item(item)?;
        place_stored(item, &entry_dir.join(&item.stored), cancelled)?;
    }
    for item in &manifest.items {
        verify_inventory_item(item)?;
    }
    save_manifest_new(entry_dir, manifest)?;
    sync_directory(&entry_dir.join(ITEMS_DIRECTORY))?;
    sync_directory(entry_dir)
}

pub(super) fn place_stored(
    item: &StoredItem,
    destination: &Path,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    let source = parse_path(&item.source)?;
    match item.kind.as_str() {
        "dir" => {
            secure::create_directory_noreplace(destination, PRIVATE_DIRECTORY_MODE)?;
            copy_directory_xattrs(&source, destination)
        }
        "symlink" => {
            let target = secure::read_link(&source)?;
            if target != item.link_target() {
                return Err(io::Error::other(format!(
                    "{} changed while it was being stored",
                    source.display()
                )));
            }
            secure::create_symlink_noreplace(destination, &target)
        }
        "file" => copy_regular_private(&source, destination, item.size, cancelled),
        _ => Err(io::Error::other("invalid stored item type")),
    }
}

pub(super) fn copy_regular_private(
    source: &Path,
    destination: &Path,
    expected_size: u64,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    let before = secure::entry_stat(source)?;
    if before.kind != EntryKind::File
        || before.size > MAX_ITEM_BYTES
        || before.size != expected_size
    {
        return Err(io::Error::other(format!(
            "{} is not a regular file of {expected_size} bytes",
            source.display()
        )));
    }
    let mut reader = secure::open_file_read(source)?;
    let mut writer = secure::create_file_noreplace(destination, PRIVATE_FILE_MODE)?;
    let outcome = (|| {
        let mut buffer = [0_u8; COPY_CHUNK];
        let mut written = 0_u64;
        loop {
            check_cancelled(cancelled)?;
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            writer.write_all(&buffer[..count])?;
            written = written.saturating_add(count as u64);
        }
        let metadata = reader.metadata()?;
        if written != expected_size
            || metadata.len() != expected_size
            || metadata.dev() != before.dev
            || metadata.ino() != before.ino
            || metadata.mtime() != before.mtime
            || metadata.mtime_nsec() != before.mtime_nsec
        {
            return Err(io::Error::other(format!(
                "{} changed while it was being copied",
                source.display()
            )));
        }
        copy_xattrs(&reader, &writer);
        writer.sync_all()
    })();
    if outcome.is_err() {
        let _ = secure::remove_nondirectory(destination);
    }
    outcome
}

pub(super) fn save_manifest_new(entry_dir: &Path, manifest: &Manifest) -> io::Result<()> {
    let bytes = encode_manifest(manifest)?;
    crate::lease::durable::write_new_private(&entry_dir.join(MANIFEST_NAME), &bytes)
}

pub(super) fn save_manifest(entry_dir: &Path, manifest: &Manifest) -> io::Result<()> {
    let bytes = encode_manifest(manifest)?;
    crate::lease::durable::write_private_atomic(&entry_dir.join(MANIFEST_NAME), &bytes)
}

pub(super) fn encode_manifest(manifest: &Manifest) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        Err(io::Error::other("manifest exceeds its storage limit"))
    } else {
        Ok(bytes)
    }
}

pub(super) fn entry_dir_for(module: &str, entry_id: &str) -> Result<PathBuf, String> {
    if !valid_module(module) {
        return Err(format!("{module:?} is not a valid bin module name"));
    }
    let name = entry_id.strip_prefix(BIN_PREFIX).unwrap_or_default();
    if !valid_entry_name(name) {
        return Err(format!("{entry_id:?} is not a bin entry id"));
    }
    let root = bin_root(module).map_err(|error| error.to_string())?;
    let entry_dir = root.join(name);
    if secure::verify_private_directory(&entry_dir).is_err()
        || secure::verify_private_file(&entry_dir.join(MANIFEST_NAME)).is_err()
    {
        return Err(format!("no bin entry named {name}"));
    }
    Ok(entry_dir)
}

pub(super) fn load_manifest(
    entry_dir: &Path,
    expected_module: Option<&str>,
) -> io::Result<Option<(Manifest, usize)>> {
    let path = entry_dir.join(MANIFEST_NAME);
    let Some(bytes) = secure::read_private_bounded(&path, MAX_MANIFEST_BYTES)? else {
        return Ok(None);
    };
    let size = bytes.len();
    let manifest: Manifest = match serde_json::from_slice(&bytes) {
        Ok(manifest) => manifest,
        Err(_) => return Ok(None),
    };
    if !valid_manifest(&manifest, expected_module) {
        return Ok(None);
    }
    Ok(Some((manifest, size)))
}

pub(super) fn valid_manifest(manifest: &Manifest, expected_module: Option<&str>) -> bool {
    if manifest.schema_version != SCHEMA_VERSION
        || !valid_module(&manifest.module)
        || expected_module.is_some_and(|module| module != manifest.module)
        || manifest.items.len() > MAX_TREE_ENTRIES
        || manifest.helper_record_id.as_ref().is_some_and(|id| {
            id.len() != 32
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        || (manifest.helper_restored && manifest.restore_helper.is_none())
    {
        return false;
    }
    let mut stored = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut kinds = BTreeMap::new();
    let mut bytes = 0_u64;
    for item in &manifest.items {
        if !valid_stored_item(item)
            || !stored.insert(item.stored.clone())
            || !sources.insert(item.source.clone())
        {
            return false;
        }
        bytes = bytes.saturating_add(item.size);
        kinds.insert(item.stored.clone(), item.kind.as_str());
    }
    if bytes > MAX_TREE_BYTES {
        return false;
    }
    for item in &manifest.items {
        let stored_path = Path::new(&item.stored);
        if stored_path.components().count() > 2 {
            let Some(parent) = stored_path.parent() else {
                return false;
            };
            if kinds.get(&path_text(parent)).copied() != Some("dir") {
                return false;
            }
        }
    }
    let roots: BTreeSet<_> = root_items(manifest)
        .into_iter()
        .map(|item| item.stored)
        .collect();
    manifest
        .restore_completed
        .iter()
        .all(|stored| roots.contains(stored))
}

pub(super) fn valid_stored_item(item: &StoredItem) -> bool {
    if !matches!(item.kind.as_str(), "file" | "symlink" | "dir")
        || !(item.source.starts_with('/') || item.source.starts_with("file://"))
        || parse_path(&item.source).is_err()
        || item.source.contains('\0')
        || item.target.contains('\0')
        || item
            .target_bytes
            .as_ref()
            .is_some_and(|bytes| bytes.contains(&0))
        || item.stored.contains('\0')
        || Path::new(&item.source)
            .components()
            .any(|component| component == Component::ParentDir)
        || item.size > MAX_ITEM_BYTES
        || (item.kind != "file" && item.size != 0)
    {
        return false;
    }
    if item.stored.starts_with('/')
        || item.stored.ends_with('/')
        || item
            .stored
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return false;
    }
    let components: Vec<_> = Path::new(&item.stored).components().collect();
    components.len() >= 2
        && components[0] == Component::Normal(ITEMS_DIRECTORY.as_ref())
        && components
            .iter()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(super) fn apply_restored_metadata(
    root: &StoredItem,
    subtree: &[StoredItem],
    base: &Path,
) -> io::Result<()> {
    for item in subtree.iter().rev() {
        if item.kind == "symlink" {
            continue;
        }
        let destination = mapped_destination(item, root, base)?;
        let mode = restored_mode(item);
        match (item.atime_ns, item.mtime_ns) {
            (Some(atime), Some(mtime)) => secure::set_mode_and_times(
                &destination,
                mode,
                split_nanoseconds(atime),
                split_nanoseconds(mtime),
            )?,
            _ => secure::set_mode(&destination, mode)?,
        }
    }
    Ok(())
}

pub(super) fn restored_mode(item: &StoredItem) -> u32 {
    let mode = item.mode & 0o777;
    if mode != 0 {
        mode
    } else if item.kind == "dir" {
        0o755
    } else {
        0o644
    }
}

pub(super) fn copy_xattrs(source: &File, destination: &File) {
    let Ok(names) = source.list_xattr() else {
        return;
    };
    for name in names {
        if let Ok(Some(value)) = source.get_xattr(&name) {
            let _ = destination.set_xattr(&name, &value);
        }
    }
}

pub(super) fn copy_directory_xattrs(source: &Path, destination: &Path) -> io::Result<()> {
    let source: File = secure::open_directory_entry(source)?.into();
    let destination: File = secure::open_directory_entry(destination)?.into();
    copy_xattrs(&source, &destination);
    Ok(())
}

pub(super) fn sync_directory(path: &Path) -> io::Result<()> {
    let directory = secure::open_directory_nofollow(path)?;
    rustix::fs::fsync(&directory).map_err(io::Error::from)
}
