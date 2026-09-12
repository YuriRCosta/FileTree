#[path = "isolated.rs"]
mod isolated;

#[test]
fn navigation_writes_preserve_file_caches_but_file_mutations_invalidate_them() {
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
    assert!(!Arc::ptr_eq(
        &index,
        &crate::index::acquire(root.path(), false, false)
    ));
}
