use fileblade::secure;
use rustix::fs::{CWD, Mode, mkfifoat};
use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn preview_rejects_fifo_without_waiting_for_a_writer() {
    let temporary = tempdir().unwrap();
    let fifo = temporary.path().join("preview.txt");
    mkfifoat(CWD, &fifo, Mode::from_raw_mode(0o600)).unwrap();
    let result = Command::new("timeout")
        .args([
            "2",
            env!("CARGO_BIN_EXE_filetree"),
            "_backend",
            "preview",
            "--path",
        ])
        .arg(&fifo)
        .output()
        .unwrap();
    assert_ne!(
        result.status.code(),
        Some(124),
        "opening a FIFO must not block"
    );
    assert!(secure::read_private_bounded(&fifo, 1024).is_err());
    assert!(secure::verify_private_file(&fifo).is_err());
    assert!(secure::open_private_lock(&fifo).is_err());
    assert!(secure::open_private_append(&fifo).is_err());
    assert!(fs::symlink_metadata(fifo).is_ok());
}
