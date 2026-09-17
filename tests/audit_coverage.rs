use fileblade::audit::is_audited;

#[test]
fn every_verb_that_deletes_or_runs_something_is_audited() {
    for verb in [
        "copy",
        "move",
        "rename",
        "create",
        "trash",
        "trash-restore",
        "trash-delete",
        "trash-empty",
        "trash-prune",
        "undo",
        "redo",
        "bin-put",
        "bin-remove",
        "bin-restore",
        "bin-purge",
        "archive-extract",
        "plugin-add",
        "set-default",
        "drop-run",
        "action-run",
        "recover",
    ] {
        assert!(is_audited(verb), "{verb} must be audited");
    }
}

#[test]
fn reads_and_retired_verbs_are_not_audited() {
    for verb in [
        "children-window",
        "stat-batch",
        "state-read",
        "bin-delete",
        "children",
        "update-apply",
        "action-list",
        "module-dirs",
    ] {
        assert!(!is_audited(verb), "{verb} must not be audited");
    }
}
