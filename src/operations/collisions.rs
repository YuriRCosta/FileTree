use super::*;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

const MAX_ITEMS: usize = 4096;
const MAX_PLANS: usize = 64;
const MAX_RETAINED_ITEMS: usize = 16_384;
const PLAN_LIFETIME: Duration = Duration::from_secs(300);
static PLANS: Mutex<Option<HashMap<String, Plan>>> = Mutex::new(None);

pub(super) struct PlannedItem {
    pub parent: Option<usize>,
    pub expanded: bool,
    pub target_parent: secure::EntryIdentity,
    pub source: PathBuf,
    pub target: PathBuf,
    pub source_stat: secure::EntryStat,
    pub target_stat: Option<secure::EntryStat>,
    pub incoming_collision: bool,
    pub same_target: bool,
}

pub(super) struct Plan {
    pub copy: bool,
    pub destination: PathBuf,
    pub destination_identity: secure::EntryIdentity,
    pub items: Vec<PlannedItem>,
    created: Instant,
}

pub fn preflight(
    copy: bool,
    sources: &[String],
    destination: &str,
    cancelled: &AtomicBool,
) -> Value {
    match prepare(copy, sources, destination, cancelled) {
        Ok(plan) => retain(plan),
        Err(error) => json!({"ok": false, "error": error.to_string()}),
    }
}

fn prepare(
    copy: bool,
    sources: &[String],
    destination: &str,
    cancelled: &AtomicBool,
) -> AppResult<Plan> {
    if sources.is_empty() || sources.len() > MAX_ITEMS {
        return Err(AppError::invalid(
            "transfer requires between 1 and 4096 sources",
        ));
    }
    let destination = local_path(destination)?;
    let destination = checked_destination(&path_text(&destination))?;
    let destination_identity = secure::stat_in(&destination.directory, OsStr::new("."))?.identity();
    let mut items = Vec::with_capacity(sources.len());
    let mut selected = HashSet::new();
    let mut reserved = HashSet::new();
    for source in sources {
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let source = local_path(source)?;
        if !selected.insert(source.clone()) {
            return Err(AppError::invalid("duplicate transfer source"));
        }
        ensure_not_nested(&source, &destination.path)?;
        let resolved = secure::resolved_parent(&source)?;
        let source_stat = secure::entry_stat_resolved(&resolved)?;
        if source_stat.kind == EntryKind::Other {
            return Err(AppError::invalid(
                "special files are not supported transfer sources",
            ));
        }
        let name = source
            .file_name()
            .ok_or_else(|| AppError::invalid("source has no file name"))?;
        let target = secure::resolved_child(&destination.directory, &destination.path, name)?;
        let same_target = secure::same_resolved_target(&resolved, &target)?;
        let target_stat = match secure::entry_stat_resolved(&target) {
            Ok(stat) => Some(stat),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let target = target.full_path();
        let incoming_collision = !reserved.insert(target.clone());
        items.push(PlannedItem {
            parent: None,
            expanded: false,
            target_parent: destination_identity,
            source,
            target,
            source_stat,
            target_stat,
            incoming_collision,
            same_target,
        });
    }
    if items.iter().any(|item| {
        item.source
            .ancestors()
            .skip(1)
            .any(|parent| selected.contains(parent))
    }) {
        return Err(AppError::invalid(
            "a folder and its descendant cannot be transferred together",
        ));
    }
    Ok(Plan {
        copy,
        destination: destination.path,
        destination_identity,
        items,
        created: Instant::now(),
    })
}

fn retain(plan: Plan) -> Value {
    let mut retained = match PLANS.lock() {
        Ok(retained) => retained,
        Err(_) => return json!({"ok": false, "error": "transfer decisions unavailable"}),
    };
    let plans = retained.get_or_insert_with(HashMap::new);
    plans.retain(|_, plan| plan.created.elapsed() < PLAN_LIFETIME);
    let item_count: usize = plans.values().map(|plan| plan.items.len()).sum();
    if plans.len() >= MAX_PLANS || item_count.saturating_add(plan.items.len()) > MAX_RETAINED_ITEMS
    {
        return json!({"ok": false, "error": "too many pending transfer decisions; finish or cancel a previous decision"});
    }
    let id = format!("transfer-{}", uuid::Uuid::new_v4().simple());
    let items: Vec<_> = plan.items.iter().enumerate().map(|(index, item)| {
        let collision = item.target_stat.is_some() || item.incoming_collision;
        let choices = choices(item, plan.copy);
        json!({"id": index.to_string(), "parent": item.parent.map(|id| id.to_string()), "source": path_text(&item.source), "destination": path_text(&item.target), "collision": collision, "incoming_collision": item.incoming_collision, "same_target": item.same_target, "choices": if collision { choices } else { vec![] }, "source_identity": identity(item.source_stat), "destination_identity": item.target_stat.map(identity)})
    }).collect();
    let result = json!({"ok": true, "decision_id": id, "operation": if plan.copy { "copy" } else { "move" }, "destination": path_text(&plan.destination), "destination_identity": {"device": plan.destination_identity.dev, "inode": plan.destination_identity.ino}, "expires_in_ms": PLAN_LIFETIME.as_millis(), "items": items});
    plans.insert(id, plan);
    result
}

pub fn discard(decision_id: &str) -> bool {
    PLANS
        .lock()
        .ok()
        .and_then(|mut retained| {
            retained
                .as_mut()
                .and_then(|plans| plans.remove(decision_id))
        })
        .is_some()
}

fn identity(stat: secure::EntryStat) -> Value {
    json!({"device": stat.dev, "inode": stat.ino, "size": stat.size, "modified_seconds": stat.mtime, "modified_nanoseconds": stat.mtime_nsec, "changed_seconds": stat.ctime, "changed_nanoseconds": stat.ctime_nsec, "kind": match stat.kind { EntryKind::File => "file", EntryKind::Directory => "directory", EntryKind::Symlink => "symlink", EntryKind::Other => "other" }})
}

fn local_path(raw: &str) -> AppResult<PathBuf> {
    if raw.contains("://") && url::Url::parse(raw).is_ok_and(|uri| uri.scheme() != "file") {
        return Err(AppError::invalid(
            "transfer requires a local path or a validated local representation",
        ));
    }
    Ok(parse_path(raw)?)
}

fn choices(item: &PlannedItem, copy: bool) -> Vec<&'static str> {
    let mut values = vec!["skip", "cancel"];
    if !item.same_target || copy {
        values.insert(0, "keep-both");
        if !item.same_target
            && !item.incoming_collision
            && item
                .target_stat
                .is_some_and(|stat| stat.kind != EntryKind::Other)
        {
            values.insert(0, "replace");
            if item.source_stat.kind == EntryKind::Directory
                && item
                    .target_stat
                    .is_some_and(|stat| stat.kind == EntryKind::Directory)
            {
                values.insert(0, "merge");
            }
        }
    }
    values
}

fn expand_merges(
    items: &mut Vec<PlannedItem>,
    selected: &[String],
    cancelled: &AtomicBool,
) -> AppResult<bool> {
    let mut changed = false;
    for (index, action) in selected.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let item = &items[index];
        if action != "merge" || item.expanded {
            continue;
        }
        let source = item.source.clone();
        let target = item.target.clone();
        let target_dir = checked_destination(&path_text(&target))?;
        let target_parent = secure::stat_in(&target_dir.directory, OsStr::new("."))?.identity();
        let mut names = secure::directory_names(&source)?;
        names.sort();
        if items.len().saturating_add(names.len()) > MAX_ITEMS {
            return Err(AppError::invalid(
                "merge preflight exceeds 4096 items; transfer smaller selections",
            ));
        }
        for name in names {
            if cancelled.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let source = source.join(&name);
            let resolved = secure::resolved_parent(&source)?;
            let source_stat = secure::entry_stat_resolved(&resolved)?;
            if source_stat.kind == EntryKind::Other {
                return Err(AppError::invalid(
                    "special files are not supported transfer sources",
                ));
            }
            let target = secure::resolved_child(&target_dir.directory, &target, &name)?;
            let target_stat = optional_stat(&target)?;
            items.push(PlannedItem {
                parent: Some(index),
                expanded: false,
                target_parent,
                source,
                target: target.full_path(),
                source_stat,
                target_stat,
                same_target: secure::same_resolved_target(&resolved, &target)?,
                incoming_collision: false,
            });
        }
        items[index].expanded = true;
        changed = true;
    }
    Ok(changed)
}

fn optional_stat(path: &secure::ResolvedParent) -> std::io::Result<Option<secure::EntryStat>> {
    match secure::entry_stat_resolved(path) {
        Ok(stat) => Ok(Some(stat)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

mod execute;
pub use execute::{Decision, execute};
