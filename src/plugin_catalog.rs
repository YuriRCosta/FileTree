use crate::actions::{MAX_MANIFEST_BYTES, plugin_root, read_manifest};
use crate::command::{CommandSpec, which};
use crate::common::path_text;
use crate::paths::xdg_home;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

const BLADE_SOCKET: &str = "data-goblin.fileblade/blade";
const HELPER_SOCKET: &str = "data-goblin.fileblade/helper";
const ACTION_SOCKET: &str = "data-goblin.fileblade/action";
const MAX_ENTRY_BYTES: usize = 512;
const MAX_ENTRY_SEGMENTS: usize = 8;
const CORE_ID: &str = "data-goblin.fileblade";
const MAX_ENTRIES: usize = 256;
const MAX_PROVIDERS: usize = 128;
const MAX_MANIFEST_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_DIAGNOSTICS: usize = 32;
const MAX_ROWS: usize = 512;
const CLI_TIMEOUT: Duration = Duration::from_secs(2);
const CLI_STDOUT_LIMIT: usize = 128 * 1024;
const CLI_STDERR_LIMIT: usize = 4 * 1024;

pub fn catalog() -> crate::AppResult<Value> {
    let mut diagnostics: Vec<Value> = Vec::new();
    let plugins = match plugins_dir() {
        Ok(plugins) => plugins,
        Err(error) => {
            return Ok(json!({
                "ok": true,
                "providers": [],
                "activation": "unknown",
                "truncated": false,
                "diagnostics": [json!({"source": "plugins", "error": bounded(&error, 256)})],
            }));
        }
    };
    let (activation, activation_state) = enabled_ids();
    let mut providers = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut ambiguous: HashSet<String> = HashSet::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    let mut entries: Vec<PathBuf> = Vec::new();
    match std::fs::read_dir(&plugins) {
        Ok(reader) => {
            for entry in reader.take(MAX_ENTRIES + 1) {
                if entries.len() >= MAX_ENTRIES {
                    truncated = true;
                    break;
                }
                match entry {
                    Ok(entry) => entries.push(entry.path()),
                    Err(error) => {
                        truncated = true;
                        note(&mut diagnostics, "plugins", &error.to_string());
                    }
                }
            }
        }
        Err(error) => {
            truncated = true;
            note(&mut diagnostics, &path_text(&plugins), &error.to_string());
        }
    }
    entries.sort();
    for path in entries {
        if providers.len() >= MAX_PROVIDERS || bytes >= MAX_MANIFEST_TOTAL_BYTES {
            truncated = true;
            break;
        }
        let name = match path.file_name().and_then(|value| value.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        if name.starts_with('.') || name == CORE_ID {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        let root = match plugin_root(&path.to_string_lossy()) {
            Ok(root) => root,
            Err(error) => {
                note(&mut diagnostics, &name, &error);
                continue;
            }
        };
        let manifest_path = root.join("manifest.json");
        let declared = match std::fs::symlink_metadata(&manifest_path) {
            Ok(metadata) if metadata.file_type().is_file() => metadata.len() as usize,
            Ok(_) => {
                note(
                    &mut diagnostics,
                    &name,
                    "manifest.json is not a regular file",
                );
                continue;
            }
            Err(_) => continue,
        };
        let readable = declared.min(MAX_MANIFEST_BYTES);
        if bytes.saturating_add(readable) > MAX_MANIFEST_TOTAL_BYTES {
            truncated = true;
            break;
        }
        bytes = bytes.saturating_add(readable);
        let manifest = match read_manifest(&root) {
            Ok(manifest) => manifest,
            Err(error) => {
                note(&mut diagnostics, &name, &error);
                continue;
            }
        };
        let id = manifest
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if id.is_empty() || id != name || id == CORE_ID || id.starts_with("omarchy.") {
            if !id.is_empty() && id != name {
                note(
                    &mut diagnostics,
                    &name,
                    "the manifest id does not match its installed directory",
                );
            }
            continue;
        }
        if !contributes(&manifest) {
            continue;
        }
        if let Err(error) = entries_are_confined(&manifest, &root) {
            note(&mut diagnostics, &id, &error);
            continue;
        }
        if !seen.insert(id.clone()) {
            ambiguous.insert(id.clone());
            note(&mut diagnostics, &id, "two installed plugins carry this id");
            continue;
        }
        providers.push(json!({
            "id": id,
            "dir": path_text(&root),
            "path": path_text(&path),
            "manifest": manifest,
            "enabled": activation.get(&id).copied().unwrap_or(false),
        }));
    }
    let providers: Vec<Value> = providers
        .into_iter()
        .filter(|provider| {
            let id = provider
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            !ambiguous.contains(id)
        })
        .collect();
    Ok(json!({
        "ok": true,
        "providers": providers,
        "activation": if truncated { "unknown" } else { activation_state },
        "complete": !truncated,
        "truncated": truncated,
        "diagnostics": diagnostics,
    }))
}

fn contributes(manifest: &Value) -> bool {
    let extensions = match manifest.get("extensions") {
        Some(Value::Object(extensions)) => extensions,
        _ => return false,
    };
    [BLADE_SOCKET, HELPER_SOCKET, ACTION_SOCKET]
        .iter()
        .any(|socket| matches!(extensions.get(*socket), Some(Value::Array(entries)) if !entries.is_empty()))
}

fn declared_entries(manifest: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let extensions = match manifest.get("extensions") {
        Some(Value::Object(extensions)) => extensions,
        _ => return found,
    };
    for socket in [BLADE_SOCKET, HELPER_SOCKET, ACTION_SOCKET] {
        let Some(Value::Array(entries)) = extensions.get(socket) else {
            continue;
        };
        for entry in entries.iter().take(MAX_PROVIDERS) {
            for key in ["entry", "provider"] {
                match entry.get(key) {
                    Some(Value::String(value)) => found.push(value.clone()),
                    Some(Value::Null) | None => {}
                    Some(_) => found.push(String::new()),
                }
            }
        }
    }
    found
}

fn entries_are_confined(manifest: &Value, root: &std::path::Path) -> Result<(), String> {
    let canonical = std::fs::canonicalize(root).map_err(|error| error.to_string())?;
    for entry in declared_entries(manifest) {
        if entry.is_empty()
            || entry.len() > MAX_ENTRY_BYTES
            || entry.starts_with('/')
            || entry.chars().any(char::is_control)
        {
            return Err(format!("{entry:?} is not a usable entry path"));
        }
        let relative = std::path::Path::new(&entry);
        let segments = relative.components().count();
        if segments == 0 || segments > MAX_ENTRY_SEGMENTS {
            return Err(format!("{entry:?} is not a usable entry path"));
        }
        if relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(format!("{entry:?} leaves the plugin directory"));
        }
        let target = canonical.join(relative);
        match std::fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => return Err(format!("{entry:?} is not a regular file")),
            Err(error) => return Err(format!("{}: {error}", path_text(&target))),
        }
        let resolved = std::fs::canonicalize(&target)
            .map_err(|error| format!("{}: {error}", path_text(&target)))?;
        if !resolved.starts_with(&canonical) {
            return Err(format!("{entry:?} leaves the plugin directory"));
        }
    }
    Ok(())
}

fn plugins_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .ok_or("HOME is not set")?;
    let plugins = PathBuf::from(home).join(".config/omarchy/plugins");
    std::fs::canonicalize(&plugins).map_err(|error| format!("{}: {error}", path_text(&plugins)))
}

fn enabled_ids() -> (BTreeMap<String, bool>, &'static str) {
    let program = match which("omarchy") {
        Some(program) => program,
        None => return (BTreeMap::new(), "unknown"),
    };
    let output = CommandSpec::new(program)
        .args(["plugin", "list", "--json"])
        .env("LC_ALL", "C")
        .timeout(CLI_TIMEOUT)
        .limits(CLI_STDOUT_LIMIT, CLI_STDERR_LIMIT)
        .run();
    let output = match output {
        Ok(output) if output.status.success() && !output.stdout_truncated => output,
        _ => return (BTreeMap::new(), "unknown"),
    };
    let rows = match serde_json::from_slice::<Value>(&output.stdout) {
        Ok(Value::Array(rows)) => rows,
        _ => return (BTreeMap::new(), "unknown"),
    };
    if rows.len() > MAX_ROWS {
        return (BTreeMap::new(), "unknown");
    }
    match merge_activation(&rows, shell_activation().as_ref()) {
        Some(activation) => (activation, "known"),
        None => (BTreeMap::new(), "unknown"),
    }
}

const SHELL_CONFIG_LIMIT: u64 = 1024 * 1024;
const BAR_SECTIONS: [&str; 3] = ["left", "center", "right"];

struct ShellActivation {
    listed: HashSet<String>,
    disabled: HashSet<String>,
}

impl ShellActivation {
    fn parse(config: &Value) -> Self {
        let mut listed = HashSet::new();
        let mut disabled = HashSet::new();
        for row in config["plugins"].as_array().into_iter().flatten() {
            if let Some(id) = row["id"].as_str() {
                listed.insert(id.to_string());
            }
        }
        for section in BAR_SECTIONS {
            for row in config["bar"]["layout"][section]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(id) = row["id"].as_str() {
                    listed.insert(id.to_string());
                }
            }
        }
        for id in config["disabledPlugins"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            disabled.insert(id.to_string());
        }
        Self { listed, disabled }
    }

    fn enabled(&self, id: &str) -> bool {
        !self.disabled.contains(id) && self.listed.contains(id)
    }
}

fn shell_activation() -> Option<ShellActivation> {
    let path = xdg_home("XDG_CONFIG_HOME", "~/.config").join("omarchy/shell.json");
    let metadata = std::fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > SHELL_CONFIG_LIMIT {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let config: Value = serde_json::from_slice(&bytes).ok()?;
    config.is_object().then(|| ShellActivation::parse(&config))
}

fn listed_by_bar_only(row: &Value) -> bool {
    let kinds: Vec<&str> = row["kinds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    kinds.contains(&"bar-widget") && kinds.iter().any(|kind| *kind != "bar-widget")
}

fn merge_activation(
    rows: &[Value],
    shell: Option<&ShellActivation>,
) -> Option<BTreeMap<String, bool>> {
    let mut activation = BTreeMap::new();
    for row in rows {
        let id = row.get("id").and_then(Value::as_str).unwrap_or_default();
        let enabled = row.get("enabled").and_then(Value::as_bool);
        let (false, Some(listed)) = (id.is_empty(), enabled) else {
            return None;
        };
        let enabled =
            listed || (listed_by_bar_only(row) && shell.is_some_and(|config| config.enabled(id)));
        if activation.insert(id.to_string(), enabled).is_some() {
            return None;
        }
    }
    Some(activation)
}

fn note(diagnostics: &mut Vec<Value>, source: &str, error: &str) {
    if diagnostics.len() >= MAX_DIAGNOSTICS {
        return;
    }
    diagnostics.push(json!({
        "source": bounded(source, 128),
        "error": bounded(error, 256),
    }));
}

fn bounded(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

pub fn require_enabled(provider: &str, directory: &std::path::Path) -> crate::AppResult<()> {
    let snapshot = catalog()?;
    let enabled = snapshot["complete"] == true
        && snapshot["activation"] == "known"
        && snapshot["providers"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["id"] == provider
                    && row["enabled"] == true
                    && row["dir"]
                        .as_str()
                        .and_then(|path| crate::common::parse_path(path).ok())
                        .is_some_and(|path| path == directory)
            })
        });
    if enabled {
        Ok(())
    } else {
        Err(crate::AppError::invalid(
            "the helper provider must be installed and explicitly enabled; enable its Omarchy plugin and retry",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, enabled: bool, kinds: &[&str]) -> Value {
        json!({ "id": id, "enabled": enabled, "kinds": kinds })
    }

    fn shell() -> ShellActivation {
        ShellActivation::parse(&json!({
            "plugins": [{ "id": "a.service" }, { "id": "a.both" }, { "id": "a.dropped" }],
            "bar": { "layout": { "left": [{ "id": "a.widget" }], "center": [], "right": [] } },
            "disabledPlugins": ["a.dropped"]
        }))
    }

    #[test]
    fn plain_rows_keep_the_listed_answer() {
        let rows = [
            row("a.service", true, &["service"]),
            row("a.other", false, &["service"]),
        ];
        let merged = merge_activation(&rows, Some(&shell())).unwrap();
        assert!(merged["a.service"]);
        assert!(!merged["a.other"]);
    }

    #[test]
    fn a_service_that_also_offers_a_bar_widget_is_enabled_from_the_plugin_list() {
        let rows = [
            row("a.both", false, &["service", "bar-widget"]),
            row("a.dropped", false, &["service", "bar-widget"]),
            row("a.absent", false, &["service", "bar-widget"]),
            row("a.widget", false, &["bar-widget"]),
        ];
        let merged = merge_activation(&rows, Some(&shell())).unwrap();
        assert!(merged["a.both"]);
        assert!(!merged["a.dropped"]);
        assert!(!merged["a.absent"]);
        assert!(!merged["a.widget"]);
        let without_config = merge_activation(&rows, None).unwrap();
        assert!(!without_config["a.both"]);
    }

    #[test]
    fn malformed_rows_leave_activation_unknown() {
        assert!(merge_activation(&[json!({ "enabled": true })], None).is_none());
        assert!(merge_activation(&[json!({ "id": "a.b" })], None).is_none());
        assert!(merge_activation(&[row("a.b", true, &[]), row("a.b", true, &[])], None).is_none());
    }
}
