use super::text::{estimated_tokens, word_count};
use chrono::{DateTime, Local};
use serde_json::{Map, Value, json};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_DESCRIPTOR_BYTES: u64 = 8 * 1024;

pub fn minute_stamp(seconds: i64, nanoseconds: u32) -> String {
    let moment = if seconds >= 0 {
        UNIX_EPOCH.checked_add(Duration::new(seconds as u64, nanoseconds))
    } else {
        UNIX_EPOCH.checked_sub(Duration::new(seconds.unsigned_abs(), 0))
    };
    match moment {
        Some(moment) => DateTime::<Local>::from(moment)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => String::new(),
    }
}

fn modified_stamp(time: SystemTime) -> String {
    match time.duration_since(UNIX_EPOCH) {
        Ok(elapsed) => minute_stamp(elapsed.as_secs() as i64, elapsed.subsec_nanos()),
        Err(error) => minute_stamp(-(error.duration().as_secs() as i64), 0),
    }
}

pub fn creation_time(path: &Path) -> String {
    use rustix::fs::{AtFlags, CWD, StatxFlags, statx};
    statx(CWD, path, AtFlags::empty(), StatxFlags::BTIME)
        .ok()
        .filter(|stat| StatxFlags::from_bits_retain(stat.stx_mask).contains(StatxFlags::BTIME))
        .filter(|stat| stat.stx_btime.tv_sec > 0)
        .map(|stat| minute_stamp(stat.stx_btime.tv_sec, stat.stx_btime.tv_nsec))
        .unwrap_or_default()
}

pub fn artifact_metrics(path: &Path, text: &str, size: u64) -> Map<String, Value> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Map::new();
    };
    let complete = size <= MAX_DESCRIPTOR_BYTES;
    let counted = |value: Value| if complete { value } else { Value::Null };
    let updated = metadata.modified().map(modified_stamp).unwrap_or_default();
    let mut metrics = Map::new();
    metrics.insert("updated".to_string(), json!(updated));
    metrics.insert("created".to_string(), json!(creation_time(path)));
    metrics.insert("bytes".to_string(), json!(size));
    metrics.insert(
        "characters".to_string(),
        counted(json!(text.chars().count())),
    );
    metrics.insert("words".to_string(), counted(json!(word_count(text))));
    metrics.insert("tokens".to_string(), counted(json!(estimated_tokens(text))));
    metrics
}
