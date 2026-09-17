use super::*;

pub(super) fn build_catalog(context: &TrashContext, cancelled: &AtomicBool) -> Catalog {
    let mut catalog = Catalog {
        entries: Vec::new(),
        store_count: 0,
        watch_paths: Vec::new(),
        errors: Vec::new(),
        scanned: 0,
        truncated: false,
        cancelled: false,
    };
    if cancelled.load(Ordering::Relaxed) {
        catalog.cancelled = true;
        return catalog;
    }
    let stores = discover_stores(context, &mut catalog.errors);
    catalog.store_count = stores.len();
    for store in &stores {
        catalog.watch_paths.push(store.path.join("info"));
        catalog.watch_paths.push(store.path.join("files"));
    }
    let mut budget = MetadataBudget {
        remaining: MAX_METADATA_BYTES,
        exhausted: false,
    };
    for store in stores {
        if cancelled.load(Ordering::Relaxed) {
            catalog.cancelled = true;
            catalog.truncated = true;
            break;
        }
        if catalog.scanned >= MAX_CANDIDATES {
            catalog.truncated = true;
            break;
        }
        scan_store(&store, &mut catalog, &mut budget, cancelled);
    }
    if budget.exhausted {
        catalog.truncated = true;
        push_error(
            &mut catalog.errors,
            "Trash metadata exceeds the 4 MiB aggregate limit",
        );
    }
    catalog.entries.sort_by(|left, right| {
        let left_date = left
            .metadata
            .as_ref()
            .map(|metadata| metadata.deleted_at.as_str())
            .unwrap_or("");
        let right_date = right
            .metadata
            .as_ref()
            .map(|metadata| metadata.deleted_at.as_str())
            .unwrap_or("");
        right_date.cmp(left_date).then_with(|| {
            display_name(left)
                .to_lowercase()
                .cmp(&display_name(right).to_lowercase())
        })
    });
    catalog
}

pub(super) fn scan_store(
    store: &TrashStore,
    catalog: &mut Catalog,
    budget: &mut MetadataBudget,
    cancelled: &AtomicBool,
) {
    let info_directory = store.path.join("info");
    let files_directory = store.path.join("files");
    let mut names = BTreeSet::new();
    match secure_directory_names(&info_directory, MAX_STORE_CANDIDATES) {
        Ok((values, truncated)) => {
            catalog.truncated |= truncated;
            for value in values {
                if let Some(stem) = value.as_bytes().strip_suffix(b".trashinfo")
                    && valid_stored_name(stem)
                {
                    names.insert(OsString::from_vec(stem.to_vec()));
                }
            }
        }
        Err(error) => push_error(
            &mut catalog.errors,
            &format!("Unable to read {}: {error}", info_directory.display()),
        ),
    }
    match secure_directory_names(&files_directory, MAX_STORE_CANDIDATES) {
        Ok((values, truncated)) => {
            catalog.truncated |= truncated;
            for value in values {
                if valid_stored_name(value.as_bytes()) {
                    names.insert(value);
                }
            }
        }
        Err(error) => push_error(
            &mut catalog.errors,
            &format!("Unable to read {}: {error}", files_directory.display()),
        ),
    }
    let directory_sizes = read_directory_sizes(store, budget);
    for name in names {
        if cancelled.load(Ordering::Relaxed) {
            catalog.cancelled = true;
            catalog.truncated = true;
            return;
        }
        if catalog.scanned >= MAX_CANDIDATES {
            catalog.truncated = true;
            return;
        }
        catalog.scanned += 1;
        let info_path = trashinfo_path(store, &name);
        let stored_path = store.path.join("files").join(&name);
        let info_stat = optional_stat(&info_path);
        let stored_stat = optional_stat(&stored_path);
        let metadata = if info_stat.is_some() {
            match read_trash_metadata(store, &info_path, info_stat, budget) {
                Ok(metadata) => Some(metadata),
                Err(error) => {
                    push_error(
                        &mut catalog.errors,
                        &format!(
                            "Invalid Trash metadata for {}: {error}",
                            name.to_string_lossy()
                        ),
                    );
                    None
                }
            }
        } else {
            None
        };
        let size = entry_size(&name, stored_stat, info_stat, &directory_sizes);
        let id = entry_id(store, &name, info_stat, stored_stat);
        catalog.entries.push(TrashEntry {
            id,
            store: store.clone(),
            stored_name: name,
            info_path,
            stored_path,
            info_stat,
            stored_stat,
            metadata,
            size,
        });
    }
}

pub(super) fn catalog_document(catalog: &Catalog, limit: usize) -> Value {
    let estimated_size = catalog
        .entries
        .iter()
        .filter_map(|entry| entry.size)
        .fold(0_u64, u64::saturating_add);
    let unknown_size = catalog
        .entries
        .iter()
        .filter(|entry| entry.size.is_none())
        .count();
    let watch_paths = catalog
        .watch_paths
        .iter()
        .map(|path| path_text(path))
        .collect::<Vec<_>>();
    let mut document = json!({
        "ok": !catalog.cancelled,
        "resource": "trash:///",
        "entries": [],
        "count": catalog.entries.len(),
        "scanned": catalog.scanned,
        "stores": catalog.store_count,
        "watch_paths": watch_paths,
        "estimated_size": estimated_size,
        "estimated_size_text": human_size(Some(estimated_size)),
        "unknown_size": unknown_size,
        "limit": limit,
        "truncated": true,
        "cancelled": catalog.cancelled,
        "errors": catalog.errors,
        "error": if catalog.cancelled { "operation cancelled" } else { "" }
    });
    let mut entries = Vec::new();
    let mut response_bytes = serde_json::to_vec(&document)
        .map(|value| value.len())
        .unwrap_or(MAX_RESPONSE_BYTES);
    let mut response_truncated = false;
    for entry in catalog.entries.iter().take(limit) {
        let row = entry_document(entry);
        let row_bytes = serde_json::to_vec(&row)
            .map(|value| value.len().saturating_add(1))
            .unwrap_or(MAX_RESPONSE_BYTES);
        if response_bytes.saturating_add(row_bytes) > MAX_RESPONSE_BYTES {
            response_truncated = true;
            break;
        }
        response_bytes = response_bytes.saturating_add(row_bytes);
        entries.push(row);
    }
    let truncated = catalog.truncated
        || response_truncated
        || catalog.entries.len() > entries.len()
        || catalog.scanned > catalog.entries.len();
    document["entries"] = json!(entries);
    document["truncated"] = json!(truncated);
    document
}

pub(super) fn entry_document(entry: &TrashEntry) -> Value {
    let metadata = entry.metadata.as_ref();
    let name = display_name(entry);
    let original = metadata
        .map(|value| path_text(&value.original))
        .unwrap_or_default();
    let original_parent = metadata
        .and_then(|value| value.original.parent())
        .map(path_text)
        .unwrap_or_default();
    let deleted_at = metadata
        .map(|value| value.deleted_at.clone())
        .unwrap_or_default();
    let kind = entry_kind_text(entry.stored_stat.map(|stat| stat.kind));
    let is_dir = entry
        .stored_stat
        .is_some_and(|stat| stat.kind == EntryKind::Directory);
    let is_link = entry
        .stored_stat
        .is_some_and(|stat| stat.kind == EntryKind::Symlink);
    let available = entry.stored_stat.is_some();
    let can_restore = available && metadata.is_some();
    let mut actions = vec!["delete_permanently"];
    if can_restore {
        actions.insert(0, "restore_to");
        actions.insert(0, "restore");
    }
    json!({
        "id": entry.id,
        "source": "desktop",
        "module": "",
        "artifact_id": "",
        "resource": format!("trash:///{}", entry.id),
        "name": name,
        "stored_name": display_path(Path::new(&entry.stored_name)),
        "original_path": original,
        "original_parent": original_parent,
        "deleted_at": deleted_at,
        "size": entry.size,
        "size_text": human_size(entry.size),
        "kind": kind,
        "mime": entry_mime(&name, is_dir),
        "is_dir": is_dir,
        "is_link": is_link,
        "metadata_valid": metadata.is_some(),
        "emergency": metadata.is_none() || !available,
        "stored_available": available,
        "source_mount": path_text(&entry.store.source_mount),
        "store": path_text(&entry.store.path),
        "mount_available": true,
        "can_restore": can_restore,
        "can_restore_to": can_restore,
        "can_delete": true,
        "requires_module_restore": false,
        "actions": actions
    })
}

pub(super) fn display_name(entry: &TrashEntry) -> String {
    display_path(Path::new(
        entry
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.original.file_name())
            .unwrap_or(&entry.stored_name),
    ))
}

pub(super) fn entry_kind_text(kind: Option<EntryKind>) -> &'static str {
    match kind {
        Some(EntryKind::Directory) => "Directory",
        Some(EntryKind::File) => "File",
        Some(EntryKind::Symlink) => "Symbolic link",
        Some(EntryKind::Other) => "Other",
        None => "Missing",
    }
}

pub(super) fn read_directory_sizes(
    store: &TrashStore,
    budget: &mut MetadataBudget,
) -> HashMap<OsString, (u64, i64)> {
    if budget.remaining == 0 {
        budget.exhausted = true;
        return HashMap::new();
    }
    let path = store.path.join("directorysizes");
    let limit = budget.remaining.min(MAX_DIRECTORYSIZES_BYTES);
    let data = match secure::read_bounded_nofollow(&path, limit) {
        Ok(Some(data)) => data,
        Ok(None) => return HashMap::new(),
        Err(_) => {
            if limit < MAX_DIRECTORYSIZES_BYTES {
                budget.exhausted = true;
            }
            return HashMap::new();
        }
    };
    budget.remaining = budget.remaining.saturating_sub(data.len());
    let mut values = HashMap::new();
    for line in data.split(|byte| *byte == b'\n').take(MAX_STORE_CANDIDATES) {
        let fields = line
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|field| !field.is_empty())
            .collect::<Vec<_>>();
        if fields.len() != 3 {
            continue;
        }
        let Ok(size_text) = std::str::from_utf8(fields[0]) else {
            continue;
        };
        let Ok(mtime_text) = std::str::from_utf8(fields[1]) else {
            continue;
        };
        let (Ok(size), Ok(mtime), Some(name)) = (
            size_text.parse::<u64>(),
            mtime_text.parse::<i64>(),
            percent_decode(fields[2]),
        ) else {
            continue;
        };
        if valid_stored_name(&name) {
            values.insert(OsString::from_vec(name), (size, mtime));
        }
    }
    values
}

pub(super) fn entry_size(
    name: &OsStr,
    stored_stat: Option<EntryStat>,
    info_stat: Option<EntryStat>,
    directory_sizes: &HashMap<OsString, (u64, i64)>,
) -> Option<u64> {
    let stored = stored_stat?;
    if stored.kind != EntryKind::Directory {
        return Some(stored.size);
    }
    let info = info_stat?;
    directory_sizes
        .get(name)
        .filter(|(_, mtime)| *mtime == info.mtime)
        .map(|(size, _)| *size)
}

pub(super) fn read_trash_metadata(
    store: &TrashStore,
    path: &Path,
    before: Option<EntryStat>,
    budget: &mut MetadataBudget,
) -> Result<TrashMetadata, String> {
    if budget.remaining == 0 {
        budget.exhausted = true;
        return Err("aggregate metadata limit reached".to_string());
    }
    let limit = budget.remaining.min(MAX_TRASHINFO_BYTES);
    let data = secure::read_bounded_nofollow(path, limit)
        .map_err(|error| {
            if limit < MAX_TRASHINFO_BYTES {
                budget.exhausted = true;
            }
            error.to_string()
        })?
        .ok_or_else(|| "metadata disappeared".to_string())?;
    budget.remaining = budget.remaining.saturating_sub(data.len());
    let after = optional_stat(path);
    if !same_identity_and_time(before, after) {
        return Err("metadata changed while it was read".to_string());
    }
    parse_trash_metadata(store, &data)
}

pub(super) fn parse_trash_metadata(
    store: &TrashStore,
    data: &[u8],
) -> Result<TrashMetadata, String> {
    let mut lines = data.split(|byte| *byte == b'\n');
    if trim_carriage_return(lines.next().unwrap_or_default()) != b"[Trash Info]" {
        return Err("first line is not [Trash Info]".to_string());
    }
    let mut path_value = None;
    let mut date_value = None;
    for line in lines {
        let line = trim_carriage_return(line);
        if path_value.is_none()
            && let Some(value) = line.strip_prefix(b"Path=")
        {
            path_value = Some(value.to_vec());
            continue;
        }
        if date_value.is_none()
            && let Some(value) = line.strip_prefix(b"DeletionDate=")
        {
            date_value = Some(value.to_vec());
        }
    }
    let encoded_path = path_value.ok_or_else(|| "Path is missing".to_string())?;
    let decoded_path = percent_decode(&encoded_path)
        .filter(|value| !value.is_empty() && !value.contains(&0))
        .ok_or_else(|| "Path has invalid percent encoding".to_string())?;
    let original = trusted_original_path(store, &decoded_path)?;
    let date = date_value.ok_or_else(|| "DeletionDate is missing".to_string())?;
    let date = std::str::from_utf8(&date).map_err(|_| "DeletionDate is not UTF-8".to_string())?;
    let parsed = NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(date, "%Y%m%dT%H:%M:%S"))
        .map_err(|_| "DeletionDate has an invalid format".to_string())?;
    Ok(TrashMetadata {
        original,
        deleted_at: parsed.format("%Y-%m-%dT%H:%M:%S").to_string(),
    })
}

pub(super) fn trusted_original_path(store: &TrashStore, value: &[u8]) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(OsString::from_vec(value.to_vec()));
    if candidate.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::Prefix(_) | Component::CurDir
        )
    }) {
        return Err("Path contains an unsafe component".to_string());
    }
    let (base, mount) = match &store.kind {
        StoreKind::Home { base } if candidate.is_absolute() => (base, None),
        StoreKind::Home { .. } => {
            return Err("home Trash Path is not absolute".to_string());
        }
        StoreKind::Mount { top } if !candidate.is_absolute() => (top, Some(top)),
        StoreKind::Mount { .. } => {
            return Err("mounted Trash Path is not relative".to_string());
        }
    };
    let original = if candidate.is_absolute() {
        normalize_path(&candidate)
    } else {
        normalize_path(&base.join(&candidate))
    };
    if !original.is_absolute() || original.file_name().is_none() {
        return Err("Path does not identify an item".to_string());
    }
    if !candidate.is_absolute() && !original.starts_with(base) {
        return Err("relative Path escapes its Trash mount".to_string());
    }
    if let Some(top) = mount
        && !original.starts_with(top)
    {
        return Err("Path points outside its Trash mount".to_string());
    }
    Ok(original)
}

pub(super) fn percent_decode(value: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if value[index] != b'%' {
            output.push(value[index]);
            index += 1;
            continue;
        }
        if index + 2 >= value.len() {
            return None;
        }
        let high = (value[index + 1] as char).to_digit(16)?;
        let low = (value[index + 2] as char).to_digit(16)?;
        output.push((high * 16 + low) as u8);
        index += 3;
    }
    Some(output)
}

pub(super) fn trim_carriage_return(value: &[u8]) -> &[u8] {
    value.strip_suffix(b"\r").unwrap_or(value)
}

pub(super) fn entry_id(
    store: &TrashStore,
    name: &OsStr,
    info_stat: Option<EntryStat>,
    stored_stat: Option<EntryStat>,
) -> String {
    let mut value = String::from("v1-");
    append_hex(&mut value, store.path.as_os_str().as_bytes());
    value.push('-');
    append_hex(&mut value, name.as_bytes());
    for part in identity_parts(info_stat)
        .into_iter()
        .chain(identity_parts(stored_stat))
    {
        value.push('-');
        value.push_str(&part.to_string());
    }
    value
}

pub(super) fn identity_parts(stat: Option<EntryStat>) -> [i128; 5] {
    let Some(stat) = stat else {
        return [0, 0, 0, 0, 0];
    };
    [
        stat.dev as i128,
        stat.ino as i128,
        stat.mtime as i128,
        stat.mtime_nsec as i128,
        stat.size as i128,
    ]
}

pub(super) fn append_hex(output: &mut String, value: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in value {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

pub(super) fn same_identity_and_time(left: Option<EntryStat>, right: Option<EntryStat>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.dev == right.dev
                && left.ino == right.ino
                && left.kind == right.kind
                && left.size == right.size
                && left.mtime == right.mtime
                && left.mtime_nsec == right.mtime_nsec
        }
        _ => false,
    }
}

pub(super) fn same_entry_identity(left: Option<EntryStat>, right: Option<EntryStat>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.dev == right.dev
                && left.ino == right.ino
                && left.kind == right.kind
                && left.size == right.size
                && left.mtime == right.mtime
                && left.mtime_nsec == right.mtime_nsec
        }
        _ => false,
    }
}

pub(super) fn bounded_error(message: &str) -> String {
    let mut value = message.replace(['\n', '\r', '\0'], " ");
    value.truncate(value.floor_char_boundary(MAX_ERROR_BYTES));
    value
}

pub(super) fn push_error(errors: &mut Vec<String>, message: &str) {
    if errors.len() < MAX_ERRORS {
        errors.push(bounded_error(message));
    }
}
