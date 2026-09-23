use fileblade::archive;
use std::fs;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use tempfile::tempdir;

#[test]
fn list_and_extract_round_trip_through_bsdtar() {
    if !std::process::Command::new("bsdtar")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    unsafe { std::env::set_var("FILETREE_JOURNAL", root.join("journal.json")) };
    fs::create_dir_all(root.join("project/src")).unwrap();
    fs::write(root.join("project/src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("project/README.md"), "hello\n").unwrap();
    let cancelled = AtomicBool::new(false);
    let archive_path = root.join("project.tar.zst");
    let packed = std::process::Command::new("bsdtar")
        .args(["-a", "-cf", archive_path.to_str().unwrap(), "--", "project"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(packed.success());
    let listed = archive::list(archive_path.to_str().unwrap(), &cancelled);
    assert_eq!(listed["ok"], true, "{listed}");
    let names = listed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"project/src/main.rs".to_string()),
        "{names:?}"
    );
    fs::rename(root.join("project"), root.join("original")).unwrap();
    let extracted = archive::extract(archive_path.to_str().unwrap(), None, false, &cancelled);
    assert_eq!(extracted["ok"], true, "{extracted}");
    assert_eq!(
        extracted["destination"],
        root.join("project").to_str().unwrap()
    );
    assert_eq!(
        fs::read_to_string(root.join("project/project/src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    let refused = archive::extract(archive_path.to_str().unwrap(), None, false, &cancelled);
    assert_eq!(refused["ok"], false);
    assert!(archive::is_archive("Photos.TAR.GZ"));
    assert!(!archive::is_archive("notes.md"));
}

#[test]
fn extract_refuses_a_populated_destination_unless_merging() {
    if Command::new("bsdtar").arg("--version").output().is_err() {
        return;
    }
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    fs::create_dir_all(root.join("project")).unwrap();
    fs::write(root.join("project/a.txt"), "a").unwrap();
    let archive = root.join("project.tar");
    assert!(
        Command::new("bsdtar")
            .current_dir(root)
            .args(["-cf", "project.tar", "project"])
            .status()
            .unwrap()
            .success()
    );
    let destination = root.join("existing");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("keep.txt"), "keep").unwrap();
    let cancelled = AtomicBool::new(false);
    let refused = fileblade::archive::extract(
        archive.to_str().unwrap(),
        Some(destination.to_str().unwrap()),
        false,
        &cancelled,
    );
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(!destination.join("project").exists());
    let merged = fileblade::archive::extract(
        archive.to_str().unwrap(),
        Some(destination.to_str().unwrap()),
        true,
        &cancelled,
    );
    assert_eq!(merged["ok"], true, "{merged}");
    assert!(destination.join("project/a.txt").exists());
    assert!(destination.join("keep.txt").exists());
}
