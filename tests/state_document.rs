use serde_json::Value;
use std::fs;
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
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(text.trim()).unwrap_or_else(|error| panic!("{error}: {text}"))
}

fn state_read(root: &Path) -> Value {
    backend(root, &["state-read"])
}

fn state_file(root: &Path) -> std::path::PathBuf {
    let written = backend(root, &["state-write", "--document", "{}"]);
    assert_eq!(written["ok"], true, "{written}");
    let mut pending = vec![root.join("state")];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().is_some_and(|name| name == "state.json") {
                return path;
            }
        }
    }
    panic!("state-write created no state.json under {}", root.display());
}

#[test]
fn unparseable_state_is_quarantined_and_reported_as_empty() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let state = state_file(root);
    fs::write(&state, "{\"favorites\": [\"/home/me\"],}").unwrap();

    let response = state_read(root);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["text"], "");
    let quarantined = response["quarantined"].as_str().expect("quarantine path");
    assert!(quarantined.contains("state.json.corrupt-"), "{quarantined}");
    assert!(!state.exists(), "the corrupt document must be moved aside");
    assert_eq!(
        fs::read_to_string(quarantined).unwrap(),
        "{\"favorites\": [\"/home/me\"],}"
    );

    let again = state_read(root);
    assert_eq!(again["ok"], true);
    assert!(again.get("quarantined").is_none(), "{again}");
}

#[test]
fn valid_state_is_returned_verbatim() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    fs::write(state_file(root), "{\"favorites\": []}\n").unwrap();
    let response = state_read(root);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["text"], "{\"favorites\": []}\n");
}
