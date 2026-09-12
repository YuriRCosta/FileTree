use super::*;
use crate::backend::LocationListArgs;
use std::path::{Component, Path};

fn refused(options: &LocationListArgs, id: &str, reason: impl ToString) -> Value {
    json!({"ok":false,"error_id":id,"error":reason.to_string(),"location":options.location,
        "generation":options.generation,"entries":[]})
}

pub fn list(options: &LocationListArgs, cancelled: &AtomicBool) -> Value {
    if options.location.starts_with("tailnet:") {
        return super::sftp::list(options, cancelled);
    }
    let (session, root_directory) =
        match validated_directory(&options.location, &options.generation) {
            Ok(validated) => validated,
            Err(error) => return refused(options, "stale-location", error),
        };
    if !session.capabilities.contains(&Capability::List) {
        return refused(
            options,
            "unsupported-capability",
            "location does not support listing",
        );
    }
    let relative = Path::new(&options.path);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        || options.path.contains("://")
    {
        return refused(
            options,
            "invalid-location-path",
            "location path must be relative and cannot traverse a parent",
        );
    }
    let path = session.proof.path.join(relative);
    let directory = match secure::open_directory_within_mount(&root_directory, relative) {
        Ok(directory) => directory,
        Err(error) => return refused(options, "location-unavailable", error),
    };
    let before = match secure::stat_in(&directory, OsStr::new(".")) {
        Ok(stat) => stat.identity(),
        Err(error) => return refused(options, "location-unavailable", error),
    };
    use rustix::fs::{Access, AtFlags, accessat};
    if let Err(error) = accessat(
        &directory,
        ".",
        Access::READ_OK | Access::EXEC_OK,
        AtFlags::EACCESS,
    ) {
        return refused(options, "capability-unavailable", error);
    }
    if cancelled.load(Ordering::Relaxed) {
        return json!({"ok":false,"cancelled":true,"entries":[]});
    }
    let mut result = crate::listing::window_from_directory(
        &crate::listing::WindowRequest {
            path: path_text(&path),
            show_hidden: options.show_hidden,
            start: options.start,
            count: options
                .count
                .clamp(1, crate::filesystem::DIRECTORY_ENTRY_LIMIT),
            sort: options.sort.clone(),
            descending: options.desc,
            filter: serde_json::from_str::<Value>(&options.filter)
                .ok()
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({})),
            include_created: options.include_created,
            fresh: options.fresh,
            git_enabled: !options.no_git,
            fresh_git: options.fresh_git,
        },
        &directory,
        cancelled,
    );
    if let Err(error) = validate_local(&options.location, &options.generation) {
        crate::listing::forget(&path_text(&path));
        return refused(options, "stale-location", error);
    }
    let same = secure::open_directory_within_mount(&root_directory, relative)
        .and_then(|directory| secure::stat_in(&directory, OsStr::new(".")))
        .is_ok_and(|stat| stat.identity() == before);
    if !same {
        crate::listing::forget(&path_text(&path));
        return refused(
            options,
            "stale-location",
            "listed directory changed during the request",
        );
    }
    result["location"] = json!(options.location);
    result["generation"] = json!(options.generation);
    result
}
