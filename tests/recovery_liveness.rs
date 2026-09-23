use fileblade::{recovery, secure};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use tempfile::tempdir;

fn start_server() -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_filetree"))
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"v\":1,\"type\":\"hello\"}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    serde_json::from_slice(output.stdout.split(|byte| *byte == b'\n').next().unwrap()).unwrap()
}

#[test]
fn active_intents_are_leased_and_doctor_never_recovers() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    unsafe {
        std::env::set_var("XDG_STATE_HOME", root.join("state"));
    }
    let source = root.join("source.txt");
    let destination = root.join("copy.txt");
    fs::write(&source, "body").unwrap();
    let mut callbacks = 0;
    secure::copy_path_noreplace(&source, &destination, &AtomicBool::new(false), &mut |_| {
        callbacks += 1;
        let hello = start_server();
        assert_eq!(
            hello["recovered"]["removed"],
            serde_json::json!([]),
            "{hello}"
        );
        assert!(
            fs::read_dir(recovery::inflight_dir())
                .unwrap()
                .next()
                .is_some()
        );
    })
    .unwrap();
    assert!(callbacks > 0);
    assert_eq!(fs::read_to_string(destination).unwrap(), "body");

    let quarantine = secure::quarantine_path(&source).unwrap();
    let staged = quarantine.path();
    let hello = start_server();
    assert_eq!(
        hello["recovered"]["restored"],
        serde_json::json!([]),
        "{hello}"
    );
    assert!(staged.exists());
    assert!(!source.exists());
    drop(quarantine);
    let doctor = Command::new(env!("CARGO_BIN_EXE_filetree"))
        .arg("doctor")
        .env("PATH", "/nonexistent")
        .output()
        .unwrap();
    assert!(!doctor.stdout.is_empty());
    assert!(
        staged.exists(),
        "doctor must leave abandoned intents alone too"
    );
    assert!(!source.exists());
    let hello = start_server();
    assert_eq!(
        hello["recovered"]["restored"],
        serde_json::json!([source]),
        "{hello}"
    );
    assert_eq!(fs::read_to_string(&source).unwrap(), "body");

    let blocked = root.join("blocked");
    fs::write(&blocked, "not a directory").unwrap();
    unsafe {
        std::env::set_var("XDG_STATE_HOME", &blocked);
    }
    assert!(secure::quarantine_path(&source).is_err());
    assert!(
        source.exists(),
        "an intent failure must happen before securing the source"
    );
}
