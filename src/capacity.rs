use crate::mounts::human_size;
use crate::mounts::mountinfo::MountTable;
use crate::{AppError, AppResult};
use rustix::fs::StatVfs;
use serde_json::{Value, json};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

static PROBE_RUNNING: AtomicBool = AtomicBool::new(false);

struct ProbeGuard;

impl Drop for ProbeGuard {
    fn drop(&mut self) {
        PROBE_RUNNING.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Usage {
    pub size: u64,
    pub used: u64,
    pub available: u64,
    pub fraction: Option<f64>,
    pub percent: Option<u64>,
}

pub fn usage_of(stat: &StatVfs) -> Option<Usage> {
    let (frsize, blocks, bfree, bavail) =
        (stat.f_frsize, stat.f_blocks, stat.f_bfree, stat.f_bavail);
    if frsize == 0
        || blocks == 0
        || bfree > blocks
        || bavail > bfree
        || [blocks, bfree, bavail].contains(&u64::MAX)
    {
        return None;
    }
    let frsize = u128::from(frsize);
    let size = u64::try_from(u128::from(blocks) * frsize).ok()?;
    let used = u64::try_from(u128::from(blocks - bfree) * frsize).ok()?;
    let available = u64::try_from(u128::from(bavail) * frsize).ok()?;
    let denominator = u128::from(used) + u128::from(available);
    let (fraction, percent) = if denominator == 0 {
        (None, None)
    } else {
        let percent = (u128::from(used) * 100).div_ceil(denominator);
        (
            Some(used as f64 / denominator as f64),
            Some(u64::try_from(percent).ok()?),
        )
    };
    Some(Usage {
        size,
        used,
        available,
        fraction,
        percent,
    })
}

pub fn usage_at(mountpoint: &str) -> Option<Usage> {
    let path = crate::common::parse_path(mountpoint).ok()?;
    usage_of(&rustix::fs::statvfs(&path).ok()?)
}

pub fn usage_json(usage: Option<&Usage>) -> Value {
    match usage {
        Some(usage) => json!({
            "size": usage.size,
            "used": usage.used,
            "available": usage.available,
            "fraction": usage.fraction,
            "percent": usage.percent,
            "size_label": human_size(usage.size),
            "used_label": human_size(usage.used),
            "available_label": human_size(usage.available),
        }),
        None => json!({
            "size": null,
            "used": null,
            "available": null,
            "fraction": null,
            "percent": null,
            "size_label": null,
            "used_label": null,
            "available_label": null,
        }),
    }
}

fn hold_for_tests() {
    #[cfg(debug_assertions)]
    if let Some(milliseconds) = std::env::var("FILEBLADE_CAPACITY_HOLD_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(milliseconds.min(60_000)));
    }
}

pub fn probe(raw_path: &str) -> AppResult<Value> {
    let path = crate::common::parse_path(raw_path)?;
    if PROBE_RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(json!({
            "ok": false,
            "busy": true,
            "error": "capacity probe still running",
        }));
    }
    let _guard = ProbeGuard;
    hold_for_tests();
    let canonical = std::fs::canonicalize(&path).map_err(|error| {
        AppError::command(format!("{}: {error}", crate::common::path_text(&path)))
    })?;
    if !canonical.is_dir() {
        return Err(AppError::invalid(format!(
            "{} is not a folder",
            crate::common::path_text(&canonical)
        )));
    }
    let directory = crate::secure::open_directory_nofollow(&canonical)?;
    let stat = rustix::fs::fstatvfs(&directory).map_err(io::Error::from)?;
    let usage = usage_of(&stat);
    let record = crate::secure::directory_mount_id(&directory)
        .ok()
        .and_then(|mount_id| {
            MountTable::read()
                .ok()
                .and_then(|table| table.record_for(mount_id, &canonical).cloned())
        });
    let mut document = usage_json(usage.as_ref());
    let object = document
        .as_object_mut()
        .expect("usage document is an object");
    object.insert("ok".into(), Value::Bool(true));
    object.insert(
        "path".into(),
        Value::String(crate::common::path_text(&canonical)),
    );
    object.insert(
        "mountpoint".into(),
        record.as_ref().map_or(Value::Null, |record| {
            Value::String(crate::common::path_text(&record.mountpoint))
        }),
    );
    object.insert(
        "filesystem".into(),
        record.as_ref().map_or(Value::Null, |record| {
            Value::String(record.filesystem.clone())
        }),
    );
    object.insert(
        "source".into(),
        record
            .as_ref()
            .map_or(Value::Null, |record| Value::String(record.source.clone())),
    );
    Ok(document)
}

pub fn describe(document: &Value) -> String {
    let text = |key: &str| document[key].as_str().map(str::to_string);
    let place = match (text("mountpoint"), text("filesystem")) {
        (Some(mountpoint), Some(filesystem)) => format!(" on {mountpoint} ({filesystem})"),
        (Some(mountpoint), None) => format!(" on {mountpoint}"),
        _ => String::new(),
    };
    match (
        text("used_label"),
        text("size_label"),
        document["percent"].as_u64(),
        text("available_label"),
    ) {
        (Some(used), Some(size), Some(percent), Some(available)) => {
            format!("{used} of {size} used, {percent}% full, {available} free{place}")
        }
        _ => format!("capacity unavailable{place}"),
    }
}
