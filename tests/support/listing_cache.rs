#[path = "isolated.rs"]
mod isolated;

#[test]
fn navigation_writes_keep_the_caches_and_a_mutation_patches_the_index_in_place() {
    if !isolated::child(None) {
        return;
    }
    use crate::backend;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_str().unwrap();
    let cancelled = AtomicBool::new(false);
    let listing = super::acquire(root.path(), false, false, &cancelled).unwrap();
    let index = crate::index::acquire(root.path(), false, false);
    for arguments in [
        vec!["state-write", "--document", "{}"],
        vec!["layout-write", "--document", "{}"],
        vec!["frecency-visit", "--path", path],
        vec!["visit", "--path", path],
    ] {
        let command =
            backend::parse(std::iter::once("fileblade").chain(arguments.clone())).unwrap();
        assert!(backend::mutating(&command), "{arguments:?}");
        let result = backend::dispatch(command, &cancelled, &mut |_| Ok(())).unwrap();
        if arguments[0] != "visit" {
            assert_eq!(result["ok"], true, "{result}");
        }
        assert!(
            Arc::ptr_eq(
                &listing,
                &super::acquire(root.path(), false, false, &cancelled).unwrap()
            ),
            "{arguments:?} evicted the directory listing"
        );
        assert!(
            Arc::ptr_eq(&index, &crate::index::acquire(root.path(), false, false)),
            "{arguments:?} invalidated the file index"
        );
    }
    let command =
        backend::parse(["fileblade", "create", "--parent", path, "--name", "new.txt"]).unwrap();
    let result = backend::dispatch(command, &cancelled, &mut |_| Ok(())).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    let refreshed = super::acquire(root.path(), false, false, &cancelled).unwrap();
    assert!(!Arc::ptr_eq(&listing, &refreshed));
    assert_eq!(super::lock(&refreshed).entries.len(), 1);

    let patched = crate::index::acquire(root.path(), false, false);
    assert!(
        Arc::ptr_eq(&index, &patched),
        "a mutation rebuilt the index instead of patching it"
    );
    assert!(
        index_finds(&patched, "new.txt"),
        "the created file never reached the index"
    );

    std::fs::remove_file(root.path().join("new.txt")).unwrap();
    crate::index::invalidate_within(&root.path().join("new.txt"));
    let after = crate::index::acquire(root.path(), false, false);
    assert!(Arc::ptr_eq(&index, &after), "a removal rebuilt the index");
    assert!(
        !index_finds(&after, "new.txt"),
        "a removed file is still offered by the index"
    );
}

fn index_finds(
    index: &std::sync::Arc<std::sync::Mutex<crate::index::PathIndex>>,
    name: &str,
) -> bool {
    let mut guard = index.lock().unwrap();
    guard.set_pattern(name, crate::index::case_matching(false));
    for _ in 0..200 {
        if !guard.tick(10).running {
            break;
        }
    }
    let mut accept = |_: &str, _: &crate::index::IndexFlags| true;
    guard
        .hits(50, &mut accept)
        .iter()
        .any(|hit| hit.entry.relative == name)
}
