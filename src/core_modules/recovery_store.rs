use rustix::fs::{
    AtFlags, CWD, Dir, FlockOperation, Mode, OFlags, RawMode, flock, fstat, fsync, mkdirat, openat,
    renameat, statat, unlinkat,
};
use serde_json::{Map, Value, json};
use std::fmt;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_RECORD_BYTES: usize = 1024 * 1024 + 8192;
pub const MAX_RECORDS: usize = 64;
pub const RESTORED_LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;
pub const IDENTIFIER_LENGTH: usize = 32;
pub const MAX_SCANNED_RECORDS: usize = 512;
pub const MAX_STORE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum RecoveryError {
    Store(String),
    Full(String),
    Invalid(String),
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(message) | Self::Full(message) | Self::Invalid(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for RecoveryError {}

impl From<RecoveryError> for crate::AppError {
    fn from(error: RecoveryError) -> Self {
        Self::invalid(error.to_string())
    }
}

impl From<rustix::io::Errno> for RecoveryError {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Store(std::io::Error::from(error).to_string())
    }
}

type Result<T> = std::result::Result<T, RecoveryError>;

fn store(message: &str) -> RecoveryError {
    RecoveryError::Store(message.to_string())
}

pub fn valid_identifier(value: &str) -> bool {
    value.len() == IDENTIFIER_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

fn private_mode(mode: RawMode) -> bool {
    let permissions = mode & 0o7777;
    permissions == 0o600 || permissions == 0o400
}

fn private_stat(stat: &rustix::fs::Stat) -> bool {
    rustix::fs::FileType::from_raw_mode(stat.st_mode as RawMode)
        == rustix::fs::FileType::RegularFile
        && stat.st_uid == rustix::process::geteuid().as_raw()
        && private_mode(stat.st_mode as RawMode)
        && stat.st_nlink == 1
}

fn random_hex(bytes: usize) -> String {
    let mut text = String::with_capacity(bytes * 2);
    while text.len() < bytes * 2 {
        let value = uuid::Uuid::new_v4().simple().to_string();
        text.push_str(&value);
    }
    text.truncate(bytes * 2);
    text
}

#[derive(Clone, Debug)]
pub struct RecoveryRecord {
    pub record_id: String,
    pub bytes: usize,
    pub document: Map<String, Value>,
}

impl RecoveryRecord {
    pub fn created_at(&self) -> i64 {
        self.document
            .get("createdAt")
            .and_then(Value::as_i64)
            .unwrap_or_default()
    }

    pub fn restored(&self) -> bool {
        self.document.contains_key("restoredAt")
    }

    pub fn payload(&self) -> &Value {
        self.document.get("payload").unwrap_or(&Value::Null)
    }

    pub fn context(&self) -> &Value {
        self.document.get("context").unwrap_or(&Value::Null)
    }

    pub fn transaction_id(&self) -> &str {
        self.document
            .get("transactionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    pub fn definition_id(&self) -> &str {
        self.document
            .get("definitionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }
}

struct Lock(OwnedFd);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = flock(self.0.as_fd(), FlockOperation::Unlock);
    }
}

pub struct RecoveryStore {
    directory: PathBuf,
}

impl RecoveryStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn record_path(&self, record_id: &str) -> PathBuf {
        self.directory.join(format!("{record_id}.json"))
    }

    fn open_directory(&self, create: bool) -> Result<Option<OwnedFd>> {
        let path = if self.directory.is_absolute() {
            self.directory.clone()
        } else {
            std::env::current_dir()
                .map_err(|error| RecoveryError::Store(error.to_string()))?
                .join(&self.directory)
        };
        let mut parent = openat(
            CWD,
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        for component in path.components() {
            let name = match component {
                Component::RootDir => continue,
                Component::Normal(name) => name,
                _ => {
                    return Err(store(
                        "recovery directory must have a normalized absolute path",
                    ));
                }
            };
            if create {
                match mkdirat(parent.as_fd(), name, Mode::from_raw_mode(0o700)) {
                    Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            let child = match openat(
                parent.as_fd(),
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            ) {
                Ok(child) => child,
                Err(rustix::io::Errno::NOENT) if !create => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            parent = child;
        }
        let info = fstat(parent.as_fd())?;
        if info.st_uid != rustix::process::geteuid().as_raw()
            || (info.st_mode as RawMode & 0o7777) != 0o700
        {
            return Err(store(
                "recovery directory must be owned by this user with mode 0700",
            ));
        }
        Ok(Some(parent))
    }

    fn locked(&self, parent: BorrowedFd<'_>) -> Result<Lock> {
        let descriptor = openat(
            parent,
            ".lock",
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?;
        if !private_stat(&fstat(descriptor.as_fd())?) {
            return Err(store("recovery lock is not private"));
        }
        flock(descriptor.as_fd(), FlockOperation::NonBlockingLockExclusive).map_err(|_| {
            RecoveryError::Full("the undo store is busy; retry the operation".to_string())
        })?;
        Ok(Lock(descriptor))
    }

    fn names(&self, parent: BorrowedFd<'_>) -> Result<Vec<String>> {
        let duplicate = openat(
            parent,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let mut names = Vec::new();
        for entry in Dir::new(duplicate)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "." || name == ".." {
                continue;
            }
            if names.len() >= MAX_SCANNED_RECORDS {
                return Err(RecoveryError::Full(
                    "the undo store exceeds its directory entry limit".to_string(),
                ));
            }
            names.push(name);
        }
        names.sort();
        Ok(names)
    }

    fn read_at(
        &self,
        parent: BorrowedFd<'_>,
        record_id: &str,
        limit: usize,
    ) -> Result<Option<RecoveryRecord>> {
        if !valid_identifier(record_id) {
            return Ok(None);
        }
        let limit = limit.min(MAX_RECORD_BYTES);
        let descriptor = match openat(
            parent,
            format!("{record_id}.json"),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let info = fstat(descriptor.as_fd())?;
        if !private_stat(&info) || info.st_size as usize > limit {
            return Err(store(
                "recovery record is not a bounded private regular file",
            ));
        }
        let mut data = Vec::new();
        let mut buffer = [0u8; 65536];
        while data.len() <= limit {
            let wanted = (limit + 1 - data.len()).min(buffer.len());
            let count = rustix::io::read(descriptor.as_fd(), &mut buffer[..wanted])?;
            if count == 0 {
                break;
            }
            data.extend_from_slice(&buffer[..count]);
        }
        if data.len() > limit {
            return Err(store("recovery record exceeds its byte limit"));
        }
        let document: Value =
            serde_json::from_slice(&data).map_err(|_| store("recovery record is invalid JSON"))?;
        let Some(document) = document.as_object() else {
            return Err(store("recovery record has an unsupported format"));
        };
        let created = document.get("createdAt").and_then(Value::as_i64);
        if document.get("formatVersion") != Some(&json!(1))
            || !created.is_some_and(|value| value > 0)
            || !document.get("payload").is_some_and(Value::is_object)
            || !document.get("context").is_some_and(Value::is_object)
        {
            return Err(store("recovery record has an unsupported format"));
        }
        Ok(Some(RecoveryRecord {
            record_id: record_id.to_string(),
            bytes: data.len(),
            document: document.clone(),
        }))
    }

    pub fn read(&self, record_id: &str) -> Option<RecoveryRecord> {
        if !valid_identifier(record_id) {
            return None;
        }
        match self.open_directory(false) {
            Ok(Some(parent)) => self
                .read_at(parent.as_fd(), record_id, MAX_RECORD_BYTES)
                .ok()?,
            _ => None,
        }
    }

    fn snapshot(&self, parent: BorrowedFd<'_>) -> Result<(Vec<RecoveryRecord>, usize)> {
        let mut found = Vec::new();
        let mut total = 0usize;
        for name in self.names(parent)? {
            if name == ".lock" {
                continue;
            }
            if name.ends_with(".staged") {
                let info = statat(parent, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
                if !private_stat(&info) || info.st_size as usize > MAX_RECORD_BYTES {
                    return Err(store(
                        "staged recovery record is not a bounded private regular file",
                    ));
                }
                total += info.st_size as usize;
                if total > MAX_STORE_BYTES {
                    return Err(RecoveryError::Full(
                        "the undo store exceeds its aggregate byte limit".to_string(),
                    ));
                }
                continue;
            }
            let stem = name
                .strip_suffix(".json")
                .filter(|stem| valid_identifier(stem));
            let Some(stem) = stem else {
                return Err(RecoveryError::Full(
                    "the undo store contains an unrecognized entry".to_string(),
                ));
            };
            if let Some(record) = self.read_at(parent, stem, MAX_STORE_BYTES - total)? {
                total += record.bytes;
                found.push(record);
            }
        }
        Ok((found, total))
    }

    pub fn records(&self) -> Result<Vec<RecoveryRecord>> {
        match self.open_directory(false)? {
            Some(directory) => Ok(self.snapshot(directory.as_fd())?.0),
            None => Ok(Vec::new()),
        }
    }

    pub fn inventory(&self) -> Value {
        match self.records() {
            Ok(records) => {
                let rows: Vec<Value> = records
                    .iter()
                    .map(|record| {
                        json!({
                            "recordId": record.record_id,
                            "createdAt": record.created_at(),
                            "restored": record.restored(),
                            "trackedTransaction": !record.transaction_id().is_empty(),
                        })
                    })
                    .collect();
                json!({"ok": true, "schemaVersion": 1, "records": rows})
            }
            Err(error) => {
                json!({"ok": false, "schemaVersion": 1, "records": [], "message": error.to_string()})
            }
        }
    }

    pub fn live_records(&self) -> Result<usize> {
        Ok(self
            .records()?
            .iter()
            .filter(|record| !record.restored())
            .count())
    }

    pub fn find(&self, payload: &Value) -> Option<RecoveryRecord> {
        let records = self.records().ok()?;
        let matches: Vec<RecoveryRecord> = records
            .into_iter()
            .filter(|record| record.payload() == payload)
            .collect();
        matches
            .iter()
            .find(|record| !record.restored())
            .cloned()
            .or_else(|| matches.first().cloned())
    }

    fn encode(document: &Map<String, Value>) -> Result<Vec<u8>> {
        let mut trimmed = document.clone();
        trimmed.remove("recordId");
        trimmed.remove("_bytes");
        let encoded = serde_json::to_vec(&Value::Object(trimmed))
            .map_err(|error| RecoveryError::Invalid(error.to_string()))?;
        if encoded.len() > MAX_RECORD_BYTES {
            return Err(RecoveryError::Invalid(
                "the recovery record exceeds the store size limit".to_string(),
            ));
        }
        Ok(encoded)
    }

    fn publish(
        &self,
        parent: BorrowedFd<'_>,
        name: &str,
        encoded: &[u8],
        replace: &str,
    ) -> Result<()> {
        let descriptor = openat(
            parent,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?;
        let written = (|| -> Result<()> {
            let mut offset = 0;
            while offset < encoded.len() {
                let count = rustix::io::write(descriptor.as_fd(), &encoded[offset..])?;
                if count == 0 {
                    return Err(store("the recovery record could not be written"));
                }
                offset += count;
            }
            fsync(descriptor.as_fd())?;
            if !replace.is_empty() {
                renameat(parent, name, parent, replace)?;
            }
            fsync(parent)?;
            Ok(())
        })();
        if written.is_err() {
            let _ = unlinkat(parent, name, AtFlags::empty());
        }
        written
    }

    pub fn write(
        &self,
        payload: &Value,
        definition_id: &str,
        context: &Value,
        transaction_id: &str,
    ) -> Result<String> {
        if !transaction_id.is_empty() && !valid_identifier(transaction_id) {
            return Err(RecoveryError::Invalid(
                "invalid recovery transaction id".to_string(),
            ));
        }
        let directory = self
            .open_directory(true)?
            .ok_or_else(|| store("recovery directory is missing"))?;
        let parent = directory.as_fd();
        let _lock = self.locked(parent)?;
        self.expire(parent)?;
        let (records, total) = self.snapshot(parent)?;
        for record in &records {
            if record.payload() == payload
                && record.context() == context
                && record.transaction_id() == transaction_id
                && !record.restored()
            {
                return Ok(record.record_id.clone());
            }
        }
        if records.iter().filter(|record| !record.restored()).count() >= MAX_RECORDS {
            return Err(RecoveryError::Full(
                "the undo store is full; restore or discard earlier removals before removing another"
                    .to_string(),
            ));
        }
        let mut document = Map::new();
        document.insert("formatVersion".to_string(), json!(1));
        document.insert("createdAt".to_string(), json!(now_seconds()));
        document.insert("definitionId".to_string(), json!(definition_id));
        document.insert("context".to_string(), context.clone());
        document.insert("payload".to_string(), payload.clone());
        document.insert("transactionId".to_string(), json!(transaction_id));
        let encoded = Self::encode(&document)?;
        if total + encoded.len() > MAX_STORE_BYTES {
            return Err(RecoveryError::Full(
                "the undo store is full; its aggregate byte limit would be exceeded".to_string(),
            ));
        }
        let record_id = if transaction_id.is_empty() {
            random_hex(IDENTIFIER_LENGTH / 2)
        } else {
            transaction_id.to_string()
        };
        if self
            .read_at(parent, &record_id, MAX_RECORD_BYTES)?
            .is_some()
        {
            return Err(RecoveryError::Invalid(
                "recovery transaction id is already in use".to_string(),
            ));
        }
        self.publish(parent, &format!("{record_id}.json"), &encoded, "")?;
        Ok(record_id)
    }

    pub fn mark_restored(&self, record_id: &str) {
        let _ = self.try_mark_restored(record_id);
    }

    fn try_mark_restored(&self, record_id: &str) -> Result<()> {
        let Some(directory) = self.open_directory(false)? else {
            return Ok(());
        };
        let parent = directory.as_fd();
        let _lock = self.locked(parent)?;
        let Some(mut record) = self.read_at(parent, record_id, MAX_RECORD_BYTES)? else {
            return Ok(());
        };
        if record.restored() {
            return Ok(());
        }
        record
            .document
            .insert("restoredAt".to_string(), json!(now_seconds()));
        let staged = format!("{record_id}.{}.staged", random_hex(8));
        self.publish(
            parent,
            &staged,
            &Self::encode(&record.document)?,
            &format!("{record_id}.json"),
        )
    }

    pub fn discard(&self, record_id: &str) -> Result<()> {
        if !valid_identifier(record_id) {
            return Err(RecoveryError::Invalid(
                "invalid recovery record id".to_string(),
            ));
        }
        let Some(directory) = self.open_directory(false)? else {
            return Ok(());
        };
        let parent = directory.as_fd();
        let _lock = self.locked(parent)?;
        if self.read_at(parent, record_id, MAX_RECORD_BYTES)?.is_some() {
            unlinkat(parent, format!("{record_id}.json"), AtFlags::empty())?;
            fsync(parent)?;
        }
        Ok(())
    }

    pub fn discard_payload(&self, record_id: &str, payload: Option<&Value>) -> Value {
        match self.try_discard_payload(record_id, payload) {
            Ok(()) => json!({"ok": true, "schemaVersion": 1}),
            Err(error) => json!({"ok": false, "schemaVersion": 1, "message": error.to_string()}),
        }
    }

    fn try_discard_payload(&self, record_id: &str, payload: Option<&Value>) -> Result<()> {
        if let Some(payload) = payload
            && !payload.is_object()
        {
            return Err(RecoveryError::Invalid(
                "recovery payload must be an object".to_string(),
            ));
        }
        if !valid_identifier(record_id) && payload.is_none() {
            return Err(RecoveryError::Invalid(
                "discard needs a recovery record id or a matching payload".to_string(),
            ));
        }
        let Some(directory) = self.open_directory(false)? else {
            return Ok(());
        };
        let parent = directory.as_fd();
        let _lock = self.locked(parent)?;
        let record = if valid_identifier(record_id) {
            self.read_at(parent, record_id, MAX_RECORD_BYTES)?
        } else {
            let wanted = payload.expect("payload present without a record id");
            let matches: Vec<RecoveryRecord> = self
                .snapshot(parent)?
                .0
                .into_iter()
                .filter(|record| record.payload() == wanted)
                .collect();
            matches
                .iter()
                .find(|record| !record.restored())
                .cloned()
                .or_else(|| matches.first().cloned())
        };
        let Some(record) = record else {
            return Ok(());
        };
        if let Some(payload) = payload
            && record.payload() != payload
        {
            return Err(RecoveryError::Invalid(
                "recovery record does not match this payload".to_string(),
            ));
        }
        unlinkat(
            parent,
            format!("{}.json", record.record_id),
            AtFlags::empty(),
        )?;
        fsync(parent)?;
        Ok(())
    }

    fn expire(&self, parent: BorrowedFd<'_>) -> Result<()> {
        let now = now_seconds();
        let records = self.snapshot(parent)?.0;
        for name in self.names(parent)? {
            if !name.ends_with(".staged") {
                continue;
            }
            let info = statat(parent, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
            if private_stat(&info) && now - info.st_mtime as i64 > RESTORED_LIFETIME_SECONDS {
                unlinkat(parent, name.as_str(), AtFlags::empty())?;
            }
        }
        let mut completed: Vec<(i64, String)> = Vec::new();
        for record in records {
            let Some(restored) = record.document.get("restoredAt") else {
                continue;
            };
            let Some(restored) = restored.as_i64() else {
                return Err(store("recovery completion timestamp is invalid"));
            };
            let name = format!("{}.json", record.record_id);
            if now - restored > RESTORED_LIFETIME_SECONDS {
                unlinkat(parent, name.as_str(), AtFlags::empty())?;
            } else {
                completed.push((restored, name));
            }
        }
        completed.sort();
        let excess = completed.len().saturating_sub(MAX_RECORDS);
        for (_, name) in completed.into_iter().take(excess) {
            unlinkat(parent, name.as_str(), AtFlags::empty())?;
        }
        fsync(parent)?;
        Ok(())
    }

    pub fn expired(&self) -> Result<()> {
        let Some(directory) = self.open_directory(false)? else {
            return Ok(());
        };
        let parent = directory.as_fd();
        let _lock = self.locked(parent)?;
        self.expire(parent)
    }
}
