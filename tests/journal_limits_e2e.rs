use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn backend(root: &Path, arguments: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_filetree"))
        .arg("_backend")
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("FILETREE_JOURNAL", root.join("journal.json"))
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let last = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    serde_json::from_str(last).unwrap_or_else(|error| panic!("{error}: {text}"))
}

fn write_journal(root: &Path, document: &Value) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(root.join("journal.json"))
        .unwrap();
    file.write_all(document.to_string().as_bytes()).unwrap();
}

fn entry(id: usize, items: usize) -> Value {
    let items: Vec<Value> = (0..items)
        .map(|index| {
            json!({
                "source": format!("/tmp/journal-limit/source-{id}-{index}-{}", "x".repeat(120)),
                "target": format!("/tmp/journal-limit/target-{id}-{index}-{}", "y".repeat(120)),
            })
        })
        .collect();
    json!({
        "id": format!("op-{id:032}"),
        "kind": "copy",
        "label": format!("Copy batch {id}"),
        "at": "2026-09-03T10:00:00Z",
        "items": items,
    })
}

#[test]
fn a_full_journal_drops_its_oldest_entries_instead_of_failing_the_copy() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let undo: Vec<Value> = (0..36).map(|id| entry(id, 300)).collect();
    write_journal(root, &json!({"version": 1, "undo": undo, "redo": []}));
    let before = fs::metadata(root.join("journal.json")).unwrap().len();
    assert!(
        before > 3 * 1024 * 1024 && before < 4 * 1024 * 1024,
        "fixture must sit just under the 4 MiB limit: {before}"
    );

    let source = root.join("bundle");
    fs::create_dir(&source).unwrap();
    for index in 0..1500 {
        fs::write(source.join(format!("file-{index:04}.txt")), "x").unwrap();
    }
    let destination = root.join("out");
    fs::create_dir(&destination).unwrap();
    let copied = backend(
        root,
        &[
            "copy",
            "--source",
            source.to_str().unwrap(),
            "--destination",
            destination.to_str().unwrap(),
        ],
    );
    assert_eq!(copied["ok"], true, "{copied}");
    assert!(copied.get("journal_warning").is_none(), "{copied}");
    assert_eq!(
        fs::read_dir(destination.join("bundle")).unwrap().count(),
        1500
    );

    let after = fs::metadata(root.join("journal.json")).unwrap().len();
    assert!(
        after <= 4 * 1024 * 1024,
        "journal must fit after the save: {after}"
    );
    let document = backend(root, &["journal", "--limit", "5"]);
    assert_eq!(document["ok"], true, "{document}");
    let newest = &document["undo"][0];
    assert_eq!(newest["kind"], "copy");
    assert_eq!(
        newest["count"], 1,
        "the fresh copy is the newest undo entry: {document}"
    );
    assert!(
        document["undoCount"].as_u64().unwrap_or(0) < 37,
        "an old entry made room: {document}"
    );
}

#[test]
fn an_undo_interrupted_by_a_crash_comes_back_as_an_interrupted_entry() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let interrupted = json!({
        "id": "op-crashed",
        "kind": "move",
        "label": "Move 3 items",
        "at": "2026-09-03T10:00:00Z",
        "items": [{"source": "/tmp/a", "target": "/tmp/b"}],
    });
    write_journal(
        root,
        &json!({"version": 1, "undo": [], "redo": [], "interrupted": interrupted}),
    );

    let document = backend(root, &["journal", "--limit", "5"]);
    assert_eq!(document["interrupted"]["id"], "op-crashed", "{document}");

    let parent = root.join("folder");
    fs::create_dir(&parent).unwrap();
    let created = backend(
        root,
        &[
            "create",
            "--parent",
            parent.to_str().unwrap(),
            "--name",
            "note.txt",
        ],
    );
    assert_eq!(created["ok"], true, "{created}");

    let document = backend(root, &["journal", "--limit", "5"]);
    assert!(document["interrupted"].is_null(), "{document}");
    let labels: Vec<String> = document["undo"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["label"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        labels
            .iter()
            .any(|label| label.starts_with("(interrupted) Move 3 items")),
        "{labels:?}"
    );
}
