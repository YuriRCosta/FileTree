use fileblade::{locations, mounts::mountinfo::MountTable, secure};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Mounted(PathBuf);

impl Mounted {
    fn bind(source: &Path, target: &Path) -> Self {
        let output = Command::new("mount")
            .arg("--bind")
            .arg(source)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self(target.to_path_buf())
    }
}

impl Drop for Mounted {
    fn drop(&mut self) {
        let output = Command::new("umount").arg(&self.0).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn location(path: &Path, id: &str, table: &MountTable) -> locations::Descriptor {
    locations::local(
        id,
        locations::Kind::Local,
        path.to_str().unwrap(),
        "Fixture",
        table,
    )
    .unwrap()
}

#[test]
#[ignore = "disposable VM private mount namespace only; run with one test thread"]
fn fresh_mount_observation_refuses_reused_numeric_ids_with_changed_classification() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("location");
    fs::create_dir(&directory).unwrap();
    let id = format!("fixture:admission:{}", directory.display());
    let target = PathBuf::from(format!("/proc/{}/mountinfo", std::process::id()));
    for change in [
        "fuse.gvfsd-fuse",
        "nfs4",
        "cifs",
        "read-only",
        "super-read-only",
        "detached",
    ] {
        let text = fs::read_to_string("/proc/self/mountinfo").unwrap();
        let snapshot = MountTable::from_text(&text);
        let original = location(&directory, &id, &snapshot);
        assert_eq!(original.connection, locations::Connection::Connected);
        let fd = secure::open_directory_nofollow(&directory).unwrap();
        let mount_id = secure::directory_mount_id(&fd).unwrap();
        let observation = text
            .lines()
            .filter_map(|line| {
                if !line.starts_with(&format!("{mount_id} ")) {
                    return Some(line.to_owned());
                }
                if change == "detached" {
                    return None;
                }
                let mut fields: Vec<_> = line.split(' ').collect();
                let separator = fields.iter().position(|field| *field == "-").unwrap();
                match change {
                    "read-only" => fields[5] = "ro",
                    "super-read-only" => fields[separator + 3] = "ro",
                    filesystem => fields[separator + 1] = filesystem,
                }
                Some(fields.join(" "))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let injected = root.path().join("observation");
        fs::write(&injected, observation).unwrap();
        {
            let _mounted = Mounted::bind(&injected, &target);
            assert_eq!(secure::directory_mount_id(&fd).unwrap(), mount_id);
            let refused = location(&directory, &id, &snapshot);
            assert_eq!(
                refused.connection,
                locations::Connection::Unavailable,
                "{change}: {refused:?}"
            );
            assert!(refused.capabilities.is_empty());
            assert!(refused.local_representation.is_none());
            assert!(refused.session_generation.is_empty());
            assert!(locations::validate_local(&id, &original.session_generation).is_err());
        }
        let refreshed = location(&directory, &id, &MountTable::read().unwrap());
        assert_eq!(refreshed.connection, locations::Connection::Connected);
        assert_ne!(original.session_generation, refreshed.session_generation);
    }
    locations::invalidate(&id);
}

#[test]
#[ignore = "disposable VM private mount namespace only; run with one test thread"]
fn read_only_remount_requires_refresh_and_never_advertises_writes() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("mounted");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("item"), "fixture").unwrap();
    let mounted = Mounted::bind(&source, &target);
    let snapshot = MountTable::read().unwrap();
    let id = format!("fixture:readonly:{}", target.display());
    let first = location(&target, &id, &snapshot);
    assert_eq!(first.connection, locations::Connection::Connected);
    assert!(first.capabilities.contains(&locations::Capability::Write));
    let output = Command::new("mount")
        .args(["-o", "remount,bind,ro"])
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stale = location(&target, &id, &snapshot);
    assert_eq!(stale.connection, locations::Connection::Unavailable);
    assert!(stale.session_generation.is_empty());
    assert!(locations::validate_local(&id, &first.session_generation).is_err());
    let current = location(&target, &id, &MountTable::read().unwrap());
    assert_eq!(current.connection, locations::Connection::Connected);
    assert!(current.capabilities.contains(&locations::Capability::Read));
    assert!(current.capabilities.contains(&locations::Capability::List));
    for capability in [
        locations::Capability::Write,
        locations::Capability::Mkdir,
        locations::Capability::Rename,
        locations::Capability::AtomicRename,
        locations::Capability::Trash,
        locations::Capability::Permissions,
    ] {
        assert!(!current.capabilities.contains(&capability));
    }
    drop(mounted);
    assert!(locations::validate_local(&id, &current.session_generation).is_err());
    locations::invalidate(&id);
}
