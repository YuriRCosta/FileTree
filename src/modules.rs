use crate::common::{parse_path, path_error, path_text};
use crate::filesystem::read_regular_file;
use serde_json::{Value, json};
use std::path::Path;

pub const MAX_DEFINITION_BYTES: usize = 16 * 1024;
pub const MAX_DEFINITIONS: usize = 128;
pub const MAX_CANDIDATES: usize = 256;
const MAX_AGGREGATE_BYTES: usize = MAX_DEFINITION_BYTES * MAX_DEFINITIONS;

pub fn discover_blade_modules(builtin_root: &str, user_root: &str) -> Value {
    let mut modules = Vec::new();
    let mut candidates = 0_usize;
    let mut aggregate = 0_usize;
    let builtin = match parse_path(builtin_root) {
        Ok(path) => path.join("modules"),
        Err(error) => return path_error(builtin_root, &error),
    };
    let mut truncated = discover_root(
        &builtin,
        "builtin",
        &mut modules,
        &mut candidates,
        &mut aggregate,
    );
    if !truncated && !user_root.is_empty() {
        let user = match parse_path(user_root) {
            Ok(path) => path,
            Err(error) => return path_error(user_root, &error),
        };
        truncated = discover_root(&user, "user", &mut modules, &mut candidates, &mut aggregate);
    }
    json!({"ok": true, "modules": modules, "truncated": truncated})
}

fn discover_root(
    root: &Path,
    source: &str,
    result: &mut Vec<Value>,
    candidates: &mut usize,
    aggregate: &mut usize,
) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    for entry in entries {
        *candidates += 1;
        if *candidates > MAX_CANDIDATES
            || result.len() >= MAX_DEFINITIONS
            || *aggregate >= MAX_AGGREGATE_BYTES
        {
            return true;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let source_dir = entry.path();
        let definition_path = source_dir.join("blade.json");
        let Ok(data) = read_regular_file(&definition_path, MAX_DEFINITION_BYTES) else {
            continue;
        };
        if aggregate.saturating_add(data.len()) > MAX_AGGREGATE_BYTES {
            return true;
        }
        let Ok(definition) = serde_json::from_slice::<Value>(&data) else {
            continue;
        };
        if !definition.is_object() {
            continue;
        }
        *aggregate += data.len();
        result.push(json!({
            "definition": definition,
            "source_dir": path_text(&source_dir),
            "source": source
        }));
    }
    false
}
