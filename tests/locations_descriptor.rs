use fileblade::locations::{self, Capability, Connection, Kind};
use fileblade::mounts::mountinfo::MountTable;
use std::fs;
use std::os::unix::fs::symlink;

#[test]
fn local_identity_has_stable_generation_until_replacement_or_disconnect() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("folder");
    fs::create_dir(&directory).unwrap();
    let id = format!("fixture:{}", root.path().display());
    let table = MountTable::read().unwrap();
    let first = locations::local(
        &id,
        Kind::Local,
        directory.to_str().unwrap(),
        "Folder",
        &table,
    )
    .unwrap();
    assert_eq!(first.connection, Connection::Connected);
    assert!(first.capabilities.contains(&Capability::List));
    assert!(first.capabilities.contains(&Capability::Read));
    assert!(first.capabilities.contains(&Capability::Write));
    assert_eq!(
        locations::validate_local(&id, &first.session_generation).unwrap(),
        directory
    );
    let repeated = locations::local(
        &id,
        Kind::Local,
        directory.to_str().unwrap(),
        "Folder",
        &table,
    )
    .unwrap();
    assert_eq!(first.session_generation, repeated.session_generation);
    fs::rename(&directory, root.path().join("old")).unwrap();
    fs::create_dir(&directory).unwrap();
    assert!(locations::validate_local(&id, &first.session_generation).is_err());
    let replaced = locations::local(
        &id,
        Kind::Local,
        directory.to_str().unwrap(),
        "Folder",
        &table,
    )
    .unwrap();
    assert_ne!(first.session_generation, replaced.session_generation);
    locations::invalidate(&id);
    assert!(locations::validate_local(&id, &replaced.session_generation).is_err());
    let reconnected = locations::local(
        &id,
        Kind::Local,
        directory.to_str().unwrap(),
        "Folder",
        &table,
    )
    .unwrap();
    assert_ne!(replaced.session_generation, reconnected.session_generation);
    locations::invalidate(&id);
}

#[test]
fn read_only_and_unvalidated_provider_mounts_never_advertise_writes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_str().unwrap();
    let directory = fileblade::secure::open_directory_nofollow(root.path()).unwrap();
    let mount_id = fileblade::secure::directory_mount_id(&directory).unwrap();
    let table = MountTable::from_text(&format!(
        "{mount_id} 1 0:40 / {path} ro - ext4 /dev/example ro\n"
    ));
    let id = format!("readonly:{path}");
    let local = locations::local(&id, Kind::Usb, path, "Read-only", &table).unwrap();
    assert_eq!(local.connection, Connection::Connected);
    assert!(local.capabilities.contains(&Capability::Read));
    for capability in [
        Capability::Write,
        Capability::Mkdir,
        Capability::Rename,
        Capability::AtomicRename,
        Capability::Permissions,
        Capability::Trash,
    ] {
        assert!(!local.capabilities.contains(&capability));
    }
    let table = MountTable::from_text(&format!(
        "{mount_id} 1 0:40 / {path} rw - ext4 /dev/example ro\n"
    ));
    let super_readonly =
        locations::local(&id, Kind::Usb, path, "Read-only filesystem", &table).unwrap();
    assert!(!super_readonly.capabilities.contains(&Capability::Write));
    let table = MountTable::from_text(&format!(
        "42 1 0:41 / {path} rw - fuse.gvfsd-fuse gvfsd-fuse rw\n"
    ));
    let provider = locations::local(&id, Kind::Local, path, "Provider", &table).unwrap();
    assert_eq!(provider.connection, Connection::Unavailable);
    assert!(provider.capabilities.is_empty());
    assert!(provider.local_representation.is_none());
    assert!(locations::validate_local(&id, &local.session_generation).is_err());
}

#[test]
fn remote_descriptors_preserve_provider_uris_without_any_posix_fallback() {
    for (kind, uri) in [
        (Kind::Mtp, "mtp://Device_SERIAL/"),
        (Kind::Mtp, "mtp://[usb:005,009]/"),
        (Kind::Sftp, "sftp://kurt@tailnet-host/home/kurt/"),
    ] {
        let descriptor =
            locations::disconnected("remote", kind, uri, "Remote", Connection::Locked).unwrap();
        assert_eq!(descriptor.canonical_uri, uri);
        assert!(descriptor.capabilities.is_empty());
        assert!(descriptor.local_representation.is_none());
        assert!(descriptor.session_generation.is_empty());
        let encoded = serde_json::to_value(descriptor).unwrap();
        assert!(encoded.get("local_representation").is_none());
        assert_eq!(encoded["capabilities"], serde_json::json!([]));
        assert!(
            locations::local(
                "remote",
                Kind::Local,
                uri,
                "Remote",
                &MountTable::read().unwrap()
            )
            .is_err()
        );
    }
    assert!(locations::canonical_uri(Kind::Sftp, "sftp://user:secret@host/path").is_err());
    assert!(locations::canonical_uri(Kind::Sftp, "sftp://host/path?override=1").is_err());
    assert!(locations::canonical_uri(Kind::Local, "file://remote/path").is_err());
    assert!(locations::canonical_uri(Kind::Mtp, "mtp://[usb:000,009]/").is_err());
}

#[test]
fn a_symlink_location_does_not_gain_capabilities_for_its_target() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("link");
    fs::create_dir(&target).unwrap();
    symlink(&target, &link).unwrap();
    let id = format!("symlink:{}", root.path().display());
    let descriptor = locations::local(
        &id,
        Kind::Local,
        link.to_str().unwrap(),
        "Link",
        &MountTable::read().unwrap(),
    )
    .unwrap();
    assert_eq!(descriptor.connection, Connection::Unavailable);
    assert!(descriptor.capabilities.is_empty());
    assert!(locations::validate_local(&id, &descriptor.session_generation).is_err());
}
