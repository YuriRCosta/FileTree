use super::*;

pub fn record_permissions(
    path: &Path,
    before: secure::EntryStat,
    after: secure::EntryStat,
    entry_id: &str,
) -> AppResult<String> {
    let item = JournalItem {
        source: String::new(),
        target: path_text(path),
        is_dir: after.kind == EntryKind::Directory,
        trash_dir: String::new(),
        trash_name: String::new(),
        before_value: format!("{:03o}", before.mode & 0o7777),
        after_value: format!("{:03o}", after.mode & 0o7777),
        fingerprint: Some(permission_fingerprint(after)),
        unverified: false,
    };
    let mut journal = JournalStore::open()?;
    let id = if entry_id.is_empty() {
        new_entry_id()
    } else {
        entry_id.into()
    };
    if let Some(entry) = journal.data.undo.iter_mut().find(|entry| entry.id == id) {
        if entry.kind != "permissions" {
            return Err(AppError::invalid(
                "journal identity is used by another operation",
            ));
        }
        entry.items.push(item);
        entry.label = entry_label("permissions", &entry.items);
    } else {
        let items = vec![item];
        journal.data.undo.push(JournalEntry {
            id: id.clone(),
            kind: "permissions".into(),
            label: entry_label("permissions", &items),
            at: now_text(),
            items,
        });
        journal.data.redo.clear();
    }
    journal.save()?;
    Ok(id)
}

fn permission_fingerprint(stat: secure::EntryStat) -> Fingerprint {
    Fingerprint {
        dev: stat.dev,
        ino: stat.ino,
        size: stat.size,
        mtime_ns: stat
            .mtime
            .saturating_mul(1_000_000_000)
            .saturating_add(stat.mtime_nsec),
        ctime_ns: Some(
            stat.ctime
                .saturating_mul(1_000_000_000)
                .saturating_add(stat.ctime_nsec),
        ),
        kind: if stat.kind == EntryKind::Directory {
            "dir"
        } else {
            "file"
        }
        .into(),
        tree: None,
    }
}

pub(super) fn valid_permissions(value: &str) -> bool {
    (value.len() == 3 || value.len() == 4) && value.bytes().all(|byte| matches!(byte, b'0'..=b'7'))
}

pub(super) fn apply_permissions(
    entry: &mut JournalEntry,
    reverse: bool,
    cancelled: &AtomicBool,
) -> Result<Value, ApplyFailure> {
    let mut planned = Vec::new();
    for item in &entry.items {
        let path = parse_path(&item.target).map_err(|error| partial(0, error))?;
        let stat = secure::entry_stat_resolved(
            &secure::resolved_parent(&path).map_err(|error| partial(0, error))?,
        )
        .map_err(|error| partial(0, error))?;
        let expected_mode = u32::from_str_radix(
            if reverse {
                &item.after_value
            } else {
                &item.before_value
            },
            8,
        )
        .map_err(|error| partial(0, io::Error::other(error)))?;
        if item.fingerprint.as_ref().and_then(fingerprint_identity) != Some(stat.identity())
            || stat.mode & 0o7777 != expected_mode
        {
            return Ok(refusal(
                if reverse { "undo" } else { "redo" },
                entry,
                "item identity or permissions changed after the operation".into(),
            ));
        }
        let mode = u32::from_str_radix(
            if reverse {
                &item.before_value
            } else {
                &item.after_value
            },
            8,
        )
        .map_err(|error| partial(0, io::Error::other(error)))?;
        planned.push((path, stat, mode));
    }
    for (index, (path, stat, mode)) in planned.into_iter().enumerate() {
        check_cancelled(cancelled).map_err(|error| partial(index, error))?;
        let after = secure::set_permissions_matching(&path, stat, mode)
            .map_err(|error| partial(index, error))?;
        entry.items[index].fingerprint = Some(permission_fingerprint(after));
    }
    Ok(
        json!({"ok":true,"operation":"permissions","paths":entry.items.iter().map(|item| &item.target).collect::<Vec<_>>(),"mappings":[]}),
    )
}
