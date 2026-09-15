use std::path::Path;
use std::process::Command;

#[test]
fn straight_corners_hold_across_the_interface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .arg("tests/design_check.py")
        .current_dir(root)
        .output()
        .expect("design check");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn every_registered_exception_names_a_live_surface_and_a_reason() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let registry =
        std::fs::read_to_string(root.join("tools/design-exceptions.json")).expect("registry");
    let document: serde_json::Value = serde_json::from_str(&registry).expect("registry json");
    let entries = document["no-rounded-corners"]
        .as_array()
        .expect("rule entries");
    assert!(!entries.is_empty());
    for entry in entries {
        let path = entry["path"].as_str().expect("path");
        let binding = entry["binding"].as_str().expect("binding");
        let reason = entry["reason"].as_str().expect("reason");
        assert!(
            reason.len() > 12,
            "{path}: an exception needs a real reason"
        );
        let source = std::fs::read_to_string(root.join(path))
            .unwrap_or_else(|_| panic!("registered file is gone: {path}"));
        assert!(
            source.lines().any(|line| line.trim() == binding),
            "stale exception, the binding no longer exists: {path}: {binding}"
        );
    }
}
