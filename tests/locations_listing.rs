use fileblade::locations::Kind;
use fileblade::mounts::mountinfo::MountTable;
use fileblade::{backend, locations};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn descriptor(path: &Path, id: &str) -> locations::Descriptor {
    locations::local(
        id,
        Kind::Local,
        path.to_str().unwrap(),
        "Fixture",
        &MountTable::read().unwrap(),
    )
    .unwrap()
}

fn request(id: &str, generation: &str, arguments: &[&str]) -> Value {
    let command = backend::parse(
        [
            "filetree",
            "list",
            "--location",
            id,
            "--generation",
            generation,
            "--no-git",
        ]
        .into_iter()
        .chain(arguments.iter().copied()),
    )
    .unwrap();
    assert!(!backend::mutating(&command));
    backend::dispatch(command, &AtomicBool::new(false), &mut |_| Ok(())).unwrap()
}

#[test]
fn location_listing_uses_normal_paging_and_rejects_replaced_or_disconnected_roots() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("folder");
    fs::create_dir(&directory).unwrap();
    for name in ["a", "b", "c", ".hidden"] {
        fs::write(directory.join(name), name).unwrap();
    }
    let id = format!("fixture:{}", root.path().display());
    let first = descriptor(&directory, &id);
    let page = request(
        &id,
        &first.session_generation,
        &["--start", "1", "--count", "1"],
    );
    assert_eq!(page["ok"], true, "{page}");
    assert_eq!(page["total"], 3);
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["entries"][0]["name"], "b");
    assert_eq!(page["generation"], first.session_generation);
    fs::rename(&directory, root.path().join("old")).unwrap();
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("replacement"), "replacement").unwrap();
    let stale = request(&id, &first.session_generation, &[]);
    assert_eq!(stale["error_id"], "stale-location", "{stale}");
    assert_eq!(stale["entries"], serde_json::json!([]));
    let replacement = descriptor(&directory, &id);
    let page = request(&id, &replacement.session_generation, &[]);
    assert_eq!(page["ok"], true, "{page}");
    assert_eq!(page["total"], 1);
    assert_eq!(page["entries"][0]["name"], "replacement");
    locations::invalidate(&id);
    assert_eq!(
        request(&id, &replacement.session_generation, &[])["error_id"],
        "stale-location"
    );
}

#[test]
fn provider_uris_parent_paths_and_symlink_traversal_never_fall_back_to_local_listing() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("folder");
    fs::create_dir_all(directory.join("nested")).unwrap();
    fs::write(directory.join("nested/item"), "item").unwrap();
    symlink(root.path(), directory.join("escape")).unwrap();
    let id = format!("fixture:{}", root.path().display());
    let current = descriptor(&directory, &id);
    let page = request(&id, &current.session_generation, &["--path", "nested"]);
    assert_eq!(page["ok"], true, "{page}");
    assert_eq!(page["entries"][0]["name"], "item");
    for path in [
        "..",
        "nested/../../",
        "/",
        "sftp://host/path",
        "mtp://[usb:005,009]/",
    ] {
        let result = request(&id, &current.session_generation, &["--path", path]);
        assert_eq!(result["ok"], false, "{result}");
        assert_eq!(result["error_id"], "invalid-location-path", "{result}");
        assert_eq!(result["entries"], serde_json::json!([]));
    }
    assert_eq!(
        request(&id, &current.session_generation, &["--path", "escape"])["error_id"],
        "location-unavailable"
    );
    assert_eq!(
        request("sftp://host/path", "unvalidated", &[])["error_id"],
        "stale-location"
    );
    locations::invalidate(&id);
}

#[test]
fn stale_capability_snapshots_invalidate_a_previously_issued_generation() {
    let root = tempfile::tempdir().unwrap();
    let id = format!("fixture:{}", root.path().display());
    let first = descriptor(root.path(), &id);
    let mut text = fs::read_to_string("/proc/self/mountinfo").unwrap();
    let mount_id = first.local_representation.as_ref().unwrap().mount_id;
    text = text
        .lines()
        .map(|line| {
            if line.starts_with(&format!("{mount_id} ")) {
                let mut fields: Vec<_> = line.split(' ').collect();
                fields[5] = "ro";
                fields.join(" ")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let read_only = locations::local(
        &id,
        Kind::Local,
        root.path().to_str().unwrap(),
        "Read-only",
        &MountTable::from_text(&text),
    )
    .unwrap();
    assert_eq!(read_only.connection, locations::Connection::Unavailable);
    assert!(read_only.session_generation.is_empty());
    assert_eq!(
        request(&id, &first.session_generation, &[])["error_id"],
        "stale-location"
    );
    let refreshed = descriptor(root.path(), &id);
    assert_ne!(first.session_generation, refreshed.session_generation);
    assert_eq!(request(&id, &refreshed.session_generation, &[])["ok"], true);
    locations::invalidate(&id);
}
