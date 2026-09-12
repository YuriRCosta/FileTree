#[path = "support/isolated.rs"]
mod isolated;
use fileblade::{journal, operations::permissions, secure};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn stat(path: &Path) -> secure::EntryStat {
    secure::entry_stat_resolved(&secure::resolved_parent(path).unwrap()).unwrap()
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn unreadable_file_permissions_round_trip_and_conflicting_edits_are_refused() {
    if !isolated::child(None) {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("file");
    fs::write(&path, "preserve").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let cancelled = AtomicBool::new(false);
    let result = permissions::set(&[path.to_str().unwrap().into()], "000", &cancelled);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(mode(&path), 0);
    let result = journal::undo(false, false, &cancelled);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(mode(&path), 0o600);
    let result = journal::redo(false, false, &cancelled);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(mode(&path), 0);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let result = journal::undo(false, true, &cancelled);
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(mode(&path), 0o640);
    assert_eq!(fs::read_to_string(&path).unwrap(), "preserve");
}

#[test]
fn symlink_and_replaced_identity_never_change_the_replacement_or_link_target() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("file");
    let link = root.path().join("link");
    fs::write(&path, "preserve").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&path, &link).unwrap();
    assert!(secure::set_permissions_matching(&link, stat(&link), 0o777).is_err());
    assert_eq!(mode(&path), 0o600);
    let expected = stat(&path);
    fs::rename(&path, root.path().join("old")).unwrap();
    fs::write(&path, "replacement").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(secure::set_permissions_matching(&path, expected, 0o777).is_err());
    assert_eq!(mode(&path), 0o640);
    assert_eq!(mode(&root.path().join("old")), 0o600);
}

#[test]
fn entire_selection_is_validated_before_changing_any_permissions() {
    if !isolated::child(None) {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("file");
    let link = root.path().join("link");
    fs::write(&path, "preserve").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&path, &link).unwrap();
    let result = permissions::set(
        &[path.to_str().unwrap().into(), link.to_str().unwrap().into()],
        "777",
        &AtomicBool::new(false),
    );
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(mode(&path), 0o600);
    let result = permissions::set(
        &[path.to_str().unwrap().into()],
        "777",
        &AtomicBool::new(true),
    );
    assert_eq!(result["cancelled"], true, "{result}");
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn ordinary_directory_edits_preserve_existing_sticky_bit() {
    if !isolated::child(None) {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("directory");
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o1700)).unwrap();
    let cancelled = AtomicBool::new(false);
    let result = permissions::set(&[path.to_str().unwrap().into()], "750", &cancelled);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(mode(&path), 0o1750);
    let result = journal::undo(false, false, &cancelled);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(mode(&path), 0o1700);
}

#[test]
#[ignore = "requires an isolated read-only test mount"]
fn read_only_volume_refuses_permission_changes() {
    if !isolated::child(None) {
        return;
    }
    let path = std::env::var_os("FILEBLADE_PERMISSION_FAULT_PATH")
        .expect("read-only fixture path is required");
    let path = Path::new(&path);
    let before = mode(path);
    let result = permissions::set(
        &[path.to_str().unwrap().into()],
        "777",
        &AtomicBool::new(false),
    );
    assert_eq!(result["ok"], false, "{result}");
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("Read-only file system"),
        "{result}"
    );
    assert_eq!(mode(path), before);
    assert_eq!(result["paths"].as_array().unwrap().len(), 0);
}
