use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{fs, process::Command};

struct Fixture {
    root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }
    fn run(&self, arguments: &[&str]) -> Value {
        let result = Command::new(
            std::env::var_os("FILETREE_BINARY")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_filetree").into()),
        )
        .arg("_backend")
        .args(arguments)
        .env("HOME", self.root.path())
        .env("XDG_CONFIG_HOME", self.root.path().join("config"))
        .env("XDG_STATE_HOME", self.root.path().join("state"))
        .env("XDG_DATA_HOME", self.root.path().join("data"))
        .output()
        .unwrap();
        if result.stdout.is_empty() && !result.status.success() {
            return json!({"ok":false,"error":String::from_utf8_lossy(&result.stderr)});
        }
        serde_json::from_slice(&result.stdout)
            .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&result.stderr)))
    }
    fn config(&self) -> std::path::PathBuf {
        self.root.path().join("config/omarchy/filetree")
    }
}

#[test]
fn fresh_and_existing_installs_need_an_answer_and_every_offered_answer_persists() {
    for days in [0, 1, 7, 30, 90] {
        let fixture = Fixture::new();
        assert!(fixture.run(&["preferences-read"])["settings"]["trashRetentionDays"].is_null());
        assert!(!fixture.config().join("settings.json").exists());
        fs::create_dir_all(fixture.config()).unwrap();
        fs::write(
            fixture.config().join("state.json"),
            r#"{"trashRetentionDays":7}"#,
        )
        .unwrap();
        assert!(fixture.run(&["preferences-read"])["settings"]["trashRetentionDays"].is_null());
        let saved = fixture.run(&[
            "preferences-set",
            "--trash-retention-days",
            &days.to_string(),
        ]);
        assert_eq!(saved["ok"], true, "{saved}");
        let next_start = fixture.run(&["preferences-read"]);
        assert_eq!(next_start["settings"]["trashRetentionDays"], days);
        let path = fixture.config().join("settings.json");
        let config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(config["version"], 1);
        assert_eq!(config["filetreeVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn keybindings_gain_release_metadata_without_losing_custom_bindings() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.config()).unwrap();
    let path = fixture.config().join("keybindings.json");
    fs::write(&path, r#"{"version":1,"bindings":{"next":["n"]}}"#).unwrap();
    let response = fixture.run(&["keybindings-prepare"]);
    assert_eq!(response["ok"], true, "{response}");
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["bindings"], json!({"next":["n"]}));
    assert_eq!(saved["filetreeVersion"], env!("CARGO_PKG_VERSION"));
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    fixture.run(&["keybindings-prepare"]);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    let future = r#"{"version":99,"bindings":{"next":["n"]}}"#;
    fs::write(&path, future).unwrap();
    assert_eq!(fixture.run(&["keybindings-prepare"])["ok"], false);
    assert_eq!(fs::read_to_string(path).unwrap(), future);
}
