use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn qml_routes_color_changes_through_the_operation_journal() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let operations = fs::read_to_string(root.join("controllers/OperationController.qml")).unwrap();
    let service = fs::read_to_string(root.join("Service.qml")).unwrap();

    assert!(
        operations.contains("\"copy\", \"move\", \"rename\", \"create\", \"color\", \"trash\"")
    );
    assert!(operations.contains("service.backendCommand(\"color\")"));
    assert!(operations.contains("service.applyFolderColorChanges(colors)"));
    assert!(service.contains("return operationController.setFolderColor(path, value)"));
    assert!(service.contains("return operationController.setSelectionFolderColor(value, entries)"));
}

#[test]
fn reset_to_default_undoes_and_redoes_the_exact_color() {
    let temporary = tempdir().unwrap();
    let path = temporary.path().join("orange.txt");
    fs::write(&path, "color history").unwrap();
    let path = path.to_str().unwrap();

    let colored = backend(
        temporary.path(),
        &[
            "color",
            "--path",
            path,
            "--before",
            "",
            "--after",
            "#d19a66",
            "--journal-id",
            "color-orange",
        ],
    );
    assert_eq!(colored["ok"], true, "{colored}");
    assert_eq!(colored["colors"][0]["value"], "#d19a66");

    let reset = backend(
        temporary.path(),
        &[
            "color",
            "--path",
            path,
            "--before",
            "#d19a66",
            "--after",
            "",
            "--journal-id",
            "color-default",
        ],
    );
    assert_eq!(reset["ok"], true, "{reset}");
    assert_eq!(reset["colors"][0]["value"], "");

    let journal = backend(temporary.path(), &["journal"]);
    assert_eq!(journal["undo"][0]["kind"], "color");
    assert_eq!(journal["undo"][0]["label"], "Reset color orange.txt");

    let undone = backend(temporary.path(), &["undo"]);
    assert_eq!(undone["ok"], true, "{undone}");
    assert_eq!(undone["operation"], "color");
    assert_eq!(undone["colors"][0]["path"], path);
    assert_eq!(undone["colors"][0]["value"], "#d19a66");

    let redone = backend(temporary.path(), &["redo"]);
    assert_eq!(redone["ok"], true, "{redone}");
    assert_eq!(redone["colors"][0]["value"], "");
}

#[test]
fn rename_and_color_use_one_ordered_history() {
    let temporary = tempdir().unwrap();
    let original = temporary.path().join("before.txt");
    let renamed = temporary.path().join("after.txt");
    fs::write(&original, "ordered history").unwrap();

    let rename = backend(
        temporary.path(),
        &[
            "rename",
            "--path",
            original.to_str().unwrap(),
            "--name",
            "after.txt",
            "--journal-id",
            "rename-1",
        ],
    );
    assert_eq!(rename["ok"], true, "{rename}");
    assert!(renamed.exists());

    let color = backend(
        temporary.path(),
        &[
            "color",
            "--path",
            renamed.to_str().unwrap(),
            "--before",
            "",
            "--after",
            "#d19a66",
            "--journal-id",
            "color-1",
        ],
    );
    assert_eq!(color["ok"], true, "{color}");

    let undo_color = backend(temporary.path(), &["undo"]);
    assert_eq!(undo_color["operation"], "color");
    assert_eq!(undo_color["colors"][0]["value"], "");
    assert!(renamed.exists());

    let undo_rename = backend(temporary.path(), &["undo"]);
    assert_eq!(undo_rename["operation"], "rename");
    assert_eq!(
        undo_rename["mappings"][0]["source"],
        renamed.to_str().unwrap()
    );
    assert_eq!(
        undo_rename["mappings"][0]["destination"],
        original.to_str().unwrap()
    );
    assert!(original.exists());
    assert!(!renamed.exists());
}

fn backend(root: &Path, arguments: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_filetree"))
        .args(["--output", "json", "_backend"])
        .args(arguments)
        .env("XDG_DATA_HOME", root.join("data"))
        .env("FILETREE_JOURNAL", root.join("state/journal.json"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .unwrap()
}
