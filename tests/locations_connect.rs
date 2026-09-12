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
