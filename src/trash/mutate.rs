use super::*;

pub(super) fn restore_entry(
    entry: &TrashEntry,
    destination_directory: &str,
    recreate_parent: bool,
    cancelled: &AtomicBool,
    trash_stores: &[PathBuf],
) -> Value {
    if cancelled.load(Ordering::Relaxed) {
        return mutation_error("trash-restore", "operation cancelled");
    }
    if destination_directory.len() > MAX_PATH_BYTES {
        return mutation_error("trash-restore", "Destination exceeds 64 KiB");
    }
    if !entry_unchanged(entry) {
        return mutation_error("trash-restore", "Trash entry is missing or changed");
    }
    let Some(metadata) = entry.metadata.as_ref() else {
        return mutation_error("trash-restore", "Trash metadata is missing or invalid");
    };
    if entry.stored_stat.is_none() {
        return mutation_error("trash-restore", "Stored Trash content is missing");
    }
    let (destination, parent) = if destination_directory.trim().is_empty() {
        let destination = metadata.original.clone();
        let Some(parent) = destination.parent().map(Path::to_path_buf) else {
            return mutation_error("trash-restore", "Original destination has no parent");
        };
        (destination, parent)
    } else {
        let directory = match parse_path(destination_directory) {
            Ok(path) => path,
            Err(error) => return mutation_error("trash-restore", &error.to_string()),
        };
        let Some(name) = metadata.original.file_name() else {
            return mutation_error("trash-restore", "Original item has no file name");
        };
        (directory.join(name), directory)
    };
    if trash_stores
        .iter()
        .any(|store| destination == *store || destination.starts_with(store))
    {
        return mutation_error("trash-restore", "Destination is inside a Trash store");
    }
    let parent_result = if recreate_parent {
        secure::ensure_directories(&parent, 0o755)
    } else {
        secure::open_directory_nofollow(&parent)
    };
    let parent_directory = match parent_result {
        Ok(directory) => directory,
        Err(error) => {
            return json!({
                "ok": false,
                "operation": "trash-restore",
                "id": entry.id,
                "destination": path_text(&destination),
                "missing_parent": matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory),
                "error": if recreate_parent {
                    bounded_error(&format!("Unable to recreate {}: {error}", parent.display()))
                } else {
                    bounded_error(&format!("The folder {} is missing; recreate it or restore elsewhere", parent.display()))
                }
            });
        }
    };
    let Some(destination_name) = destination.file_name() else {
        return mutation_error("trash-restore", "Destination has no file name");
    };
    let destination_target =
        match secure::resolved_child(&parent_directory, &parent, destination_name) {
            Ok(target) => target,
            Err(error) => return mutation_error("trash-restore", &error.to_string()),
        };
    if secure::entry_exists_resolved(&destination_target).unwrap_or(true) {
        return json!({
            "ok": false,
            "operation": "trash-restore",
            "id": entry.id,
            "destination": path_text(&destination),
            "collision": true,
            "error": "Destination already exists"
        });
    }
    let stored_quarantine = match quarantine_verified(&entry.stored_path, entry.stored_stat, false)
    {
        Ok(path) => path,
        Err(error) => return mutation_error("trash-restore", &error.to_string()),
    };
    let info_quarantine = match quarantine_verified(&entry.info_path, entry.info_stat, true) {
        Ok(path) => path,
        Err(error) => {
            let rollback = rollback_quarantine(&stored_quarantine);
            return mutation_error(
                "trash-restore",
                &with_rollback_error(error.to_string(), rollback),
            );
        }
    };
    let mut ignore_progress = |_: &Path| {};
    if let Err(error) = stored_quarantine.relocate_noreplace_to(
        &destination_target,
        cancelled,
        &mut ignore_progress,
    ) {
        let info_rollback = rollback_quarantine(&info_quarantine);
        let detail = with_rollback_error(error.to_string(), info_rollback);
        return json!({
            "ok": false,
            "operation": "trash-restore",
            "id": entry.id,
            "destination": path_text(&destination),
            "collision": error.kind() == io::ErrorKind::AlreadyExists,
            "cancelled": error.kind() == io::ErrorKind::Interrupted,
            "error": bounded_error(&detail)
        });
    }
    let info_path = info_quarantine.path();
    let info_removed = info_quarantine.remove();
    match info_removed {
        Ok(()) => json!({
            "ok": true,
            "operation": "trash-restore",
            "id": entry.id,
            "destination": path_text(&destination),
            "restored": true,
            "error": ""
        }),
        Err(error) => json!({
            "ok": false,
            "operation": "trash-restore",
            "id": entry.id,
            "destination": path_text(&destination),
            "restored": true,
            "metadata_removed": false,
            "partial": true,
            "error": bounded_error(&with_rollback_error(
                format!("Item was restored but Trash metadata could not be removed: {error}"),
                Some(format!("data may remain at {}", info_path.display()))
            ))
        }),
    }
}

pub(super) fn delete_selected(
    context: &TrashContext,
    ids: &[String],
    cancelled: &AtomicBool,
) -> Value {
    if ids.len() > MAX_SELECTED_ENTRIES {
        return mutation_error(
            "trash-delete",
            "Delete Permanently accepts at most 512 entries",
        );
    }
    if cancelled.load(Ordering::Relaxed) {
        return cancelled_mutation("trash-delete", 0, ids.len());
    }
    let catalog = build_catalog(context, cancelled);
    if catalog.cancelled {
        return cancelled_mutation("trash-delete", 0, ids.len());
    }
    let entries = catalog
        .entries
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut completed = 0_usize;
    let mut failed = 0_usize;
    let mut results = Vec::new();
    let mut result_bytes = 0_usize;
    let mut errors = Vec::new();
    let mut was_cancelled = false;
    for id in ids {
        if cancelled.load(Ordering::Relaxed) {
            was_cancelled = true;
            break;
        }
        let result = if id.is_empty() || id.len() > MAX_ID_BYTES {
            json!({"ok": false, "id": id, "error": "Invalid Trash entry identity"})
        } else if !seen.insert(id.clone()) {
            json!({"ok": false, "id": id, "error": "Duplicate Trash entry identity"})
        } else if let Some(entry) = entries.get(id) {
            delete_entry(entry)
        } else {
            json!({"ok": false, "id": id, "error": "Trash entry is missing or changed"})
        };
        if result["ok"].as_bool().unwrap_or(false) {
            completed += 1;
        } else {
            failed += 1;
            if let Some(error) = result["error"].as_str() {
                push_error(&mut errors, error);
            }
        }
        push_mutation_result(&mut results, &mut result_bytes, result);
    }
    let error = mutation_summary_error(was_cancelled, failed, false, &errors);
    let results_truncated = completed.saturating_add(failed) > results.len();
    json!({
        "ok": !was_cancelled && failed == 0,
        "operation": "trash-delete",
        "requested": ids.len(),
        "completed": completed,
        "failed": failed,
        "cancelled": was_cancelled,
        "partial": completed > 0 && (failed > 0 || was_cancelled),
        "results": results,
        "results_truncated": results_truncated,
        "errors": errors,
        "error": error
    })
}

pub(super) fn empty_context(
    context: &TrashContext,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(Value),
) -> Value {
    if cancelled.load(Ordering::Relaxed) {
        return cancelled_mutation("trash-empty", 0, 0);
    }
    let catalog = build_catalog(context, cancelled);
    if catalog.cancelled {
        return cancelled_mutation("trash-empty", 0, catalog.entries.len());
    }
    let total = catalog.entries.len();
    let estimated_size = catalog
        .entries
        .iter()
        .filter_map(|entry| entry.size)
        .fold(0_u64, u64::saturating_add);
    progress(json!({
        "operation": "trash-empty",
        "phase": "start",
        "completed": 0,
        "total": total,
        "estimated_size": estimated_size,
        "truncated": catalog.truncated
    }));
    let mut completed = 0_usize;
    let mut failed = 0_usize;
    let mut results = Vec::new();
    let mut result_bytes = 0_usize;
    let mut errors = catalog.errors;
    let mut was_cancelled = false;
    for entry in &catalog.entries {
        if cancelled.load(Ordering::Relaxed) {
            was_cancelled = true;
            break;
        }
        progress(json!({
            "operation": "trash-empty",
            "phase": "deleting",
            "completed": completed,
            "failed": failed,
            "total": total,
            "id": entry.id,
            "name": display_name(entry)
        }));
        let result = delete_entry(entry);
        if result["ok"].as_bool().unwrap_or(false) {
            completed += 1;
        } else {
            failed += 1;
            if let Some(error) = result["error"].as_str() {
                push_error(&mut errors, error);
            }
        }
        push_mutation_result(&mut results, &mut result_bytes, result);
    }
    let incomplete = catalog.truncated;
    let error = mutation_summary_error(was_cancelled, failed, incomplete, &errors);
    let results_truncated = completed.saturating_add(failed) > results.len();
    progress(json!({
        "operation": "trash-empty",
        "phase": "complete",
        "completed": completed,
        "failed": failed,
        "total": total,
        "cancelled": was_cancelled,
        "truncated": incomplete
    }));
    json!({
        "ok": !was_cancelled && failed == 0 && !incomplete,
        "operation": "trash-empty",
        "total": total,
        "completed": completed,
        "failed": failed,
        "estimated_size": estimated_size,
        "cancelled": was_cancelled,
        "truncated": incomplete,
        "partial": completed > 0 && (failed > 0 || was_cancelled || incomplete),
        "results": results,
        "results_truncated": results_truncated,
        "errors": errors,
        "error": error
    })
}

pub(super) fn prune_context(
    context: &TrashContext,
    cutoff: NaiveDateTime,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(Value),
) -> Value {
    if cancelled.load(Ordering::Relaxed) {
        return cancelled_mutation("trash-prune", 0, 0);
    }
    let catalog = build_catalog(context, cancelled);
    if catalog.cancelled {
        return cancelled_mutation("trash-prune", 0, catalog.entries.len());
    }
    let eligible = catalog
        .entries
        .iter()
        .filter(|entry| {
            entry.metadata.as_ref().is_some_and(|metadata| {
                NaiveDateTime::parse_from_str(&metadata.deleted_at, "%Y-%m-%dT%H:%M:%S")
                    .is_ok_and(|deleted_at| deleted_at <= cutoff)
            })
        })
        .collect::<Vec<_>>();
    let total = eligible.len();
    let unknown_age = catalog
        .entries
        .iter()
        .filter(|entry| {
            entry.metadata.as_ref().is_none_or(|metadata| {
                NaiveDateTime::parse_from_str(&metadata.deleted_at, "%Y-%m-%dT%H:%M:%S").is_err()
            })
        })
        .count();
    progress(json!({
        "operation": "trash-prune",
        "phase": "start",
        "completed": 0,
        "total": total,
        "unknown_age": unknown_age,
        "truncated": catalog.truncated
    }));
    let mut completed = 0_usize;
    let mut failed = 0_usize;
    let mut results = Vec::new();
    let mut result_bytes = 0_usize;
    let mut errors = catalog.errors;
    let mut was_cancelled = false;
    for entry in eligible {
        if cancelled.load(Ordering::Relaxed) {
            was_cancelled = true;
            break;
        }
        progress(json!({
            "operation": "trash-prune",
            "phase": "deleting",
            "completed": completed,
            "failed": failed,
            "total": total,
            "id": entry.id,
            "name": display_name(entry)
        }));
        let result = delete_entry(entry);
        if result["ok"].as_bool().unwrap_or(false) {
            completed += 1;
        } else {
            failed += 1;
            if let Some(error) = result["error"].as_str() {
                push_error(&mut errors, error);
            }
        }
        push_mutation_result(&mut results, &mut result_bytes, result);
    }
    let incomplete = catalog.truncated;
    let error = mutation_summary_error(was_cancelled, failed, incomplete, &errors);
    let results_truncated = completed.saturating_add(failed) > results.len();
    progress(json!({
        "operation": "trash-prune",
        "phase": "complete",
        "completed": completed,
        "failed": failed,
        "total": total,
        "cancelled": was_cancelled,
        "truncated": incomplete
    }));
    json!({
        "ok": !was_cancelled && failed == 0 && !incomplete,
        "operation": "trash-prune",
        "examined": catalog.entries.len(),
        "eligible": total,
        "completed": completed,
        "failed": failed,
        "unknown_age": unknown_age,
        "cancelled": was_cancelled,
        "truncated": incomplete,
        "partial": completed > 0 && (failed > 0 || was_cancelled || incomplete),
        "results": results,
        "results_truncated": results_truncated,
        "errors": errors,
        "error": error
    })
}

pub(super) fn delete_entry(entry: &TrashEntry) -> Value {
    if !entry_unchanged(entry) {
        return json!({
            "ok": false,
            "id": entry.id,
            "name": display_name(entry),
            "error": "Trash entry is missing or changed"
        });
    }
    let content_quarantine = match entry.stored_stat {
        Some(stat) => match quarantine_verified(&entry.stored_path, Some(stat), false) {
            Ok(path) => Some(path),
            Err(error) => {
                return delete_error(entry, false, false, false, &error.to_string());
            }
        },
        None => None,
    };
    let metadata_quarantine = match entry.info_stat {
        Some(stat) => match quarantine_verified(&entry.info_path, Some(stat), true) {
            Ok(path) => Some(path),
            Err(error) => {
                let rollback = content_quarantine.as_ref().and_then(rollback_quarantine);
                let partial = rollback.is_some();
                return delete_error(
                    entry,
                    false,
                    false,
                    partial,
                    &with_rollback_error(error.to_string(), rollback),
                );
            }
        },
        None => None,
    };
    let mut content_removed = content_quarantine.is_none();
    let mut metadata_removed = metadata_quarantine.is_none();
    if let Some(quarantine) = content_quarantine {
        let path = quarantine.path();
        if let Err(error) = quarantine.remove() {
            let metadata_rollback = metadata_quarantine.as_ref().and_then(rollback_quarantine);
            let detail = with_rollback_error(
                format!("{error}; data may remain at {}", path.display()),
                metadata_rollback,
            );
            return delete_error(entry, false, false, true, &detail);
        }
        content_removed = true;
    }
    if let Some(quarantine) = metadata_quarantine {
        let path = quarantine.path();
        if let Err(error) = quarantine.remove() {
            return delete_error(
                entry,
                content_removed,
                false,
                true,
                &format!(
                    "Content was deleted but Trash metadata could not be removed: {error}; data may remain at {}",
                    path.display()
                ),
            );
        }
        metadata_removed = true;
    }
    json!({
        "ok": true,
        "id": entry.id,
        "name": display_name(entry),
        "content_removed": content_removed,
        "metadata_removed": metadata_removed,
        "error": ""
    })
}

pub(super) fn delete_error(
    entry: &TrashEntry,
    content_removed: bool,
    metadata_removed: bool,
    partial: bool,
    error: &str,
) -> Value {
    json!({
        "ok": false,
        "id": entry.id,
        "name": display_name(entry),
        "content_removed": content_removed,
        "metadata_removed": metadata_removed,
        "partial": partial,
        "error": bounded_error(error)
    })
}

pub(super) fn quarantine_verified(
    path: &Path,
    expected: Option<EntryStat>,
    compare_time: bool,
) -> io::Result<secure::QuarantinedEntry> {
    let quarantine = secure::quarantine_path(path)?;
    let current = Some(quarantine.stat());
    let unchanged = if compare_time {
        same_identity_and_time(expected, current)
    } else {
        same_entry_identity(expected, current)
    };
    if unchanged {
        return Ok(quarantine);
    }
    let rollback = rollback_quarantine(&quarantine);
    Err(io::Error::other(with_rollback_error(
        "Trash entry changed before it could be secured".to_string(),
        rollback,
    )))
}

pub(super) fn rollback_quarantine(quarantine: &secure::QuarantinedEntry) -> Option<String> {
    quarantine.restore().err().map(|error| {
        format!(
            "data remains at {} because rollback failed: {error}",
            quarantine.path().display()
        )
    })
}

pub(super) fn with_rollback_error(message: String, rollback: Option<String>) -> String {
    match rollback {
        Some(rollback) => format!("{message}; {rollback}"),
        None => message,
    }
}

pub(super) fn entry_unchanged(entry: &TrashEntry) -> bool {
    same_identity_and_time(entry.info_stat, optional_stat(&entry.info_path))
        && same_entry_identity(entry.stored_stat, optional_stat(&entry.stored_path))
}

pub(super) fn push_mutation_result(results: &mut Vec<Value>, bytes: &mut usize, result: Value) {
    if results.len() >= MAX_MUTATION_RESULTS {
        return;
    }
    let size = serde_json::to_vec(&result)
        .map(|value| value.len())
        .unwrap_or(MAX_MUTATION_RESULT_BYTES);
    if bytes.saturating_add(size) > MAX_MUTATION_RESULT_BYTES {
        return;
    }
    *bytes = bytes.saturating_add(size);
    results.push(result);
}

pub(super) fn mutation_summary_error(
    cancelled: bool,
    failed: usize,
    truncated: bool,
    errors: &[String],
) -> String {
    if cancelled {
        return "operation cancelled".to_string();
    }
    if truncated {
        return format!(
            "Trash contains more than {MAX_CANDIDATES} bounded entries; run Empty Trash again"
        );
    }
    if failed > 0 {
        return errors
            .first()
            .cloned()
            .unwrap_or_else(|| format!("{failed} Trash entries could not be deleted"));
    }
    String::new()
}

pub(super) fn mutation_error(operation: &str, error: &str) -> Value {
    json!({
        "ok": false,
        "operation": operation,
        "completed": 0,
        "failed": 0,
        "cancelled": error == "operation cancelled",
        "partial": false,
        "error": bounded_error(error)
    })
}

pub(super) fn cancelled_mutation(operation: &str, completed: usize, total: usize) -> Value {
    json!({
        "ok": false,
        "operation": operation,
        "total": total,
        "completed": completed,
        "failed": 0,
        "cancelled": true,
        "partial": completed > 0,
        "error": "operation cancelled"
    })
}
