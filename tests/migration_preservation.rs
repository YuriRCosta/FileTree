use serde_json::{Value, json};
use std::fs;
use std::process::Command;

fn backend(root: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    Command::new(
        std::env::var_os("FILEBLADE_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_fileblade").into()),
    )
    .arg("_backend")
    .args(arguments)
    .env("HOME", root)
    .env("XDG_CONFIG_HOME", root.join("config"))
    .env("XDG_STATE_HOME", root.join("state"))
    .env("XDG_DATA_HOME", root.join("data"))
    .output()
    .unwrap()
}

fn response(root: &std::path::Path, arguments: &[&str]) -> Value {
    let output = backend(root, arguments);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{:?}", output))
}

#[test]
fn binding_metadata_updates_preserve_unknown_fields_and_refuse_newer_schemas() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config/omarchy/fileblade");
    fs::create_dir_all(&config).unwrap();
    let path = config.join("keybindings.json");
    let original = json!({"version":1,"bindings":{"next":["n"],"custom":["CTRL+K"]},
        "future":{"user":true,"ordered":[null,false,42]},"owner":"user"});
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    let run = || backend(root.path(), &["keybindings-prepare"]);
    let response = run();
    assert!(response.status.success(), "{:?}", response);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for key in ["bindings", "future", "owner"] {
        assert_eq!(saved[key], original[key]);
    }
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(run().status.success());
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    for data in [
        br#"{"version":2,"future":{"keep":true}}"#.as_slice(),
        b"{malformed",
    ] {
        fs::write(&path, data).unwrap();
        let response = run();
        let refused = !response.status.success()
            || serde_json::from_slice::<Value>(&response.stdout)
                .is_ok_and(|value| value["ok"] == false);
        assert!(refused, "{:?}", response);
        assert_eq!(fs::read(&path).unwrap(), data);
    }
}

#[test]
fn preference_writes_keep_untouched_choices_absent_and_explicit_defaults_present() {
    for (option, value, untouched, fallback) in [
        (
            "--trash-retention-days",
            "7",
            "agentManagement",
            json!(false),
        ),
        (
            "--agent-management",
            "true",
            "trashRetentionDays",
            Value::Null,
        ),
        (
            "--agent-management",
            "false",
            "trashRetentionDays",
            Value::Null,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config/omarchy/fileblade/settings.json");
        let initial = response(root.path(), &["preferences-read"]);
        assert_eq!(initial["settings"][untouched], fallback);
        assert!(!path.exists());
        let changed = response(root.path(), &["preferences-set", option, value]);
        assert_eq!(changed["ok"], true, "{changed}");
        assert_eq!(changed["settings"][untouched], fallback);
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(saved.get(untouched).is_none(), "{saved}");
        let before = fs::read(&path).unwrap();
        let reread = response(root.path(), &["preferences-read"]);
        assert_eq!(reread["settings"], changed["settings"]);
        assert_eq!(fs::read(&path).unwrap(), before);
        let original = json!({"version":1,"filebladeVersion":"999.0.0",
            "agentManagement":false,"trashRetentionDays":null,
            "future":{"ordered":[null,false,42]},"dropWheel":{"actions":{}}});
        fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        let changed = response(root.path(), &["preferences-set", option, value]);
        assert_eq!(changed["ok"], true, "{changed}");
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for key in [untouched, "future", "dropWheel", "version"] {
            assert_eq!(saved[key], original[key]);
        }
        assert_eq!(saved["filebladeVersion"], env!("CARGO_PKG_VERSION"));
    }
}

#[test]
fn unsupported_or_oversized_preferences_remain_recoverable() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config/omarchy/fileblade/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    for bytes in [
        br#"{"version":2,"agentManagement":true,"future":[42]}"#.to_vec(),
        br#"{"version":1,"agentManagement":"yes"}"#.to_vec(),
        b"{malformed".to_vec(),
        serde_json::to_vec(&json!({"version":1,"future":vec![0; 12000]})).unwrap(),
    ] {
        assert!(bytes.len() < 64 * 1024);
        fs::write(&path, &bytes).unwrap();
        let output = backend(
            root.path(),
            &["preferences-set", "--agent-management", "true"],
        );
        assert!(
            !output.status.success()
                || serde_json::from_slice::<Value>(&output.stdout)
                    .is_ok_and(|value| value["ok"] == false),
            "{output:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
