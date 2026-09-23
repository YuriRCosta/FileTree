use fileblade::command::CommandSpec;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[test]
fn input_and_both_outputs_make_progress_with_bounded_captures() {
    let program = r#"
import os
while True:
    data = os.read(0, 4096)
    if not data:
        break
    os.write(1, data)
    os.write(2, data)
os.write(1, b'out-end')
os.write(2, b'err-end')
"#;
    for tail in [false, true] {
        let result = CommandSpec::new("/usr/bin/python3")
            .args(["-c", program])
            .stdin(vec![0xff; 3 * 1024 * 1024])
            .limits(8000, 3000)
            .retain_tail(tail)
            .timeout(Duration::from_secs(5))
            .run()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout.len(), 8000);
        assert_eq!(result.stderr.len(), 3000);
        assert!(result.stdout_truncated && result.stderr_truncated);
        if tail {
            assert!(result.stdout.ends_with(b"out-end"));
            assert!(result.stderr.ends_with(b"err-end"));
        } else {
            assert!(
                result
                    .stdout
                    .iter()
                    .chain(&result.stderr)
                    .all(|byte| *byte == 0xff)
            );
        }
    }
}

#[test]
fn zero_limits_drain_output_without_retaining_it() {
    let result = CommandSpec::new("/bin/sh")
        .args(["-c", "printf out; printf err >&2"])
        .limits(0, 0)
        .run()
        .unwrap();
    assert!(result.status.success());
    assert!(result.stdout.is_empty() && result.stderr.is_empty());
    assert!(result.stdout_truncated && result.stderr_truncated);
}

#[test]
fn the_command_signal_is_reported_without_signalling_the_supervisor() {
    let result = CommandSpec::new("/bin/sh")
        .args(["-c", "kill -TERM $$"])
        .run()
        .unwrap();
    assert_eq!(result.status.signal(), Some(libc::SIGTERM));
    assert_eq!(result.status.code(), None);
}

#[test]
fn rust_still_reports_exec_failure_and_preserves_native_command_setup() {
    let directory = tempfile::tempdir().unwrap();
    let missing = CommandSpec::new(directory.path().join("absent"))
        .run()
        .unwrap_err();
    assert!(missing.to_string().contains("could not start"));
    assert!(missing.to_string().contains("os error 2"));
    let raw = std::ffi::OsStr::from_bytes(b"native-\xff");
    let result = CommandSpec::new("sh")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("FILETREE_NATIVE_TEST", raw)
        .cwd(directory.path())
        .args([
            std::ffi::OsStr::new("-c"),
            std::ffi::OsStr::new("printf '%s\n%s\n%s' \"$PWD\" \"$FILETREE_NATIVE_TEST\" \"$1\""),
            std::ffi::OsStr::new("sh"),
            raw,
        ])
        .run()
        .unwrap();
    let expected = [
        directory.path().as_os_str().as_bytes(),
        b"\nnative-\xff\nnative-\xff",
    ]
    .concat();
    assert_eq!(result.stdout, expected);
}

#[test]
fn cancellation_before_spawn_has_no_command_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("not-created");
    let result = CommandSpec::new("/usr/bin/touch")
        .args([marker.as_os_str()])
        .run_cancellable(&AtomicBool::new(true));
    assert!(matches!(result, Err(fileblade::AppError::Cancelled)));
    assert!(!marker.exists());
}

#[test]
fn many_concurrent_commands_keep_their_input_and_results_separate() {
    std::thread::scope(|scope| {
        for worker in 0..8 {
            scope.spawn(move || {
                for index in 0..20 {
                    let input = format!("worker {worker}, request {index}\n");
                    let result = CommandSpec::new("/bin/cat")
                        .stdin(input.as_bytes())
                        .run()
                        .unwrap();
                    assert!(result.status.success());
                    assert_eq!(result.stdout, input.as_bytes());
                    assert!(result.stderr.is_empty());
                    assert!(!result.stdout_truncated && !result.stderr_truncated);
                }
            });
        }
    });
}
