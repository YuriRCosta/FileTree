use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct Resident {
    _state: tempfile::TempDir,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    frames: Receiver<Value>,
    reader: Option<JoinHandle<()>>,
}

impl Resident {
    pub fn start(max_concurrency: usize) -> Self {
        Self::start_with(max_concurrency, &[])
    }

    pub fn start_with(max_concurrency: usize, environment: &[(&str, &str)]) -> Self {
        let state = tempfile::tempdir().expect("resident state directory");
        let mut child = Command::new(env!("CARGO_BIN_EXE_fileblade"))
            .args(["serve", "--max-concurrency", &max_concurrency.to_string()])
            .envs(environment.iter().copied())
            .env("HOME", state.path().join("home"))
            .env("XDG_CONFIG_HOME", state.path().join("config"))
            .env("XDG_STATE_HOME", state.path().join("state"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start resident backend");
        let stdin = child.stdin.take().expect("resident stdin");
        let stdout = child.stdout.take().expect("resident stdout");
        let (sender, frames) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(frame) = serde_json::from_str(&line) else {
                    break;
                };
                if sender.send(frame).is_err() {
                    break;
                }
            }
        });
        let mut resident = Self {
            _state: state,
            child: Some(child),
            stdin: Some(stdin),
            frames,
            reader: Some(reader),
        };
        resident.send(serde_json::json!({"v": 1, "type": "hello"}));
        let hello = resident.receive();
        assert_eq!(hello["type"], "hello", "{hello}");
        assert_eq!(hello["ok"], true, "{hello}");
        resident
    }

    pub fn send(&mut self, frame: Value) {
        let stdin = self.stdin.as_mut().expect("resident stdin is open");
        serde_json::to_writer(&mut *stdin, &frame).expect("write protocol frame");
        stdin.write_all(b"\n").expect("terminate protocol frame");
        stdin.flush().expect("flush protocol frame");
    }

    pub fn request(&mut self, id: &str, command: &str, arguments: &[String], deadline_ms: u64) {
        self.send(serde_json::json!({
            "v": 1,
            "type": "request",
            "id": id,
            "generation": 1,
            "command": command,
            "arguments": arguments,
            "deadline_ms": deadline_ms
        }));
    }

    pub fn receive(&self) -> Value {
        self.frames
            .recv_timeout(Duration::from_secs(20))
            .expect("resident protocol response")
    }

    pub fn receive_where(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let frame = self
                .frames
                .recv_timeout(remaining)
                .expect("matching resident protocol response");
            if predicate(&frame) {
                return frame;
            }
        }
    }

    pub fn response(&self, id: &str) -> Value {
        self.receive_where(|frame| frame["id"] == id && frame["type"] == "response")
    }

    pub fn finish(mut self) {
        self.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        let child = self.child.as_mut().expect("resident child");
        loop {
            if let Some(status) = child.try_wait().expect("resident child status") {
                assert!(status.success(), "resident backend exited with {status}");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "resident backend did not exit after stdin closed"
            );
            thread::sleep(Duration::from_millis(10));
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for Resident {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
