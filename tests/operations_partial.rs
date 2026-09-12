use fileblade::{journal, operations::collisions};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[path = "support/isolated.rs"]
mod isolated;

#[test]
fn cancellation_after_one_completed_file_preserves_mappings_and_undo() {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/operation-fixtures");
    fs::create_dir_all(&parent).unwrap();
    if !isolated::child(Some(&parent)) {
        return;
    }
    for copy in [true, false] {
        let root = tempfile::tempdir_in(&parent).unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        for name in ["first", "second"] {
            fs::write(source.join(name), name).unwrap();
        }
        let paths = ["first", "second"].map(|name| source.join(name).to_str().unwrap().to_string());
        let cancelled = AtomicBool::new(false);
        let plan = collisions::preflight(copy, &paths, target.to_str().unwrap(), &cancelled);
        assert_eq!(plan["ok"], true, "{plan}");
        let mut progress = Vec::new();
        let result = collisions::execute(
            plan["decision_id"].as_str().unwrap(),
            &[],
            false,
            &mut |frame| {
                if frame["phase"] == "completed" {
                    cancelled.store(true, Ordering::Relaxed);
                }
                progress.push(frame);
            },
            &cancelled,
        );
        assert_eq!(result["ok"], false, "{result}");
        assert_eq!(result["cancelled"], true, "{result}");
        assert_eq!(result["partial"], true, "{result}");
        assert_eq!(result["mappings"].as_array().unwrap().len(), 1, "{result}");
        assert_eq!(result["mappings"][0]["source"], paths[0]);
        assert_eq!(
            result["completed_sources"].as_array().unwrap().len(),
            usize::from(!copy)
        );
        assert_eq!(
            progress
                .iter()
                .filter(|frame| frame["phase"] == "completed")
                .count(),
            1
        );
        assert_eq!(fs::read_to_string(target.join("first")).unwrap(), "first");
        assert_eq!(source.join("first").exists(), copy);
        assert_eq!(fs::read_to_string(source.join("second")).unwrap(), "second");
        assert!(!target.join("second").exists());
        let undone = journal::undo(false, false, &AtomicBool::new(false));
        assert_eq!(undone["ok"], true, "{undone}");
        assert!(!target.join("first").exists());
        for name in ["first", "second"] {
            assert_eq!(fs::read_to_string(source.join(name)).unwrap(), name);
        }
        let redone = journal::redo(false, false, &AtomicBool::new(false));
        assert_eq!(redone["ok"], true, "{redone}");
        assert_eq!(fs::read_to_string(target.join("first")).unwrap(), "first");
        assert!(!target.join("second").exists());
        assert_eq!(fs::read_to_string(source.join("second")).unwrap(), "second");
    }
}
