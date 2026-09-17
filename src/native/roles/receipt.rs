use crate::secure;
use crate::{AppError, AppResult};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const RECEIPT_BYTES: usize = 64 * 1024;
pub const RECEIPT_SCHEMA: u64 = 1;
const TEXT_LIMIT: usize = 16 * 1024;

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
        if path.is_empty() || path.len() > 4096 || !path.starts_with('/') {
            return None;
        }
        let owned = value.get("owned")?.as_str()?;
        if owned.len() > TEXT_LIMIT {
            return None;
        }
        let text = |name: &str| -> Option<Option<String>> {
            match value.get(name) {
                None | Some(Value::Null) => Some(None),
                Some(Value::String(found)) if found.len() <= TEXT_LIMIT => {
                    Some(Some(found.clone()))
                }
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoleState {
    pub enabled: bool,
    pub intent: Option<String>,
    pub entries: Vec<OwnedEntry>,
}

#[derive(Clone, Debug, Default)]
pub struct Receipt {
    pub roles: BTreeMap<String, RoleState>,
}

impl Receipt {
    pub fn state(&self, role: &str) -> RoleState {
        self.roles.get(role).cloned().unwrap_or_default()
    }

    pub fn state_mut(&mut self, role: &str) -> &mut RoleState {
        self.roles.entry(role.to_string()).or_default()
    }
}

pub fn receipt_path() -> PathBuf {
    crate::lease::native_config_root().join("desktop-roles.json")
}

fn corrupt(detail: impl std::fmt::Display) -> AppError {
    AppError::invalid(format!(
        "desktop role receipt {detail}; refusing to overwrite {}",
        receipt_path().display()
    ))
}

pub fn load() -> AppResult<Receipt> {
    let path = receipt_path();
    let bytes = match secure::read_private_bounded(&path, RECEIPT_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(Receipt::default()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Receipt::default());
        }
        Err(error) => return Err(corrupt(format!("is unreadable ({error})"))),
    };
    let document: Value =
        serde_json::from_slice(&bytes).map_err(|_| corrupt("is not valid JSON"))?;
    let schema = document.get("schema").and_then(Value::as_u64).unwrap_or(0);
    if schema != RECEIPT_SCHEMA {
        return Err(corrupt(format!(
            "is schema {schema}, this runtime writes schema {RECEIPT_SCHEMA}"
        )));
    }
    let mut receipt = Receipt::default();
    let roles = document
        .get("roles")
        .and_then(Value::as_object)
        .ok_or_else(|| corrupt("has no roles object"))?;
    for (role, value) in roles {
        let entries = value
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| corrupt(format!("role {role} has no entry list")))?
            .iter()
            .map(|entry| {
                OwnedEntry::from_json(entry)
                    .ok_or_else(|| corrupt(format!("role {role} has an unreadable entry")))
            })
            .collect::<AppResult<Vec<_>>>()?;
        let intent = match value.get("intent") {
            None | Some(Value::Null) => None,
            Some(Value::String(intent))
                if matches!(intent.as_str(), "enabling" | "enabled" | "disabling") =>
            {
                Some(intent.clone())
            }
            Some(_) => return Err(corrupt(format!("role {role} has an unknown intent"))),
        };
        receipt.roles.insert(
            role.clone(),
            RoleState {
                enabled: value
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                intent,
                entries,
            },
        );
    }
    Ok(receipt)
}

pub fn save(receipt: &Receipt) -> AppResult<()> {
    let roles: serde_json::Map<String, Value> = receipt
        .roles
        .iter()
        .filter(|(_, state)| state.enabled || !state.entries.is_empty() || state.intent.is_some())
        .map(|(role, state)| {
            (
                role.clone(),
                json!({
                    "enabled": state.enabled,
                    "intent": state.intent,
                    "entries": state.entries.iter().map(OwnedEntry::to_json).collect::<Vec<_>>(),
                }),
            )
        })
        .collect();
    let document = json!({"schema": RECEIPT_SCHEMA, "roles": Value::Object(roles)});
    let mut bytes = serde_json::to_vec_pretty(&document)?;
    bytes.push(b'\n');
    if bytes.len() > RECEIPT_BYTES {
        return Err(AppError::invalid(
            "desktop role receipt exceeds its byte limit",
        ));
    }
    crate::lease::durable::write_private_atomic(&receipt_path(), &bytes)?;
    Ok(())
}
