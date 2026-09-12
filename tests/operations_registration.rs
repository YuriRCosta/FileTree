use fileblade::backend;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::AtomicBool;

#[path = "support/isolated.rs"]
mod isolated;

fn request(arguments: &[&str]) -> Value {
    let command =
        backend::parse(std::iter::once("fileblade").chain(arguments.iter().copied())).unwrap();
    assert!(backend::mutating(&command));
    backend::dispatch(command, &AtomicBool::new(false), &mut |_| Ok(())).unwrap()
}

#[test]
fn archive_and_permissions_dispatch_through_the_mutation_boundary() {
    if !isolated::child(None) {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let archive = root.path().join("source.tar.zst");
    fs::write(&source, "registered archive contents").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
    let result = request(&[
        "archive-create",
        "--source",
        source.to_str().unwrap(),
        "--destination",
        archive.to_str().unwrap(),
    ]);
    assert_eq!(result["ok"], true, "{result}");
    assert!(archive.metadata().unwrap().len() > 0);
    let output = std::process::Command::new("bsdtar")
        .arg("-xOf")
        .arg(&archive)
        .arg("./source")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"registered archive contents");
    let result = request(&[
        "permissions-set",
        "--path",
        source.to_str().unwrap(),
        "--mode",
        "640",
    ]);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        source.metadata().unwrap().permissions().mode() & 0o777,
        0o640
    );
    let result = request(&["undo"]);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        source.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    let refused = request(&[
        "permissions-set",
        "--path",
        source.to_str().unwrap(),
        "--mode",
        "7777",
    ]);
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(
        source.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
}
