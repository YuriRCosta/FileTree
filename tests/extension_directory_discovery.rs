use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

#[path = "common/plugin_environment.rs"]
mod plugin_environment;

#[test]
fn shell_service_activation_preserves_disabled_and_absent_providers() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    fs::create_dir(root.join("bin")).unwrap();
    plugin_environment::install(root, "a.both");
    let rows = json!([
        {"id":"a.both","enabled":false,"kinds":["service","bar-widget"]},
        {"id":"a.dropped","enabled":false,"kinds":["service","bar-widget"]},
        {"id":"a.absent","enabled":false,"kinds":["service","bar-widget"]},
        {"id":"a.widget","enabled":false,"kinds":["bar-widget"]},
        {"id":"a.service","enabled":true,"kinds":["service"]}
    ]);
    for row in rows.as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        let directory = root.join(".config/omarchy/plugins").join(id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("Module.qml"), "import QtQuick\nItem {}\n").unwrap();
        fs::write(
            directory.join("manifest.json"),
            json!({"id":id,
                "extensions":{"data-goblin.fileblade/blade":[{"id":"fixture","entry":"Module.qml"}]}
            })
            .to_string(),
        )
        .unwrap();
    }
    fs::write(
        root.join(".config/omarchy/shell.json"),
        json!({
            "plugins":[{"id":"a.both"},{"id":"a.dropped"}],
            "bar":{"layout":{"left":[{"id":"a.widget"}]}},"disabledPlugins":["a.dropped"]
        })
        .to_string(),
    )
    .unwrap();
    for listed in [
        rows,
        json!([{"enabled":true}]),
        json!([{"id":"a.both"}]),
        json!([{"id":"a.both","enabled":true},{"id":"a.both","enabled":true}]),
    ] {
        fs::write(root.join("enabled.json"), listed.to_string()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_fileblade"));
        plugin_environment::configure(&mut command, root);
        let output = command
            .args(["_backend", "plugin-catalog"])
            .env("XDG_CONFIG_HOME", root.join(".config"))
            .env_remove("FILEBLADE_NATIVE_STATE_ROOT")
            .output()
            .unwrap();
        let catalog: Value = serde_json::from_slice(&output.stdout).unwrap();
        let known = listed.as_array().unwrap().len() == 5;
        assert_eq!(
            catalog["activation"],
            if known { "known" } else { "unknown" },
            "{catalog}"
        );
        for row in catalog["providers"].as_array().unwrap() {
            assert_eq!(
                row["enabled"],
                known && matches!(row["id"].as_str(), Some("a.both" | "a.service")),
                "{row}"
            );
        }
    }
}

#[test]
fn incomplete_extension_directories_remain_discoverable_until_their_files_arrive() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let plugins = root.join("home/.config/omarchy/plugins");
    fs::create_dir_all(&plugins).unwrap();
    let id = "acme.directory-fixture";
    let directory = plugins.join(id);
    let binary = std::env::var_os("FILEBLADE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_fileblade")));
    let catalog = || {
        let output = Command::new(&binary)
            .args(["_backend", "plugin-catalog"])
            .env_clear()
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("PATH", "")
            .output()
            .expect("run plugin catalog");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}: {}",
                String::from_utf8_lossy(&output.stdout).trim()
            )
        })
    };
    let assert_unresolved = || {
        let document = catalog();
        assert_eq!(document["directory_names"], json!([id]));
        assert_eq!(document["providers"], json!([]));
    };
    fs::create_dir(&directory).unwrap();
    assert_unresolved();

    fs::write(directory.join("manifest.json"), b"{").unwrap();
    assert_unresolved();

    fs::write(
        directory.join("manifest.json"),
        json!({
            "schemaVersion": 1,
            "id": id,
            "name": "Directory fixture",
            "version": "0.1.0",
            "kinds": ["service"],
            "entryPoints": {"service": "Provider.qml"},
            "extensions": {
                "data-goblin.fileblade/blade": [{
                    "id": "fixture",
                    "name": "Directory fixture",
                    "entry": "Module.qml",
                    "provider": "Provider.qml",
                    "hostContract": 2
                }]
            }
        })
        .to_string(),
    )
    .unwrap();
    assert_unresolved();

    fs::write(directory.join("Module.qml"), b"Item {}\n").unwrap();
    fs::write(directory.join("Provider.qml"), b"Item {}\n").unwrap();
    let complete = catalog();
    assert_eq!(complete["directory_names"], json!([id]));
    assert_eq!(complete["providers"][0]["id"], id);
    assert_eq!(complete["providers"].as_array().unwrap().len(), 1);

    fs::write(plugins.join("regular-file"), b"fixture").unwrap();
    let with_file = catalog();
    assert_eq!(with_file["directory_names"], json!([id]));

    for index in 0..256 {
        fs::create_dir(plugins.join(format!("zz-directory-{index:03}"))).unwrap();
    }
    let bounded = catalog();
    assert!(bounded["directory_names"].as_array().unwrap().len() <= 256);
    assert_eq!(bounded["truncated"], true);
}
