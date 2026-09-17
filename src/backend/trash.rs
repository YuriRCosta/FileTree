use super::*;

pub(super) fn trash_empty(
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(Value) -> AppResult<()>,
) -> AppResult<Value> {
    let mut output_error = None;
    let mut callback = |value| {
        if output_error.is_some() {
            return;
        }
        if let Err(error) = progress(value) {
            cancelled.store(true, Ordering::Relaxed);
            output_error = Some(error);
        }
    };
    let desktop = crate::trash::TrashContext::system().empty(cancelled, &mut callback);
    let satellites = if cancelled.load(Ordering::Relaxed) {
        json!({"ok": false, "cancelled": true, "completed": 0, "failed": 0, "error": "operation cancelled"})
    } else {
        crate::artifact_bin::empty_all(cancelled, &mut callback)
    };
    if let Some(error) = output_error {
        return Err(error);
    }
    let completed =
        desktop["completed"].as_u64().unwrap_or(0) + satellites["completed"].as_u64().unwrap_or(0);
    let failed =
        desktop["failed"].as_u64().unwrap_or(0) + satellites["failed"].as_u64().unwrap_or(0);
    let total = desktop["total"].as_u64().unwrap_or(0) + satellites["total"].as_u64().unwrap_or(0);
    let ok =
        desktop["ok"].as_bool().unwrap_or(false) && satellites["ok"].as_bool().unwrap_or(false);
    let error = combined_error(ok, &desktop, &satellites, "Trash could not be emptied");
    Ok(json!({
        "ok": ok,
        "operation": "trash-empty",
        "total": total,
        "completed": completed,
        "failed": failed,
        "cancelled": cancelled.load(Ordering::Relaxed),
        "desktop": desktop,
        "satellites": satellites,
        "error": error
    }))
}

pub(super) fn trash_list(limit: usize, cancelled: &AtomicBool) -> Value {
    let desktop = crate::trash::TrashContext::system().list(1000, cancelled);
    let satellites = if cancelled.load(Ordering::Relaxed) {
        json!({"ok": false, "entries": [], "count": 0, "modules": 0, "watch_paths": [], "errors": [], "cancelled": true})
    } else {
        crate::artifact_bin::trash_rows(1000, cancelled)
    };
    let mut entries = desktop["entries"].as_array().cloned().unwrap_or_default();
    entries.extend(
        satellites["entries"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    entries.sort_by(|left, right| {
        right["deleted_at"]
            .as_str()
            .unwrap_or("")
            .cmp(left["deleted_at"].as_str().unwrap_or(""))
            .then_with(|| {
                left["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .cmp(&right["name"].as_str().unwrap_or("").to_lowercase())
            })
    });
    let count = desktop["count"].as_u64().unwrap_or(0) + satellites["count"].as_u64().unwrap_or(0);
    let estimated_size = entries.iter().fold(0_u64, |total, entry| {
        total.saturating_add(entry["size"].as_u64().unwrap_or(0))
    });
    let unknown_size = entries
        .iter()
        .filter(|entry| entry["size"].is_null())
        .count();
    let response_truncated = entries.len() > limit;
    entries.truncate(limit);
    let mut watch_paths = desktop["watch_paths"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    watch_paths.extend(
        satellites["watch_paths"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    watch_paths.sort_by(|left, right| {
        left.as_str()
            .unwrap_or("")
            .cmp(right.as_str().unwrap_or(""))
    });
    watch_paths.dedup();
    let mut errors = desktop["errors"].as_array().cloned().unwrap_or_default();
    errors.extend(satellites["errors"].as_array().cloned().unwrap_or_default());
    let ok =
        desktop["ok"].as_bool().unwrap_or(false) && satellites["ok"].as_bool().unwrap_or(false);
    let truncated = response_truncated
        || desktop["truncated"].as_bool().unwrap_or(false)
        || satellites["truncated"].as_bool().unwrap_or(false);
    json!({
        "ok": ok,
        "resource": "trash:///",
        "entries": entries,
        "count": count,
        "stores": desktop["stores"].as_u64().unwrap_or(0),
        "satellite_modules": satellites["modules"].as_u64().unwrap_or(0),
        "watch_paths": watch_paths,
        "estimated_size": estimated_size,
        "estimated_size_text": crate::common::human_size(Some(estimated_size)),
        "unknown_size": unknown_size,
        "limit": limit,
        "truncated": truncated,
        "cancelled": cancelled.load(Ordering::Relaxed),
        "errors": errors,
        "error": if ok { "" } else { "Unable to read all Trash stores" }
    })
}

pub(super) fn combined_error(ok: bool, first: &Value, second: &Value, fallback: &str) -> String {
    if ok {
        return String::new();
    }
    [first["error"].as_str(), second["error"].as_str()]
        .into_iter()
        .flatten()
        .find(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

pub(super) fn trash_prune(
    days: i64,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(Value) -> AppResult<()>,
) -> AppResult<Value> {
    if !(1..=3650).contains(&days) {
        return Ok(json!({
            "ok": false,
            "operation": "trash-prune",
            "error": "Trash retention must be between 1 and 3650 days"
        }));
    }
    let mut output_error = None;
    let mut callback = |value| {
        if output_error.is_some() {
            return;
        }
        if let Err(error) = progress(value) {
            cancelled.store(true, Ordering::Relaxed);
            output_error = Some(error);
        }
    };
    let desktop = crate::trash::TrashContext::system().prune(days as u32, cancelled, &mut callback);
    let satellites = if cancelled.load(Ordering::Relaxed) {
        json!({"ok": false, "cancelled": true, "completed": 0, "failed": 0, "error": "operation cancelled"})
    } else {
        crate::artifact_bin::prune_all(days as u32, cancelled, &mut callback)
    };
    if let Some(error) = output_error {
        return Err(error);
    }
    let completed =
        desktop["completed"].as_u64().unwrap_or(0) + satellites["completed"].as_u64().unwrap_or(0);
    let failed =
        desktop["failed"].as_u64().unwrap_or(0) + satellites["failed"].as_u64().unwrap_or(0);
    let eligible =
        desktop["eligible"].as_u64().unwrap_or(0) + satellites["eligible"].as_u64().unwrap_or(0);
    let unknown_age = desktop["unknown_age"].as_u64().unwrap_or(0)
        + satellites["unknown_age"].as_u64().unwrap_or(0);
    let ok =
        desktop["ok"].as_bool().unwrap_or(false) && satellites["ok"].as_bool().unwrap_or(false);
    let error = combined_error(
        ok,
        &desktop,
        &satellites,
        "Trash retention cleanup did not complete",
    );
    Ok(json!({
        "ok": ok,
        "operation": "trash-prune",
        "days": days,
        "eligible": eligible,
        "completed": completed,
        "failed": failed,
        "unknown_age": unknown_age,
        "cancelled": cancelled.load(Ordering::Relaxed),
        "desktop": desktop,
        "satellites": satellites,
        "error": error
    }))
}

pub(super) fn seconds(value: f64, minimum: f64, maximum: f64) -> Duration {
    let seconds = if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        minimum
    };
    Duration::from_secs_f64(seconds)
}

pub(super) fn limited(value: i64, maximum: usize) -> usize {
    value.clamp(1, maximum as i64) as usize
}
