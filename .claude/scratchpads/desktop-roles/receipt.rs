use crate::secure;
use crate::{AppError, AppResult};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const RECEIPT_BYTES: usize = 64 * 1024;
pub const RECEIPT_VERSION: u64 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedEntry {
    pub path: PathBuf,
    pub key: Option<String>,
    pub prior: Option<String>,
    pub owned: String,
}

impl OwnedEntry {
    fn to_json(&self) -> Value {
        json!({
            "path": crate::common::path_text(&self.path),
            "key": self.key,
            "prior": self.prior,
            "owned": self.owned,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let path = value.get("path")?.as_str()?;
        if path.is_empty() || path.len() > 4096 {
            return None;
        }
        let owned = value.get("owned")?.as_str()?;
        if owned.is_empty() || owned.len() > 4096 {
            return None;
        }
        let text = |name: &str| -> Option<Option<String>> {
            match value.get(name) {
                None | Some(Value::Null) => Some(None),
                Some(Value::String(found)) if found.len() <= 4096 => Some(Some(found.clone())),
                Some(_) => None,
            }
        };
        Some(Self {
            path: PathBuf::from(path),
            key: text("key")?,
            prior: text("prior")?,
            owned: owned.to_string(),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct Receipt {
    pub roles: BTreeMap<String, Vec<OwnedEntry>>,
}

impl Receipt {
    pub fn entries(&self, role: &str) -> &[OwnedEntry] {
        self.roles.get(role).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn enabled(&self, role: &str) -> bool {
        self.roles.contains_key(role)
    }

    pub fn set(&mut self, role: &str, entries: Vec<OwnedEntry>) {
        self.roles.insert(role.to_string(), entries);
    }

    pub fn clear(&mut self, role: &str) {
        self.roles.remove(role);
    }
}

pub fn receipt_path() -> PathBuf {
    crate::paths::config_dir().join("desktop-roles.json")
}

pub fn load() -> AppResult<Receipt> {
    let path = receipt_path();
    let bytes = match secure::read_bounded_nofollow(&path, RECEIPT_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(Receipt::default()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Receipt::default());
        }
        Err(error) => return Err(error.into()),
    };
    let document: Value = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::invalid("desktop role receipt is not valid JSON; refusing to overwrite it"))?;
    let version = document.get("version").and_then(Value::as_u64).unwrap_or(0);
    if version != RECEIPT_VERSION {
        return Err(AppError::invalid(format!(
            "desktop role receipt is version {version}, this runtime writes version {RECEIPT_VERSION}; refusing to overwrite it"
        )));
    }
    let mut receipt = Receipt::default();
    let roles = document
        .get("roles")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::invalid("desktop role receipt has no roles object"))?;
    for (role, value) in roles {
        let list = value
            .as_array()
            .ok_or_else(|| AppError::invalid(format!("desktop role {role} has no entry list")))?;
        let mut entries = Vec::new();
        for entry in list {
            entries.push(OwnedEntry::from_json(entry).ok_or_else(|| {
                AppError::invalid(format!("desktop role {role} has an unreadable entry"))
            })?);
        }
        receipt.roles.insert(role.clone(), entries);
    }
    Ok(receipt)
}

pub fn save(receipt: &Receipt) -> AppResult<()> {
    let path = receipt_path();
    if let Some(parent) = path.parent() {
        secure::ensure_directories(parent, 0o700)?;
    }
    let roles: serde_json::Map<String, Value> = receipt
        .roles
        .iter()
        .map(|(role, entries)| {
            (
                role.clone(),
                Value::Array(entries.iter().map(OwnedEntry::to_json).collect()),
            )
        })
        .collect();
    let document = json!({"version": RECEIPT_VERSION, "roles": Value::Object(roles)});
    let mut bytes = serde_json::to_vec_pretty(&document)
        .map_err(|_| AppError::command("desktop role receipt could not be encoded"))?;
    bytes.push(b'\n');
    if bytes.len() > RECEIPT_BYTES {
        return Err(AppError::invalid("desktop role receipt exceeds its byte limit"));
    }
    secure::write_private_atomic(&path, &bytes)?;
    Ok(())
}
