use super::*;

pub fn list(providers: &[String], user_dir: &str) -> Value {
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    let mut truncated = false;
    for provider in providers.iter().take(MAX_PROVIDERS) {
        if rows.len() >= MAX_ACTIONS_TOTAL || errors.len() >= MAX_ERRORS_TOTAL {
            truncated = true;
            break;
        }
        truncated |= scan_provider(provider, &mut seen, &mut rows, &mut errors);
    }
    if providers.len() > MAX_PROVIDERS {
        truncated = true;
    }
    if !user_dir.is_empty() && rows.len() < MAX_ACTIONS_TOTAL {
        truncated |= scan_user(user_dir, &mut rows, &mut errors);
    } else if !user_dir.is_empty() {
        truncated = true;
    }
    json!({
        "ok": true,
        "actions": rows,
        "truncated": truncated,
        "errors": errors,
    })
}

pub fn plugin_actions(plugin: &str, plugin_dir: &str) -> Result<(PathBuf, Vec<Action>), String> {
    if plugin == USER_PLUGIN_ID {
        return Err(reserved_id());
    }
    if !valid_provider_id(plugin) {
        return Err("plugin id is not usable for a FileTree action".to_string());
    }
    let root = plugin_root(plugin_dir)?;
    let manifest = read_manifest(&root)?;
    if manifest.get("id").and_then(Value::as_str).unwrap_or("") != plugin {
        return Err(format!(
            "{} does not carry plugin id {plugin}",
            path_text(&root)
        ));
    }
    let (actions, _, _) = normalize_source(&socket_entries(&manifest));
    Ok((root, actions))
}

pub fn user_action(actions_dir: &str, id: &str) -> Result<(PathBuf, Action), String> {
    if !valid_id(id) {
        return Err(rejected_id(id));
    }
    let root = plugin_root(actions_dir)?;
    let path = root.join(format!("{id}.json"));
    let data = read_regular_file(&path, MAX_USER_ACTION_BYTES)
        .map_err(|error| format!("{}: {error}", path_text(&path)))?;
    let raw = serde_json::from_slice::<Value>(&data)
        .map_err(|error| format!("{}: {error}", path_text(&path)))?;
    let action = normalize(&raw)?;
    if action.id != id {
        return Err(format!("{}.json declares action id {}", id, action.id));
    }
    Ok((root, action))
}

fn scan_provider(
    provider: &str,
    seen: &mut HashSet<String>,
    rows: &mut Vec<Value>,
    errors: &mut Vec<Value>,
) -> bool {
    let Some((plugin, directory)) = provider.split_once('=') else {
        return record(errors, provider, "provider must be given as id=directory");
    };
    if plugin.is_empty() || directory.is_empty() {
        return record(errors, provider, "provider must be given as id=directory");
    }
    if !valid_provider_id(plugin) {
        return record(
            errors,
            plugin,
            "plugin id is not usable for a FileTree action",
        );
    }
    if plugin == USER_PLUGIN_ID {
        return record(errors, plugin, &reserved_id());
    }
    if !seen.insert(plugin.to_string()) {
        return record(errors, plugin, "provider named more than once");
    }
    let root = match plugin_root(directory) {
        Ok(path) => path,
        Err(error) => return record(errors, plugin, &error),
    };
    let manifest = match read_manifest(&root) {
        Ok(manifest) => manifest,
        Err(error) => return record(errors, plugin, &error),
    };
    if manifest.get("id").and_then(Value::as_str).unwrap_or("") != plugin {
        return record(
            errors,
            plugin,
            "manifest.json carries a different plugin id",
        );
    }
    let (actions, problems, mut truncated) = normalize_source(&socket_entries(&manifest));
    for problem_text in problems {
        truncated |= record(errors, plugin, &problem_text);
    }
    for action in &actions {
        if rows.len() >= MAX_ACTIONS_TOTAL {
            return true;
        }
        rows.push(row(action, Source::Plugin, plugin, &root));
    }
    truncated
}

fn scan_user(user_dir: &str, rows: &mut Vec<Value>, errors: &mut Vec<Value>) -> bool {
    let root = match plugin_root(user_dir) {
        Ok(path) => path,
        Err(error) => return record(errors, USER_PLUGIN_ID, &error),
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return false;
    };
    let mut identifiers: Vec<String> = Vec::new();
    let mut candidates = 0_usize;
    let mut truncated = false;
    for entry in entries.flatten() {
        candidates += 1;
        if candidates > MAX_USER_ACTION_CANDIDATES {
            truncated = true;
            break;
        }
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
            continue;
        };
        if valid_id(id) {
            identifiers.push(id.to_string());
        }
    }
    identifiers.sort();
    truncated |= identifiers.len() > MAX_USER_ACTION_FILES;
    for id in identifiers.into_iter().take(MAX_USER_ACTION_FILES) {
        if rows.len() >= MAX_ACTIONS_TOTAL {
            return true;
        }
        match user_action(user_dir, &id) {
            Ok((_, action)) => rows.push(row(&action, Source::User, "", &root)),
            Err(error) => {
                truncated |= record(errors, &format!("{USER_PLUGIN_ID}/{id}"), &error);
            }
        }
    }
    truncated
}

pub fn row(action: &Action, source: Source, plugin: &str, root: &Path) -> Value {
    json!({
        "key": key_for(source, plugin, &action.id),
        "source": source.as_str(),
        "plugin": plugin,
        "pluginRoot": path_text(root),
        "id": action.id,
        "title": action.title,
        "glyph": action.glyph,
        "description": action.description,
        "contexts": action.contexts.iter().map(|context| context.as_str()).collect::<Vec<_>>(),
        "confirm": action.confirm,
        "detach": action.detach,
        "output": match action.output { OutputMode::Silent => "silent", OutputMode::Notice => "notice" },
        "timeout": action.timeout,
        "program": action.program(),
    })
}

pub(crate) fn read_manifest(root: &Path) -> Result<Value, String> {
    let path = root.join("manifest.json");
    let data = read_regular_file(&path, MAX_MANIFEST_BYTES)
        .map_err(|error| format!("{}: {error}", path_text(&path)))?;
    let manifest = serde_json::from_slice::<Value>(&data)
        .map_err(|error| format!("{}: {error}", path_text(&path)))?;
    if !manifest.is_object() {
        return Err(format!("{} is not a JSON object", path_text(&path)));
    }
    Ok(manifest)
}

pub fn plugin_root(directory: &str) -> Result<PathBuf, String> {
    let expanded = parse_path(directory).map_err(|error| error.to_string())?;
    Ok(std::fs::canonicalize(&expanded).unwrap_or(expanded))
}

fn record(errors: &mut Vec<Value>, source: &str, error: &str) -> bool {
    if errors.len() >= MAX_ERRORS_TOTAL {
        return true;
    }
    errors.push(json!({"source": plain(source, 128), "error": plain(error, 512)}));
    false
}

pub(crate) fn valid_provider_id(value: &str) -> bool {
    !value.contains('/') && crate::module_dirs::dir_name(value).is_some()
}

fn reserved_id() -> String {
    format!("plugin id {USER_PLUGIN_ID} is reserved for user actions")
}
