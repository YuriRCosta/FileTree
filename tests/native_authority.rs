use fileblade::lease::{Authority, LeaseError, WriteMode};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::{TempDir, tempdir};

#[test]
fn aliases_contend_on_the_same_kernel_lock_and_release_without_replacing_it() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("state");
    let first = Authority::acquire(&root).unwrap();
    let inode = fs::metadata(root.join("authority.lock")).unwrap().ino();
    let alias = temporary.path().join("alias");
    symlink(&root, &alias).unwrap();
    match Authority::acquire(&alias).unwrap_err() {
        LeaseError::Held { diagnostic } => assert_eq!(diagnostic.unwrap().pid, std::process::id()),
        other => panic!("{other}"),
    }
    drop(first);
    let second = Authority::acquire(&alias).unwrap();
    assert_eq!(second.root(), root);
    assert_eq!(
        fs::metadata(root.join("authority.lock")).unwrap().ino(),
        inode
    );
}

#[test]
fn symlinks_hardlinks_and_writable_roots_are_refused_without_touching_the_target() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    fs::write(&target, b"preserve").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    let root = temporary.path().join("state");
    fs::create_dir(&root).unwrap();
    let lock = root.join("authority.lock");
    symlink(&target, &lock).unwrap();
    assert!(Authority::acquire(&root).is_err());
    fs::remove_file(&lock).unwrap();
    fs::hard_link(&target, &lock).unwrap();
    assert!(Authority::acquire(&root).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"preserve");
    fs::remove_file(&lock).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(Authority::acquire(&root).is_err());
}

#[test]
fn replaced_storage_invalidates_the_authority() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("state");
    let authority = Authority::acquire(&root).unwrap();
    fs::rename(&root, temporary.path().join("moved")).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(authority.verify().is_err());
    fs::remove_dir(&root).unwrap();
    fs::rename(temporary.path().join("moved"), &root).unwrap();
    assert!(authority.verify().is_err());
}

#[test]
fn different_state_roots_cannot_own_the_same_config_or_recovery() {
    let temporary = tempdir().unwrap();
    let config = temporary.path().join("config");
    let recovery = temporary.path().join("recovery");
    let state = temporary.path().join("state");
    let first = Authority::acquire_bound(&state, &config, &recovery).unwrap();
    for (next_config, next_recovery) in [
        (config.clone(), temporary.path().join("other-recovery")),
        (temporary.path().join("other-config"), recovery.clone()),
    ] {
        assert!(matches!(
            Authority::acquire_bound(
                temporary.path().join("other-state"),
                next_config,
                next_recovery
            ),
            Err(LeaseError::Held { .. })
        ));
    }
    let record: Value =
        serde_json::from_slice(&fs::read(state.join("authority.lock")).unwrap()).unwrap();
    for (role, path) in [("config", &config), ("recovery", &recovery)] {
        let metadata = fs::metadata(path).unwrap();
        assert_eq!(record["roots"][role]["device"], metadata.dev());
        assert_eq!(record["roots"][role]["inode"], metadata.ino());
    }
    drop(first);
    Authority::acquire_bound(temporary.path().join("other-state"), config, recovery).unwrap();
}

#[test]
fn an_alias_change_latches_identity_loss_even_for_a_deduplicated_root() {
    let temporary = tempdir().unwrap();
    let state = temporary.path().join("state");
    fs::create_dir(&state).unwrap();
    let alias = temporary.path().join("alias");
    symlink(&state, &alias).unwrap();
    let authority = Authority::acquire_bound(&state, &alias, &state).unwrap();
    fs::remove_file(&alias).unwrap();
    fs::create_dir(&alias).unwrap();
    assert!(authority.verify().is_err());
    fs::remove_dir(&alias).unwrap();
    symlink(&state, &alias).unwrap();
    assert!(authority.verify().is_err());
}

#[test]
fn migration_modes_are_set_once_and_refuse_persistence_with_the_reason() {
    let temporary = tempdir().unwrap();
    let authority = Authority::acquire(temporary.path()).unwrap();
    let path = temporary.path().join("journal.json");
    assert!(
        authority
            .persistence_anchor(&path)
            .unwrap_err()
            .to_string()
            .contains("migration has not been prepared")
    );
    authority
        .set_write_mode(WriteMode::ReadOnly {
            reason: "legacy writer active".into(),
        })
        .unwrap();
    assert!(
        authority
            .persistence_anchor(&path)
            .unwrap_err()
            .to_string()
            .contains("migration-refused: legacy writer active")
    );
    assert!(authority.set_write_mode(WriteMode::Full).is_err());
    assert!(!path.exists());
}

#[test]
fn a_previously_opened_anchor_never_redirects_persistence_to_a_replacement_root() {
    use std::os::fd::AsRawFd;
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("state");
    let authority = Authority::acquire(&root).unwrap();
    authority.set_write_mode(WriteMode::Full).unwrap();
    let (directory, relative) = authority
        .persistence_anchor(&root.join("journal.json"))
        .unwrap()
        .unwrap();
    assert_eq!(relative, Path::new("journal.json"));
    let moved = temporary.path().join("original-state");
    fs::rename(&root, &moved).unwrap();
    fs::create_dir(&root).unwrap();
    fs::write(
        format!("/proc/self/fd/{}/journal.json", directory.as_raw_fd()),
        b"original",
    )
    .unwrap();
    assert_eq!(fs::read(moved.join("journal.json")).unwrap(), b"original");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    assert!(authority.verify().is_err());
    assert!(
        authority
            .persistence_anchor(&root.join("audit.jsonl"))
            .unwrap_err()
            .to_string()
            .contains("authority-lost")
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
}

struct Resident {
    temporary: TempDir,
    root: PathBuf,
    child: Child,
}

impl Resident {
    fn start() -> Self {
        Self::with_mode(true)
    }

    fn with_mode(isolated: bool) -> Self {
        let temporary = tempdir().unwrap();
        let root = temporary.path().join("state/omarchy/fileblade");
        let mut command = isolated_command(temporary.path(), &root);
        command.args(["serve", "--native-authority", "--no-recover"]);
        if isolated {
            command.arg("--native-isolated");
        }
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut resident = Self {
            temporary,
            root,
            child,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while fileblade::lease::transport::probe(&resident.root).is_err() {
            assert!(
                resident.child.try_wait().unwrap().is_none(),
                "authority exited before ready"
            );
            assert!(Instant::now() < deadline, "authority did not become ready");
            std::thread::sleep(Duration::from_millis(20));
        }
        resident
    }

    fn session(&self) -> Session {
        Session::open(&self.root)
    }
}

#[test]
fn in_flight_root_replacement_never_receives_persistence_and_reports_authority_lost() {
    for role in ["state", "config", "recovery"] {
        let resident = Resident::start();
        let source = resident.temporary.path().join("source");
        let destination = resident.temporary.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::File::create(&source)
            .unwrap()
            .set_len(1024 * 1024 * 1024)
            .unwrap();
        let mut view = resident.session();
        view.send(json!({"v":1,"type":"request","id":"replace-copy","generation":1,"command":"copy",
            "arguments":["--source",source,"--destination",destination,"--journal-id","replacement-copy"]}));
        let accepted = view.receive();
        assert_eq!(accepted["type"], "accepted");
        let op = accepted["op"].as_str().unwrap().to_owned();
        let deadline = Instant::now() + Duration::from_secs(10);
        let partial = loop {
            if let Some(path) = fs::read_dir(&destination)
                .unwrap()
                .flatten()
                .map(|entry| entry.path().join("item"))
                .find(|path| fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0))
            {
                break path;
            }
            assert!(
                Instant::now() < deadline,
                "copy did not reach temporary output"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(
            unsafe { libc::kill(resident.child.id() as i32, libc::SIGSTOP) },
            0
        );
        assert!(
            fs::metadata(&partial).unwrap().len() < 1024 * 1024 * 1024,
            "copy finished before replacement barrier"
        );
        let replaced = match role {
            "state" => resident.root.clone(),
            "config" => resident.temporary.path().join("config/omarchy/fileblade"),
            _ => resident.temporary.path().join("state/fileblade"),
        };
        let original = resident.temporary.path().join("pinned-original");
        fs::rename(&replaced, &original).unwrap();
        fs::create_dir(&replaced).unwrap();
        fs::write(replaced.join("sentinel"), b"unchanged").unwrap();
        assert_eq!(
            unsafe { libc::kill(resident.child.id() as i32, libc::SIGCONT) },
            0
        );
        let terminal = loop {
            let frame = view.receive();
            if frame["type"] == "response" {
                break frame;
            }
        };
        assert_eq!(terminal["error_id"], "authority-lost", "{terminal}");
        assert_eq!(terminal["ok"], false);
        assert_eq!(terminal["cancelled"], false);
        assert_eq!(view.result(&op), terminal);
        assert!(source.exists());
        assert_eq!(
            fs::read_dir(&replaced).unwrap().count(),
            1,
            "new {role} received writes"
        );
        assert_eq!(fs::read(replaced.join("sentinel")).unwrap(), b"unchanged");
        view.send(json!({"v":1,"type":"request","id":"late","generation":2,"command":"frecency-visit","arguments":["--path",source]}));
        let refused = view.receive();
        assert_eq!(refused["ok"], false);
        assert!(
            refused["error"]
                .as_str()
                .unwrap()
                .contains("authority-lost"),
            "{refused}"
        );
        fs::remove_file(replaced.join("sentinel")).unwrap();
        fs::remove_dir(&replaced).unwrap();
        fs::rename(&original, &replaced).unwrap();
        view.send(json!({"v":1,"type":"request","id":"restored","generation":3,"command":"frecency-visit","arguments":["--path",source]}));
        assert!(
            view.receive()["error"]
                .as_str()
                .unwrap()
                .contains("authority-lost")
        );
    }
}

#[test]
fn private_record_writers_refuse_replacement_roots_and_keep_open_parents_pinned() {
    use fileblade::lease::{durable, persistence::PersistenceSession};
    use std::sync::Arc;
    for role in ["state", "config", "recovery"] {
        let temporary = tempdir().unwrap();
        let root = temporary.path().join(role);
        let authority = Arc::new(
            Authority::acquire_bound(
                temporary.path().join("state"),
                temporary.path().join("config"),
                temporary.path().join("recovery"),
            )
            .unwrap(),
        );
        authority.set_write_mode(WriteMode::Full).unwrap();
        let _session = PersistenceSession::open(authority).unwrap();
        let document = root.join("records/document.json");
        durable::write_private_atomic(&document, b"original").unwrap();
        let manifest = root.join("records/manifest.json");
        durable::write_new_private(&manifest, b"manifest").unwrap();
        assert!(durable::write_new_private(&manifest, b"overwrite").is_err());
        let parent = fileblade::secure::resolved_parent(&document).unwrap();
        let moved = temporary.path().join("original");
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("sentinel"), b"untouched").unwrap();
        for outcome in [
            durable::write_private_atomic(&document, b"replacement"),
            durable::write_new_private(&root.join("new.json"), b"replacement"),
            fileblade::secure::write_private_atomic(&document, b"replacement"),
            fileblade::secure::write_new_private(&root.join("new.json"), b"replacement"),
            fileblade::secure::open_private_append(&document).map(|_| ()),
            fileblade::secure::ensure_private_directory(&root.join("new")).map(|_| ()),
            fileblade::secure::open_record_lock(&root.join("new.lock")).map(|_| ()),
        ] {
            assert!(outcome.unwrap_err().to_string().contains("authority-lost"));
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(fs::read(root.join("sentinel")).unwrap(), b"untouched");
        assert_eq!(
            fs::read(moved.join("records/document.json")).unwrap(),
            b"original"
        );
        assert_eq!(
            fs::read(moved.join("records/manifest.json")).unwrap(),
            b"manifest"
        );
        let stat = rustix::fs::fstat(&parent.directory).unwrap();
        assert_eq!(
            stat.st_ino,
            fs::metadata(moved.join("records")).unwrap().ino()
        );
    }
}

#[test]
fn an_unprepared_authority_refuses_layout_writes_before_admission() {
    let resident = Resident::with_mode(false);
    let mut view = resident.session();
    view.send(json!({"v":1,"type":"request","id":"layout","generation":1,"command":"layout-write","arguments":["--document","{}"]}));
    let frame = view.receive();
    assert_eq!(frame["error_id"], "migration-refused", "{frame}");
    assert!(
        !resident
            .temporary
            .path()
            .join("config/omarchy/fileblade/blades.json")
            .exists()
    );
}

impl Drop for Resident {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().ok().flatten().is_none() {
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn isolated_command(temporary: &Path, root: &Path) -> Command {
    let binary = std::env::var_os("FILEBLADE_TEST_BIN")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fileblade").into());
    let mut command = Command::new(binary);
    command
        .env("HOME", temporary)
        .env("XDG_STATE_HOME", temporary.join("state"))
        .env("XDG_CONFIG_HOME", temporary.join("config"))
        .env("XDG_DATA_HOME", temporary.join("data"))
        .env("FILEBLADE_SPIKE_HOME", temporary)
        .env("FILEBLADE_NATIVE_STATE_ROOT", root);
    command
}

struct Session {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Session {
    fn open(root: &Path) -> Self {
        let stream = UnixStream::connect(root.join("authority.sock")).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut session = Self { stream, reader };
        session.send(json!({"v":1,"type":"hello"}));
        assert_eq!(session.receive()["authority"], true);
        session
    }

    fn send(&mut self, frame: Value) {
        serde_json::to_writer(&mut self.stream, &frame).unwrap();
        self.stream.write_all(b"\n").unwrap();
    }

    fn receive(&mut self) -> Value {
        let mut line = String::new();
        assert!(
            self.reader.read_line(&mut line).unwrap() > 0,
            "unexpected EOF"
        );
        serde_json::from_str(&line).unwrap()
    }

    fn result(&mut self, op: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.send(json!({"v":1,"type":"operation","action":"get","op":op,"id":"query","generation":1}));
            let response = self.receive();
            assert_eq!(response["ok"], true, "{response}");
            if response["payload"]["complete"] == true {
                return response["payload"]["result"].clone();
            }
            assert!(Instant::now() < deadline, "operation did not complete");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn accepted_copy_survives_eof_and_result_can_be_fetched_by_a_new_view() {
    let mut resident = Resident::start();
    let source = resident.temporary.path().join("source");
    let destination = resident.temporary.path().join("destination");
    fs::create_dir(&destination).unwrap();
    fs::File::create(&source)
        .unwrap()
        .set_len(256 * 1024 * 1024)
        .unwrap();
    let mut view = resident.session();
    view.send(
        json!({"v":1,"type":"request","id":"copy","generation":1,"command":"copy",
        "arguments":["--source",source,"--destination",destination]}),
    );
    let accepted = view.receive();
    assert_eq!(accepted["type"], "accepted", "{accepted}");
    let op = accepted["op"].as_str().unwrap();
    drop(view);
    assert!(resident.child.try_wait().unwrap().is_none());
    let mut replacement = resident.session();
    let result = replacement.result(op);
    assert_eq!(result["payload"]["ok"], true, "{result}");
    assert_eq!(
        fs::metadata(destination.join("source")).unwrap().len(),
        256 * 1024 * 1024
    );
    assert!(
        result["payload"]["mappings"]
            .as_array()
            .is_some_and(|mappings| !mappings.is_empty()),
        "{result}"
    );
    replacement.send(
        json!({"v":1,"type":"operation","action":"fetch","op":op,"id":"fetch","generation":1}),
    );
    assert_eq!(replacement.receive()["payload"]["result"], result);
    replacement
        .send(json!({"v":1,"type":"operation","action":"get","op":op,"id":"gone","generation":1}));
    assert_eq!(replacement.receive()["ok"], false);
}

#[test]
fn accepted_move_survives_a_lost_progress_reader() {
    let resident = Resident::start();
    let destination = resident.temporary.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let paths: Vec<_> = (0..128)
        .map(|index| {
            let path = resident.temporary.path().join(format!("file-{index}"));
            fs::write(&path, format!("content-{index}")).unwrap();
            path
        })
        .collect();
    let mut arguments = vec![
        "--destination".to_string(),
        destination.display().to_string(),
    ];
    for path in &paths {
        arguments.extend(["--source".into(), path.display().to_string()]);
    }
    let mut view = resident.session();
    view.send(json!({"v":1,"type":"request","id":"move","generation":1,"command":"move","arguments":arguments}));
    let accepted = view.receive();
    assert_eq!(accepted["type"], "accepted", "{accepted}");
    view.stream.shutdown(std::net::Shutdown::Read).unwrap();
    let mut replacement = resident.session();
    let result = replacement.result(accepted["op"].as_str().unwrap());
    assert_eq!(result["payload"]["ok"], true, "{result}");
    for path in paths {
        assert!(!path.exists());
        assert!(destination.join(path.file_name().unwrap()).is_file());
    }
}

#[test]
fn second_authority_and_native_direct_mutations_fail_before_writing() {
    let resident = Resident::start();
    let second = isolated_command(resident.temporary.path(), &resident.root)
        .args(["serve", "--native-authority", "--no-recover"])
        .output()
        .unwrap();
    assert!(!second.status.success());
    let target = resident.temporary.path().join("forbidden");
    for arguments in [
        vec![
            "_backend",
            "create",
            "--parent",
            resident.temporary.path().to_str().unwrap(),
            "--name",
            "forbidden",
        ],
        vec!["preferences", "--agent-management", "true"],
        vec!["list", "--from", target.to_str().unwrap()],
        vec!["_backend", "dim-windows", "--state", "off"],
        vec!["_companion-mutate"],
    ] {
        let output = isolated_command(resident.temporary.path(), &resident.root)
            .args(arguments)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(combined.contains("owner-unavailable"), "{combined}");
    }
    assert!(!target.exists());
}

#[test]
fn explicit_operation_cancel_works_after_the_accepting_view_detaches() {
    let resident = Resident::start();
    let source = resident.temporary.path().join("large");
    let destination = resident.temporary.path().join("destination");
    fs::create_dir(&destination).unwrap();
    fs::File::create(&source)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let mut view = resident.session();
    view.send(
        json!({"v":1,"type":"request","id":"copy","generation":1,"command":"copy",
        "arguments":["--source",source,"--destination",destination]}),
    );
    let accepted = view.receive();
    assert_eq!(accepted["type"], "accepted", "{accepted}");
    let op = accepted["op"].as_str().unwrap();
    let mut replacement = resident.session();
    replacement.send(json!({"v":1,"type":"cancel","op":op}));
    let cancelled = replacement.receive();
    assert_eq!(cancelled["accepted"], true, "{cancelled}");
    drop(view);
    let result = replacement.result(op);
    assert!(
        result["cancelled"] == true || result["payload"]["cancelled"] == true,
        "{result}"
    );
    assert!(source.exists());
}

#[test]
fn a_killed_holder_releases_the_kernel_lock_despite_stale_diagnostics() {
    let mut resident = Resident::start();
    let inode = fs::metadata(resident.root.join("authority.lock"))
        .unwrap()
        .ino();
    resident.child.kill().unwrap();
    resident.child.wait().unwrap();
    let authority = Authority::acquire(&resident.root).unwrap();
    authority.verify().unwrap();
    assert_eq!(
        fs::metadata(resident.root.join("authority.lock"))
            .unwrap()
            .ino(),
        inode
    );
}

#[test]
fn an_idle_view_relay_does_not_block_the_qt_pipe_availability_query() {
    use std::os::fd::AsRawFd;
    let resident = Resident::start();
    let mut child = isolated_command(resident.temporary.path(), &resident.root)
        .args(["serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let probe = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        let mut available: libc::c_int = -1;
        let result = unsafe { libc::ioctl(stdout.as_raw_fd(), libc::FIONREAD, &mut available) };
        sender.send((result, available)).unwrap();
        stdout
    });
    let result = receiver.recv_timeout(Duration::from_millis(800));
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let stdout = probe.join().unwrap();
    assert_eq!(result.unwrap(), (0, 0));
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"{\"v\":1,\"type\":\"hello\"}\n")
        .unwrap();
    let mut response = String::new();
    BufReader::new(stdout).read_line(&mut response).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&response).unwrap()["authority"],
        true
    );
    child.kill().unwrap();
    child.wait().unwrap();
}
