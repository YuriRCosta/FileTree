use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Resident(Child);

impl Drop for Resident {
    fn drop(&mut self) {
        unsafe { libc::kill(self.0.id() as i32, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.0.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = self.0.kill();
                let _ = self.0.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn command(root: &Path, native: bool) -> Command {
    let binary = std::env::var_os("FILEBLADE_TEST_BIN")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fileblade").into());
    let mut command = Command::new(binary);
    command
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("desktop-data"))
        .env("FILEBLADE_SPIKE_HOME", root)
        .env("CLIPBOARD_CAPTURE", root.join("capture"))
        .env("CLIPBOARD_PID", root.join("owner.pid"))
        .env("PATH", format!("{}:/usr/bin:/bin", root.display()));
    if native {
        command.env(
            "FILEBLADE_NATIVE_STATE_ROOT",
            root.join("state/omarchy/fileblade"),
        );
    } else {
        command.env_remove("FILEBLADE_NATIVE_STATE_ROOT");
    }
    command
}

fn send(writer: &mut impl Write, frame: Value) {
    serde_json::to_writer(&mut *writer, &frame).unwrap();
    writer.write_all(b"\n").unwrap();
}

fn read(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}

fn terminal(reader: &mut impl BufRead, id: &str) -> Value {
    loop {
        let frame = read(reader);
        if frame["id"] == id
            && matches!(
                frame["type"].as_str(),
                Some("response" | "result" | "error")
            )
        {
            return frame;
        }
    }
}

fn exercise(native: bool) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    fs::write(root.join("wl-copy"), "#!/bin/sh\nprintf '%s' \"$$\" > \"$CLIPBOARD_PID\"\n/bin/cat > \"$CLIPBOARD_CAPTURE\"\nexec /bin/sleep 600\n").unwrap();
    fs::set_permissions(root.join("wl-copy"), fs::Permissions::from_mode(0o700)).unwrap();
    let source = root.join("space #percent%.txt");
    fs::write(&source, "unchanged").unwrap();
    let state = root.join("state/omarchy/fileblade");
    let authority = native.then(|| {
        let mut holder = Resident(
            command(root, true)
                .args([
                    "serve",
                    "--native-authority",
                    "--native-isolated",
                    "--no-recover",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while fileblade::lease::transport::probe(&state).is_err() {
            assert!(holder.0.try_wait().unwrap().is_none());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        holder
    });
    let mut view = Resident(
        command(root, native)
            .args(["serve", "--no-recover"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut input = view.0.stdin.take().unwrap();
    let mut output = BufReader::new(view.0.stdout.take().unwrap());
    send(&mut input, json!({"v":1,"type":"hello"}));
    assert_eq!(read(&mut output)["ok"], true);
    send(
        &mut input,
        json!({"v":1,"type":"request","id":"clipboard","generation":1,
        "command":"clipboard-write","arguments":["--path",source]}),
    );
    let result = terminal(&mut output, "clipboard");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["payload"]["ok"], true, "{result}");
    let owner: i32 = fs::read_to_string(root.join("owner.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(unsafe { libc::kill(owner, 0) }, 0);
    assert_eq!(
        fs::read_to_string(root.join("capture")).unwrap(),
        format!("{}\r\n", url::Url::from_file_path(&source).unwrap())
    );
    send(
        &mut input,
        json!({"v":1,"type":"request","id":"bin","generation":1,
        "command":"bin-put","arguments":["--module","session-test","--item",json!({"id":"metadata-only","name":"metadata-only","paths":[]}).to_string()]}),
    );
    let result = terminal(&mut output, "bin");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["payload"]["ok"], true, "{result}");
    let native_bin = state.join("artifact-bin/session-test");
    let legacy_bin = root.join("desktop-data/fileblade/bin/session-test");
    assert_eq!(native_bin.is_dir(), native);
    assert_eq!(legacy_bin.is_dir(), !native);
    let bin = if native { native_bin } else { legacy_bin };
    let entries = fs::read_dir(bin)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].path().join("manifest.json").is_file());
    drop(input);
    let deadline = Instant::now() + Duration::from_secs(5);
    while view.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "view did not exit after EOF");
        std::thread::sleep(Duration::from_millis(10));
    }
    if native {
        assert_eq!(
            unsafe { libc::kill(owner, 0) },
            0,
            "view EOF killed the clipboard owner"
        );
    }
    drop(authority);
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(owner, 0) } == 0 {
        assert!(
            Instant::now() < deadline,
            "server shutdown left the clipboard owner alive"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read_to_string(source).unwrap(), "unchanged");
}

#[test]
fn legacy_server_holds_clipboard_until_eof_and_keeps_the_legacy_artifact_root() {
    exercise(false);
}

#[test]
fn native_authority_holds_clipboard_after_view_eof_and_uses_the_leased_artifact_root() {
    exercise(true);
}
