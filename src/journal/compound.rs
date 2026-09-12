use super::*;

pub fn compound_transfer_item(copy: bool, mapping: &TransferMapping) -> JournalItem {
    let mut item = transfer_item(mapping);
    if copy {
        item.source.clear();
    }
    item
}

pub fn record_compound(items: &[JournalItem], entry_id: &str) -> AppResult<String> {
    if items.iter().any(|item| !valid_compound_item(item)) {
        return Err(AppError::invalid("invalid compound transfer recovery item"));
    }
    record_items("transfer", items.iter().rev().cloned().collect(), entry_id)
}

pub(super) fn valid_compound_item(item: &JournalItem) -> bool {
    match action(item) {
        Some(kind) => {
            valid_item(kind, item) && item.before_value.is_empty() && item.after_value.is_empty()
        }
        None => false,
    }
}

fn action(item: &JournalItem) -> Option<&'static str> {
    match (item.source.is_empty(), item.target.is_empty()) {
        (true, false) => Some("create"),
        (false, true) => Some("trash"),
        (false, false) if item.source != item.target => Some("move"),
        _ => None,
    }
}

pub(super) fn apply_compound(
    entry: &mut JournalEntry,
    reverse: bool,
    force: bool,
    cancelled: &AtomicBool,
) -> Result<Value, ApplyFailure> {
    let mut paths = Vec::new();
    let mut mappings = Vec::new();
    for (index, item) in entry.items.iter_mut().enumerate() {
        check_cancelled(cancelled).map_err(|error| partial(index, error))?;
        let kind = action(item)
            .ok_or_else(|| partial(index, io::Error::other("invalid compound transfer action")))?;
        if !reverse && kind == "trash" {
            let path = parse_path(&item.source).map_err(|error| partial(index, error))?;
            let was_empty = item.fingerprint.as_ref().is_some_and(|value| {
                value.kind == "dir"
                    && value
                        .tree
                        .as_ref()
                        .is_some_and(|tree| !tree.truncated && tree.count == 0)
            });
            if was_empty {
                if !same_identity(item.fingerprint.as_ref(), &path)
                    || !secure::directory_names(&path)
                        .map_err(|error| partial(index, error))?
                        .is_empty()
                {
                    return Err(partial(
                        index,
                        io::Error::other("restored directory changed after undo"),
                    ));
                }
            } else if let Some(error) = check_removal(item, &path, false) {
                return Err(partial(index, io::Error::other(error)));
            }
        }
        let mut step = JournalEntry {
            id: entry.id.clone(),
            kind: kind.into(),
            label: entry.label.clone(),
            at: entry.at.clone(),
            items: vec![item.clone()],
        };
        let result = if reverse {
            reverse_entry(&mut step, force, cancelled)
        } else {
            forward_entry(&mut step, cancelled)
        };
        *item = step.items.remove(0);
        match result {
            Ok(result) if result["ok"] == true => {
                if let Some(values) = result["paths"].as_array() {
                    paths.extend(values.iter().cloned());
                }
                if let Some(values) = result["mappings"].as_array() {
                    mappings.extend(values.iter().cloned());
                }
            }
            Ok(result) => {
                return Err(partial(
                    index,
                    io::Error::other(
                        result["error"]
                            .as_str()
                            .unwrap_or("compound transfer step refused"),
                    ),
                ));
            }
            Err(ApplyFailure::Io(error)) => return Err(partial(index, error)),
            Err(ApplyFailure::Partial { done, error }) => return Err(partial(index + done, error)),
        }
    }
    Ok(json!({"ok": true, "operation": "transfer", "paths": paths, "mappings": mappings}))
}
