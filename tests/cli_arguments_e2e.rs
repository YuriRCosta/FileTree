use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::process::Command;

#[test]
fn a_non_utf8_argument_is_answered_not_panicked() {
    let hostile = OsStr::from_bytes(b"/tmp/caf\xe9.txt");
    let output = Command::new(env!("CARGO_BIN_EXE_filetree"))
        .args(["_backend", "project-root", "--path"])
        .arg(hostile)
        .output()
        .unwrap();
    let code = output.status.code();
    assert_ne!(code, Some(101), "the CLI must not panic on non-UTF-8 argv");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
}
