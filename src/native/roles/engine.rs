use super::receipt::{self, OwnedEntry, Receipt};
use super::{Role, entry};
use crate::{AppError, AppResult};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub struct Outcome {
    pub status: &'static str,
    pub conflict: String,
    pub remaining: Vec<PathBuf>,
    pub error: String,
}

impl Outcome {
    fn new(status: &'static str) -> Self {
        Self {
            status,
            conflict: String::new(),
            remaining: Vec::new(),
            error: String::new(),
        }
    }

    fn refused(error: impl std::fmt::Display) -> Self {
        Self {
            error: error.to_string(),
            ..Self::new("refused")
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "status": self.status,
            "conflict": self.conflict,
            "remaining_owned_entries": self.remaining.iter().map(|path| crate::common::path_text(path)).collect::<Vec<_>>(),
            "error": self.error,
        })
    }
}

pub fn stable_launcher() -> AppResult<PathBuf> {
    let app_root = std::fs::canonicalize(crate::paths::app_root()?)?;
    let local = crate::paths::xdg_home("HOME", "~").join(".local/bin/fileblade");
    let installed = std::fs::canonicalize(&local)
        .ok()
        .and_then(|launcher| launcher.parent().map(Path::to_path_buf))
        .and_then(|installation| std::fs::canonicalize(installation.join("active/runtime")).ok())
        .is_some_and(|runtime| runtime == app_root);
    if installed {
        return Ok(local);
    }
    let system = PathBuf::from("/usr/bin/fileblade");
    if system.is_file() {
        return Ok(system);
    }
    Err(AppError::command(
        "no stable launcher: neither ~/.local/bin/fileblade for this runtime nor /usr/bin/fileblade exists",
    ))
}

pub fn enable(receipt: &mut Receipt, role: Role) -> Outcome {
    let state = receipt.state(role.name());
    if state.enabled {
        return Outcome::new("already_on");
    }
    if !state.entries.is_empty() {
        return Outcome::refused(format!(
            "desktop role {} has an unfinished change; disable it first",
            role.name()
        ));
    }
    let launcher = match stable_launcher() {
        Ok(launcher) => launcher,
        Err(error) => return Outcome::refused(error),
    };
    let planned = role.plan(&launcher);
    let mut outcome = Outcome::new("enabled");
    outcome.conflict = role.conflict();
    let result = (|| -> AppResult<()> {
        receipt.state_mut(role.name()).intent = Some("enabling".into());
        receipt::save(receipt)?;
        for item in planned {
            let prior = entry::current(&item.path, item.key.as_deref())?;
            receipt.state_mut(role.name()).entries.push(OwnedEntry {
                path: item.path.clone(),
                key: item.key.clone(),
                prior,
                owned: item.owned.clone(),
            });
            receipt::save(receipt)?;
            entry::write(&item.path, item.key.as_deref(), Some(&item.owned))?;
        }
        let state = receipt.state_mut(role.name());
        state.enabled = true;
        state.intent = None;
        receipt::save(receipt)
    })();
    if let Err(error) = result {
        outcome.status = "partial";
        outcome.error = error.to_string();
        outcome.remaining = receipt
            .state(role.name())
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        return outcome;
    }
    outcome.error = role.after();
    outcome
}

pub fn disable(receipt: &mut Receipt, role: Role) -> Outcome {
    let state = receipt.state(role.name());
    if state.entries.is_empty() {
        if receipt
            .roles
            .remove(role.name())
            .is_some_and(|state| state.enabled || state.intent.is_some())
            && let Err(error) = receipt::save(receipt)
        {
            return Outcome::refused(error);
        }
        return Outcome::new("already_off");
    }
    let mut outcome = Outcome::new("restored");
    let mut failures = Vec::new();
    receipt.state_mut(role.name()).intent = Some("disabling".into());
    if let Err(error) = receipt::save(receipt) {
        outcome.status = "partial";
        outcome.error = error.to_string();
        outcome.remaining = state
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        return outcome;
    }
    for entry in state.entries.iter().rev() {
        match reverse(entry) {
            Ok(true) => {}
            Ok(false) => outcome.status = "preserved_newer",
            Err(error) => {
                failures.push(format!("{}: {error}", entry.path.display()));
                continue;
            }
        }
        let owned = &mut receipt.state_mut(role.name()).entries;
        owned.retain(|kept| kept != entry);
        if let Err(error) = receipt::save(receipt) {
            failures.push(format!("receipt: {error}"));
            break;
        }
    }
    let remaining = receipt.state(role.name()).entries;
    if remaining.is_empty() {
        receipt.roles.remove(role.name());
        if let Err(error) = receipt::save(receipt) {
            failures.push(format!("receipt: {error}"));
        }
    }
    if !failures.is_empty() {
        outcome.status = "partial";
        outcome.error = failures.join("; ");
        outcome.remaining = remaining.iter().map(|entry| entry.path.clone()).collect();
        return outcome;
    }
    outcome.error = role.after();
    outcome
}

fn reverse(entry: &OwnedEntry) -> AppResult<bool> {
    let current = entry::current(&entry.path, entry.key.as_deref())?;
    if current.as_deref() == Some(entry.owned.as_str()) {
        entry::write(&entry.path, entry.key.as_deref(), entry.prior.as_deref())?;
        return Ok(true);
    }
    Ok(current == entry.prior)
}

pub fn status(receipt: &Receipt) -> Value {
    let launcher = stable_launcher()
        .ok()
        .map(|path| crate::common::path_text(&path));
    let roles: serde_json::Map<String, Value> = Role::ALL
        .iter()
        .map(|role| {
            let state = receipt.state(role.name());
            (
                role.name().to_string(),
                json!({
                    "enabled": state.enabled,
                    "launcher": launcher,
                    "conflict": if state.enabled { role.conflict() } else { String::new() },
                    "entries": state.entries.iter().map(|entry| crate::common::path_text(&entry.path)).collect::<Vec<_>>(),
                }),
            )
        })
        .collect();
    json!({"schema": 1, "action": "roles_status", "error": "", "roles": roles})
}
