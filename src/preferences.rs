use crate::{AppError, AppResult, secure};
use serde_json::{Value, json};

const SETTINGS_VERSION: u64 = 1;
const KEYBINDINGS_VERSION: u64 = 1;
const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn writer(document: &Value) -> (String, bool) {
    let stamp = document["filebladeVersion"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let newer = match (
        semver::Version::parse(&stamp),
        semver::Version::parse(BUILD_VERSION),
    ) {
        (Ok(written), Ok(running)) => written.cmp_precedence(&running).is_gt(),
        _ => false,
    };
    (stamp, newer)
}

fn version_is(document: &Value, expected: u64) -> bool {
    match document.get("version") {
        None => true,
        Some(version) => version.as_u64() == Some(expected),
    }
}

#[derive(Clone, Debug, clap::Args)]
pub struct Changes {
    #[arg(long, value_parser = clap::value_parser!(u16).range(0..=3650))]
    pub trash_retention_days: Option<u16>,
}

pub fn read() -> AppResult<Value> {
    read_document().map(with_defaults)
}

fn with_defaults(mut settings: Value) -> Value {
    let fields = settings.as_object_mut().unwrap();
    fields.entry("trashRetentionDays").or_insert(Value::Null);
    settings
}

pub(crate) fn read_document() -> AppResult<Value> {
    let path = crate::paths::config_dir().join("settings.json");
    let Some(bytes) = secure::read_private_bounded(&path, 64 * 1024).or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        }
    })?
    else {
        return Ok(json!({"version":SETTINGS_VERSION,"filebladeVersion":BUILD_VERSION}));
    };
    let settings: Value = serde_json::from_slice(&bytes)?;
    if !settings.is_object()
        || !version_is(&settings, SETTINGS_VERSION)
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
    settings["version"] = json!(SETTINGS_VERSION);
    settings["filebladeVersion"] = json!(BUILD_VERSION);
    let encoded = serde_json::to_vec_pretty(&settings)?;
    if encoded.len() > 64 * 1024 {
        return Err(AppError::invalid(
            "versioned settings exceed 64 KiB; the file was preserved",
        ));
    }
    crate::lease::durable::write_private_atomic(&path, &encoded)?;
    Ok(with_defaults(settings))
}

pub fn keybindings() -> AppResult<Value> {
    let path = crate::paths::config_dir().join("keybindings.json");
    let _directory = secure::ensure_private_directory(path.parent().unwrap())?;
    let _lock = secure::try_open_private_lock(&crate::paths::state_dir().join("keybindings.lock"))?
        .ok_or_else(|| AppError::invalid("keybindings are busy; retry"))?;
    let bytes = secure::read_bounded_nofollow(&path, 64 * 1024)?;
    let mut document: Value = match bytes.as_deref() {
        Some(bytes) => serde_json::from_slice(bytes)?,
        None => json!({"version":KEYBINDINGS_VERSION,"bindings":{}}),
    };
    let object = document
        .as_object()
        .ok_or_else(|| AppError::invalid("keybindings must be an object"))?;
    if !version_is(&document, KEYBINDINGS_VERSION)
        || object
            .get("bindings")
            .is_some_and(|bindings| !bindings.is_object())
    {
        return Err(AppError::invalid(
            "keybindings have an unsupported version or format; the file was preserved",
        ));
    }
    let (written_by, newer) = writer(&document);
    let stale_stamp = !newer
        && semver::Version::parse(&written_by)
            .ok()
            .zip(semver::Version::parse(BUILD_VERSION).ok())
            .is_none_or(|(written, running)| written.cmp_precedence(&running).is_lt());
    let stamp = document.get("version").is_none() || stale_stamp;
    if stamp {
        document["version"] = json!(KEYBINDINGS_VERSION);
        document["filebladeVersion"] = json!(BUILD_VERSION);
    }
    let encoded = serde_json::to_string_pretty(&document)?;
    if encoded.len() > 64 * 1024 {
        return Err(AppError::invalid("versioned keybindings exceed 64 KiB"));
    }
    let changed = stamp
        && bytes
            .as_deref()
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            .as_ref()
            != Some(&document);
    if changed {
        crate::lease::durable::write_private_atomic(&path, encoded.as_bytes())?
    }
    let (written_by, newer) = writer(&document);
    Ok(json!({
        "text": encoded,
        "version": KEYBINDINGS_VERSION,
        "writtenBy": written_by,
        "newerWriter": newer,
        "buildVersion": BUILD_VERSION,
    }))
}
