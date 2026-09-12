use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

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
