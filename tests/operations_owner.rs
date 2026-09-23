use fileblade::command::CommandSpec;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn command(root: &Path, script: &str) -> CommandSpec {
    CommandSpec::new("/bin/sh")
        .args(["-c", script])
        .env("OWNER_ROOT", root)
        .timeout(Duration::from_secs(2))
        .limits(4096, 4096)
}

fn wait(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(Instant::now() < deadline, "owner did not settle");
        thread::sleep(Duration::from_millis(10));
    }
}

fn process(root: &Path, name: &str) -> std::path::PathBuf {
    Path::new("/proc").join(fs::read_to_string(root.join(name)).unwrap().trim())
}

#[test]
fn foreground_owner_receives_payload_and_drop_reaps_its_group() {
    let root = tempdir().unwrap();
    let owner = command(root.path(), "/bin/cat > \"$OWNER_ROOT/payload\"; echo $$ > \"$OWNER_ROOT/pid\"; /bin/sleep 600 & echo $! > \"$OWNER_ROOT/descendant\"; wait")
        .spawn_owner(b"cut\nfile:///tmp/space%20name".to_vec()).unwrap();
    assert_eq!(
        fs::read(root.path().join("payload")).unwrap(),
        b"cut\nfile:///tmp/space%20name"
    );
    let parent = process(root.path(), "pid");
    let child = process(root.path(), "descendant");
    assert!(parent.exists() && child.exists());
    assert!(!owner.is_finished());
    drop(owner);
    assert!(!parent.exists() && !child.exists());
}

#[test]
fn early_exit_reports_stderr_and_never_accepts_an_owner() {
    let root = tempdir().unwrap();
    let result = command(
        root.path(),
        "/bin/cat >/dev/null; printf 'cannot claim clipboard' >&2; exit 7",
    )
    .spawn_owner(vec![1]);
    let error = match result {
        Ok(_) => panic!("dead owner accepted"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("cannot claim clipboard"),
        "{error}"
    );
}

#[test]
fn full_input_pipe_times_out_and_reaps_the_process() {
    let root = tempdir().unwrap();
    let start = Instant::now();
    let result = command(
        root.path(),
        "echo $$ > \"$OWNER_ROOT/pid\"; exec /bin/sleep 600",
    )
    .timeout(Duration::from_millis(80))
    .spawn_owner(vec![0; 1024 * 1024]);
    assert!(result.is_err());
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(!process(root.path(), "pid").exists());
}

#[test]
fn cancellation_during_startup_cleans_the_owner() {
    let root = tempdir().unwrap();
    let cancelled = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_millis(50));
            cancelled.store(true, Ordering::Relaxed);
        });
        let result = command(
            root.path(),
            "/bin/cat >/dev/null; echo $$ > \"$OWNER_ROOT/pid\"; exec /bin/sleep 600",
        )
        .spawn_owner_cancellable(vec![0], &cancelled);
        assert!(matches!(result, Err(fileblade::AppError::Cancelled)));
    });
    assert!(!process(root.path(), "pid").exists());
}

#[test]
fn replaced_selection_owner_is_reaped_without_another_write() {
    let root = tempdir().unwrap();
    let owner = command(
        root.path(),
        "/bin/cat >/dev/null; echo $$ > \"$OWNER_ROOT/pid\"; exec /bin/sleep 0.4",
    )
    .spawn_owner(vec![0])
    .unwrap();
    wait(|| owner.is_finished());
    assert!(!process(root.path(), "pid").exists());
}

#[test]
fn output_flood_after_acceptance_is_bounded_and_cleaned() {
    let root = tempdir().unwrap();
    let owner = command(root.path(), "/bin/cat >/dev/null; echo $$ > \"$OWNER_ROOT/pid\"; /bin/sleep 0.4; /bin/head -c 65536 /dev/zero; exec /bin/sleep 600")
        .spawn_owner(vec![0]).unwrap();
    wait(|| owner.is_finished());
    assert!(!process(root.path(), "pid").exists());
}

#[test]
fn resident_clipboard_slot_has_one_scoped_owner() {
    use fileblade::clipboard::Session;
    let session = Session::open().unwrap();
    assert!(Session::open().is_err());
    drop(session);
    let session = Session::open().unwrap();
    drop(session);
    let result = fileblade::clipboard::write("text/plain", vec![], &AtomicBool::new(false));
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("resident FileTree server")
    );
}
