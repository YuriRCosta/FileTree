use super::canonical::compact_ascii_json;
use serde_json::{Map, Value, json};

pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const OVERFLOW_DOCUMENT: &str =
    "{\"ok\":false,\"schemaVersion\":1,\"truncated\":true,\"items\":[]}\n";

pub fn serialized(value: &Value) -> Vec<u8> {
    let mut text = compact_ascii_json(value);
    text.push('\n');
    text.into_bytes()
}

fn truncated_flag(payload: &Map<String, Value>) -> bool {
    match payload.get("truncated") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(entries)) => !entries.is_empty(),
    }
}

pub fn encoded(payload: &Map<String, Value>, max_bytes: usize) -> Vec<u8> {
    let Some(Value::Array(items)) = payload.get("items") else {
        return serialized(&Value::Object(payload.clone()));
    };
    let document = |count: usize, truncated: bool| {
        let mut value = payload.clone();
        value.insert("items".to_string(), Value::Array(items[..count].to_vec()));
        value.insert("count".to_string(), json!(count));
        value.insert(
            "truncated".to_string(),
            json!(truncated_flag(payload) || truncated),
        );
        serialized(&Value::Object(value))
    };
    let data = document(items.len(), false);
    if data.len() <= max_bytes {
        return data;
    }
    let (mut low, mut high) = (0usize, items.len());
    let mut best = document(0, true);
    while low <= high {
        let middle = (low + high) / 2;
        let candidate = document(middle, true);
        if candidate.len() <= max_bytes {
            best = candidate;
            low = middle + 1;
        } else {
            if middle == 0 {
                break;
            }
            high = middle - 1;
        }
    }
    best
}

pub fn encoded_items(payload: &Map<String, Value>) -> Vec<u8> {
    if !matches!(payload.get("items"), Some(Value::Array(_))) {
        let mut value = payload.clone();
        value.insert("items".to_string(), Value::Array(Vec::new()));
        value.insert("count".to_string(), json!(0));
        value.insert("truncated".to_string(), json!(truncated_flag(payload)));
        let data = serialized(&Value::Object(value));
        return if data.len() <= MAX_OUTPUT_BYTES {
            data
        } else {
            OVERFLOW_DOCUMENT.as_bytes().to_vec()
        };
    }
    encoded(payload, MAX_OUTPUT_BYTES)
}

pub fn encoded_results(payload: &Map<String, Value>) -> Vec<u8> {
    let data = serialized(&Value::Object(payload.clone()));
    if data.len() <= MAX_OUTPUT_BYTES {
        return data;
    }
    let mut trimmed = payload.clone();
    trimmed.insert("results".to_string(), Value::Array(Vec::new()));
    trimmed.insert("ok".to_string(), json!(false));
    trimmed.insert(
        "message".to_string(),
        json!("apply output exceeded the size bound"),
    );
    serialized(&Value::Object(trimmed))
}
