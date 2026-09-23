use fileblade::{locations, mounts::mountinfo::MountTable, secure};
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn escaped_mount_paths_resolve_the_original_native_directory() {
    use std::os::unix::ffi::OsStrExt;
    let root = tempfile::tempdir().unwrap();
    for name in [b"\xff disk".as_slice(), "é disk".as_bytes()] {
        let directory = root.path().join(std::ffi::OsStr::from_bytes(name));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("item"), "fixture").unwrap();
        let escaped = directory
            .as_os_str()
            .as_bytes()
            .iter()
            .map(|byte| {
                if byte.is_ascii_graphic() && *byte != b'\\' {
                    (*byte as char).to_string()
                } else {
                    format!("\\{byte:03o}")
                }
            })
            .collect::<String>();
        let table = MountTable::from_text(&format!("1 0 8:1 / {escaped} rw - ext4 /dev/test rw\n"));
        let record = table.primary("8:1").unwrap();
        assert_eq!(record.mountpoint, directory);
        let response = fileblade::filesystem::children(
            &fileblade::common::path_text(&record.mountpoint),
            false,
        );
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["entries"][0]["name"], "item");
    }
}

fn list(location: &locations::Descriptor, path: &str) -> serde_json::Value {
    let command = fileblade::backend::parse([
        "filetree",
        "list",
        "--location",
        &location.id,
        "--generation",
        &location.session_generation,
        "--path",
        path,
        "--no-git",
    ])
    .unwrap();
    fileblade::backend::dispatch(
        command,
        &std::sync::atomic::AtomicBool::new(false),
        &mut |_| Ok(()),
    )
    .unwrap()
}

#[test]
fn confined_open_rejects_mounts_and_keeps_directory_identity_after_rename() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    let directory = secure::open_directory_nofollow(root.path()).unwrap();
    let nested = secure::open_directory_within_mount(&directory, Path::new("nested")).unwrap();
    let identity = secure::stat_in(&nested, std::ffi::OsStr::new("."))
        .unwrap()
        .identity();
    fs::rename(root.path().join("nested"), root.path().join("old")).unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    assert_eq!(
        secure::stat_in(&nested, std::ffi::OsStr::new("."))
            .unwrap()
            .identity(),
        identity
    );
    let replacement = secure::open_directory_within_mount(&directory, Path::new("nested")).unwrap();
    assert_ne!(
        secure::stat_in(&replacement, std::ffi::OsStr::new("."))
            .unwrap()
            .identity(),
        identity
    );
    assert!(secure::open_directory_within_mount(&directory, Path::new("../")).is_err());
    let root = secure::open_directory_nofollow(Path::new("/")).unwrap();
    let proc = secure::open_directory_nofollow(Path::new("/proc")).unwrap();
    assert_ne!(
        secure::directory_mount_id(&root).unwrap(),
        secure::directory_mount_id(&proc).unwrap()
    );
    assert!(secure::open_directory_within_mount(&root, Path::new("proc")).is_err());
}

#[test]
#[ignore = "disposable VM private mount namespace only"]
fn nested_bind_mount_requires_its_own_generation_and_remount_invalidates_it() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let parent = root.path().join("parent");
    let mounted = parent.join("mounted");
    fs::create_dir(&source).unwrap();
    fs::create_dir_all(&mounted).unwrap();
    fs::write(source.join("item"), "fixture").unwrap();
    let id = format!("fixture:{}", parent.display());
    let descriptor = locations::local(
        &id,
        locations::Kind::Local,
        parent.to_str().unwrap(),
        "parent",
        &MountTable::read().unwrap(),
    )
    .unwrap();
    let pinned = secure::open_directory_nofollow(&parent).unwrap();
    let identity = secure::stat_in(&pinned, std::ffi::OsStr::new("."))
        .unwrap()
        .identity();
    let bind = || {
        assert!(
            Command::new("mount")
                .arg("--bind")
                .arg(&source)
                .arg(&mounted)
                .status()
                .unwrap()
                .success()
        )
    };
    let unmount = || {
        assert!(
            Command::new("umount")
                .arg(&mounted)
                .status()
                .unwrap()
                .success()
        )
    };
    let before_mount = list(&descriptor, "");
    assert_eq!(before_mount["ok"], true, "{before_mount}");
    assert_eq!(before_mount["entries"][0]["name"], "mounted");
    assert!(before_mount["entries"][0].get("mount_boundary").is_none());
    bind();
    let boundary = list(&descriptor, "");
    assert_eq!(boundary["ok"], true, "{boundary}");
    assert_eq!(boundary["entries"][0]["mount_boundary"], true);
    let refused = list(&descriptor, "mounted");
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["entries"], serde_json::json!([]));
    assert_eq!(
        secure::stat_in(&pinned, std::ffi::OsStr::new("."))
            .unwrap()
            .identity(),
        identity
    );
    assert!(locations::validate_local(&id, &descriptor.session_generation).is_ok());
    assert!(secure::open_directory_within_mount(&pinned, Path::new("mounted")).is_err());
    let nested_id = format!("fixture:{}", mounted.display());
    let first = locations::local(
        &nested_id,
        locations::Kind::Local,
        mounted.to_str().unwrap(),
        "nested",
        &MountTable::read().unwrap(),
    )
    .unwrap();
    assert!(locations::validate_local(&nested_id, &first.session_generation).is_ok());
    let nested_listing = list(&first, "");
    assert_eq!(nested_listing["ok"], true, "{nested_listing}");
    assert_eq!(nested_listing["entries"][0]["name"], "item");
    assert_eq!(nested_listing["generation"], first.session_generation);
    assert_eq!(list(&descriptor, "mounted")["ok"], false);
    let table = fs::read_to_string("/proc/self/mountinfo").unwrap();
    let mount_id = first.local_representation.as_ref().unwrap().mount_id;
    let text = table
        .lines()
        .map(|line| {
            if line.starts_with(&format!("{mount_id} ")) {
                let (head, _) = line.split_once(" - ").unwrap();
                format!("{head} - fuse.gvfsd-fuse fixture rw")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let provider = locations::local(
        "fixture:unvalidated-provider",
        locations::Kind::Local,
        mounted.to_str().unwrap(),
        "provider",
        &MountTable::from_text(&text),
    )
    .unwrap();
    assert_eq!(provider.connection, locations::Connection::Unavailable);
    assert!(provider.local_representation.is_none());
    let unavailable = list(&provider, "");
    assert_eq!(unavailable["ok"], false, "{unavailable}");
    assert_eq!(unavailable["entries"], serde_json::json!([]));
    assert_eq!(list(&descriptor, "mounted")["ok"], false);
    assert!(secure::open_directory_within_mount(&pinned, Path::new("mounted")).is_err());
    unmount();
    bind();
    assert!(locations::validate_local(&nested_id, &first.session_generation).is_err());
    let stale = list(&first, "");
    assert_eq!(stale["error_id"], "stale-location", "{stale}");
    assert_eq!(stale["entries"], serde_json::json!([]));
    let next = locations::local(
        &nested_id,
        locations::Kind::Local,
        mounted.to_str().unwrap(),
        "replacement",
        &MountTable::read().unwrap(),
    )
    .unwrap();
    assert_eq!(
        first.local_representation.as_ref().unwrap().inode,
        next.local_representation.as_ref().unwrap().inode
    );
    assert_ne!(first.session_generation, next.session_generation);
    assert!(locations::validate_local(&nested_id, &next.session_generation).is_ok());
    let reconnected = list(&next, "");
    assert_eq!(reconnected["ok"], true, "{reconnected}");
    assert_eq!(reconnected["entries"][0]["name"], "item");
    assert_eq!(reconnected["generation"], next.session_generation);
    assert_eq!(list(&descriptor, "mounted")["ok"], false);
    assert!(secure::open_directory_within_mount(&pinned, Path::new("mounted")).is_err());
    unmount();
    locations::invalidate(&id);
    locations::invalidate(&nested_id);
}
