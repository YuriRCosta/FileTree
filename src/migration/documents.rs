use crate::{AppError, AppResult};
use serde_json::Value;

#[derive(Clone, Copy, Debug)]
pub enum DocumentKind {
    State,
    Layout,
    Settings,
    Keybindings,
}

impl DocumentKind {
    pub fn limit(self) -> usize {
        match self {
            Self::State | Self::Layout => 256 * 1024,
            Self::Settings | Self::Keybindings => 64 * 1024,
        }
    }

    fn version(self) -> u64 {
        match self {
            Self::State => 12,
            _ => 1,
        }
    }
}

pub fn preflight_document(kind: DocumentKind, bytes: &[u8]) -> AppResult<Value> {
    let refused = |reason: &str| {
        AppError::invalid(format!(
            "migration {kind:?}: {reason}; original document preserved"
        ))
    };
    if bytes.len() > kind.limit() {
        return Err(refused("document exceeds its byte bound"));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| refused("malformed JSON"))?;
    if !value.is_object() {
        return Err(refused("expected a JSON object"));
    }
    let version = value["version"]
        .as_u64()
        .ok_or_else(|| refused("missing or invalid schema version"))?;
    if version > kind.version() {
        return Err(refused(
            "newer schema; downgrade requires the original runtime",
        ));
    }
    if version != kind.version() {
        return Err(refused("older schema has no verified migration"));
    }
    match kind {
        DocumentKind::Settings => {
            if value
                .get("agentManagement")
                .is_some_and(|value| !value.is_boolean())
                || value.get("trashRetentionDays").is_some_and(|value| {
                    !value.is_null() && !value.as_u64().is_some_and(|days| days <= 3650)
                })
            {
                return Err(refused("invalid consent or Trash retention preference"));
            }
        }
        DocumentKind::Keybindings => {
            if value
                .get("bindings")
                .is_some_and(|bindings| !bindings.is_object())
            {
                return Err(refused("invalid bindings object"));
            }
        }
        DocumentKind::Layout => {
            if !value["blades"].is_object() {
                return Err(refused("missing blades object"));
            }
            for edge in ["left", "right"] {
                if let Some(blade) = value["blades"].get(edge)
                    && (!blade.is_object()
                        || blade.get("slots").is_some_and(|slots| !slots.is_array()))
                {
                    return Err(refused("invalid blade or slots"));
                }
            }
        }
        DocumentKind::State => {}
    }
    Ok(value)
}
