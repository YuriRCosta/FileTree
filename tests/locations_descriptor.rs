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
fn stale_classification_with_a_matching_mount_id_never_issues_capabilities() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_str().unwrap();
    let id = format!("stale-initial:{path}");
    let text = fs::read_to_string("/proc/self/mountinfo").unwrap();
    let directory = fileblade::secure::open_directory_nofollow(root.path()).unwrap();
    let mount_id = fileblade::secure::directory_mount_id(&directory).unwrap();
    for field in ["filesystem", "flags", "super-flags", "source"] {
        let first = locations::local(
            &id,
            Kind::Local,
            path,
            "Current",
            &MountTable::read().unwrap(),
        )
        .unwrap();
        assert_eq!(first.connection, Connection::Connected);
        let stale = text
            .lines()
            .map(|line| {
                if !line.starts_with(&format!("{mount_id} ")) {
                    return line.to_owned();
                }
                let mut fields: Vec<_> = line.split(' ').collect();
                let separator = fields.iter().position(|field| *field == "-").unwrap();
                match field {
                    "filesystem" => {
                        fields[separator + 1] = if fields[separator + 1] == "ext4" {
                            "xfs"
                        } else {
                            "ext4"
                        }
                    }
                    "flags" => fields[5] = "ro",
                    "super-flags" => fields[separator + 3] = "ro",
                    "source" => fields[separator + 2] = "stale-source",
                    _ => unreachable!(),
                }
                fields.join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let refused = locations::local(
            &id,
            Kind::Local,
            path,
            "Stale",
            &MountTable::from_text(&stale),
        )
        .unwrap();
        assert_eq!(
            refused.connection,
            Connection::Unavailable,
            "{field}: {refused:?}"
        );
        assert!(refused.session_generation.is_empty());
        assert!(refused.local_representation.is_none());
        assert!(refused.capabilities.is_empty());
        assert!(locations::validate_local(&id, &first.session_generation).is_err());
    }
    let refreshed = locations::local(
        &id,
        Kind::Local,
        path,
        "Current",
        &MountTable::read().unwrap(),
    )
    .unwrap();
    assert_eq!(refreshed.connection, Connection::Connected);
    assert!(refreshed.capabilities.contains(&Capability::Write));
    locations::invalidate(&id);
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
