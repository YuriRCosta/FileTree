use fileblade::command::CommandSpec;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const FIXTURE: &str = r#"
import os, signal, sys
read, write = os.pipe()
child = os.fork()
if child == 0:
    os.close(read)
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    if sys.argv[2] == 'independent':
        os.setsid()
    if sys.argv[2] == 'closed':
        null = os.open('/dev/null', os.O_RDWR)
        for fd in range(3):
            os.dup2(null, fd)
        os.close(null)
    os.write(write, b'1')
    os.close(write)
    while True:
        signal.pause()
os.close(write)
os.read(read, 1)
os.close(read)
with open(sys.argv[1], 'w') as output:
    output.write(f'{os.getpid()} {child}')
if sys.argv[2] in ('exit', 'closed', 'independent'):
    print('done', flush=True)
    sys.exit(7)
while True:
    signal.pause()
"#;

struct Fixture {
    directory: tempfile::TempDir,
    worker: Option<Child>,
}

impl Fixture {
    fn directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("child.py");
        fs::write(&script, format!("#!/usr/bin/python3\n{FIXTURE}")).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }

    fn start(mode: &str) -> Self {
        let directory = Self::directory();
        let worker = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "command_worker", "--nocapture"])
            .env("FILETREE_COMMAND_FIXTURE", directory.path())
            .env("FILETREE_COMMAND_MODE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            directory,
            worker: Some(worker),
        }
    }

    fn resident() -> Self {
        let directory = Self::directory();
        let manifest = serde_json::json!({
            "id": "test.native-lifetime",
            "version": "0.1.0",
            "extensions": {"yuricosta.filetree/action": [{
                "id": "owned", "title": "Owned command", "contexts": ["none"],
                "argv": ["child.py", directory.path().join("pids"), "hold"],
                "timeout": 30
            }]}
        });
        fs::write(directory.path().join("manifest.json"), manifest.to_string()).unwrap();
        let worker = Command::new(env!("CARGO_BIN_EXE_filetree"))
            .args(["serve", "--no-recover"])
            .env("HOME", directory.path())
            .env("XDG_STATE_HOME", directory.path().join("state"))
            .env("XDG_CONFIG_HOME", directory.path().join("config"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut fixture = Self {
            directory,
            worker: Some(worker),
        };
        fixture.send(serde_json::json!({"v": 1, "type": "hello"}));
        fixture.send(serde_json::json!({
            "v": 1, "type": "request", "id": "owned", "generation": 1,
            "command": "action-run", "deadline_ms": 30000,
            "arguments": ["--source", "plugin", "--plugin", "test.native-lifetime",
                "--plugin-dir", fixture.directory.path(), "--action", "owned", "--context", "none"]
        }));
        fixture
    }

    fn send(&mut self, message: serde_json::Value) {
        writeln!(
            self.worker.as_mut().unwrap().stdin.as_mut().unwrap(),
            "{message}"
        )
        .unwrap();
    }

    fn pids(&self) -> Vec<i32> {
        fs::read_to_string(self.directory.path().join("pids"))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|value| value.parse().ok())
            .collect()
    }

    fn wait_ready(&self) {
        wait_until(|| self.pids().len() == 2, "command fixture did not start");
    }

    fn finish(&mut self) {
        let worker = self.worker.as_mut().unwrap();
        wait_until(
            || worker.try_wait().unwrap().is_some(),
            "command runner did not finish within its bound",
        );
        let output = self.worker.take().unwrap().wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn assert_reaped(&self) {
        let pids = self.pids();
        assert_eq!(pids.len(), 2);
        wait_until(
            || pids.iter().all(|pid| !proc_path(*pid).exists()),
            "owned command processes survived completion",
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            let _ = worker.kill();
            let _ = worker.wait();
        }
        let script = self.directory.path().join("child.py");
        for pid in self.pids() {
            let arguments = fs::read(proc_path(pid).join("cmdline")).unwrap_or_default();
            if arguments
                .split(|byte| *byte == 0)
                .any(|argument| argument == script.as_os_str().as_encoded_bytes())
            {
                let pid = rustix::process::Pid::from_raw(pid).unwrap();
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            }
        }
    }
}

fn proc_path(pid: i32) -> PathBuf {
    PathBuf::from(format!("/proc/{pid}"))
}

fn wait_until(mut ready: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready() {
        assert!(Instant::now() < deadline, "{message}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn command_worker() {
    let Some(directory) = std::env::var_os("FILETREE_COMMAND_FIXTURE") else {
        return;
    };
    let directory = Path::new(&directory);
    let mode = std::env::var("FILETREE_COMMAND_MODE").unwrap();
    if mode == "fds" {
        let count = || fs::read_dir("/proc/self/fd").unwrap().count();
        let before = count();
        for _ in 0..80 {
            assert!(
                CommandSpec::new("/bin/true")
                    .run()
                    .unwrap()
                    .status
                    .success()
            );
        }
        assert_eq!(count(), before, "supervisor or pipe descriptor leaked");
        return;
    }
    if mode == "stdio" {
        for fd in 0..=2 {
            unsafe { libc::close(fd) };
        }
    }
    let child_mode = match mode.as_str() {
        "blocked" => "exit",
        "stdio" => "closed",
        "cancel" | "timeout" | "kill" => "hold",
        other => other,
    };
    let cancelled = AtomicBool::new(false);
    thread::scope(|scope| {
        if mode == "cancel" {
            let cancelled = &cancelled;
            scope.spawn(move || {
                wait_until(
                    || directory.join("pids").exists(),
                    "cancellation fixture missing",
                );
                cancelled.store(true, Ordering::Relaxed);
            });
        }
        let started = Instant::now();
        let result = CommandSpec::new("/usr/bin/python3")
            .args([
                directory.join("child.py").as_os_str(),
                directory.join("pids").as_os_str(),
                child_mode.as_ref(),
            ])
            .stdin(vec![b'x'; 1024 * 1024])
            .timeout(Duration::from_millis(if mode == "timeout" {
                500
            } else {
                10_000
            }))
            .run_cancellable(&cancelled);
        match mode.as_str() {
            "cancel" => assert!(matches!(result, Err(fileblade::AppError::Cancelled))),
            "timeout" => assert!(result.unwrap_err().to_string().contains("500 ms")),
            _ => {
                let output = result.unwrap();
                assert_eq!(output.status.code(), Some(7));
                assert_eq!(output.stdout, b"done\n");
            }
        }
        assert!(started.elapsed() < Duration::from_secs(2));
    });
}

#[test]
fn success_cleans_stubborn_descendants_even_with_closed_output() {
    let mut fixture = Fixture::start("closed");
    fixture.wait_ready();
    fixture.finish();
    fixture.assert_reaped();
}

#[test]
fn descendants_cannot_block_input_after_the_leader_exits() {
    let mut fixture = Fixture::start("blocked");
    fixture.wait_ready();
    fixture.finish();
    fixture.assert_reaped();
}

#[test]
fn cancellation_reaps_term_resistant_descendants() {
    let mut fixture = Fixture::start("cancel");
    fixture.wait_ready();
    fixture.finish();
    fixture.assert_reaped();
}

#[test]
fn timeout_reaps_term_resistant_descendants() {
    let mut fixture = Fixture::start("timeout");
    fixture.wait_ready();
    fixture.finish();
    fixture.assert_reaped();
}

#[test]
fn abrupt_owner_death_cleans_the_owned_group() {
    let mut fixture = Fixture::start("kill");
    fixture.wait_ready();
    let worker = fixture.worker.as_mut().unwrap();
    worker.kill().unwrap();
    worker.wait().unwrap();
    fixture.worker.take();
    fixture.assert_reaped();
}

#[test]
fn independent_daemons_survive_without_holding_input_or_output() {
    let mut fixture = Fixture::start("independent");
    fixture.wait_ready();
    fixture.finish();
    let pids = fixture.pids();
    assert!(!proc_path(pids[0]).exists());
    assert!(proc_path(pids[1]).exists());
}

#[test]
fn private_control_pipes_work_when_standard_descriptors_are_closed() {
    let mut fixture = Fixture::start("stdio");
    fixture.wait_ready();
    fixture.finish();
    fixture.assert_reaped();
}

#[test]
fn repeated_commands_release_all_private_descriptors() {
    Fixture::start("fds").finish();
}

#[test]
fn resident_request_cancellation_reaps_owned_descendants() {
    let mut fixture = Fixture::resident();
    fixture.wait_ready();
    fixture.send(serde_json::json!({"v": 1, "type": "cancel", "id": "owned", "generation": 1}));
    fixture.assert_reaped();
    assert!(
        fixture
            .worker
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none()
    );
    fixture.worker.as_mut().unwrap().stdin.take();
    fixture.finish();
}

#[test]
fn resident_sigterm_waits_for_command_cleanup() {
    let mut fixture = Fixture::resident();
    fixture.wait_ready();
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(fixture.worker.as_ref().unwrap().id() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    fixture.finish();
    fixture.assert_reaped();
}

#[test]
fn resident_sigkill_still_cleans_owned_descendants() {
    let mut fixture = Fixture::resident();
    fixture.wait_ready();
    let worker = fixture.worker.as_mut().unwrap();
    worker.kill().unwrap();
    worker.wait().unwrap();
    fixture.worker.take();
    fixture.assert_reaped();
}
