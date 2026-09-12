use serde_json::{Value, json};
use std::fs;
use std::process::Command;

#[test]
fn binding_metadata_updates_preserve_unknown_fields_and_refuse_newer_schemas() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config/omarchy/fileblade");
    fs::create_dir_all(&config).unwrap();
    let path = config.join("keybindings.json");
    let original = json!({"version":1,"bindings":{"next":["n"],"custom":["CTRL+K"]},
        "future":{"user":true,"ordered":[null,false,42]},"owner":"user"});
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    let run = || {
        Command::new(
            std::env::var_os("FILEBLADE_BINARY")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_fileblade").into()),
        )
        .args(["_backend", "keybindings-prepare"])
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_STATE_HOME", root.path().join("state"))
        .output()
        .unwrap()
    };
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
