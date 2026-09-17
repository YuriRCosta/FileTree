use fileblade::locations::{Connection, sftp, tailnet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn fixture() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("FILEBLADE_CONNECT_FIXTURE") {
        return Some(root.into());
    }
    let root = tempfile::tempdir().unwrap();
    let tool = root.path().join("gio");
    fs::write(&tool, r#"#!/usr/bin/python3
import os, sys, time
from pathlib import Path
root = Path(os.environ['FILEBLADE_CONNECT_FIXTURE'])
args = sys.argv[1:]
mode = (root/'mode').read_text()
mounted = root/'mounted'
if args[0] == 'mount':
    if '--unmount' in args:
        if (root/'cleanup-fails').exists(): sys.exit(1)
        mounted.unlink(missing_ok=True)
        with (root/'unmounted').open('a') as output: output.write('unmounted\n')
    else:
        if mounted.exists(): sys.exit(1)
        mounted.touch()
elif args[0] == 'info':
    if not mounted.exists(): sys.exit(1)
    if mode == 'pause':
        (root/'blocked').touch()
        while not (root/'release').exists(): time.sleep(.01)
    if mode == 'info-fails': sys.exit(1)
    print('standard::type: ' + ('1' if mode == 'file' else '2'))
    if not mode.startswith('unknown'): print('access::can-read: ' + ('FALSE' if mode == 'denied' else 'TRUE'))
elif args[0] == 'list':
    (root/'listed').touch()
    if (root/'listing').exists(): print((root/'listing').read_text(), end='')
    if mode == 'unknown-denied': sys.exit(1)
else:
    sys.exit(2)
"#).unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
    let name = std::thread::current().name().unwrap().to_owned();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &name, "--nocapture"])
        .env("FILEBLADE_CONNECT_FIXTURE", root.path())
        .env("PATH", root.path())
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_STATE_HOME", root.path().join("state"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    None
}

fn candidate() -> tailnet::Candidate {
    tailnet::candidates(&serde_json::json!({"BackendState":"Running","Peer":{"test":{"ID":"test","DNSName":"peer.tail.test"}}})).unwrap().remove(0)
}

fn saved() -> sftp::Saved {
    sftp::Saved {
        host: "peer.tail.test".into(),
        user: "user".into(),
        path: "/files".into(),
    }
}

fn mode(root: &Path, mode: &str) {
    for name in [
        "mounted",
        "unmounted",
        "blocked",
        "release",
        "listed",
        "cleanup-fails",
    ] {
        let _ = fs::remove_file(root.join(name));
    }
    fs::write(root.join("mode"), mode).unwrap();
}

#[test]
fn changed_host_is_refused_after_backend_rediscovery_before_any_gio_effect() {
    let Some(root) = fixture() else { return };
    let status = serde_json::json!({"BackendState":"Running","Peer":{"test":{"ID":"test","DNSName":"changed.tail.test"}}});
    let tool = root.join("tailscale");
    fs::write(
        &tool,
        format!("#!/usr/bin/python3\nprint({:?})\n", status.to_string()),
    )
    .unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
    mode(&root, "allowed");
    let command = fileblade::backend::parse([
        "fileblade",
        "location-connect",
        "--location",
        &candidate().location.id,
        "--expected-host",
        "peer.tail.test",
        "--user",
        "user",
        "--path",
        "/files",
    ])
    .unwrap();
    let result = fileblade::backend::dispatch(command, &AtomicBool::new(false), &mut |_| Ok(()));
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("selected peer changed")
    );
    assert!(!root.join("mounted").exists());
    assert!(sftp::snapshot(&candidate().location.id).is_none());
}

#[test]
fn refreshed_discovery_and_cancellation_cannot_publish_an_older_connect() {
    let Some(root) = fixture() else { return };
    let candidate = candidate();
    for change in ["removed", "changed", "returned", "cancelled"] {
        mode(&root, "pause");
        let cancelled = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let pending = scope.spawn(|| sftp::connect(&candidate, &saved(), &cancelled));
            let deadline = Instant::now() + Duration::from_secs(5);
            while !root.join("blocked").exists() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            if change == "changed" {
                let mut changed = candidate.clone();
                changed.host = "changed.tail.test".into();
                sftp::retain_candidates(&[changed]);
            } else if change == "cancelled" {
                cancelled.store(true, Ordering::Relaxed);
            } else {
                sftp::retain_candidates(&[]);
                if change == "returned" {
                    sftp::retain_candidates(std::slice::from_ref(&candidate));
                }
            }
            fs::write(root.join("release"), "").unwrap();
            assert!(pending.join().unwrap().is_err(), "{change}");
        });
        assert!(sftp::snapshot(&candidate.location.id).is_none());
        assert!(!root.join("mounted").exists());
        assert!(root.join("unmounted").exists());
        mode(&root, "allowed");
        let connected = sftp::connect(&candidate, &saved(), &AtomicBool::new(false)).unwrap();
        assert_eq!(connected.connection, Connection::Connected);
        assert_eq!(
            sftp::disconnect(
                &connected.id,
                &connected.session_generation,
                &AtomicBool::new(false)
            )["ok"],
            true
        );
    }
}

#[test]
fn admission_requires_read_evidence_and_keeps_cleanup_retriable_without_touching_shared_mounts() {
    let Some(root) = fixture() else { return };
    let candidate = candidate();
    let cancelled = AtomicBool::new(false);
    for failure in ["info-fails", "file", "denied", "unknown-denied"] {
        mode(&root, failure);
        assert!(sftp::connect(&candidate, &saved(), &cancelled).is_err());
        assert!(sftp::snapshot(&candidate.location.id).is_none());
        assert!(!root.join("mounted").exists());
        assert!(root.join("unmounted").exists());
    }
    for failure in ["file", "denied", "unknown-denied"] {
        mode(&root, failure);
        fs::write(root.join("mounted"), "existing").unwrap();
        assert!(sftp::connect(&candidate, &saved(), &cancelled).is_err());
        assert_eq!(
            fs::read_to_string(root.join("mounted")).unwrap(),
            "existing"
        );
        assert!(!root.join("unmounted").exists());
    }
    mode(&root, "unknown");
    let connected = sftp::connect(&candidate, &saved(), &cancelled).unwrap();
    assert!(root.join("listed").exists());
    assert_eq!(
        sftp::disconnect(&connected.id, &connected.session_generation, &cancelled)["ok"],
        true
    );
    mode(&root, "denied");
    fs::write(root.join("cleanup-fails"), "").unwrap();
    let cleanup = sftp::connect(&candidate, &saved(), &cancelled).unwrap();
    assert_eq!(cleanup.connection, Connection::Unavailable);
    assert!(cleanup.capabilities.is_empty());
    assert!(
        cleanup
            .error
            .as_ref()
            .unwrap()
            .contains("disconnect required")
    );
    sftp::retain_candidates(&[]);
    assert_eq!(sftp::cleanup_locations().len(), 1);
    assert_eq!(
        sftp::disconnect(&cleanup.id, &cleanup.session_generation, &cancelled)["ok"],
        false
    );
    assert!(sftp::snapshot(&cleanup.id).is_some());
    fs::remove_file(root.join("cleanup-fails")).unwrap();
    assert_eq!(
        sftp::disconnect(&cleanup.id, &cleanup.session_generation, &cancelled)["ok"],
        true
    );
    assert!(sftp::snapshot(&cleanup.id).is_none());
    assert!(!root.join("mounted").exists());
}

#[test]
fn remote_listing_preserves_names_and_refuses_other_authorities_or_parent_paths() {
    let Some(root) = fixture() else { return };
    mode(&root, "normal");
    let cancelled = AtomicBool::new(false);
    let connected = sftp::connect(&candidate(), &saved(), &cancelled).unwrap();
    let list = |path: &str| {
        let command = fileblade::backend::parse([
            "fileblade",
            "list",
            "--location",
            &connected.id,
            "--generation",
            &connected.session_generation,
            "--path",
            path,
        ])
        .unwrap();
        fileblade::backend::dispatch(command, &cancelled, &mut |_| Ok(())).unwrap()
    };
    fs::write(
        root.join("listing"),
        "sftp://user@peer.tail.test/files/a%0Ab%25\t4\t(regular)\ttime::modified=1700000000\n",
    )
    .unwrap();
    let response = list("");
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["entries"][0]["name"], "a\nb%");
    assert_eq!(response["entries"][0]["modified"], "2023-11-14T22:13:20Z");
    for uri in [
        "file:///files/name",
        "sftp://user@other/files/name",
        "sftp://other@peer.tail.test/files/name",
        "sftp://user@peer.tail.test/elsewhere/name",
        "sftp://user@peer.tail.test/files/a%2Fb",
        "broken record",
    ] {
        fs::write(root.join("listing"), format!("{uri}\t4\t(regular)\n")).unwrap();
        let response = list("");
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["entries"], serde_json::json!([]));
    }
    for path in ["..", "/files", "../escape", "sftp://other/files"] {
        assert_eq!(list(path)["error_id"], "invalid-location-path");
    }
    assert_eq!(
        sftp::disconnect(&connected.id, &connected.session_generation, &cancelled)["ok"],
        true
    );
}

#[test]
fn pending_connection_counts_toward_capacity_and_releases_its_reservation() {
    let Some(root) = fixture() else { return };
    mode(&root, "normal");
    let cancelled = AtomicBool::new(false);
    for index in 0..63 {
        let mut peer = candidate();
        peer.location.id = format!("tailnet:capacity-{index}");
        sftp::connect(&peer, &saved(), &cancelled).unwrap();
    }
    fs::write(root.join("mode"), "pause").unwrap();
    std::thread::scope(|scope| {
        let pending = scope.spawn(|| sftp::connect(&candidate(), &saved(), &cancelled));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !root.join("blocked").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            sftp::connect(&candidate(), &saved(), &cancelled)
                .unwrap_err()
                .to_string()
                .contains("already pending")
        );
        let mut overflow = candidate();
        overflow.location.id = "tailnet:overflow".into();
        assert!(
            sftp::connect(&overflow, &saved(), &cancelled)
                .unwrap_err()
                .to_string()
                .contains("at most 64")
        );
        cancelled.store(true, Ordering::Relaxed);
        fs::write(root.join("release"), "").unwrap();
        assert!(pending.join().unwrap().is_err());
    });
    fs::write(root.join("mode"), "normal").unwrap();
    let connected = sftp::connect(&candidate(), &saved(), &AtomicBool::new(false)).unwrap();
    assert_eq!(connected.connection, Connection::Connected);
}

#[test]
fn saved_remote_locations_encode_literal_names_and_refuse_extra_authority_fields() {
    let Some(root) = fixture() else { return };
    mode(&root, "normal");
    let selected = sftp::Saved {
        path: "/space #percent%/雪".into(),
        ..saved()
    };
    fileblade::locations::saved::remember(&selected).unwrap();
    let stored = fileblade::locations::saved::read().unwrap();
    assert_eq!(stored, [selected]);
    let descriptor = sftp::connect(&candidate(), &stored[0], &AtomicBool::new(false)).unwrap();
    assert!(
        descriptor
            .canonical_uri
            .ends_with("/space%20%23percent%25/%E9%9B%AA")
    );
    let path = root.join("config/omarchy/fileblade/locations.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(document["entries"][0].as_object().unwrap().len(), 3);
    document["entries"][0]["password"] = serde_json::json!("secret");
    fs::write(&path, document.to_string()).unwrap();
    assert!(fileblade::locations::saved::read().is_err());
    for host in ["user:secret@host", "host/path", "-oProxyCommand=bad"] {
        assert!(
            fileblade::locations::saved::remember(&sftp::Saved {
                host: host.into(),
                ..saved()
            })
            .is_err()
        );
    }
}
