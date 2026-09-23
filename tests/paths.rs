use std::fs;

fn scratch(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("filetree-paths-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn legacy_state_directory_moves_once_and_never_over_an_existing_one() {
    let root = scratch("state");
    unsafe { std::env::set_var("XDG_STATE_HOME", &root) };
    let legacy = root.join("omarchy/fileblade");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("state.json"), "{}").unwrap();

    let current = fileblade::paths::state_dir();
    assert_eq!(current, root.join("omarchy/filetree"));
    assert!(current.join("state.json").is_file());
    assert!(!legacy.exists());

    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("state.json"), "stale").unwrap();
    assert_eq!(fileblade::paths::state_dir(), current);
    assert_eq!(
        fs::read_to_string(current.join("state.json")).unwrap(),
        "{}"
    );
    assert!(legacy.join("state.json").is_file());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn legacy_config_symlink_is_left_alone() {
    let root = scratch("config");
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &root) };
    let elsewhere = root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::create_dir_all(root.join("omarchy")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.join("omarchy/fileblade")).unwrap();

    let current = fileblade::paths::config_dir();
    assert_eq!(current, root.join("omarchy/filetree"));
    assert!(current.symlink_metadata().is_err());
    assert!(
        root.join("omarchy/fileblade")
            .symlink_metadata()
            .unwrap()
            .is_symlink()
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_replaced_binary_path_drops_the_deleted_suffix_when_the_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("filetree");
    std::fs::write(&binary, b"").unwrap();
    let deleted = std::path::PathBuf::from(format!("{} (deleted)", binary.display()));
    assert_eq!(fileblade::common::replaced_binary_path(&deleted), binary);
    let missing =
        std::path::PathBuf::from(format!("{} (deleted)", dir.path().join("gone").display()));
    assert_eq!(fileblade::common::replaced_binary_path(&missing), missing);
    assert_eq!(fileblade::common::replaced_binary_path(&binary), binary);
}

#[test]
fn a_fileblade_install_keeps_its_layout_after_the_rename_to_filetree() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("config");
    let state = home.path().join("state");
    let old_config = config.join("omarchy/fileblade");
    let old_state = state.join("omarchy/fileblade");
    fs::create_dir_all(&old_config).unwrap();
    fs::create_dir_all(&old_state).unwrap();
    let layout = r#"{"version":1,"blades":{"left":{"open":true,"width":500,"slots":[{"id":"files","modules":[{"module":"files","state":{"root":"/home/example","toolbarButtons":["home","downloads"]}}]}]}}}"#;
    fs::write(old_config.join("blades.json"), layout).unwrap();
    fs::write(
        old_state.join("state.json"),
        r#"{"favorites":["/home/example/src"]}"#,
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_filetree"))
        .args(["_backend", "layout-read"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_STATE_HOME", &state)
        .env_remove("FILETREE_NATIVE_STATE_ROOT")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["text"], layout);

    let new_config = config.join("omarchy/filetree");
    assert_eq!(
        fs::read_to_string(new_config.join("blades.json")).unwrap(),
        layout
    );
    assert!(!old_config.exists());

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_filetree"))
        .args(["_backend", "state-read"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_STATE_HOME", &state)
        .env_remove("FILETREE_NATIVE_STATE_ROOT")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(state.join("omarchy/filetree/state.json")).unwrap(),
        r#"{"favorites":["/home/example/src"]}"#
    );
    assert!(!old_state.exists());
}
