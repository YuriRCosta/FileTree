use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;
fn fixture() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/operation-fixtures");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}

fn backend(root: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_filetree"))
        .args(["--output", "json", "_backend"])
        .args(args)
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("FILETREE_JOURNAL", root.join("journal.json"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_str(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap()
}

fn journal(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join("journal.json")).unwrap()).unwrap()
}

fn replace_history(root: &Path, items: Vec<Value>) {
    fs::write(root.join("journal.json"), serde_json::to_vec(&json!({"version":1,"undo":[{"id":"compound-fixture","kind":"transfer","label":"Transfer","at":"2026-09-11T00:00:00","items":items}],"redo":[]})).unwrap()).unwrap();
}

fn replacement(root: &Path) -> (std::path::PathBuf, Value) {
    let source = root.join("source");
    let target = root.join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    let old = target.join("same");
    let new = source.join("same");
    fs::write(&old, "old").unwrap();
    fs::write(&new, "new").unwrap();
    let result = backend(root, &["trash", "--path", old.to_str().unwrap()]);
    assert_eq!(result["ok"], true, "{result}");
    let removed = journal(root)["undo"][0]["items"][0].clone();
    let result = backend(
        root,
        &[
            "copy",
            "--source",
            new.to_str().unwrap(),
            "--destination",
            target.to_str().unwrap(),
        ],
    );
    assert_eq!(result["ok"], true, "{result}");
    let mut created = journal(root)["undo"][1]["items"][0].clone();
    created["source"] = json!("");
    replace_history(root, vec![created, removed.clone()]);
    (old, removed)
}

#[test]
fn replacement_undo_and_redo_preserve_both_versions() {
    let root = fixture();
    let (target, _) = replacement(root.path());
    for _ in 0..2 {
        let undone = backend(root.path(), &["undo"]);
        assert_eq!(undone["ok"], true, "{undone}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "old");
        let redone = backend(root.path(), &["redo"]);
        assert_eq!(redone["ok"], true, "{redone}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }
    assert_eq!(
        fs::read_to_string(root.path().join("source/same")).unwrap(),
        "new"
    );
}

#[test]
fn missing_backup_preserves_completed_undo_for_redo() {
    let root = fixture();
    let (target, removed) = replacement(root.path());
    fs::remove_file(
        Path::new(removed["trash_dir"].as_str().unwrap())
            .join("files")
            .join(removed["trash_name"].as_str().unwrap()),
    )
    .unwrap();
    let undone = backend(root.path(), &["undo"]);
    assert_eq!(undone["ok"], false, "{undone}");
    assert_eq!(undone["partial"], true, "{undone}");
    assert!(!target.exists());
    let data = journal(root.path());
    assert_eq!(data["undo"].as_array().unwrap().len(), 1);
    assert_eq!(data["redo"].as_array().unwrap().len(), 1);
    let redone = backend(root.path(), &["redo"]);
    assert_eq!(redone["ok"], true, "{redone}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
}

#[test]
fn modified_new_output_is_refused_before_backup_restore() {
    let root = fixture();
    let (target, _) = replacement(root.path());
    fs::write(&target, "edited after replacement").unwrap();
    let undone = backend(root.path(), &["undo"]);
    assert_eq!(undone["ok"], false, "{undone}");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "edited after replacement"
    );
    assert_eq!(journal(root.path())["redo"].as_array().unwrap().len(), 0);
}

#[test]
fn edited_restored_version_is_not_overwritten_by_redo() {
    let root = fixture();
    let (target, _) = replacement(root.path());
    assert_eq!(backend(root.path(), &["undo"])["ok"], true);
    fs::write(&target, "edited old version").unwrap();
    let result = backend(root.path(), &["redo"]);
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(fs::read_to_string(target).unwrap(), "edited old version");
}

#[test]
fn merged_move_restores_its_source_directory_before_children() {
    let root = fixture();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("child"), "moved").unwrap();
    fs::write(target.join("unrelated"), "kept").unwrap();
    let result = backend(
        root.path(),
        &[
            "move",
            "--source",
            source.join("child").to_str().unwrap(),
            "--destination",
            target.to_str().unwrap(),
        ],
    );
    assert_eq!(result["ok"], true, "{result}");
    let moved = journal(root.path())["undo"][0]["items"][0].clone();
    let result = backend(root.path(), &["trash", "--path", source.to_str().unwrap()]);
    assert_eq!(result["ok"], true, "{result}");
    let removed = journal(root.path())["undo"][1]["items"][0].clone();
    replace_history(root.path(), vec![removed, moved]);
    let result = backend(root.path(), &["undo"]);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(fs::read_to_string(source.join("child")).unwrap(), "moved");
    let result = backend(root.path(), &["redo"]);
    assert_eq!(result["ok"], true, "{result}");
    assert!(!source.exists());
    assert_eq!(fs::read_to_string(target.join("child")).unwrap(), "moved");
    assert_eq!(
        fs::read_to_string(target.join("unrelated")).unwrap(),
        "kept"
    );
}
