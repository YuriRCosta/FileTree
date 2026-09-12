use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn session_request() {
    let Some(arguments) = std::env::var_os("FILEBLADE_CLIPBOARD_TEST_ARGUMENTS") else {
        return;
    };
    let arguments: Vec<String> = serde_json::from_str(arguments.to_str().unwrap()).unwrap();
    let command = fileblade::backend::parse(
        std::iter::once("fileblade").chain(arguments.iter().map(String::as_str)),
    );
    let session = fileblade::clipboard::Session::open().unwrap();
    let response = match command {
        Ok(command) => fileblade::backend::dispatch(
            command,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .unwrap(),
        Err(error) => serde_json::json!({"ok":false,"error":error.to_string()}),
    };
    drop(session);
    fs::write(
        std::env::var_os("FILEBLADE_CLIPBOARD_TEST_RESPONSE").unwrap(),
        serde_json::to_vec(&response).unwrap(),
    )
    .unwrap();
}

pub fn request(root: &Path, arguments: &[&str], tools: &Path) -> Value {
    let response = root.join("response.json");
    let mut paths = vec![tools.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let mut child = Process(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "resident_clipboard::session_request",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("CLIPBOARD_CAPTURE", root.join("capture"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env(
                "FILEBLADE_CLIPBOARD_TEST_ARGUMENTS",
                serde_json::to_string(arguments).unwrap(),
            )
            .env("FILEBLADE_CLIPBOARD_TEST_RESPONSE", &response)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "clipboard fixture failed: {status}");
            break;
        }
        assert!(Instant::now() < deadline, "clipboard fixture did not stop");
        thread::sleep(Duration::from_millis(10));
    }
    serde_json::from_slice(&fs::read(response).unwrap()).unwrap()
}
