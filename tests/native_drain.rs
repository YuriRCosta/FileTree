use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn drain_consumes_bounded_control_output_while_the_child_runs() {
    let root = tempfile::tempdir().unwrap();
    let qs = root.path().join("qs");
    for (body, status, error) in [
        (
            "import json; print(json.dumps({'dirty_note_ids': ['x' * 1000] * 100}))",
            "error",
            "live without its authority",
        ),
        (
            "import sys; sys.stderr.write('x' * 100000 + 'No running instances')",
            "already_stopped",
            "",
        ),
        (
            "print('x' * (1024 * 1024 + 1))",
            "error",
            "exceeds its byte limit",
        ),
        ("import time; time.sleep(10)", "busy", "did not respond"),
    ] {
        fs::write(&qs, format!("#!/usr/bin/python3\n{body}\n")).unwrap();
        fs::set_permissions(&qs, fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_fileblade"))
            .args(["native", "drain", "--json", "--timeout-ms", "1000"])
            .env("HOME", root.path())
            .env("XDG_STATE_HOME", root.path().join("state"))
            .env(
                "FILEBLADE_NATIVE_STATE_ROOT",
                root.path().join("state/omarchy/fileblade"),
            )
            .env("FILEBLADE_APP_ROOT", env!("CARGO_MANIFEST_DIR"))
            .env("PATH", root.path())
            .output()
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(4));
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], status, "{response}");
        assert!(
            response["error"].as_str().unwrap().contains(error),
            "{response}"
        );
    }
}
