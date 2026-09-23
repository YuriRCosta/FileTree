use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn backend(root: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_filetree"))
        .arg("_backend")
        .args(args)
        .env("XDG_STATE_HOME", root.join("state-home"))
        .env("FILETREE_JOURNAL", root.join("journal.json"))
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn names_survive_archive_preview_without_display_escapes() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let names = [
        "two  spaces.txt",
        "tab\tname.txt",
        "line\nbreak.txt",
        "back\\slash.txt",
        " leading.txt",
        "üñícode.txt",
        "arrow -> name.txt",
    ];
    for name in names {
        fs::write(root.join(name), "body").unwrap();
    }
    std::os::unix::fs::symlink("two  spaces.txt", root.join("link -> name")).unwrap();
    let status = Command::new("bsdtar")
        .current_dir(root)
        .args(["-cf", "names.tar", "--"])
        .args(names)
        .arg("link -> name")
        .status()
        .unwrap();
    assert!(status.success());
    let archive = root.join("names.tar");
    let listed = backend(root, &["archive-list", "--path", archive.to_str().unwrap()]);
    assert_eq!(listed["ok"], true, "{listed}");
    let entries = listed["entries"].as_array().unwrap();
    for name in names {
        let entry = entries
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap_or_else(|| panic!("missing {name:?}: {listed}"));
        assert_eq!(entry["size"], 4);
    }
    assert!(
        entries
            .iter()
            .any(|entry| entry["name"] == "link -> name" && entry["is_symlink"] == true)
    );
}

#[test]
fn corrupt_archives_leave_the_destination_unchanged() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    fs::write(root.join("first.txt"), "first").unwrap();
    fs::write(root.join("second.txt"), vec![b'x'; 65_536]).unwrap();
    assert!(
        Command::new("bsdtar")
            .current_dir(root)
            .args([
                "--format=ustar",
                "-cf",
                "broken.tar",
                "first.txt",
                "second.txt"
            ])
            .status()
            .unwrap()
            .success()
    );
    let archive = root.join("broken.tar");
    fs::OpenOptions::new()
        .write(true)
        .open(&archive)
        .unwrap()
        .set_len(4096)
        .unwrap();
    let destination = root.join("unpacked");
    let args = [
        "archive-extract",
        "--path",
        archive.to_str().unwrap(),
        "--destination",
        destination.to_str().unwrap(),
    ];
    let failed = backend(root, &args);
    assert_eq!(failed["ok"], false, "{failed}");
    assert_eq!(failed["paths"], serde_json::json!([]));
    assert!(!destination.exists());
    let nested = root.join("missing/parents/unpacked");
    let nested_failed = backend(
        root,
        &[
            "archive-extract",
            "--path",
            archive.to_str().unwrap(),
            "--destination",
            nested.to_str().unwrap(),
        ],
    );
    assert_eq!(nested_failed["ok"], false, "{nested_failed}");
    assert!(!root.join("missing").exists());
    fs::create_dir(&destination).unwrap();
    let failed = backend(root, &args);
    assert_eq!(failed["ok"], false, "{failed}");
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
    assert!(!fs::read_dir(root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".filetree-partial-")
    }));
}

#[test]
fn missing_destination_parents_publish_with_the_extracted_tree() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    fs::write(root.join("note.txt"), "body").unwrap();
    assert!(
        Command::new("bsdtar")
            .current_dir(root)
            .args(["-cf", "notes.tar", "note.txt"])
            .status()
            .unwrap()
            .success()
    );
    let archive = root.join("notes.tar");
    let destination = root.join("new/parents/unpacked");
    let result = backend(
        root,
        &[
            "archive-extract",
            "--path",
            archive.to_str().unwrap(),
            "--destination",
            destination.to_str().unwrap(),
            "--merge",
        ],
    );
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["undoable"], true, "{result}");
    assert!(result["journal_id"].is_string());
    assert_eq!(
        fs::read_to_string(destination.join("note.txt")).unwrap(),
        "body"
    );
}
