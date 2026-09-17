use super::*;

pub fn rows(module: &str) -> Value {
    if !valid_module(module) {
        return json!({
            "ok": false,
            "schemaVersion": SCHEMA_VERSION,
            "items": [],
            "message": format!("{module:?} is not a valid bin module name"),
        });
    }
    let root = match bin_root(module) {
        Ok(root) => root,
        Err(error) => {
            return json!({
                "ok": false,
                "schemaVersion": SCHEMA_VERSION,
                "module": module,
                "items": [],
                "message": error.to_string(),
            });
        }
    };
    if !secure::entry_exists(&root).unwrap_or(false) {
        return json!({
            "ok": true,
            "schemaVersion": SCHEMA_VERSION,
            "module": module,
            "items": [],
        });
    }
    if let Err(error) = secure::verify_private_directory(&root) {
        return json!({
            "ok": false,
            "schemaVersion": SCHEMA_VERSION,
            "module": module,
            "items": [],
            "message": error.to_string(),
        });
    }
    let (mut names, entry_truncated) =
        secure::directory_names_bounded(&root, MAX_ENTRIES).unwrap_or_default();
    names.sort_by(|left, right| right.cmp(left));
    let started = Instant::now();
    let mut found = Vec::new();
    let mut manifest_bytes = 0_usize;
    let mut manifest_items = 0_usize;
    let mut response_bytes = 0_usize;
    let mut unreadable = 0_usize;
    let mut reasons = Vec::new();
    if entry_truncated {
        reasons.push("entry limit".to_string());
    }
    for name in names {
        if started.elapsed() >= MAX_LIST_TIME {
            reasons.push("time limit".to_string());
            break;
        }
        let name_text = name.to_string_lossy();
        if !valid_entry_name(&name_text) {
            continue;
        }
        let entry_dir = root.join(&name);
        if secure::verify_private_directory(&entry_dir).is_err() {
            unreadable += 1;
            continue;
        }
        let Some((manifest, bytes)) = load_manifest(&entry_dir, Some(module)).ok().flatten() else {
            unreadable += 1;
            continue;
        };
        if manifest_bytes.saturating_add(bytes) > MAX_LIST_MANIFEST_BYTES {
            reasons.push("manifest byte limit".to_string());
            break;
        }
        if manifest_items.saturating_add(manifest.items.len()) > MAX_LIST_ITEMS {
            reasons.push("manifest item limit".to_string());
            break;
        }
        let row = bin_row(module, &entry_dir, &manifest);
        let row_bytes = serde_json::to_vec(&row)
            .map(|value| value.len())
            .unwrap_or(0);
        if response_bytes.saturating_add(row_bytes) > MAX_LIST_RESPONSE_BYTES {
            reasons.push("response byte limit".to_string());
            break;
        }
        manifest_bytes += bytes;
        manifest_items += manifest.items.len();
        response_bytes += row_bytes;
        found.push(row);
    }
    reasons.sort();
    reasons.dedup();
    let truncated = !reasons.is_empty();
    let mut response = json!({
        "ok": true,
        "schemaVersion": SCHEMA_VERSION,
        "module": module,
        "items": found,
    });
    if truncated || unreadable > 0 {
        response["truncated"] = Value::Bool(truncated);
        response["message"] = Value::String(if truncated {
            format!("Bin listing truncated: {}", reasons.join(", "))
        } else {
            String::new()
        });
        response["diagnostics"] = json!({
            "reasons": reasons,
            "manifestBytes": manifest_bytes,
            "manifestItems": manifest_items,
            "responseBytes": response_bytes,
            "unreadableEntries": unreadable,
            "elapsedMs": started.elapsed().as_millis(),
        });
    }
    response
}

pub fn trash_rows(limit: usize, cancelled: &AtomicBool) -> Value {
    let maximum = limit.clamp(1, MAX_ALL_ENTRIES);
    let base = match bin_base() {
        Ok(base) => base,
        Err(error) => {
            return json!({"ok": false, "entries": [], "count": 0, "errors": [error.to_string()]});
        }
    };
    if !secure::entry_exists(&base).unwrap_or(false) {
        return json!({"ok": true, "entries": [], "count": 0, "modules": 0, "watch_paths": [], "errors": []});
    }
    if let Err(error) = secure::ensure_private_directory(&base) {
        return json!({"ok": false, "entries": [], "count": 0, "errors": [format!("cannot inspect {}: {error}", base.display())]});
    }
    let (modules, modules_truncated) =
        secure::directory_names_bounded(&base, MAX_MODULES).unwrap_or_default();
    let mut found = Vec::new();
    let mut examined = 0_usize;
    let mut module_count = 0_usize;
    let mut truncated = modules_truncated;
    let mut errors = Vec::new();
    let mut watch_paths = vec![path_text(&base)];
    for module_name in modules {
        if cancelled.load(Ordering::Relaxed) {
            truncated = true;
            break;
        }
        let module = module_name.to_string_lossy();
        if !valid_module(&module) {
            continue;
        }
        let root = base.join(&module_name);
        if secure::verify_private_directory(&root).is_err() {
            push_bin_error(
                &mut errors,
                format!("Cannot inspect module Trash {}", root.display()),
            );
            continue;
        }
        module_count += 1;
        watch_paths.push(path_text(&root));
        let (entries, entries_truncated) =
            secure::directory_names_bounded(&root, MAX_ENTRIES).unwrap_or_default();
        truncated |= entries_truncated;
        for name in entries {
            if found.len() >= maximum || examined >= MAX_ALL_ENTRIES {
                truncated = true;
                break;
            }
            let name_text = name.to_string_lossy();
            if !valid_entry_name(&name_text) {
                continue;
            }
            let entry_dir = root.join(&name);
            examined += 1;
            let Some((manifest, _)) = load_manifest(&entry_dir, Some(&module)).ok().flatten()
            else {
                push_bin_error(
                    &mut errors,
                    format!("Unreadable satellite Trash entry in {module}"),
                );
                continue;
            };
            found.push(artifact_trash_row(&module, &entry_dir, &manifest));
        }
        if found.len() >= maximum || examined >= MAX_ALL_ENTRIES {
            truncated = true;
            break;
        }
    }
    found.sort_by(|left, right| {
        right["deleted_at"]
            .as_str()
            .unwrap_or("")
            .cmp(left["deleted_at"].as_str().unwrap_or(""))
            .then_with(|| {
                left["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .cmp(&right["name"].as_str().unwrap_or("").to_lowercase())
            })
    });
    json!({
        "ok": !cancelled.load(Ordering::Relaxed),
        "entries": found,
        "count": found.len(),
        "modules": module_count,
        "watch_paths": watch_paths,
        "truncated": truncated,
        "cancelled": cancelled.load(Ordering::Relaxed),
        "errors": errors
    })
}

pub fn empty_all(cancelled: &AtomicBool, progress: &mut dyn FnMut(Value)) -> Value {
    let _lease = match mutation_lease() {
        Ok(lease) => lease,
        Err(error) => return refusal(error, ""),
    };
    mutate_all(None, "artifact-trash-empty", cancelled, progress)
}

pub fn prune_all(days: u32, cancelled: &AtomicBool, progress: &mut dyn FnMut(Value)) -> Value {
    if !(1..=3650).contains(&days) {
        return refusal("Trash retention must be between 1 and 3650 days", "");
    }
    if cancelled.load(Ordering::Relaxed) {
        return refusal("operation cancelled", "");
    }
    let _lease = match mutation_lease() {
        Ok(lease) => lease,
        Err(error) => return refusal(error, ""),
    };
    let cutoff = (Local::now() - ChronoDuration::days(i64::from(days))).timestamp();
    let base = match bin_base() {
        Ok(base) => base,
        Err(error) => return refusal(error.to_string(), ""),
    };
    if !secure::entry_exists(&base).unwrap_or(false) {
        return json!({
            "ok": true,
            "operation": "artifact-trash-prune",
            "examined": 0,
            "eligible": 0,
            "completed": 0,
            "failed": 0,
            "unknown_age": 0,
            "errors": []
        });
    }
    if let Err(error) = secure::ensure_private_directory(&base) {
        return refusal(format!("cannot inspect {}: {error}", base.display()), "");
    }
    let (modules, modules_truncated) =
        secure::directory_names_bounded(&base, MAX_MODULES).unwrap_or_default();
    let mut candidates = Vec::new();
    let mut examined = 0_usize;
    let mut unknown_age = 0_usize;
    let mut truncated = modules_truncated;
    let mut errors = Vec::new();
    for module_name in modules {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let module = module_name.to_string_lossy();
        if !valid_module(&module) {
            continue;
        }
        let root = base.join(&module_name);
        if secure::verify_private_directory(&root).is_err() {
            push_bin_error(
                &mut errors,
                format!("Cannot inspect module Trash {}", root.display()),
            );
            continue;
        }
        let (entries, entries_truncated) =
            secure::directory_names_bounded(&root, MAX_ENTRIES).unwrap_or_default();
        truncated |= entries_truncated;
        for name in entries {
            if examined >= MAX_ALL_ENTRIES {
                truncated = true;
                break;
            }
            let name_text = name.to_string_lossy();
            if !valid_entry_name(&name_text) {
                continue;
            }
            let entry_dir = root.join(&name);
            examined += 1;
            let Some((manifest, _)) = load_manifest(&entry_dir, Some(&module)).ok().flatten()
            else {
                unknown_age += 1;
                continue;
            };
            match manifest_deletion_epoch(&manifest) {
                Some(deleted_at) if deleted_at <= cutoff => candidates.push(entry_dir),
                Some(_) => {}
                None => unknown_age += 1,
            }
        }
        if examined >= MAX_ALL_ENTRIES {
            truncated = true;
            break;
        }
    }
    let total = candidates.len();
    progress(json!({
        "operation": "artifact-trash-prune",
        "phase": "start",
        "completed": 0,
        "total": total,
        "unknown_age": unknown_age,
        "truncated": truncated
    }));
    let mut completed = 0_usize;
    let mut failed = 0_usize;
    for entry_dir in candidates {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let manifest = load_manifest(&entry_dir, None).ok().flatten();
        let result = match manifest {
            Some((manifest, _))
                if manifest_deletion_epoch(&manifest).is_some_and(|value| value <= cutoff) =>
            {
                discard_entry(&entry_dir, Some(&manifest))
            }
            _ => Err(
                "Trash entry is missing, changed, or has no trustworthy deletion date".to_string(),
            ),
        };
        match result {
            Ok(()) => completed += 1,
            Err(error) => {
                failed += 1;
                push_bin_error(&mut errors, error);
            }
        }
        progress(json!({
            "operation": "artifact-trash-prune",
            "phase": "deleting",
            "completed": completed,
            "failed": failed,
            "total": total
        }));
    }
    let was_cancelled = cancelled.load(Ordering::Relaxed);
    let error = if was_cancelled {
        "operation cancelled".to_string()
    } else if failed > 0 {
        errors
            .first()
            .cloned()
            .unwrap_or_else(|| "Some satellite Trash entries could not be removed".to_string())
    } else if truncated {
        "Satellite Trash scan was truncated; remaining entries were kept".to_string()
    } else {
        String::new()
    };
    json!({
        "ok": !was_cancelled && failed == 0 && !truncated,
        "operation": "artifact-trash-prune",
        "examined": examined,
        "eligible": total,
        "completed": completed,
        "failed": failed,
        "unknown_age": unknown_age,
        "cancelled": was_cancelled,
        "truncated": truncated,
        "errors": errors,
        "error": error
    })
}

pub(super) fn mutate_all(
    cutoff: Option<i64>,
    operation: &str,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(Value),
) -> Value {
    if cancelled.load(Ordering::Relaxed) {
        return refusal("operation cancelled", "");
    }
    let listed = trash_rows(MAX_ALL_ENTRIES, cancelled);
    if listed["ok"] != true {
        return json!({
            "ok": false,
            "operation": operation,
            "completed": 0,
            "failed": 0,
            "cancelled": cancelled.load(Ordering::Relaxed),
            "error": listed["errors"].as_array().and_then(|values| values.first()).and_then(Value::as_str).unwrap_or("Unable to inspect satellite Trash")
        });
    }
    let entries = listed["entries"].as_array().cloned().unwrap_or_default();
    let eligible = entries
        .into_iter()
        .filter(|entry| {
            cutoff.is_none_or(|value| {
                entry["deleted_at_epoch"]
                    .as_i64()
                    .is_some_and(|deleted_at| deleted_at <= value)
            })
        })
        .collect::<Vec<_>>();
    let total = eligible.len();
    progress(json!({"operation": operation, "phase": "start", "completed": 0, "total": total}));
    let mut completed = 0_usize;
    let mut failed = 0_usize;
    let mut errors = Vec::new();
    for entry in eligible {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let module = entry["module"].as_str().unwrap_or("");
        let entry_id = entry["artifact_id"].as_str().unwrap_or("");
        let result = entry_dir_for(module, entry_id).and_then(|entry_dir| {
            let manifest = load_manifest(&entry_dir, Some(module))
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Trash entry is missing or changed".to_string())?
                .0;
            if cutoff.is_some_and(|value| {
                manifest_deletion_epoch(&manifest).is_none_or(|deleted_at| deleted_at > value)
            }) {
                return Err("Trash entry is newer than the retention cutoff".to_string());
            }
            discard_entry(&entry_dir, Some(&manifest))
        });
        match result {
            Ok(()) => completed += 1,
            Err(error) => {
                failed += 1;
                push_bin_error(&mut errors, error);
            }
        }
        progress(
            json!({"operation": operation, "phase": "deleting", "completed": completed, "failed": failed, "total": total}),
        );
    }
    let was_cancelled = cancelled.load(Ordering::Relaxed);
    let truncated = listed["truncated"].as_bool().unwrap_or(false);
    let error = if was_cancelled {
        "operation cancelled".to_string()
    } else if failed > 0 {
        errors.first().cloned().unwrap_or_default()
    } else if truncated {
        "Satellite Trash scan was truncated; remaining entries were kept".to_string()
    } else {
        String::new()
    };
    json!({
        "ok": !was_cancelled && failed == 0 && !truncated,
        "operation": operation,
        "total": total,
        "eligible": total,
        "completed": completed,
        "failed": failed,
        "cancelled": was_cancelled,
        "truncated": truncated,
        "errors": errors,
        "error": error
    })
}

pub(super) fn result(path: &str, ok: bool, changed: bool, message: impl Into<String>) -> ResultRow {
    ResultRow {
        path: path.to_string(),
        ok,
        changed,
        message: message.into(),
    }
}

pub(super) fn document(results: Vec<ResultRow>, entry: &str, payload: Value) -> Value {
    let failures: Vec<_> = results
        .iter()
        .filter(|item| !item.ok)
        .map(|item| {
            if item.path.is_empty() {
                item.message.clone()
            } else {
                format!("{}: {}", item.path, item.message)
            }
        })
        .collect();
    json!({
        "ok": failures.is_empty(),
        "schemaVersion": SCHEMA_VERSION,
        "message": failures.join("; "),
        "results": results,
        "entry": entry,
        "payload": payload,
    })
}

pub(super) fn refusal(message: impl Into<String>, path: &str) -> Value {
    document(
        vec![result(path, false, false, message.into())],
        "",
        Value::Null,
    )
}

pub(super) fn describe_paths(paths: &[String], realpath: &str) -> Result<Inventory, String> {
    let mut inventory = Inventory {
        items: Vec::new(),
        bytes: 0,
    };
    let mut seen = BTreeSet::new();
    let mut roots = Vec::new();
    for (index, raw) in paths.iter().enumerate() {
        if !(raw.starts_with('/') || raw.starts_with("file://")) {
            continue;
        }
        let logical = parse_path(raw).map_err(|error| error.to_string())?;
        let path = std::fs::canonicalize(&logical)
            .map_err(|error| format!("cannot resolve {}: {error}", logical.display()))?;
        if !seen.insert(path.clone()) {
            continue;
        }
        if roots
            .iter()
            .any(|root: &PathBuf| path.starts_with(root) || root.starts_with(&path))
        {
            return Err("paths to bin cannot overlap".to_string());
        }
        roots.push(path.clone());
        describe_root(index, &path, realpath, &mut inventory)?;
    }
    if inventory.items.is_empty() {
        Err("nothing to bin".to_string())
    } else {
        Ok(inventory)
    }
}

pub(super) fn describe_root(
    index: usize,
    path: &Path,
    realpath: &str,
    inventory: &mut Inventory,
) -> Result<(), String> {
    let stored = format!("{ITEMS_DIRECTORY}/{index}");
    let item = describe_entry(path, stored)?;
    if !realpath.is_empty() && item.kind != "dir" && realpath_of(path) != realpath {
        return Err(format!(
            "{} no longer resolves to {realpath}; nothing deleted",
            path.display()
        ));
    }
    inventory.bytes = inventory.bytes.saturating_add(item.size);
    inventory.items.push(item.clone());
    enforce_inventory_bounds(path, inventory)?;
    if item.kind == "dir" {
        protected_directory(path)?;
        walk_tree(path, &item.stored, inventory)?;
    }
    Ok(())
}

pub(super) fn describe_entry(path: &Path, stored: String) -> Result<StoredItem, String> {
    let stat = secure::entry_stat(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!("{} is missing", path.display())
        } else {
            format!("cannot inspect {}: {error}", path.display())
        }
    })?;
    let kind = match stat.kind {
        EntryKind::File => "file",
        EntryKind::Directory => "dir",
        EntryKind::Symlink => "symlink",
        EntryKind::Other => {
            return Err(format!(
                "{} is neither a file, a symlink, nor a directory",
                path.display()
            ));
        }
    };
    if kind == "file" && stat.size > MAX_ITEM_BYTES {
        return Err(format!("{} is larger than the bin allows", path.display()));
    }
    let (target, target_bytes) = if kind == "symlink" {
        let target = secure::read_link(path).map_err(|error| error.to_string())?;
        match target.to_str() {
            Some(text) => (text.to_string(), None),
            None => (
                display_path(&target),
                Some(target.as_os_str().as_bytes().to_vec()),
            ),
        }
    } else {
        (String::new(), None)
    };
    Ok(StoredItem {
        source: path_text(path),
        kind: kind.to_string(),
        target,
        target_bytes,
        mode: stat.mode & 0o777,
        stored,
        size: if kind == "file" { stat.size } else { 0 },
        dev: Some(stat.dev),
        ino: Some(stat.ino),
        atime_ns: Some(nanoseconds(stat.atime, stat.atime_nsec)),
        mtime_ns: Some(nanoseconds(stat.mtime, stat.mtime_nsec)),
    })
}

pub(super) fn walk_tree(
    root: &Path,
    stored_root: &str,
    inventory: &mut Inventory,
) -> Result<(), String> {
    let mut frontier = VecDeque::from([(root.to_path_buf(), stored_root.to_string(), 0_usize)]);
    while let Some((current, stored, depth)) = frontier.pop_front() {
        if depth > MAX_TREE_DEPTH {
            return Err(format!(
                "{} is nested deeper than {MAX_TREE_DEPTH} levels",
                current.display()
            ));
        }
        let (names, truncated) = secure::directory_names_bounded(
            &current,
            MAX_TREE_ENTRIES
                .saturating_sub(inventory.items.len())
                .saturating_add(1),
        )
        .map_err(|error| format!("cannot read {}: {error}", current.display()))?;
        if truncated {
            return Err(format!(
                "{} holds more than {MAX_TREE_ENTRIES} entries",
                root.display()
            ));
        }
        for (index, name) in names.into_iter().enumerate() {
            let child = current.join(&name);
            let item = describe_entry(&child, format!("{stored}/{index}"))?;
            inventory.bytes = inventory.bytes.saturating_add(item.size);
            inventory.items.push(item.clone());
            enforce_inventory_bounds(root, inventory)?;
            if item.kind == "dir" {
                if secure::entry_exists(&child.join(".git")).unwrap_or(false) {
                    return Err(format!(
                        "{} contains .git; a repository is never binned",
                        child.display()
                    ));
                }
                frontier.push_back((child, item.stored, depth + 1));
            }
        }
    }
    Ok(())
}

pub(super) fn enforce_inventory_bounds(root: &Path, inventory: &Inventory) -> Result<(), String> {
    if inventory.items.len() > MAX_TREE_ENTRIES {
        return Err(format!(
            "{} holds more than {MAX_TREE_ENTRIES} entries",
            root.display()
        ));
    }
    if inventory.bytes > MAX_TREE_BYTES {
        return Err(format!("{} is larger than the bin allows", root.display()));
    }
    Ok(())
}

pub(super) fn protected_directory(path: &Path) -> Result<(), String> {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let home = std::fs::canonicalize(expanded_path("~")).unwrap_or_else(|_| expanded_path("~"));
    if resolved == home || resolved == Path::new("/") || resolved.components().count() < 3 {
        return Err(format!(
            "{} is too close to the root of the filesystem to bin",
            path.display()
        ));
    }
    if secure::entry_exists(&path.join(".git")).unwrap_or(false) {
        return Err(format!(
            "{} contains .git; a repository is never binned",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn realpath_of(path: &Path) -> String {
    path_text(&std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
}

pub(super) fn push_bin_error(errors: &mut Vec<String>, error: String) {
    if errors.len() < 64 && !errors.contains(&error) {
        errors.push(truncate(&error, 1024));
    }
}

pub(super) fn bin_row(module: &str, entry_dir: &Path, manifest: &Manifest) -> Value {
    let roots = root_items(manifest);
    let root_paths: Vec<_> = roots.iter().map(|item| item.source.clone()).collect();
    let link_target = manifest_link_target(manifest);
    let stored = manifest
        .items
        .iter()
        .find(|item| item.kind == "file")
        .map(|item| entry_dir.join(&item.stored));
    let metrics = manifest_metrics(manifest, stored.as_deref());
    json!({
        "id": format!("{BIN_PREFIX}{}", entry_dir.file_name().map(|name| name.to_string_lossy()).unwrap_or_default()),
        "sourceId": manifest.id,
        "module": module,
        "name": if manifest.name.is_empty() {
            entry_dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            manifest.name.clone()
        },
        "path": if manifest.path.is_empty() {
            root_paths.first().cloned().unwrap_or_else(|| path_text(entry_dir))
        } else {
            manifest.path.clone()
        },
        "realpath": stored.as_ref().map(|path| path_text(path)).unwrap_or_else(|| path_text(entry_dir)),
        "linkTarget": link_target,
        "aliases": root_paths.into_iter().skip(1).collect::<Vec<_>>(),
        "kind": "bin",
        "scope": manifest.scope,
        "detail": manifest.detail,
        "readers": [],
        "agents": [],
        "badges": ["binned"],
        "metrics": metrics,
        "position": manifest.position,
        "groups": manifest.groups,
        "deletedAt": manifest.deleted_at,
        "originKind": manifest.kind,
        "originScope": manifest.scope,
        "payload": manifest.payload,
    })
}

pub(super) fn artifact_trash_row(module: &str, entry_dir: &Path, manifest: &Manifest) -> Value {
    let entry_name = entry_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let artifact_id = format!("{BIN_PREFIX}{entry_name}");
    let id = format!("artifact:{module}:{entry_name}");
    let roots = root_items(manifest);
    let original_path = if manifest.path.is_empty() {
        roots
            .first()
            .map(|item| item.source.clone())
            .unwrap_or_default()
    } else {
        manifest.path.clone()
    };
    let original_parent = parse_path(&original_path)
        .ok()
        .and_then(|path| path.parent().map(path_text))
        .unwrap_or_default();
    let size = manifest
        .items
        .iter()
        .filter(|item| item.kind != "dir")
        .fold(0_u64, |total, item| total.saturating_add(item.size));
    let is_dir = roots.first().is_some_and(|item| item.kind == "dir")
        || manifest.kind.eq_ignore_ascii_case("directory");
    let deleted_at_epoch = manifest_deletion_epoch(manifest);
    let deleted_at = deleted_at_epoch
        .and_then(|value| Local.timestamp_opt(value, 0).single())
        .map(|value| value.format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_else(|| manifest.deleted_at.clone());
    json!({
        "id": id,
        "artifact_id": artifact_id,
        "source": "satellite",
        "module": module,
        "resource": format!("trash:///artifact/{module}/{entry_name}"),
        "name": if manifest.name.is_empty() { entry_name } else { manifest.name.clone() },
        "original_path": original_path,
        "original_parent": original_parent,
        "deleted_at": deleted_at,
        "deleted_at_epoch": deleted_at_epoch,
        "size": size,
        "size_text": crate::common::human_size(Some(size)),
        "kind": if manifest.kind.is_empty() { format!("{module} · satellite item") } else { format!("{module} · {}", manifest.kind) },
        "mime": if is_dir { "inode/directory" } else { "application/octet-stream" },
        "is_dir": is_dir,
        "is_link": false,
        "metadata_valid": true,
        "emergency": false,
        "can_restore": true,
        "can_restore_to": false,
        "can_delete": true,
        "requires_module_restore": manifest.items.is_empty() && manifest.restore_helper.is_none(),
        "actions": ["restore", "delete_permanently"]
    })
}

pub(super) fn file_metrics(path: Option<&Path>) -> Value {
    let Some(path) = path else {
        return json!({});
    };
    let stat = match secure::entry_stat(path) {
        Ok(stat) if stat.kind == EntryKind::File => stat,
        _ => return json!({}),
    };
    let mut file = match secure::open_file_read(path) {
        Ok(file) => file,
        Err(_) => return json!({}),
    };
    let mut head = Vec::with_capacity(MAX_HEAD_BYTES);
    if Read::by_ref(&mut file)
        .take(MAX_HEAD_BYTES as u64)
        .read_to_end(&mut head)
        .is_err()
    {
        head.clear();
    }
    let complete = stat.size <= MAX_HEAD_BYTES as u64;
    let text = String::from_utf8_lossy(&head);
    let updated = Local
        .timestamp_opt(stat.mtime, stat.mtime_nsec.max(0) as u32)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    json!({
        "updated": updated,
        "created": "",
        "bytes": stat.size,
        "characters": complete.then(|| text.chars().count()),
        "words": complete.then(|| word_count(&text)),
        "tokens": complete.then(|| head.len().div_ceil(4)),
    })
}

pub(super) fn word_count(text: &str) -> usize {
    let mut count = 0;
    let mut inside = false;
    for character in text.chars() {
        let word = character.is_alphanumeric() || character == '_';
        if word && !inside {
            count += 1;
        }
        inside = word;
    }
    count
}

pub(super) fn kind_value(kind: &str) -> EntryKind {
    match kind {
        "file" => EntryKind::File,
        "dir" => EntryKind::Directory,
        "symlink" => EntryKind::Symlink,
        _ => EntryKind::Other,
    }
}
