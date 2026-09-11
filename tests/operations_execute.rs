use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

struct Server {
    child: Child,
    frames: Receiver<Value>,
    sequence: usize,
}

impl Server {
    fn new(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fileblade"))
            .args(["serve", "--no-recover"])
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("FILEBLADE_JOURNAL", root.join("journal.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, frames) = mpsc::channel();
        std::thread::spawn(move || {
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
        let mut server = Self {
            child,
            frames,
            sequence: 0,
        };
        server.send(json!({"v":1,"type":"hello"}));
        assert_eq!(server.frame()["type"], "hello");
        server
    }

    fn send(&mut self, frame: Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{frame}").unwrap();
        stdin.flush().unwrap();
    }

    fn frame(&self) -> Value {
        self.frames
            .recv_timeout(Duration::from_secs(15))
            .expect("server timed out")
    }

    fn request(&mut self, command: &str, arguments: Vec<String>) -> Value {
        self.sequence += 1;
        self.send(json!({"v":1,"type":"request","id":self.sequence.to_string(),"generation":1,"command":command,"arguments":arguments}));
        loop {
            let frame = self.frame();
            if frame["type"] == "progress" {
                continue;
            }
            assert_eq!(frame["type"], "response", "{frame}");
            return frame["payload"].clone();
        }
    }

    fn plan(&mut self, root: &Path, names: &[&str], copy: bool) -> Value {
        let mut args = vec![
            "--destination".into(),
            root.join("target").to_str().unwrap().into(),
            "--operation".into(),
            if copy { "copy" } else { "move" }.into(),
        ];
        for name in names {
            args.extend([
                "--source".into(),
                root.join("source").join(name).to_str().unwrap().into(),
            ]);
        }
        let value = self.request("transfer-preflight", args);
        assert_eq!(value["ok"], true, "{value}");
        value
    }

    fn execute(&mut self, plan: &Value, decisions: Value) -> Value {
        self.request(
            "transfer-execute",
            vec![
                "--decision-id".into(),
                plan["decision_id"].as_str().unwrap().into(),
                "--decisions".into(),
                decisions.to_string(),
            ],
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn fixture() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/operation-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent).unwrap();
    fs::create_dir(root.path().join("source")).unwrap();
    fs::create_dir(root.path().join("target")).unwrap();
    root
}

#[test]
fn replacement_preserves_old_data_and_round_trips_through_undo_redo() {
    for copy in [true, false] {
        let root = fixture();
        fs::write(root.path().join("source/same"), "new").unwrap();
        fs::write(root.path().join("target/same"), "old").unwrap();
        let mut server = Server::new(root.path());
        let plan = server.plan(root.path(), &["same"], copy);
        let result = server.execute(&plan, json!([{"id":"0","action":"replace"}]));
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(
            fs::read_to_string(root.path().join("target/same")).unwrap(),
            "new"
        );
        assert_eq!(root.path().join("source/same").exists(), copy);
        for _ in 0..2 {
            let result = server.request("undo", vec![]);
            assert_eq!(result["ok"], true, "{result}");
            assert_eq!(
                fs::read_to_string(root.path().join("target/same")).unwrap(),
                "old"
            );
            assert_eq!(
                fs::read_to_string(root.path().join("source/same")).unwrap(),
                "new"
            );
            let result = server.request("redo", vec![]);
            assert_eq!(result["ok"], true, "{result}");
            assert_eq!(
                fs::read_to_string(root.path().join("target/same")).unwrap(),
                "new"
            );
        }
    }
}

#[test]
fn nested_merge_preserves_unrelated_files_and_undo_restores_source_tree() {
    let root = fixture();
    for prefix in ["source", "target"] {
        fs::create_dir_all(root.path().join(prefix).join("folder/nested")).unwrap();
    }
    fs::write(root.path().join("source/folder/nested/file"), "new").unwrap();
    fs::write(root.path().join("target/folder/nested/file"), "old").unwrap();
    fs::write(root.path().join("target/folder/untouched"), "keep").unwrap();
    let mut server = Server::new(root.path());
    let plan = server.plan(root.path(), &["folder"], false);
    let mut reviewed = plan;
    let result = loop {
        let decisions: Vec<_> = reviewed["items"].as_array().unwrap().iter().filter(|item| item["collision"] == true).map(|item| json!({"id":item["id"],"action":if item["source_identity"]["kind"]=="directory" {"merge"} else {"replace"}})).collect();
        let result = server.execute(&reviewed, json!(decisions));
        if result["requires_decision"] != true {
            break result;
        }
        assert_eq!(
            fs::read_to_string(root.path().join("target/folder/nested/file")).unwrap(),
            "old"
        );
        reviewed = result;
    };
    assert_eq!(result["ok"], true, "{result}");
    assert!(!root.path().join("source/folder").exists());
    assert_eq!(
        fs::read_to_string(root.path().join("target/folder/nested/file")).unwrap(),
        "new"
    );
    for _ in 0..2 {
        let result = server.request("undo", vec![]);
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(
            fs::read_to_string(root.path().join("source/folder/nested/file")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("target/folder/nested/file")).unwrap(),
            "old"
        );
        let result = server.request("redo", vec![]);
        assert_eq!(result["ok"], true, "{result}");
    }
    assert_eq!(
        fs::read_to_string(root.path().join("target/folder/untouched")).unwrap(),
        "keep"
    );
}

#[test]
fn stale_later_collision_refuses_the_whole_batch_before_any_write() {
    let root = fixture();
    fs::write(root.path().join("source/fresh"), "fresh").unwrap();
    fs::write(root.path().join("source/same"), "new").unwrap();
    fs::write(root.path().join("target/same"), "old").unwrap();
    let mut server = Server::new(root.path());
    let plan = server.plan(root.path(), &["fresh", "same"], true);
    fs::write(root.path().join("target/same"), "changed meanwhile").unwrap();
    let result = server.execute(&plan, json!([{"id":"1","action":"replace"}]));
    assert_eq!(result["ok"], false, "{result}");
    assert!(!root.path().join("target/fresh").exists());
    assert_eq!(
        fs::read_to_string(root.path().join("target/same")).unwrap(),
        "changed meanwhile"
    );
    assert!(!root.path().join("journal.json").exists());
    let repeated = server.execute(&plan, json!([]));
    assert_eq!(repeated["ok"], false);
    assert!(repeated["error"].as_str().unwrap().contains("already used"));
}

#[test]
fn keep_both_scope_and_skipping_a_merge_child_preserve_sources() {
    let root = fixture();
    for prefix in ["source", "target"] {
        fs::create_dir(root.path().join(prefix).join("folder")).unwrap();
        for name in ["a", "b", "folder/child"] {
            fs::write(root.path().join(prefix).join(name), prefix).unwrap();
        }
    }
    let mut server = Server::new(root.path());
    let plan = server.plan(root.path(), &["a", "b", "folder"], false);
    let result = server.execute(
        &plan,
        json!([
            {"id":"0","action":"keep-both","apply_to_remaining":true},
            {"id":"2","action":"merge"}
        ]),
    );
    assert_eq!(result["requires_decision"], true, "{result}");
    assert!(!root.path().join("target/a copy").exists());
    let mut decisions = result["accepted_decisions"].as_array().unwrap().clone();
    decisions.push(json!({"id":"3","action":"skip"}));
    let result = server.execute(&result, json!(decisions));
    assert_eq!(result["ok"], true, "{result}");
    for name in ["a copy", "b copy"] {
        assert_eq!(
            fs::read_to_string(root.path().join("target").join(name)).unwrap(),
            "source"
        );
    }
    assert_eq!(
        fs::read_to_string(root.path().join("source/folder/child")).unwrap(),
        "source"
    );
    assert_eq!(result["retained_folders"].as_array().unwrap().len(), 1);
    assert_eq!(result["skipped"].as_array().unwrap().len(), 1);
}

#[test]
fn cancelled_and_incomplete_decisions_never_start_writes() {
    let root = fixture();
    for prefix in ["source", "target"] {
        fs::write(root.path().join(prefix).join("same"), prefix).unwrap();
    }
    let mut server = Server::new(root.path());
    for decisions in [
        json!([]),
        json!([{"id":"0","action":"cancel"}]),
        json!([{"id":"0","action":"merge"}]),
    ] {
        let plan = server.plan(root.path(), &["same"], false);
        let result = server.execute(&plan, decisions.clone());
        assert_eq!(result["ok"], false, "{result}");
        if decisions[0]["action"] == "cancel" {
            assert_eq!(result["cancelled"], true);
        }
        assert_eq!(
            fs::read_to_string(root.path().join("target/same")).unwrap(),
            "target"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("source/same")).unwrap(),
            "source"
        );
        assert!(!root.path().join("journal.json").exists());
    }
}
