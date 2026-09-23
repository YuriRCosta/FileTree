use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
fn sigterm_stops_a_server_waiting_for_the_next_protocol_line() {
    let temporary = tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_filetree"))
        .args(["serve", "--no-recover"])
        .env("XDG_STATE_HOME", temporary.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"{\"v\":1,\"type\":\"hello\"}\n").unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut hello = String::new();
    reader.read_line(&mut hello).unwrap();
    assert!(hello.contains("\"ok\":true"), "{hello}");
    stdin.write_all(b"{").unwrap();
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            break;
        }
        if started.elapsed() > Duration::from_secs(3) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("server did not shut down after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
