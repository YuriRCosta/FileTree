use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct JournalData {
    pub(super) version: u32,
    pub(super) undo: Vec<JournalEntry>,
    pub(super) redo: Vec<JournalEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) interrupted: Option<JournalEntry>,
}

pub(super) struct JournalStore {
    pub(super) path: PathBuf,
    pub(super) _lock: secure::LockedFile,
    pub(super) data: JournalData,
}

pub fn journal_document(limit: usize) -> Value {
    let path = journal_path();
    match load_journal(&path) {
        Ok(data) => {
            let undo: Vec<_> = data.undo.iter().rev().take(limit).map(summary).collect();
            let redo: Vec<_> = data.redo.iter().rev().take(limit).map(summary).collect();
            json!({
                "ok": true,
                "path": path_text(&path),
                "undoCount": undo.len(),
                "redoCount": redo.len(),
                "undo": undo,
                "redo": redo,
                "interrupted": data.interrupted.as_ref().map(summary),
            })
        }
        Err(error) => json!({
            "ok": false,
            "path": path_text(&journal_path()),
            "undo": [],
            "redo": [],
            "undoCount": 0,
            "redoCount": 0,
            "error": error.to_string(),
        }),
    }
}

impl JournalStore {
    pub(super) fn open() -> AppResult<Self> {
        let path = journal_path();
        let lock_path = appended_name(&path, ".lock")?;
        let lock = secure::open_record_lock(&lock_path)?;
        let data = load_journal(&path)?;
        let mut store = Self {
            path,
            _lock: lock,
            data,
        };
        if let Some(mut entry) = store.data.interrupted.take() {
            entry.id = format!("{}-interrupted", entry.id);
            entry.label = format!("(interrupted) {}", entry.label);
            store.data.undo.push(entry);
            store.save()?;
        }
        Ok(store)
    }

    pub(super) fn save(&mut self) -> AppResult<()> {
        trim_stack(&mut self.data.undo);
        trim_stack(&mut self.data.redo);
        let mut encoded = serde_json::to_vec(&self.data)?;
        while encoded.len() > JOURNAL_BYTES_LIMIT {
            if !self.data.redo.is_empty() {
                self.data.redo.remove(0);
            } else if self.data.undo.len() > 1 {
                self.data.undo.remove(0);
            } else {
                return Err(AppError::invalid(
                    "a single journal entry exceeds the storage limit",
                ));
            }
            encoded = serde_json::to_vec(&self.data)?;
        }
        crate::lease::durable::write_private_atomic(&self.path, &encoded)?;
        Ok(())
    }
}

pub(super) fn empty_journal() -> JournalData {
    JournalData {
        version: 1,
        undo: Vec::new(),
        redo: Vec::new(),
        interrupted: None,
    }
}

pub(super) fn load_journal(path: &Path) -> AppResult<JournalData> {
    let bytes = match secure::read_private_bounded(path, JOURNAL_BYTES_LIMIT) {
        Ok(None) => return Ok(empty_journal()),
        Ok(Some(bytes)) => bytes,
        Err(error) if matches!(error.kind(), io::ErrorKind::InvalidData) => {
            quarantine(path);
            return Ok(empty_journal());
        }
        Err(error) => return Err(error.into()),
    };
    let document: Value = match serde_json::from_slice(&bytes) {
        Ok(document) => document,
        Err(_) => {
            quarantine(path);
            return Ok(empty_journal());
        }
    };
    let Some(object) = document.as_object() else {
        quarantine(path);
        return Ok(empty_journal());
    };
    let Some(undo) = object.get("undo").and_then(Value::as_array) else {
        quarantine(path);
        return Ok(empty_journal());
    };
    let Some(redo) = object.get("redo").and_then(Value::as_array) else {
        quarantine(path);
        return Ok(empty_journal());
    };
    Ok(JournalData {
        version: 1,
        undo: valid_entries(undo),
        redo: valid_entries(redo),
        interrupted: object
            .get("interrupted")
            .and_then(|value| serde_json::from_value::<JournalEntry>(value.clone()).ok())
            .filter(valid_entry),
    })
}

pub(super) fn valid_entries(values: &[Value]) -> Vec<JournalEntry> {
    values
        .iter()
        .filter_map(|value| serde_json::from_value::<JournalEntry>(value.clone()).ok())
        .filter(valid_entry)
        .collect()
}

pub(super) fn valid_entry(entry: &JournalEntry) -> bool {
    !entry.id.is_empty()
        && matches!(
            entry.kind.as_str(),
            "copy" | "move" | "rename" | "create" | "trash" | "color" | "transfer" | "permissions"
        )
        && !entry.items.is_empty()
        && entry.items.iter().all(|item| valid_item(&entry.kind, item))
}

pub(super) fn valid_item(kind: &str, item: &JournalItem) -> bool {
    match kind {
        "transfer" => valid_compound_item(item),
        "permissions" => {
            valid_path(&item.target)
                && valid_permissions(&item.before_value)
                && valid_permissions(&item.after_value)
        }
        "copy" | "move" | "rename" => valid_path(&item.source) && valid_path(&item.target),
        "create" => valid_path(&item.target),
        "trash" => valid_path(&item.source),
        "color" => {
            valid_path(&item.target)
                && valid_color(&item.before_value)
                && valid_color(&item.after_value)
                && item.before_value != item.after_value
        }
        _ => false,
    }
}

pub(super) fn valid_path(value: &str) -> bool {
    (value.starts_with('/') || value.starts_with("file://")) && parse_path(value).is_ok()
}

pub(super) fn valid_color(value: &str) -> bool {
    value.is_empty()
        || (value.len() == 7
            && value.starts_with('#')
            && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit))
}

pub(super) fn quarantine(path: &Path) {
    if !secure::entry_exists(path).unwrap_or(false) {
        return;
    }
    let suffix = format!(
        ".corrupt-{}-{}",
        Local::now().format("%Y%m%d-%H%M%S"),
        Uuid::new_v4().simple()
    );
    if let Ok(destination) = appended_name(path, &suffix)
        && let Ok(parent) = crate::lease::persistence::record_parent(path)
    {
        let _ = rustix::fs::renameat_with(
            &parent.directory,
            &parent.name,
            &parent.directory,
            destination.file_name().unwrap(),
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .and_then(|()| rustix::fs::fsync(&parent.directory));
    }
}

pub(super) fn trim_stack(stack: &mut Vec<JournalEntry>) {
    if stack.len() > STACK_LIMIT {
        stack.drain(..stack.len() - STACK_LIMIT);
    }
}

pub(super) fn appended_name(path: &Path, suffix: &str) -> AppResult<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| AppError::invalid(format!("{} has no file name", path.display())))?;
    let mut bytes = name.as_bytes().to_vec();
    bytes.extend_from_slice(suffix.as_bytes());
    Ok(path.with_file_name(OsString::from_vec(bytes)))
}
