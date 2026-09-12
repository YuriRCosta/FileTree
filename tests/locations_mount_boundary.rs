use fileblade::{locations, mounts::mountinfo::MountTable, secure};
use std::fs;
use std::path::Path;
use std::process::Command;

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
    bind();
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
    assert!(secure::open_directory_within_mount(&pinned, Path::new("mounted")).is_err());
    unmount();
    bind();
    assert!(locations::validate_local(&nested_id, &first.session_generation).is_err());
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
    assert!(secure::open_directory_within_mount(&pinned, Path::new("mounted")).is_err());
    unmount();
    locations::invalidate(&id);
    locations::invalidate(&nested_id);
}
