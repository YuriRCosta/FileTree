use super::*;

pub fn undo(drop: bool, force: bool, cancelled: &AtomicBool) -> Value {
    step("undo", drop, force, cancelled)
}

pub fn redo(drop: bool, force: bool, cancelled: &AtomicBool) -> Value {
    step("redo", drop, force, cancelled)
}

pub(super) fn step(direction: &str, drop: bool, force: bool, cancelled: &AtomicBool) -> Value {
    let mut journal = match JournalStore::open() {
        Ok(journal) => journal,
        Err(error) => return step_error(direction, error.to_string()),
    };
    let source_is_undo = direction == "undo";
    let entry = if source_is_undo {
        journal.data.undo.pop()
    } else {
        journal.data.redo.pop()
    };
    let Some(mut entry) = entry else {
        let message = if source_is_undo {
            "Nothing to undo"
        } else {
            "Nothing to redo"
        };
        return json!({
            "ok": false,
            "empty": true,
            "operation": direction,
            "error": message,
            "paths": [],
            "mappings": [],
        });
    };
    if drop {
        let value = summary(&entry);
        if let Err(error) = journal.save() {
            return step_error(direction, error.to_string());
        }
        return json!({
            "ok": true,
            "operation": "drop",
            "dropped": direction,
            "entry": value,
            "paths": [],
            "mappings": [],
        });
    }
    journal.data.interrupted = Some(entry.clone());
    if let Err(error) = journal.save() {
        return step_error(direction, error.to_string());
    }
    let applied = if source_is_undo {
        reverse_entry(&mut entry, force, cancelled)
    } else {
        forward_entry(&mut entry, cancelled)
    };
    journal.data.interrupted = None;
    let mut result = match applied {
        Ok(result) => {
            if result.get("ok").and_then(Value::as_bool) == Some(true) {
                if entry.kind == "transfer" {
                    entry.items.reverse();
                }
                target_stack(&mut journal.data, source_is_undo).push(entry.clone());
            } else {
                source_stack(&mut journal.data, source_is_undo).push(entry.clone());
            }
            result
        }
        Err(ApplyFailure::Io(error)) => {
            source_stack(&mut journal.data, source_is_undo).push(entry.clone());
            step_error(direction, error.to_string())
        }
        Err(ApplyFailure::Partial { done, error }) => {
            if done > 0 {
                let remaining = entry.items.split_off(done);
                let mut completed = JournalEntry {
                    items: entry.items.clone(),
                    ..entry.clone()
                };
                if completed.kind == "transfer" {
                    completed.items.reverse();
                }
                target_stack(&mut journal.data, source_is_undo).push(completed);
                entry.items = remaining;
                entry.id = format!("{}-rest", entry.id);
                entry.label = entry_label(&entry.kind, &entry.items);
            }
            source_stack(&mut journal.data, source_is_undo).push(entry.clone());
            json!({
                "ok": false,
                "partial": done > 0,
                "operation": direction,
                "error": error.to_string(),
                "paths": [],
                "mappings": [],
            })
        }
    };
    if let Err(error) = journal.save() {
        return step_error(direction, error.to_string());
    }
    let object = result.as_object_mut().unwrap_or_else(|| unreachable!());
    object.insert(direction.to_string(), Value::Bool(true));
    object.insert("entry".to_string(), summary(&entry));
    object.insert("label".to_string(), Value::String(entry.label.clone()));
    result
}

pub(super) fn source_stack(data: &mut JournalData, source_is_undo: bool) -> &mut Vec<JournalEntry> {
    if source_is_undo {
        &mut data.undo
    } else {
        &mut data.redo
    }
}

pub(super) fn target_stack(data: &mut JournalData, source_is_undo: bool) -> &mut Vec<JournalEntry> {
    if source_is_undo {
        &mut data.redo
    } else {
        &mut data.undo
    }
}

pub(super) fn reverse_entry(
    entry: &mut JournalEntry,
    force: bool,
    cancelled: &AtomicBool,
) -> Result<Value, ApplyFailure> {
    match entry.kind.as_str() {
        "transfer" => apply_compound(entry, true, force, cancelled),
        "permissions" => apply_permissions(entry, true, cancelled),
        "copy" | "create" => apply_removal(entry, force, cancelled),
        "move" | "rename" => apply_relocation(entry, true, cancelled),
        "trash" => apply_restore(entry, "source", "undo", "restore", cancelled),
        "color" => Ok(color_result(&entry.items, true)),
        kind => Ok(refusal(
            "undo",
            entry,
            format!("Unknown journal entry kind {kind}"),
        )),
    }
}

pub(super) fn forward_entry(
    entry: &mut JournalEntry,
    cancelled: &AtomicBool,
) -> Result<Value, ApplyFailure> {
    match entry.kind.as_str() {
        "transfer" => apply_compound(entry, false, false, cancelled),
        "permissions" => apply_permissions(entry, false, cancelled),
        "copy" | "create" => {
            let kind = entry.kind.clone();
            apply_restore(entry, "target", "redo", &kind, cancelled)
        }
        "move" | "rename" => apply_relocation(entry, false, cancelled),
        "trash" => apply_trash_again(entry, cancelled),
        "color" => Ok(color_result(&entry.items, false)),
        kind => Ok(refusal(
            "redo",
            entry,
            format!("Unknown journal entry kind {kind}"),
        )),
    }
}

pub(super) fn color_result(items: &[JournalItem], reverse: bool) -> Value {
    let paths: Vec<_> = items.iter().map(|item| item.target.clone()).collect();
    let colors: Vec<_> = items
        .iter()
        .map(|item| {
            json!({
                "path": item.target,
                "value": if reverse { &item.before_value } else { &item.after_value },
            })
        })
        .collect();
    json!({
        "ok": true,
        "operation": "color",
        "path": paths.first().cloned().unwrap_or_default(),
        "paths": paths,
        "mappings": [],
        "colors": colors,
    })
}

pub(super) fn apply_removal(
    entry: &mut JournalEntry,
    force: bool,
    cancelled: &AtomicBool,
) -> Result<Value, ApplyFailure> {
    let targets: Vec<_> = entry.items.iter().map(|item| item.target.clone()).collect();
    for item in &mut entry.items {
        let target = parse_path(&item.target).map_err(|error| partial(0, error))?;
        if let Some(error) = check_removal(item, &target, force) {
            return Ok(refusal("undo", entry, error));
        }
    }
    run_items(&mut entry.items, |item| {
        check_cancelled(cancelled)?;
        let target = parse_path(&item.target)?;
        trash_item(item, &target, cancelled)
    })?;
    let unverified: Vec<_> = entry
        .items
        .iter_mut()
        .filter_map(|item| {
            let value = item.unverified.then(|| item.target.clone());
            item.unverified = false;
            value
        })
        .collect();
    Ok(json!({
        "ok": true,
        "operation": "trash",
        "paths": targets,
        "mappings": [],
        "unverified": unverified,
    }))
}

pub(super) fn apply_restore(
    entry: &mut JournalEntry,
    field: &str,
    direction: &str,
    operation: &str,
    cancelled: &AtomicBool,
) -> Result<Value, ApplyFailure> {
    let destinations: Vec<_> = entry
        .items
        .iter()
        .map(|item| {
            if field == "source" {
                item.source.clone()
            } else {
                item.target.clone()
            }
        })
        .collect();
    let mut targets = Vec::with_capacity(entry.items.len());
    for (item, destination) in entry.items.iter().zip(&destinations) {
        let destination = parse_path(destination).map_err(|error| partial(0, error))?;
        if let Some(error) = check_restore(item, &destination) {
            return Ok(refusal(direction, entry, error));
        }
        match resolved_destination(&destination) {
            Ok(target) => targets.push(target),
            Err(error) => return Ok(refusal(direction, entry, error)),
        }
    }
    let mut index = 0;
    while index < entry.items.len() {
        check_cancelled(cancelled).map_err(|error| partial(index, error))?;
        if let Err(error) =
            restore_from_trash_resolved(&mut entry.items[index], &targets[index], cancelled)
        {
            return Err(partial(index, error));
        }
        index += 1;
    }
    Ok(json!({
        "ok": true,
        "operation": operation,
        "paths": destinations,
        "mappings": [],
        "path": destinations.first().cloned().unwrap_or_default(),
    }))
}

pub(super) fn apply_relocation(
    entry: &mut JournalEntry,
    reverse: bool,
    cancelled: &AtomicBool,
) -> Result<Value, ApplyFailure> {
    let mut targets = Vec::with_capacity(entry.items.len());
    for item in &entry.items {
        let (origin, destination) = relocation_pair(item, reverse);
        let origin = parse_path(origin).map_err(|error| partial(0, error))?;
        let destination = parse_path(destination).map_err(|error| partial(0, error))?;
        if let Some(error) = check_relocation(item, &origin) {
            return Ok(refusal(if reverse { "undo" } else { "redo" }, entry, error));
        }
        match resolved_destination(&destination) {
            Ok(target) => targets.push(target),
            Err(error) => {
                return Ok(refusal(if reverse { "undo" } else { "redo" }, entry, error));
            }
        }
    }
    let mut mappings = Vec::new();
    let mut index = 0;
    while index < entry.items.len() {
        check_cancelled(cancelled).map_err(|error| partial(index, error))?;
        let (origin, destination) = relocation_pair(&entry.items[index], reverse);
        let origin = origin.to_string();
        let destination = destination.to_string();
        let mut ignored: fn(&Path) = ignore_path;
        let Some(expected) = entry.items[index]
            .fingerprint
            .as_ref()
            .and_then(fingerprint_identity)
        else {
            return Err(partial(
                index,
                io::Error::other("operation identity is missing"),
            ));
        };
        let source_path = parse_path(&origin).map_err(|error| partial(index, error))?;
        let source =
            secure::resolved_parent(&source_path).map_err(|error| partial(index, error))?;
        if let Err(error) = secure::relocate_noreplace_matching_resolved(
            source,
            &targets[index],
            expected,
            cancelled,
            &mut ignored,
        ) {
            return Err(partial(index, error));
        }
        entry.items[index].fingerprint =
            fingerprint(&parse_path(&destination).map_err(|error| partial(index, error))?);
        mappings.push(json!({"source": origin, "destination": destination}));
        index += 1;
    }
    let paths: Vec<_> = mappings
        .iter()
        .filter_map(|mapping| mapping.get("destination").cloned())
        .collect();
    Ok(json!({
        "ok": true,
        "operation": entry.kind,
        "path": paths.first().cloned().unwrap_or(Value::String(String::new())),
        "paths": paths,
        "mappings": mappings,
    }))
}

pub(super) fn apply_trash_again(
    entry: &mut JournalEntry,
    cancelled: &AtomicBool,
) -> Result<Value, ApplyFailure> {
    let sources: Vec<_> = entry.items.iter().map(|item| item.source.clone()).collect();
    for item in &entry.items {
        let path = parse_path(&item.source).map_err(|error| partial(0, error))?;
        if !secure::entry_exists(&path).unwrap_or(false) {
            return Ok(refusal(
                "redo",
                entry,
                format!("{} no longer exists", item.source),
            ));
        }
        if !same_identity(item.fingerprint.as_ref(), &path) {
            return Ok(refusal(
                "redo",
                entry,
                format!("{} was replaced by another item", item.source),
            ));
        }
    }
    run_items(&mut entry.items, |item| {
        check_cancelled(cancelled)?;
        let source = parse_path(&item.source)?;
        trash_item(item, &source, cancelled)
    })?;
    Ok(json!({"ok": true, "operation": "trash", "paths": sources, "mappings": []}))
}

pub(super) fn run_items(
    items: &mut [JournalItem],
    mut action: impl FnMut(&mut JournalItem) -> io::Result<()>,
) -> Result<(), ApplyFailure> {
    for (index, item) in items.iter_mut().enumerate() {
        if let Err(error) = action(item) {
            return Err(partial(index, error));
        }
    }
    Ok(())
}

pub(super) fn partial(done: usize, error: io::Error) -> ApplyFailure {
    if done == 0 {
        ApplyFailure::Io(error)
    } else {
        ApplyFailure::Partial { done, error }
    }
}

pub(super) fn refusal(direction: &str, entry: &JournalEntry, error: String) -> Value {
    json!({
        "ok": false,
        "refused": true,
        "operation": direction,
        "entry": summary(entry),
        "error": error,
    })
}

pub(super) fn step_error(direction: &str, error: String) -> Value {
    json!({
        "ok": false,
        "operation": direction,
        "error": error,
        "paths": [],
        "mappings": [],
    })
}

pub(super) fn check_removal(item: &mut JournalItem, path: &Path, force: bool) -> Option<String> {
    if !secure::entry_exists(path).unwrap_or(false) {
        return Some(format!("{} no longer exists", path.display()));
    }
    if !same_identity(item.fingerprint.as_ref(), path) {
        return Some(format!("{} was replaced by another item", path.display()));
    }
    if force {
        return None;
    }
    let (intact, unverified) = unchanged(item.fingerprint.as_ref(), path);
    item.unverified = unverified;
    if unverified {
        return Some(format!(
            "{} could not be fully checked for changes; explicit confirmation is required",
            display_name(&path_text(path))
        ));
    }
    (!intact).then(|| {
        format!(
            "{} was modified after the operation",
            display_name(&path_text(path))
        )
    })
}

pub(super) fn check_restore(item: &JournalItem, destination: &Path) -> Option<String> {
    if item.trash_name.is_empty() {
        return Some(format!(
            "{} was not recorded in Trash",
            display_name(&path_text(destination))
        ));
    }
    let stored = match stored_trash_path(item) {
        Ok(path) => path,
        Err(error) => return Some(error.to_string()),
    };
    if !secure::entry_exists(&stored).unwrap_or(false) {
        return Some(format!(
            "{} is no longer in Trash",
            display_name(&path_text(destination))
        ));
    }
    if !same_identity(item.fingerprint.as_ref(), &stored) {
        return Some(format!(
            "The Trash entry for {} was replaced",
            display_name(&path_text(destination))
        ));
    }
    None
}

pub(super) fn check_relocation(item: &JournalItem, origin: &Path) -> Option<String> {
    if !secure::entry_exists(origin).unwrap_or(false) {
        return Some(format!("{} no longer exists", origin.display()));
    }
    if !same_identity(item.fingerprint.as_ref(), origin) {
        return Some(format!("{} was replaced by another item", origin.display()));
    }
    None
}

pub(super) fn resolved_destination(destination: &Path) -> Result<secure::ResolvedParent, String> {
    let target = secure::resolved_parent(destination).map_err(|_| {
        let parent = destination.parent().unwrap_or(Path::new("/"));
        format!(
            "The folder {} no longer exists; recreate it and retry",
            parent.display()
        )
    })?;
    if secure::entry_exists_resolved(&target).unwrap_or(true) {
        return Err(format!(
            "{} is now occupied by another item",
            destination.display()
        ));
    }
    Ok(target)
}

pub(super) fn relocation_pair(item: &JournalItem, reverse: bool) -> (&str, &str) {
    if reverse {
        (&item.target, &item.source)
    } else {
        (&item.source, &item.target)
    }
}

pub(super) fn stored_trash_path(item: &JournalItem) -> io::Result<PathBuf> {
    if !valid_path(&item.trash_dir) {
        return Err(io::Error::other("Invalid Trash directory"));
    }
    let directory = parse_path(&item.trash_dir)?.join("files");
    let path = if item.trash_name.starts_with("file://") {
        parse_path(&item.trash_name)?
    } else {
        directory.join(&item.trash_name)
    };
    if path.parent() != Some(directory.as_path()) || path.file_name().is_none() {
        return Err(io::Error::other("Invalid Trash entry name"));
    }
    Ok(path)
}

pub(super) fn trash_info_path(item: &JournalItem) -> io::Result<PathBuf> {
    let stored = stored_trash_path(item)?;
    let mut name = stored.file_name().unwrap().to_os_string();
    name.push(".trashinfo");
    Ok(parse_path(&item.trash_dir)?.join("info").join(name))
}
