use super::*;
use crate::secure;
use std::hash::{DefaultHasher, Hash, Hasher};

static OWNS_BORDERS: AtomicBool = AtomicBool::new(false);

pub fn restore_owned_borders() {
    if OWNS_BORDERS.swap(false, Ordering::Relaxed) {
        let _ = dim_windows_owned("off", &[], Some(std::process::id()));
    }
}

pub fn restore_borders_after(owner: u32) -> Value {
    let wait = || -> AppResult<()> {
        let pid = i32::try_from(owner)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .ok_or_else(|| AppError::invalid("invalid border owner PID"))?;
        let fd = match rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::SRCH) => return Ok(()),
            Err(error) => return Err(std::io::Error::from(error).into()),
        };
        let mut descriptors = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        let timeout = rustix::fs::Timespec {
            tv_sec: 3,
            tv_nsec: 0,
        };
        if rustix::event::poll(&mut descriptors, Some(&timeout)).map_err(std::io::Error::from)? == 0
        {
            return Err(AppError::command("border owner has not exited"));
        }
        Ok(())
    };
    match wait() {
        Ok(()) => dim_windows_owned("off", &[], Some(owner)),
        Err(error) => json!({"ok": false, "error": error.to_string()}),
    }
}

pub fn dim_windows(state: &str, exclude_titles: &[String]) -> Value {
    dim_windows_owned(state, exclude_titles, None)
}

fn dim_windows_owned(state: &str, exclude_titles: &[String], owner: Option<u32>) -> Value {
    if state == "on" {
        OWNS_BORDERS.store(true, Ordering::Relaxed);
    }
    let outcome = || -> AppResult<Value> {
        let mut session = DefaultHasher::new();
        command_socket().hash(&mut session);
        std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .unwrap_or_default()
            .hash(&mut session);
        let path = crate::paths::state_dir()
            .join(format!("window-borders-{:016x}.json", session.finish()));
        let _lock = secure::open_private_lock(&path.with_extension("lock"))?;
        let mut records: Map<String, Value> =
            match secure::read_private_bounded(&path, 1024 * 1024)? {
                Some(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|error| AppError::invalid(error.to_string()))?,
                None => Map::new(),
            };
        if state != "on" && records.is_empty() {
            return Ok(json!({"ok": true, "state": state, "count": 0}));
        }
        let current = clients()?;
        records.retain(|address, record| {
            current
                .iter()
                .any(|client| same_window(client, address, record))
        });
        let mut changes = Vec::new();
        let mut errors = Vec::new();
        if state == "on" {
            for client in &current {
                let address = field_str(client, "address");
                if address.is_empty()
                    || !field_bool(client, "mapped", true)
                    || exclude_titles.contains(&field_str(client, "title"))
                {
                    continue;
                }
                let before = border_value(&address, "active_border_color")?;
                let dimmed = border_value(&address, "inactive_border_color")?;
                gradient(&before)?;
                gradient(&dimmed)?;
                if let Some(record) = records.get(&address) {
                    if record["dimmed"] != before {
                        continue;
                    }
                } else {
                    records.insert(address.clone(), json!({"pid": client["pid"], "class": client["class"], "before": before, "dimmed": dimmed}));
                }
                let record = records.get_mut(&address).unwrap();
                record["dimmed"] = json!(dimmed);
                record["owner"] = json!(std::process::id());
                changes.push((address, dimmed));
            }
            secure::write_private_atomic(&path, &serde_json::to_vec(&records).unwrap())?;
        } else {
            for (address, record) in &records {
                if owner.is_some_and(|pid| record["owner"] != pid) {
                    continue;
                }
                match border_value(address, "active_border_color") {
                    Ok(value) if record["dimmed"] == value => {
                        changes.push((address.clone(), field_str(record, "before")))
                    }
                    Ok(_) => {}
                    Err(error) => errors.push(error.to_string()),
                }
            }
        }
        let mut changed = 0;
        for (address, value) in changes {
            match set_border(&address, &value) {
                Ok(()) => changed += 1,
                Err(error) => {
                    errors.push(error.to_string());
                }
            }
        }
        if state != "on" && errors.is_empty() {
            records.retain(|_, record| owner.is_some_and(|pid| record["owner"] != pid));
            if records.is_empty() {
                secure::remove_nondirectory(&path)?;
            } else {
                secure::write_private_atomic(&path, &serde_json::to_vec(&records).unwrap())?;
            }
        }
        Ok(json!({"ok": errors.is_empty(), "state": state, "count": changed, "errors": errors}))
    };
    outcome()
        .unwrap_or_else(|error| json!({"ok": false, "state": state, "error": error.to_string()}))
}

fn same_window(client: &Value, address: &str, record: &Value) -> bool {
    client["address"] == address
        && client["pid"] == record["pid"]
        && client["class"] == record["class"]
}

fn border_value(address: &str, property: &str) -> AppResult<String> {
    let selector = window_selector(address)?;
    let response = hypr_query(&format!("getprop {selector} {property}"))?;
    response[property]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| AppError::command("Hyprland returned no border value"))
}

fn set_border(address: &str, value: &str) -> AppResult<()> {
    let selector = window_selector(address)?;
    let value = gradient(value)?;
    hypr_dispatch(&format!(
        "hl.dsp.window.set_prop({{ window = \"{selector}\", prop = \"active_border_color\", value = \"{value}\" }})"
    ))
}

fn gradient(value: &str) -> AppResult<String> {
    let mut colors = Vec::new();
    let mut angle = 0;
    for token in value.split_whitespace() {
        if let Some(degrees) = token.strip_suffix("deg") {
            angle = degrees
                .parse::<i32>()
                .map_err(|_| AppError::invalid("invalid border angle"))?;
        } else {
            let color = token.trim_start_matches("0x");
            if color.len() != 8 || !color.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(AppError::invalid("invalid border color"));
            }
            colors.push(format!("rgba({}{})", &color[2..], &color[..2]));
        }
    }
    if colors.is_empty() {
        return Ok("-1".into());
    }
    if colors.len() == 1 && angle == 0 {
        return Ok(colors.remove(0));
    }
    Ok(format!("-1 {} {angle}deg", colors.join(" ")))
}
