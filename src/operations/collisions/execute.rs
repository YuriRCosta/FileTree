use super::*;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    pub action: String,
    #[serde(default)]
    pub apply_to_remaining: bool,
}

pub fn execute(
    decision_id: &str,
    decisions: &[Decision],
    cancel: bool,
    progress: ProgressCallback<'_>,
    cancelled: &AtomicBool,
) -> Value {
    let plan = PLANS.lock().ok().and_then(|mut retained| {
        retained
            .as_mut()
            .and_then(|plans| plans.remove(decision_id))
    });
    let Some(mut plan) = plan.filter(|plan| plan.created.elapsed() < PLAN_LIFETIME) else {
        return json!({"ok": false, "error": "transfer decision expired or was already used; review a new preflight", "consumed": true});
    };
    if cancel {
        return json!({"ok": false, "cancelled": true, "consumed": true, "paths": [], "mappings": [], "completed_sources": []});
    }
    let selected = match selections(&plan, decisions).and_then(|selected| {
        validate(&plan, &selected, cancelled)?;
        Ok(selected)
    }) {
        Ok(selected) => selected,
        Err(error) => {
            return json!({"ok": false, "consumed": true, "cancelled": matches!(error, AppError::Cancelled), "error": error.to_string(), "paths": [], "mappings": [], "completed_sources": []});
        }
    };
    match expand_merges(&mut plan.items, &selected, cancelled) {
        Ok(true) => {
            let accepted: Vec<_> = selected.iter().enumerate().filter_map(|(index, action)| {
                if matches!(action.as_str(), "inactive" | "transfer") { return None; }
                let remaining = decisions.iter().find(|decision| decision.id == index.to_string()).is_some_and(|decision| decision.apply_to_remaining);
                Some(json!({"id": index.to_string(), "action": action, "apply_to_remaining": remaining}))
            }).collect();
            let mut result = retain(plan);
            if result["ok"] == true {
                result["requires_decision"] = json!(true);
                result["accepted_decisions"] = json!(accepted);
            }
            return result;
        }
        Ok(false) => (),
        Err(error) => {
            return json!({"ok": false, "consumed": true, "error": error.to_string(), "paths": [], "mappings": [], "completed_sources": []});
        }
    }
    let mut run = TransferRun::new(plan.copy);
    let result = (|| -> AppResult<()> {
        for (index, (item, action)) in plan.items.iter().zip(&selected).enumerate() {
            if cancelled.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            match action.as_str() {
                "inactive" | "merge" => continue,
                "skip" => {
                    run.skipped.push(path_text(&item.source));
                    continue;
                }
                _ => (),
            }
            transfer_progress(
                progress,
                run.operation(),
                "starting",
                (index + 1, plan.items.len()),
                (&item.source, &item.target),
                None,
            );
            run.item(item, action, cancelled)?;
            if let Some(mapping) = run.mappings.last() {
                transfer_progress(
                    progress,
                    run.operation(),
                    "completed",
                    (index + 1, plan.items.len()),
                    (&item.source, &parse_path(&mapping.destination)?),
                    Some(mapping),
                );
            }
        }
        if !plan.copy {
            for (item, action) in plan.items.iter().zip(&selected).rev() {
                if action != "merge" {
                    continue;
                }
                if cancelled.load(Ordering::Relaxed) {
                    return Err(AppError::Cancelled);
                }
                let source = secure::resolved_parent(&item.source)?;
                let stat = secure::entry_stat_resolved(&source)?;
                if stat.identity() != item.source_stat.identity() {
                    return Err(AppError::invalid("merged source directory was replaced"));
                }
                if !secure::directory_names(&item.source)?.is_empty() {
                    run.retained_folders.push(path_text(&item.source));
                    continue;
                }
                let removed = journal::trash_matching_resolved(source, stat, true, cancelled)?;
                run.completed_sources.push(path_text(&item.source));
                run.history.push(removed);
                run.flush()?;
            }
        }
        Ok(())
    })();
    let outcome = match (result, run.flush()) {
        (Err(error), _) => Err(error),
        (_, result) => result,
    };
    let error = outcome.as_ref().err().map(ToString::to_string);
    json!({"ok": outcome.is_ok(), "operation": run.operation(), "consumed": true,
        "cancelled": cancelled.load(Ordering::Relaxed) || matches!(&outcome, Err(AppError::Cancelled)), "partial": outcome.is_err() && !run.history.is_empty(),
        "paths": run.history.iter().map(|item| if item.target.is_empty() { &item.source } else { &item.target }).collect::<BTreeSet<_>>(),
        "displaced": run.displaced,
        "mappings": run.mappings.iter().map(mapping_value).collect::<Vec<_>>(), "completed_sources": run.completed_sources, "skipped": run.skipped,
        "retained_folders": run.retained_folders, "journal_id": if run.history.is_empty() { "" } else { &run.id },
        "error": error, "journal_warning": run.warning,
        "recovery": if run.warning.is_some() { serde_json::to_value(&run.history).unwrap_or_default() } else { Value::Null }})
}

fn selections(plan: &Plan, decisions: &[Decision]) -> AppResult<Vec<String>> {
    if decisions.len() > MAX_ITEMS {
        return Err(AppError::invalid("too many transfer decisions"));
    }
    let mut explicit = HashMap::new();
    for decision in decisions {
        let index = decision
            .id
            .parse::<usize>()
            .ok()
            .filter(|index| *index < plan.items.len())
            .ok_or_else(|| AppError::invalid("unknown transfer item"))?;
        if explicit.insert(index, decision).is_some() {
            return Err(AppError::invalid("duplicate transfer decision"));
        }
    }
    let mut selected: Vec<String> = Vec::with_capacity(plan.items.len());
    let mut scope: Option<(&PlannedItem, &str)> = None;
    for (index, item) in plan.items.iter().enumerate() {
        if item
            .parent
            .is_some_and(|parent| selected[parent] != "merge")
        {
            selected.push("inactive".into());
            continue;
        }
        let collision = item.target_stat.is_some() || item.incoming_collision;
        let decision = explicit.get(&index);
        let action = decision
            .map(|decision| decision.action.as_str())
            .or_else(|| {
                scope
                    .filter(|(previous, action)| {
                        collision
                            && same_scope(previous, item)
                            && choices(item, plan.copy).contains(action)
                    })
                    .map(|(_, action)| action)
            })
            .unwrap_or(if collision { "" } else { "transfer" });
        if action == "cancel" {
            return Err(AppError::Cancelled);
        }
        if (collision && !choices(item, plan.copy).contains(&action))
            || (!collision && !matches!(action, "transfer" | "skip"))
        {
            return Err(AppError::invalid(format!(
                "item {index} requires an applicable collision decision"
            )));
        }
        if decision.is_some_and(|decision| decision.apply_to_remaining) {
            scope = Some((item, action));
        }
        selected.push(action.into());
    }
    Ok(selected)
}

fn same_scope(first: &PlannedItem, second: &PlannedItem) -> bool {
    first.source_stat.kind == second.source_stat.kind
        && first.target_stat.map(|stat| stat.kind) == second.target_stat.map(|stat| stat.kind)
        && first.incoming_collision == second.incoming_collision
        && first.same_target == second.same_target
}

fn validate(plan: &Plan, selected: &[String], cancelled: &AtomicBool) -> AppResult<()> {
    let destination = checked_destination(&path_text(&plan.destination))?;
    if secure::stat_in(&destination.directory, OsStr::new("."))?.identity()
        != plan.destination_identity
    {
        return Err(AppError::invalid(
            "destination directory changed since preflight",
        ));
    }
    for (item, action) in plan.items.iter().zip(selected) {
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        if matches!(action.as_str(), "inactive" | "skip") {
            continue;
        }
        check_version(
            &secure::resolved_parent(&item.source)?,
            Some(item.source_stat),
        )?;
        let target = secure::resolved_parent(&item.target)?;
        check_parent(&target, item.target_parent)?;
        check_version(&target, item.target_stat)?;
    }
    Ok(())
}

fn check_parent(path: &secure::ResolvedParent, expected: secure::EntryIdentity) -> AppResult<()> {
    if secure::stat_in(&path.directory, OsStr::new("."))?.identity() != expected {
        return Err(AppError::invalid(
            "destination parent changed since preflight",
        ));
    }
    Ok(())
}

fn check_version(
    path: &secure::ResolvedParent,
    expected: Option<secure::EntryStat>,
) -> AppResult<()> {
    let current = optional_stat(path)?;
    let unchanged = match (current, expected) {
        (None, None) => true,
        (Some(current), Some(expected)) => {
            current.identity() == expected.identity()
                && current.size == expected.size
                && current.mode == expected.mode
                && current.mtime == expected.mtime
                && current.mtime_nsec == expected.mtime_nsec
                && current.ctime == expected.ctime
                && current.ctime_nsec == expected.ctime_nsec
        }
        _ => false,
    };
    if unchanged {
        Ok(())
    } else {
        Err(AppError::invalid(format!(
            "{} changed since preflight; review the transfer again",
            path.full_path().display()
        )))
    }
}

struct TransferRun {
    copy: bool,
    id: String,
    history: Vec<journal::JournalItem>,
    saved: usize,
    flushed_at: Instant,
    mappings: Vec<TransferMapping>,
    skipped: Vec<String>,
    completed_sources: Vec<String>,
    displaced: Vec<String>,
    retained_folders: Vec<String>,
    warning: Option<String>,
}

impl TransferRun {
    fn new(copy: bool) -> Self {
        Self {
            copy,
            id: journal::new_entry_id(),
            history: Vec::new(),
            saved: 0,
            flushed_at: Instant::now(),
            mappings: Vec::new(),
            skipped: Vec::new(),
            completed_sources: Vec::new(),
            displaced: Vec::new(),
            retained_folders: Vec::new(),
            warning: None,
        }
    }

    fn operation(&self) -> &'static str {
        if self.copy { "copy" } else { "move" }
    }

    fn flush(&mut self) -> AppResult<()> {
        if self.saved == self.history.len() {
            return Ok(());
        }
        match journal::record_compound(&self.history, &self.id) {
            Ok(_) => {
                self.saved = self.history.len();
                self.warning = None;
                self.flushed_at = Instant::now();
                Ok(())
            }
            Err(error) => {
                self.warning = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn item(&mut self, item: &PlannedItem, action: &str, cancelled: &AtomicBool) -> AppResult<()> {
        let source = secure::resolved_parent(&item.source)?;
        check_version(&source, Some(item.source_stat))?;
        let mut target = secure::resolved_parent(&item.target)?;
        check_parent(&target, item.target_parent)?;
        if action == "keep-both" {
            let directory = CheckedDirectory {
                directory: target.directory,
                path: target.path,
            };
            target = unique_copy_target(&directory, &target.name)?;
        } else {
            check_version(&target, item.target_stat)?;
        }
        if action == "replace" {
            let expected = item
                .target_stat
                .ok_or_else(|| AppError::invalid("replacement target is missing"))?;
            let backup = journal::trash_matching_resolved(target, expected, false, cancelled)?;
            self.displaced.push(backup.source.clone());
            self.history.push(backup);
            self.flush()?;
            target = secure::resolved_parent(&item.target)?;
            check_parent(&target, item.target_parent)?;
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let destination = target.full_path();
        let mut ignored: fn(&Path) = ignore_path;
        if self.copy {
            secure::copy_path_noreplace_resolved(&source, &target, cancelled, &mut ignored)?;
        } else {
            secure::relocate_noreplace_matching_resolved(
                source,
                &target,
                item.source_stat.identity(),
                cancelled,
                &mut ignored,
            )?;
        }
        let mapping = TransferMapping {
            source: path_text(&item.source),
            destination: path_text(&destination),
        };
        self.history
            .push(journal::compound_transfer_item(self.copy, &mapping));
        if !self.copy {
            self.completed_sources.push(mapping.source.clone());
        }
        self.mappings.push(mapping);
        if action == "replace"
            || self.history.len() - self.saved >= 32
            || self.flushed_at.elapsed() >= Duration::from_millis(250)
        {
            self.flush()?;
        }
        Ok(())
    }
}
