use super::*;
use std::sync::atomic::Ordering;

pub fn set(paths: &[String], mode: &str, cancelled: &AtomicBool) -> Value {
    let mut changed = Vec::new();
    let id = journal::new_entry_id();
    let mut warning = None;
    let outcome = (|| -> AppResult<()> {
        if paths.is_empty()
            || paths.len() > 4096
            || mode.len() != 3
            || !mode.bytes().all(|byte| matches!(byte, b'0'..=b'7'))
        {
            return Err(AppError::invalid(
                "choose 1 to 4096 files and an octal permission mode between 000 and 777",
            ));
        }
        let mode =
            u32::from_str_radix(mode, 8).map_err(|error| AppError::invalid(error.to_string()))?;
        let mut planned = Vec::new();
        let mut seen = BTreeSet::new();
        for raw in paths {
            if cancelled.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            if !raw.starts_with('/')
                && raw.contains("://")
                && !raw
                    .get(..7)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
            {
                return Err(AppError::invalid("permission changes require local paths"));
            }
            let path = parse_path(raw)?;
            if !seen.insert(path.clone()) {
                return Err(AppError::invalid("duplicate permission target"));
            }
            let stat = secure::entry_stat_resolved(&secure::resolved_parent(&path)?)?;
            if !matches!(stat.kind, EntryKind::File | EntryKind::Directory) {
                return Err(AppError::invalid(
                    "permissions can only be changed on files and directories",
                ));
            }
            planned.push((path, stat));
        }
        if planned
            .iter()
            .any(|(path, _)| path.ancestors().skip(1).any(|parent| seen.contains(parent)))
        {
            return Err(AppError::invalid(
                "a folder and its descendant cannot have permissions changed in one selection",
            ));
        }
        for (path, before) in planned {
            if cancelled.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            if before.mode & 0o777 == mode {
                continue;
            }
            let after =
                secure::set_permissions_matching(&path, before, mode | (before.mode & 0o7000))?;
            changed.push(path_text(&path));
            if let Err(error) = journal::record_permissions(&path, before, after, &id) {
                warning = Some(error.to_string());
                return Err(error);
            }
        }
        Ok(())
    })();
    json!({"ok":outcome.is_ok(),"operation":"permissions","paths":changed,"mappings":[],"journal_id":if changed.is_empty() {""} else {&id},"journal_warning":warning,
        "partial":!changed.is_empty() && outcome.is_err(),"cancelled":matches!(&outcome,Err(AppError::Cancelled)),"error":outcome.err().map(|error| error.to_string())})
}
