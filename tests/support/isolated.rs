pub fn child(parent: Option<&std::path::Path>) -> bool {
    let thread = std::thread::current();
    let name = thread.name().expect("named test thread");
    if std::env::var("FILETREE_OPERATIONS_TEST_CHILD").as_deref() == Ok(name) {
        return true;
    }
    let state = match parent {
        Some(parent) => tempfile::tempdir_in(parent).unwrap(),
        None => tempfile::tempdir().unwrap(),
    };
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--include-ignored", "--nocapture"])
        .env("FILETREE_OPERATIONS_TEST_CHILD", name)
        .env("XDG_STATE_HOME", state.path().join("state"))
        .env("XDG_DATA_HOME", state.path().join("data"))
        .env("XDG_CONFIG_HOME", state.path().join("config"))
        .env("FILETREE_JOURNAL", state.path().join("journal.json"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}
