use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn tool(root: &Path, name: &str, body: &str) {
    let path = root.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn backend(root: &Path, arguments: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_fileblade"))
        .args(["--output", "json", "_backend"])
        .args(arguments)
        .env("PATH", root)
        .env("CLIPBOARD_CAPTURE", root.join("capture"))
        .env("XDG_STATE_HOME", root.join("state"))
        .output()
        .unwrap();
    if output.status.success() {
        serde_json::from_slice(&output.stdout).unwrap()
    } else {
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        serde_json::from_slice(&output.stderr).unwrap()
    }
}

#[test]
fn exported_cut_preserves_intent_and_encoded_names_on_import() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    tool(
        root,
        "wl-copy",
        "printf '%s' \"$2\" > \"$CLIPBOARD_CAPTURE.mime\"\n/bin/cat > \"$CLIPBOARD_CAPTURE\"",
    );
    tool(
        root,
        "wl-paste",
        "case \"$1\" in\n--list-types) /bin/cat \"$CLIPBOARD_CAPTURE.mime\";;\n*) /bin/cat \"$CLIPBOARD_CAPTURE\";;\nesac",
    );
    let paths = [root.join("space #percent%\n雪.txt"), root.join("second")];
    for path in &paths {
        fs::write(path, "source remains until paste").unwrap();
    }
    let written = backend(
        root,
        &[
            "clipboard-write",
            "--cut",
            "--path",
            paths[0].to_str().unwrap(),
            "--path",
            paths[1].to_str().unwrap(),
        ],
    );
    assert_eq!(written["ok"], true, "{written}");
    assert_eq!(written["mode"], "cut");
    assert_eq!(
        fs::read_to_string(root.join("capture.mime")).unwrap(),
        "x-special/gnome-copied-files"
    );
    let data = fs::read_to_string(root.join("capture")).unwrap();
    let lines: Vec<_> = data.split('\n').collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "cut");
    for (line, path) in lines[1..].iter().zip(&paths) {
        assert_eq!(
            url::Url::parse(line).unwrap().to_file_path().unwrap(),
            *path
        );
        assert!(path.exists());
    }
    let imported = backend(root, &["clipboard"]);
    assert_eq!(imported["ok"], true, "{imported}");
    assert_eq!(imported["mode"], "cut");
    assert_eq!(imported["paths"], serde_json::json!(paths));
}

#[test]
fn ordinary_copy_retains_uri_list_and_source_files() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    tool(
        root,
        "wl-copy",
        "printf '%s' \"$2\" > \"$CLIPBOARD_CAPTURE.mime\"\n/bin/cat > \"$CLIPBOARD_CAPTURE\"",
    );
    let path = root.join("a file");
    fs::write(&path, "unchanged").unwrap();
    let written = backend(root, &["clipboard-write", "--path", path.to_str().unwrap()]);
    assert_eq!(written["ok"], true, "{written}");
    assert_eq!(written["mode"], "copy");
    assert_eq!(
        fs::read_to_string(root.join("capture.mime")).unwrap(),
        "text/uri-list"
    );
    assert_eq!(
        fs::read_to_string(root.join("capture")).unwrap(),
        format!("{}\r\n", url::Url::from_file_path(&path).unwrap())
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "unchanged");
}

#[test]
fn invalid_selection_does_not_replace_the_clipboard() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    tool(root, "wl-copy", "/bin/cat > \"$CLIPBOARD_CAPTURE\"");
    fs::write(root.join("capture"), "previous clipboard").unwrap();
    for arguments in [
        vec!["clipboard-write", "--cut"],
        vec![
            "clipboard-write",
            "--cut",
            "--path",
            "/valid",
            "--path",
            "sftp://host/path",
        ],
    ] {
        let written = backend(root, &arguments);
        assert_eq!(written["ok"], false, "{written}");
        assert_eq!(
            fs::read_to_string(root.join("capture")).unwrap(),
            "previous clipboard"
        );
    }
}

#[test]
fn clipboard_owner_failure_is_reported_without_mutating_sources() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    tool(
        root,
        "wl-copy",
        "/bin/cat > /dev/null\nprintf 'clipboard unavailable' >&2\nexit 1",
    );
    let path = root.join("source");
    fs::write(&path, "unchanged").unwrap();
    let written = backend(
        root,
        &["clipboard-write", "--cut", "--path", path.to_str().unwrap()],
    );
    assert_eq!(written["ok"], false, "{written}");
    assert_eq!(written["error"], "clipboard unavailable");
    assert_eq!(fs::read_to_string(path).unwrap(), "unchanged");
}
