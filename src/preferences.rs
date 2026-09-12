use crate::{AppError, AppResult, secure};
use serde_json::{Value, json};

#[derive(Clone, Debug, clap::Args)]
pub struct Changes {
    #[arg(long, value_parser = clap::value_parser!(u16).range(0..=3650))]
    pub trash_retention_days: Option<u16>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub agent_management: Option<bool>,
}

pub fn read() -> AppResult<Value> {
    read_document().map(with_defaults)
}

fn with_defaults(mut settings: Value) -> Value {
    let fields = settings.as_object_mut().unwrap();
    fields.entry("agentManagement").or_insert(json!(false));
    fields.entry("trashRetentionDays").or_insert(Value::Null);
    settings
}

fn read_document() -> AppResult<Value> {
    let path = crate::paths::config_dir().join("settings.json");
    let Some(bytes) = secure::read_private_bounded(&path, 64 * 1024).or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        }
    })?
    else {
        return Ok(json!({"version":1,"filebladeVersion":env!("CARGO_PKG_VERSION")}));
    };
    let settings: Value = serde_json::from_slice(&bytes)?;
    if !settings.is_object()
        || settings["version"] != 1
        || !matches!(settings.get("agentManagement"), None | Some(Value::Bool(_)))
        || settings
            .get("trashRetentionDays")
            .is_some_and(|days| !days.is_null() && !days.as_u64().is_some_and(|days| days <= 3650))
    {
        return Err(AppError::invalid(
            "settings.json has an unsupported version or invalid preferences; it was preserved",
        ));
    }
    Ok(settings)
}

pub fn change(changes: &Changes) -> AppResult<Value> {
    let path = crate::paths::config_dir().join("settings.json");
    let _directory = secure::ensure_private_directory(path.parent().unwrap())?;
    let _lock = secure::try_open_private_lock(&crate::paths::state_dir().join("preferences.lock"))?
        .ok_or_else(|| AppError::invalid("preferences are busy; retry"))?;
    let mut settings = read_document()?;
    if let Some(days) = changes.trash_retention_days {
        if days > 3650 {
            return Err(AppError::invalid("Trash retention exceeds 3650 days"));
        }
        settings["trashRetentionDays"] = json!(days);
    }
    if let Some(enabled) = changes.agent_management {
        settings["agentManagement"] = json!(enabled)
    }
    settings["version"] = json!(1);
    settings["filebladeVersion"] = json!(env!("CARGO_PKG_VERSION"));
    let encoded = serde_json::to_vec_pretty(&settings)?;
    if encoded.len() > 64 * 1024 {
        return Err(AppError::invalid(
            "versioned settings exceed 64 KiB; the file was preserved",
        ));
    }
    crate::lease::durable::write_private_atomic(&path, &encoded)?;
    Ok(with_defaults(settings))
}

pub fn require_agent_management() -> AppResult<()> {
    if read()?["agentManagement"] == true {
        Ok(())
    } else {
        Err(AppError::invalid(
            "Enable Manage agent files in FileBlade General settings before changing Skills or Memory",
        ))
    }
}

pub fn keybindings() -> AppResult<String> {
    let path = crate::paths::config_dir().join("keybindings.json");
    let _directory = secure::ensure_private_directory(path.parent().unwrap())?;
    let _lock = secure::try_open_private_lock(&crate::paths::state_dir().join("keybindings.lock"))?
        .ok_or_else(|| AppError::invalid("keybindings are busy; retry"))?;
    let bytes = secure::read_bounded_nofollow(&path, 64 * 1024)?;
    let mut document: Value = match bytes.as_deref() {
        Some(bytes) => serde_json::from_slice(bytes)?,
        None => json!({"version":1,"bindings":{}}),
    };
    let object = document
        .as_object()
        .ok_or_else(|| AppError::invalid("keybindings must be an object"))?;
    if object.get("version").is_some_and(|version| version != 1)
        || object
            .get("bindings")
            .is_some_and(|bindings| !bindings.is_object())
    {
        return Err(AppError::invalid(
            "keybindings have an unsupported version or format; the file was preserved",
        ));
    }
    document["version"] = json!(1);
    document["filebladeVersion"] = json!(env!("CARGO_PKG_VERSION"));
    let encoded = serde_json::to_string_pretty(&document)?;
    if encoded.len() > 64 * 1024 {
        return Err(AppError::invalid("versioned keybindings exceed 64 KiB"));
    }
    let changed = bytes
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .as_ref()
        != Some(&document);
    if changed {
        crate::lease::durable::write_private_atomic(&path, encoded.as_bytes())?
    }
    Ok(encoded)
}
