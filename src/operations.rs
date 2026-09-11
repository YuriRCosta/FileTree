use crate::command::{CommandSpec, which};
use crate::common::{CONTROL_TIMEOUT, parse_path, path_text};
use crate::journal::{self, TransferMapping};
use crate::secure::{self, EntryKind};
use crate::{AppError, AppResult};
use rustix::fd::OwnedFd;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub mod collisions;
mod names;
pub mod permissions;
pub use names::checked_name;
use names::{checked_rename_name, unique_copy_target};

pub type ProgressCallback<'a> = &'a mut dyn FnMut(Value);

struct CheckedDirectory {
    directory: OwnedFd,
    path: PathBuf,
}

struct MovePlan {
    source: secure::ResolvedParent,
    target: secure::ResolvedParent,
    expected: secure::EntryIdentity,
    same_target: bool,
}

fn checked_destination(path: &str) -> AppResult<CheckedDirectory> {
    let destination = parse_path(path)?;
    let directory = secure::open_directory_nofollow(&destination).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotADirectory {
            AppError::invalid(path_text(&destination))
        } else {
            AppError::from(error)
        }
    })?;
    Ok(CheckedDirectory {
        directory,
        path: destination,
    })
}

pub fn copy_paths(
    sources: &[String],
    destination: &str,
    progress: ProgressCallback<'_>,
    cancelled: &AtomicBool,
    entry_id: &str,
) -> Value {
    let mut created = Vec::new();
    let mut mappings = Vec::new();
    let journal_id = if entry_id.is_empty() {
        journal::new_entry_id()
    } else {
        entry_id.to_string()
    };
    let mut checkpoints = Checkpoints::new("copy", &journal_id);
    let outcome = (|| -> AppResult<()> {
        let target_dir = checked_destination(destination)?;
        let total = sources.len();
        for (offset, raw_source) in sources.iter().enumerate() {
            let index = offset + 1;
            let source_path = parse_path(raw_source)?;
            let source = secure::resolved_parent(&source_path)?;
            ensure_not_nested(&source_path, &target_dir.path)?;
            let name = source_path
                .file_name()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| OsStr::new("root"));
            let target = unique_copy_target(&target_dir, name)?;
            let target_path = target.full_path();
            transfer_progress(
                progress,
                "copy",
                "starting",
                (index, total),
                (&source_path, &target_path),
                None,
            );
            let mut ignored: fn(&Path) = ignore_path;
            secure::copy_path_noreplace_resolved(&source, &target, cancelled, &mut ignored)?;
            let mapping = TransferMapping {
                source: path_text(&source_path),
                destination: path_text(&target_path),
            };
            created.push(mapping.destination.clone());
            mappings.push(mapping.clone());
            checkpoints.push(mapping.clone());
            transfer_progress(
                progress,
                "copy",
                "completed",
                (index, total),
                (&source_path, &target_path),
                Some(&mapping),
            );
        }
        Ok(())
    })();
    checkpoints.flush();
    operation_result(
        "copy",
        created,
        mappings,
        &journal_id,
        outcome,
        checkpoints.warning(),
    )
}

pub fn move_paths(
    sources: &[String],
    destination: &str,
    progress: ProgressCallback<'_>,
    cancelled: &AtomicBool,
    entry_id: &str,
) -> Value {
    let mut moved = Vec::new();
    let mut mappings = Vec::new();
    let journal_id = if entry_id.is_empty() {
        journal::new_entry_id()
    } else {
        entry_id.to_string()
    };
    let mut checkpoints = Checkpoints::new("move", &journal_id);
    let outcome = (|| -> AppResult<()> {
        let destination = checked_destination(destination)?;
        let plans = move_plan(sources, &destination)?;
        validate_move_sources(&plans)?;
        let total = plans.len();
        for (offset, plan) in plans.into_iter().enumerate() {
            let index = offset + 1;
            let source = plan.source.full_path();
            let target = plan.target.full_path();
            transfer_progress(
                progress,
                "move",
                "starting",
                (index, total),
                (&source, &target),
                None,
            );
            if !plan.same_target {
                let mut ignored: fn(&Path) = ignore_path;
                secure::relocate_noreplace_matching_resolved(
                    plan.source,
                    &plan.target,
                    plan.expected,
                    cancelled,
                    &mut ignored,
                )?;
            }
            let mapping = TransferMapping {
                source: path_text(&source),
                destination: path_text(&target),
            };
            moved.push(mapping.destination.clone());
            mappings.push(mapping.clone());
            checkpoints.push(mapping.clone());
            transfer_progress(
                progress,
                "move",
                "completed",
                (index, total),
                (&source, &target),
                Some(&mapping),
            );
        }
        Ok(())
    })();
    checkpoints.flush();
    operation_result(
        "move",
        moved,
        mappings,
        &journal_id,
        outcome,
        checkpoints.warning(),
    )
}

pub fn rename_path(path: &str, name: &str, entry_id: &str) -> Value {
    rename_path_with_name_format(path, name, entry_id, false)
}

pub fn rename_path_with_name_format(
    path: &str,
    name: &str,
    entry_id: &str,
    escaped: bool,
) -> Value {
    let outcome = (|| -> AppResult<(PathBuf, TransferMapping)> {
        let source_path = parse_path(path)?;
        let source = secure::resolved_parent(&source_path)?;
        let checked = checked_rename_name(name, escaped)?;
        let target = secure::resolved_child(&source.directory, &source.path, checked.as_ref())?;
        let target_path = target.full_path();
        if target_path != source_path {
            let expected = secure::entry_stat_resolved(&source)?.identity();
            if secure::entry_exists_resolved(&target)? {
                return Err(AppError::invalid(format!(
                    "{} already exists",
                    target_path.display()
                )));
            }
            let active = AtomicBool::new(false);
            let mut ignored: fn(&Path) = ignore_path;
            secure::relocate_noreplace_matching_resolved(
                source,
                &target,
                expected,
                &active,
                &mut ignored,
            )?;
        } else if !secure::entry_exists_resolved(&source)? {
            return Err(
                std::io::Error::new(std::io::ErrorKind::NotFound, path_text(&source_path)).into(),
            );
        }
        let mapping = TransferMapping {
            source: path_text(&source_path),
            destination: path_text(&target_path),
        };
        Ok((target_path, mapping))
    })();
    match outcome {
        Ok((target, mapping)) => {
            let recorded =
                journal::record_transfer("rename", std::slice::from_ref(&mapping), entry_id);
            let mut value = json!({
            "ok": true,
            "operation": "rename",
            "path": path_text(&target),
            "mappings": [mapping_value(&mapping)],
            "journal_id": recorded.as_deref().unwrap_or_default(),
            });
            if let Err(error) = recorded {
                value["journal_warning"] = json!(error.to_string());
            }
            value
        }
        Err(error) => json!({
            "ok": false,
            "operation": "rename",
            "mappings": [],
            "error": error.to_string(),
        }),
    }
}

pub fn create_path(parent: &str, name: &str, directory: bool, entry_id: &str) -> Value {
    let outcome = (|| -> AppResult<PathBuf> {
        let parent = checked_destination(parent)?;
        let checked = checked_name(name)?;
        let target = secure::resolved_child(&parent.directory, &parent.path, checked.as_ref())?;
        let target_path = target.full_path();
        if secure::entry_exists_resolved(&target)? {
            return Err(AppError::invalid(format!(
                "{} already exists",
                target_path.display()
            )));
        }
        if directory {
            secure::create_directory_noreplace_resolved(&target, 0o777)?;
        } else {
            secure::create_file_noreplace_resolved(&target, 0o666)?.sync_all()?;
        }
        Ok(target_path)
    })();
    match outcome {
        Ok(target) => {
            let recorded = journal::record_create(&path_text(&target), directory, entry_id);
            let mut value = json!({
            "ok": true,
            "operation": "create",
            "path": path_text(&target),
            "is_dir": directory,
            "journal_id": recorded.as_deref().unwrap_or_default(),
            });
            if let Err(error) = recorded {
                value["journal_warning"] = json!(error.to_string());
            }
            value
        }
        Err(error) => json!({
            "ok": false,
            "operation": "create",
            "error": error.to_string(),
        }),
    }
}

pub fn trash_paths(
    paths: &[String],
    entry_id: &str,
    follow_symlinks: bool,
    cancelled: &AtomicBool,
) -> Value {
    let mut seen = std::collections::HashSet::new();
    let resolved: std::io::Result<Vec<_>> = paths
        .iter()
        .map(|path| {
            let logical = parse_path(path)?;
            if follow_symlinks {
                std::fs::canonicalize(&logical).map_err(|error| {
                    std::io::Error::new(
                        error.kind(),
                        format!("cannot resolve {}: {error}", logical.display()),
                    )
                })
            } else {
                Ok(logical)
            }
        })
        .filter_map(|result| match result {
            Ok(path) if seen.insert(path.clone()) => Some(Ok(path)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect();
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(error) => {
            return json!({
                "ok": false,
                "operation": "trash",
                "paths": [],
                "journal_id": "",
                "journaled": 0,
                "error": error.to_string(),
            });
        }
    };
    let mut trashed = Vec::new();
    let mut error = String::new();
    for path in &resolved {
        match journal::trash_path(path, cancelled) {
            Ok(item) => trashed.push(item),
            Err(current) => {
                error = current.to_string();
                break;
            }
        }
    }
    let journal_id = if trashed.is_empty() {
        Ok(String::new())
    } else {
        journal::record_trash(&trashed, entry_id)
    };
    let completed: Vec<_> = trashed.iter().map(|item| item.source.clone()).collect();
    let ok = error.is_empty() && trashed.len() == resolved.len();
    match journal_id {
        Ok(journal_id) => json!({
            "ok": ok,
            "operation": "trash",
            "paths": completed,
            "journal_id": journal_id,
            "journaled": trashed.len(),
            "error": error,
        }),
        Err(journal_error) => json!({
            "ok": false,
            "operation": "trash",
            "paths": completed,
            "journal_id": "",
            "journaled": 0,
            "error": journal_error.to_string(),
        }),
    }
}

pub fn set_default_application(mime: &str, desktop_id: &str, cancelled: &AtomicBool) -> Value {
    if !crate::filesystem::valid_mime(mime) || !crate::desktop::valid_desktop_id(desktop_id) {
        return json!({
            "ok": false,
            "operation": "set-default",
            "error": "Invalid MIME type or application",
        });
    }
    let outcome = (|| -> AppResult<()> {
        let program = which("xdg-mime").unwrap_or_else(|| PathBuf::from("xdg-mime"));
        let output = CommandSpec::new(program)
            .args(["default", desktop_id, mime])
            .timeout(CONTROL_TIMEOUT)
            .limits(64 * 1024, 64 * 1024)
            .run_cancellable(cancelled)?;
        if output.status.success() {
            Ok(())
        } else {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(AppError::command(if message.is_empty() {
                "xdg-mime failed".to_string()
            } else {
                message
            }))
        }
    })();
    match outcome {
        Ok(()) => json!({
            "ok": true,
            "operation": "set-default",
            "mime": mime,
            "desktop_id": desktop_id,
            "error": "",
        }),
        Err(error) => json!({
            "ok": false,
            "operation": "set-default",
            "error": error.to_string(),
        }),
    }
}

fn ensure_not_nested(source: &Path, destination: &Path) -> AppResult<()> {
    let stat = match secure::entry_stat(source) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if stat.kind != EntryKind::Directory {
        return Ok(());
    }
    let source_real = std::fs::canonicalize(source)?;
    let destination_real = std::fs::canonicalize(destination)?;
    if destination_real.starts_with(&source_real) {
        Err(AppError::invalid(
            "A directory cannot be copied or moved into itself",
        ))
    } else {
        Ok(())
    }
}

fn move_plan(sources: &[String], destination: &CheckedDirectory) -> AppResult<Vec<MovePlan>> {
    let mut plans = Vec::new();
    let mut reserved = BTreeSet::new();
    for raw_source in sources {
        let source_path = parse_path(raw_source)?;
        let source = secure::resolved_parent(&source_path)?;
        let expected = secure::entry_stat_resolved(&source)?.identity();
        ensure_not_nested(&source_path, &destination.path)?;
        let name = source_path.file_name().ok_or_else(|| {
            AppError::invalid(format!("{} has no file name", source_path.display()))
        })?;
        let target = secure::resolved_child(&destination.directory, &destination.path, name)?;
        let target_path = target.full_path();
        let same_target = secure::same_resolved_target(&source, &target)?;
        if same_target {
            plans.push(MovePlan {
                source,
                target,
                expected,
                same_target,
            });
        } else if reserved.contains(&target_path) || secure::entry_exists_resolved(&target)? {
            return Err(AppError::invalid(format!(
                "{} already exists",
                target_path.display()
            )));
        } else {
            reserved.insert(target_path);
            plans.push(MovePlan {
                source,
                target,
                expected,
                same_target,
            });
        }
    }
    Ok(plans)
}

fn validate_move_sources(plans: &[MovePlan]) -> AppResult<()> {
    for (index, plan) in plans.iter().enumerate() {
        if plan.expected.kind != EntryKind::Directory {
            continue;
        }
        if plans.iter().enumerate().any(|(other_index, other)| {
            other_index != index
                && other
                    .source
                    .full_path()
                    .starts_with(plan.source.full_path())
        }) {
            return Err(AppError::invalid(
                "A folder and one of its descendants cannot be moved together",
            ));
        }
    }
    Ok(())
}

fn transfer_progress(
    callback: &mut dyn FnMut(Value),
    operation: &str,
    phase: &str,
    position: (usize, usize),
    paths: (&Path, &Path),
    mapping: Option<&TransferMapping>,
) {
    let (index, total) = position;
    let (source, target) = paths;
    let mut payload = json!({
        "type": "progress",
        "operation": operation,
        "phase": phase,
        "index": index,
        "total": total,
        "completed": if phase == "completed" { index } else { index.saturating_sub(1) },
        "source": path_text(source),
        "target": path_text(target),
    });
    if phase == "completed" {
        payload["path"] = Value::String(path_text(target));
    }
    if let Some(mapping) = mapping {
        payload["mapping"] = mapping_value(mapping);
    }
    callback(payload);
}

struct Checkpoints {
    kind: &'static str,
    id: String,
    pending: Vec<TransferMapping>,
    flushed_at: Instant,
    warning: Option<String>,
}

impl Checkpoints {
    const BATCH_ITEMS: usize = 32;
    const BATCH_INTERVAL: Duration = Duration::from_millis(250);

    fn new(kind: &'static str, id: &str) -> Self {
        Self {
            kind,
            id: id.to_string(),
            pending: Vec::new(),
            flushed_at: Instant::now(),
            warning: None,
        }
    }

    fn push(&mut self, mapping: TransferMapping) {
        self.pending.push(mapping);
        if self.pending.len() >= Self::BATCH_ITEMS
            || self.flushed_at.elapsed() >= Self::BATCH_INTERVAL
        {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        match journal::record_transfer(self.kind, &self.pending, &self.id) {
            Ok(_) => {
                self.pending.clear();
                self.warning = None;
            }
            Err(error) => self.warning = Some(error.to_string()),
        }
        self.flushed_at = Instant::now();
    }

    fn warning(&self) -> Option<String> {
        self.warning.as_ref().map(|error| {
            format!(
                "{} item(s) were not journaled and cannot be undone: {error}",
                self.pending.len()
            )
        })
    }
}

fn operation_result(
    operation: &str,
    paths: Vec<String>,
    mappings: Vec<TransferMapping>,
    journal_id: &str,
    outcome: AppResult<()>,
    journal_warning: Option<String>,
) -> Value {
    let mappings: Vec<_> = mappings.iter().map(mapping_value).collect();
    let id = if mappings.is_empty() { "" } else { journal_id };
    let mut value = match outcome {
        Ok(()) => json!({
            "ok": true,
            "operation": operation,
            "paths": paths,
            "mappings": mappings,
            "journal_id": id,
        }),
        Err(error) => json!({
            "ok": false,
            "operation": operation,
            "paths": paths,
            "mappings": mappings,
            "journal_id": id,
            "error": error.to_string(),
        }),
    };
    if let Some(warning) = journal_warning {
        value["journal_warning"] = Value::String(warning);
    }
    value
}

fn mapping_value(mapping: &TransferMapping) -> Value {
    json!({"source": mapping.source, "destination": mapping.destination})
}

fn ignore_path(_: &Path) {}
