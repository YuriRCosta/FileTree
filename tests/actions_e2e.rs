use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

const ECHO_ENV: &str = r#"#!/bin/sh
env | grep '^FILEBLADE_' | sort
if [ -n "$FILEBLADE_SELECTION_FILE" ]; then
  echo "SELECTION_MODE=$(stat -c %a "$FILEBLADE_SELECTION_FILE")"
  echo "SELECTION_BYTES=$(wc -c < "$FILEBLADE_SELECTION_FILE")"
fi
echo "ARGS=$*"
echo "CWD=$(pwd)"
"#;

const SLEEPER: &str = r#"#!/bin/sh
echo $$ > "$FILEBLADE_STATE_DIR/sleeper.pid"
sleep 30
"#;

const SPEW: &str = r#"#!/bin/sh
index=0
while [ $index -lt 2048 ]; do
  printf '%0100d\n' $index
  index=$((index + 1))
done
"#;

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    home: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let guard = ENVIRONMENT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = tempfile::tempdir().expect("fixture home");
        let fixture = Self {
            _guard: guard,
            home,
        };
        fixture.build();
        fixture
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.path().join(relative)
    }

    fn plugin_dir(&self) -> PathBuf {
        self.path("plugins/kurt.tools")
    }

    fn actions_dir(&self) -> PathBuf {
        self.path("actions")
    }

    fn work_dir(&self) -> PathBuf {
        self.path("work")
    }

    fn outside_dir(&self) -> PathBuf {
        self.path("outside")
    }

    fn module_state(&self, module: &str) -> PathBuf {
        self.path("state/omarchy/fileblade/modules").join(module)
    }

    fn audit_path(&self) -> PathBuf {
        self.path("state/omarchy/fileblade/audit.jsonl")
    }

    fn build(&self) {
        let scripts = self.plugin_dir().join("scripts");
        fs::create_dir_all(&scripts).expect("plugin scripts");
        fs::create_dir_all(self.actions_dir()).expect("user actions");
        fs::create_dir_all(self.work_dir()).expect("work directory");
        script(&scripts.join("echo-env"), ECHO_ENV, true);
        script(&scripts.join("sleeper"), SLEEPER, true);
        script(&scripts.join("spew"), SPEW, true);
        script(&scripts.join("plain"), "#!/bin/sh\n", false);
        std::os::unix::fs::symlink("/bin/sh", scripts.join("link")).expect("symlinked argv0");
        fs::create_dir_all(self.outside_dir()).expect("outside directory");
        script(&self.outside_dir().join("tool"), ECHO_ENV, true);
        std::os::unix::fs::symlink(self.outside_dir(), self.plugin_dir().join("outside"))
            .expect("symlinked directory component");
        fs::write(
            self.plugin_dir().join("manifest.json"),
            manifest("kurt.tools").to_string(),
        )
        .expect("manifest");
        fs::write(
            self.actions_dir().join("print.json"),
            json!({
                "id": "print",
                "title": "Print",
                "contexts": ["none"],
                "argv": ["printf", "%s", "printed"],
            })
            .to_string(),
        )
        .expect("user action");
    }

    fn backend(&self, arguments: &[String]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_fileblade"))
            .arg("_backend")
            .args(arguments)
            .env("HOME", self.home.path())
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .output()
            .expect("run the backend");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("backend answered {error}: {stderr}"))
    }

    fn run(&self, action: &str, context: &str, extra: &[&str]) -> Value {
        let mut arguments = vec![
            "action-run".to_string(),
            "--source".to_string(),
            "plugin".to_string(),
            "--plugin".to_string(),
            "kurt.tools".to_string(),
            "--plugin-dir".to_string(),
            text(&self.plugin_dir()),
            "--action".to_string(),
            action.to_string(),
            "--context".to_string(),
            context.to_string(),
        ];
        arguments.extend(extra.iter().map(ToString::to_string));
        self.backend(&arguments)
    }

    fn use_process_environment(&self) {
        unsafe {
            std::env::set_var("HOME", self.home.path());
            std::env::set_var("XDG_STATE_HOME", self.path("state"));
            std::env::set_var("XDG_CONFIG_HOME", self.path("config"));
        }
    }

    fn request(&self, action: &str) -> fileblade::actions::RunRequest {
        fileblade::actions::RunRequest {
            source: "plugin".to_string(),
            plugin: "kurt.tools".to_string(),
            plugin_dir: text(&self.plugin_dir()),
            action: action.to_string(),
            context: "none".to_string(),
            root: String::new(),
            paths: Vec::new(),
            yes: false,
            screen: String::new(),
        }
    }
}

fn manifest(id: &str) -> Value {
    let mut actions = vec![
        json!({
            "id": "dump",
            "title": "Dump the environment",
            "description": "Prints every FILEBLADE_ variable",
            "contexts": ["file", "dir", "selection", "root", "none"],
            "argv": ["scripts/echo-env"],
        }),
        json!({
            "id": "append",
            "title": "Dump with appended paths",
            "contexts": ["selection"],
            "argv": ["scripts/echo-env"],
            "paths": "append",
        }),
        json!({
            "id": "guarded",
            "title": "Guarded dump",
            "contexts": ["none"],
            "argv": ["scripts/echo-env"],
            "confirm": true,
        }),
        json!({
            "id": "background",
            "title": "Background dump",
            "contexts": ["none"],
            "argv": ["scripts/echo-env"],
            "detach": true,
        }),
        json!({
            "id": "linked",
            "title": "Linked program",
            "contexts": ["none"],
            "argv": ["scripts/link"],
        }),
        json!({
            "id": "unarmed",
            "title": "Program without the exec bit",
            "contexts": ["none"],
            "argv": ["scripts/plain"],
        }),
        json!({
            "id": "spew",
            "title": "Noisy program",
            "contexts": ["none"],
            "argv": ["scripts/spew"],
        }),
    ];
    actions.push(json!({
        "id": "in-root",
        "title": "Dump from the tree root",
        "contexts": ["root"],
        "argv": ["scripts/echo-env"],
        "cwd": "root",
    }));
    actions.push(json!({
        "id": "in-target",
        "title": "Dump from the target",
        "contexts": ["file", "dir"],
        "argv": ["scripts/echo-env"],
        "cwd": "target",
    }));
    actions.push(json!({
        "id": "escape",
        "title": "Program behind a symlinked directory",
        "contexts": ["none"],
        "argv": ["outside/tool"],
    }));
    for index in 0..5 {
        actions.push(json!({
            "id": format!("sleeper{index}"),
            "title": format!("Sleeper {index}"),
            "contexts": ["none"],
            "argv": ["scripts/sleeper"],
            "timeout": 1,
        }));
    }
    json!({
        "schemaVersion": 1,
        "id": id,
        "name": "Tools",
        "version": "1.0.0",
        "kinds": ["fileblade-blade"],
        "entryPoints": {},
        "extensions": {"data-goblin.fileblade/action": actions},
    })
}

fn script(path: &Path, body: &str, executable: bool) {
    fs::write(path, body).expect("write script");
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("script mode");
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .expect("stat")
        .permissions()
        .mode()
        & 0o777
}

fn reported(response: &Value) -> HashMap<String, String> {
    response["stdout_tail"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

fn audit_entries(fixture: &Fixture) -> Vec<Value> {
    let Ok(body) = fs::read_to_string(fixture.audit_path()) else {
        return Vec::new();
    };
    body.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["command"] == "action-run")
        .collect()
}

#[test]
fn action_list_returns_rows_without_the_command_vector() {
    let fixture = Fixture::new();
    let document = fixture.backend(&[
        "action-list".to_string(),
        "--provider".to_string(),
        format!("kurt.tools={}", text(&fixture.plugin_dir())),
        "--user".to_string(),
        text(&fixture.actions_dir()),
    ]);
    assert_eq!(document["ok"], true, "{document}");
    assert_eq!(document["truncated"], false);
    assert_eq!(document["errors"].as_array().unwrap().len(), 0);
    let rows = document["actions"].as_array().unwrap();
    assert_eq!(rows.len(), 16);
    let serialized = document.to_string();
    assert!(!serialized.contains("argv"), "{serialized}");
    assert!(!serialized.contains("\"cwd\""), "{serialized}");
    let dump = rows
        .iter()
        .find(|row| row["key"] == "kurt.tools/dump")
        .expect("the dump row");
    assert_eq!(dump["source"], "plugin");
    assert_eq!(dump["plugin"], "kurt.tools");
    assert_eq!(dump["program"], "scripts/echo-env");
    assert_eq!(dump["title"], "Dump the environment");
    assert_eq!(dump["timeout"], 60);
    assert_eq!(dump["output"], "notice");
    assert_eq!(
        dump["contexts"],
        json!(["file", "dir", "selection", "root", "none"])
    );
    let user = rows
        .iter()
        .find(|row| row["key"] == "user/print")
        .expect("the user row");
    assert_eq!(user["source"], "user");
    assert_eq!(user["plugin"], "");
    assert_eq!(user["program"], "printf");
}

#[test]
fn action_discovery_bounds_invalid_entries_and_preserves_valid_siblings() {
    let fixture = Fixture::new();
    let discover = |entries: Vec<Value>| {
        fs::write(
            fixture.plugin_dir().join("manifest.json"),
            json!({
                "id":"kurt.tools", "extensions":{"data-goblin.fileblade/action":entries}
            })
            .to_string(),
        )
        .unwrap();
        fixture.backend(&[
            "action-list".into(),
            "--provider".into(),
            format!("kurt.tools={}", text(&fixture.plugin_dir())),
        ])
    };
    let definition = |patch: Value| {
        let mut value =
            json!({"id":"dump", "title":"Dump", "contexts":["dir"], "argv":["scripts/echo-env"]});
        value
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        value
    };
    for patch in [
        json!({"argv":vec!["arg";33]}),
        json!({"argv":vec!["b".repeat(1024);9]}),
        json!({"argv":["scripts/echo-env","a".repeat(1025)]}),
        json!({"argv":["a\u{7}b"]}),
        json!({"argv":[]}),
        json!({"argv":"scripts/echo-env"}),
        json!({"contexts":["nope"]}),
        json!({"contexts":[]}),
        json!({"id":"-bad"}),
        json!({"id":"a".repeat(65)}),
        json!({"title":""}),
    ] {
        let response = discover(vec![definition(patch), definition(json!({"id":"good"}))]);
        assert_eq!(
            response["actions"].as_array().unwrap().len(),
            1,
            "{response}"
        );
        assert_eq!(response["actions"][0]["id"], "good", "{response}");
        assert_eq!(
            response["errors"].as_array().unwrap().len(),
            1,
            "{response}"
        );
    }
    let response = discover(vec![definition(json!({"contexts":["dir","nope","dir"],
        "title":"Run\u{202e}gpj.exe\u{7}", "description":"one\u{2028}two\u{2066}three",
        "timeout":100000, "cwd":"invalid", "paths":"invalid", "output":"invalid"}))]);
    let row = &response["actions"][0];
    assert_eq!(row["contexts"], json!(["dir"]));
    assert_eq!(row["title"], "Rungpj.exe");
    assert_eq!(row["description"], "onetwothree");
    assert_eq!(row["timeout"], 900);
    let execution = fixture.run(
        "dump",
        "dir",
        &["--path", fixture.work_dir().to_str().unwrap()],
    );
    assert_eq!(execution["ok"], true, "{execution}");
    let environment = reported(&execution);
    assert_eq!(environment["CWD"], text(&fixture.plugin_dir()));
    assert_eq!(environment["ARGS"], "");
    assert_eq!(row["output"], "notice");
    let response = discover(
        (0..17)
            .map(|i| definition(json!({"id":format!("a{i}")})))
            .collect(),
    );
    assert_eq!(response["actions"].as_array().unwrap().len(), 16);
    assert_eq!(response["truncated"], true);
    let response = discover(vec![definition(json!({})); 2]);
    assert_eq!(response["actions"].as_array().unwrap().len(), 1);
    assert_eq!(response["errors"].as_array().unwrap().len(), 1);
    let response = discover(vec![json!({}); 500]);
    assert_eq!(response["errors"].as_array().unwrap().len(), 16);
    assert_eq!(response["truncated"], true);
}

#[test]
fn a_manifest_under_another_id_is_reported_and_never_run() {
    let fixture = Fixture::new();
    let document = fixture.backend(&[
        "action-list".to_string(),
        "--provider".to_string(),
        format!("other.tools={}", text(&fixture.plugin_dir())),
    ]);
    assert_eq!(document["ok"], true, "{document}");
    assert_eq!(document["actions"].as_array().unwrap().len(), 0);
    let errors = document["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["source"], "other.tools");
    let refused = fixture.backend(&[
        "action-run".to_string(),
        "--source".to_string(),
        "plugin".to_string(),
        "--plugin".to_string(),
        "other.tools".to_string(),
        "--plugin-dir".to_string(),
        text(&fixture.plugin_dir()),
        "--action".to_string(),
        "dump".to_string(),
        "--context".to_string(),
        "none".to_string(),
    ]);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("plugin id other.tools"),
        "{refused}"
    );
}

#[test]
fn a_selection_run_carries_the_targets_the_private_dirs_and_the_plugin_cwd() {
    let fixture = Fixture::new();
    let first = fixture.work_dir().join("first.txt");
    let second = fixture.work_dir().join("second.txt");
    fs::write(&first, "one").unwrap();
    fs::write(&second, "two").unwrap();
    let response = fixture.run(
        "dump",
        "selection",
        &[
            "--path",
            &text(&first),
            "--path",
            &text(&second),
            "--root",
            &text(&fixture.work_dir()),
            "--screen",
            "DP-1",
        ],
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["exit_code"], 0);
    assert_eq!(response["targets"], 2);
    assert_eq!(response["detached"], false);
    assert_eq!(response["timed_out"], false);
    assert_eq!(response["stdout_truncated"], false);
    assert_eq!(response["key"], "kurt.tools/dump");
    let seen = reported(&response);
    assert_eq!(seen["FILEBLADE_ACTION"], "kurt.tools/dump");
    assert_eq!(seen["FILEBLADE_SOURCE"], "plugin");
    assert_eq!(seen["FILEBLADE_PLUGIN_ID"], "kurt.tools");
    assert_eq!(seen["FILEBLADE_CONTEXT"], "selection");
    assert_eq!(seen["FILEBLADE_SELECTION_COUNT"], "2");
    assert_eq!(seen["FILEBLADE_TARGET"], text(&first));
    assert_eq!(seen["FILEBLADE_ROOT"], text(&fixture.work_dir()));
    assert_eq!(seen["FILEBLADE_SCREEN"], "DP-1");
    assert_eq!(seen["ARGS"], "");
    assert_eq!(seen["FILEBLADE_SELECTION_FILE"], "");
    assert!(seen["FILEBLADE_CLI"].ends_with("fileblade"));
    assert!(!seen["FILEBLADE_HOST_VERSION"].is_empty());
    assert_eq!(
        seen["CWD"],
        text(&fs::canonicalize(fixture.plugin_dir()).unwrap())
    );
    assert_eq!(
        seen["FILEBLADE_PLUGIN_ROOT"],
        text(&fs::canonicalize(fixture.plugin_dir()).unwrap())
    );
    let selection: Value = serde_json::from_str(&seen["FILEBLADE_SELECTION_JSON"]).unwrap();
    let rows = selection.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["path"], text(&first));
    assert_eq!(rows[0]["name"], "first.txt");
    assert_eq!(rows[0]["dir"], false);
    assert_eq!(rows[0]["symlink"], false);
    assert_eq!(rows[0]["size"], 3);
    assert!(rows[0]["mime"].is_string());
    let state = PathBuf::from(&seen["FILEBLADE_STATE_DIR"]);
    let config = PathBuf::from(&seen["FILEBLADE_CONFIG_DIR"]);
    assert_eq!(state, fixture.module_state("kurt.tools"));
    assert!(state.is_dir() && config.is_dir());
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(&config), 0o700);
}

#[test]
fn appended_paths_arrive_as_trailing_arguments() {
    let fixture = Fixture::new();
    let target = fixture.work_dir().join("one.txt");
    fs::write(&target, "one").unwrap();
    let response = fixture.run("append", "selection", &["--path", &text(&target)]);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(reported(&response)["ARGS"], text(&target));
}

#[test]
fn a_selection_past_the_environment_cap_moves_to_a_private_file_that_the_run_removes() {
    let fixture = Fixture::new();
    let mut arguments = Vec::new();
    for index in 0..200 {
        let name = format!("{index:0>200}.txt");
        let path = fixture.work_dir().join(&name);
        fs::write(&path, "body").unwrap();
        arguments.push("--path".to_string());
        arguments.push(text(&path));
    }
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let response = fixture.run("dump", "selection", &borrowed);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["targets"], 200);
    let seen = reported(&response);
    assert_eq!(seen["FILEBLADE_SELECTION_JSON"], "", "{seen:?}");
    assert_eq!(seen["SELECTION_MODE"], "600");
    assert!(seen["SELECTION_BYTES"].trim().parse::<usize>().unwrap() > 65536);
    let file = PathBuf::from(&seen["FILEBLADE_SELECTION_FILE"]);
    assert!(file.starts_with(fixture.module_state("kurt.tools")));
    assert!(!file.exists(), "the selection file survives the run");
    let leftovers = fs::read_dir(fixture.module_state("kurt.tools"))
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("selection-")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn an_argv0_that_is_a_symlink_or_lacks_the_exec_bit_is_refused() {
    let fixture = Fixture::new();
    let linked = fixture.run("linked", "none", &[]);
    assert_eq!(linked["ok"], false, "{linked}");
    assert!(
        linked["error"].as_str().unwrap().contains("regular file"),
        "{linked}"
    );
    let unarmed = fixture.run("unarmed", "none", &[]);
    assert_eq!(unarmed["ok"], false, "{unarmed}");
    assert!(
        unarmed["error"].as_str().unwrap().contains("exec bit"),
        "{unarmed}"
    );
    assert_eq!(audit_entries(&fixture).len(), 2);
}

#[test]
fn a_slow_action_is_killed_at_its_timeout_and_leaves_no_child() {
    let fixture = Fixture::new();
    let started = Instant::now();
    let response = fixture.run("sleeper0", "none", &[]);
    let elapsed = started.elapsed();
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["timed_out"], true, "{response}");
    assert!(response["exit_code"].is_null());
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
    let pid = fs::read_to_string(fixture.module_state("kurt.tools").join("sleeper.pid"))
        .expect("the sleeper recorded its pid");
    let pid = pid.trim().to_string();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && Path::new(&format!("/proc/{pid}")).exists() {
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "process {pid} outlived its deadline"
    );
}

#[test]
fn a_noisy_action_reports_a_bounded_tail() {
    let fixture = Fixture::new();
    let response = fixture.run("spew", "none", &[]);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["stdout_truncated"], true, "{response}");
    let tail = response["stdout_tail"].as_str().unwrap();
    assert!(tail.len() <= 4096, "{}", tail.len());
    assert!(tail.len() > 3900, "{}", tail.len());
    assert!(tail.ends_with("2047\n"), "{tail}");
    assert_eq!(response["stderr_tail"], "");
}

#[test]
fn a_confirming_action_needs_the_yes_flag() {
    let fixture = Fixture::new();
    let refused = fixture.run("guarded", "none", &[]);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"].as_str().unwrap().contains("confirmation"),
        "{refused}"
    );
    let allowed = fixture.run("guarded", "none", &["--yes"]);
    assert_eq!(allowed["ok"], true, "{allowed}");
}

#[test]
fn context_bounds_refuse_an_oversized_selection_and_a_mismatched_kind() {
    let fixture = Fixture::new();
    let mut arguments = Vec::new();
    for index in 0..257 {
        arguments.push("--path".to_string());
        arguments.push(text(&fixture.work_dir().join(format!("{index}.txt"))));
    }
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let refused = fixture.run("dump", "selection", &borrowed);
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"], "selection too large (256)");
    let mismatched = fixture.run("dump", "file", &["--path", &text(&fixture.work_dir())]);
    assert_eq!(mismatched["ok"], false, "{mismatched}");
    assert!(
        mismatched["error"]
            .as_str()
            .unwrap()
            .contains("is not a file"),
        "{mismatched}"
    );
    let wrong_context = fixture.run("append", "none", &[]);
    assert_eq!(wrong_context["ok"], false, "{wrong_context}");
    assert!(
        wrong_context["error"]
            .as_str()
            .unwrap()
            .contains("does not accept the none context"),
        "{wrong_context}"
    );
}

#[test]
fn a_user_action_runs_its_program_from_path() {
    let fixture = Fixture::new();
    let response = fixture.backend(&[
        "action-run".to_string(),
        "--source".to_string(),
        "user".to_string(),
        "--plugin-dir".to_string(),
        text(&fixture.actions_dir()),
        "--action".to_string(),
        "print".to_string(),
        "--context".to_string(),
        "none".to_string(),
    ]);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["key"], "user/print");
    assert_eq!(response["source"], "user");
    assert_eq!(response["plugin"], "");
    assert_eq!(response["stdout_tail"], "printed");
    assert!(fixture.module_state("user").is_dir());
}

#[test]
fn a_detached_action_returns_a_pid_and_captures_nothing() {
    let fixture = Fixture::new();
    let response = fixture.run("background", "none", &[]);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["detached"], true);
    assert!(response["pid"].as_u64().unwrap() > 0, "{response}");
    assert_eq!(response["stdout_tail"], "");
    assert!(response["exit_code"].is_null());
}

#[test]
fn every_run_lands_one_audit_line_that_holds_no_output_text() {
    let fixture = Fixture::new();
    let first = fixture.work_dir().join("first.txt");
    fs::write(&first, "one").unwrap();
    assert_eq!(
        fixture.run("dump", "selection", &["--path", &text(&first)])["ok"],
        true
    );
    assert_eq!(fixture.run("spew", "none", &[])["ok"], true);
    let entries = audit_entries(&fixture);
    assert_eq!(entries.len(), 2, "{entries:?}");
    for entry in &entries {
        assert_eq!(entry["via"], "cli");
        assert_eq!(entry["ok"], true);
        let result = &entry["result"];
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["timed_out"], false);
        assert!(result.get("stdout_tail").is_none(), "{result}");
        let serialized = entry.to_string();
        assert!(
            !serialized.contains("FILEBLADE_PLUGIN_ROOT"),
            "{serialized}"
        );
        assert!(!serialized.contains("0000000000"), "{serialized}");
    }
    assert_eq!(entries[0]["result"]["targets"], 1);
    assert_eq!(entries[1]["result"]["stdout_truncated"], true);
    assert!(
        entries[0]["arguments"]
            .as_array()
            .unwrap()
            .contains(&json!("--path"))
    );
}

#[test]
fn the_same_action_never_runs_twice_at_once() {
    let fixture = Fixture::new();
    fixture.use_process_environment();
    let request = fixture.request("sleeper0");
    let plugin_dir = request.plugin_dir.clone();
    let worker = thread::spawn(move || {
        let cancelled = AtomicBool::new(false);
        fileblade::actions::run(
            &fileblade::actions::RunRequest {
                source: "plugin".to_string(),
                plugin: "kurt.tools".to_string(),
                plugin_dir,
                action: "sleeper0".to_string(),
                context: "none".to_string(),
                root: String::new(),
                paths: Vec::new(),
                yes: false,
                screen: String::new(),
            },
            &cancelled,
        )
    });
    thread::sleep(Duration::from_millis(300));
    let cancelled = AtomicBool::new(false);
    let second = fileblade::actions::run(&request, &cancelled).unwrap();
    assert_eq!(second["ok"], false, "{second}");
    assert_eq!(second["error"], "already running");
    let first = worker.join().unwrap().unwrap();
    assert_eq!(first["timed_out"], true, "{first}");
    let third = fileblade::actions::run(&request, &cancelled).unwrap();
    assert_eq!(third["timed_out"], true, "{third}");
}

#[test]
fn a_fifth_captured_run_is_refused() {
    let fixture = Fixture::new();
    fixture.use_process_environment();
    let plugin_dir = fixture.request("sleeper0").plugin_dir;
    let workers: Vec<_> = (0..4)
        .map(|index| {
            let plugin_dir = plugin_dir.clone();
            thread::spawn(move || {
                let cancelled = AtomicBool::new(false);
                fileblade::actions::run(
                    &fileblade::actions::RunRequest {
                        source: "plugin".to_string(),
                        plugin: "kurt.tools".to_string(),
                        plugin_dir,
                        action: format!("sleeper{index}"),
                        context: "none".to_string(),
                        root: String::new(),
                        paths: Vec::new(),
                        yes: false,
                        screen: String::new(),
                    },
                    &cancelled,
                )
            })
        })
        .collect();
    thread::sleep(Duration::from_millis(400));
    let cancelled = AtomicBool::new(false);
    let refused = fileblade::actions::run(&fixture.request("sleeper4"), &cancelled).unwrap();
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"], "action limit reached (4)");
    for worker in workers {
        assert_eq!(worker.join().unwrap().unwrap()["timed_out"], true);
    }
    let allowed = fileblade::actions::run(&fixture.request("sleeper4"), &cancelled).unwrap();
    assert_eq!(allowed["timed_out"], true, "{allowed}");
}

#[test]
fn an_empty_path_is_refused_before_it_can_become_the_home_directory() {
    let fixture = Fixture::new();
    for context in ["selection", "dir", "file"] {
        let refused = fixture.run("dump", context, &["--path", ""]);
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(refused["error"], "every path must be given");
    }
}

#[test]
fn a_program_behind_a_symlinked_directory_stays_outside_the_plugin() {
    let fixture = Fixture::new();
    let refused = fixture.run("escape", "none", &[]);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("inside the plugin"),
        "{refused}"
    );
}

#[test]
fn the_user_plugin_id_is_reserved_and_a_provider_is_scanned_once() {
    let fixture = Fixture::new();
    let document = fixture.backend(&[
        "action-list".to_string(),
        "--provider".to_string(),
        format!("user={}", text(&fixture.plugin_dir())),
        "--provider".to_string(),
        format!("kurt.tools={}", text(&fixture.plugin_dir())),
        "--provider".to_string(),
        format!("kurt.tools={}", text(&fixture.plugin_dir())),
    ]);
    let rows = document["actions"].as_array().unwrap();
    assert_eq!(rows.len(), 15, "{document}");
    assert!(rows.iter().all(|row| row["source"] == "plugin"));
    let errors = document["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{document}");
    let texts: Vec<&str> = errors
        .iter()
        .map(|entry| entry["error"].as_str().unwrap())
        .collect();
    assert!(texts.iter().any(|error| error.contains("is reserved")));
    assert!(texts.iter().any(|error| error.contains("more than once")));
    let refused = fixture.backend(&[
        "action-run".to_string(),
        "--source".to_string(),
        "plugin".to_string(),
        "--plugin".to_string(),
        "user".to_string(),
        "--plugin-dir".to_string(),
        text(&fixture.plugin_dir()),
        "--action".to_string(),
        "dump".to_string(),
        "--context".to_string(),
        "none".to_string(),
    ]);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"].as_str().unwrap().contains("is reserved"),
        "{refused}"
    );
}

#[test]
fn hostile_provider_names_and_error_counts_stay_bounded() {
    let providers: Vec<String> = (0..100)
        .map(|index| format!("{}-{index}=/tmp", "!".repeat(5000)))
        .collect();
    let document = fileblade::actions::list(&providers, "");
    let errors = document["errors"].as_array().unwrap();
    assert_eq!(errors.len(), fileblade::actions::MAX_ERRORS_TOTAL);
    assert_eq!(document["truncated"], true);
    assert!(document.to_string().len() < 64 * 1024, "{document}");
    assert!(errors.iter().all(|entry| {
        entry["source"].as_str().unwrap().chars().count() <= 128
            && entry["error"].as_str().unwrap().chars().count() <= 512
    }));
}

#[test]
fn the_example_action_writes_private_valid_json_for_hostile_names() {
    let state = tempfile::tempdir().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/data-goblin.blade-example/scripts/dump-env");
    let output = Command::new(script)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("FILEBLADE_STATE_DIR", state.path())
        .env("FILEBLADE_ACTION", "test/quote\"slash\\line\nnext")
        .env("FILEBLADE_CONTEXT", "selection")
        .env("FILEBLADE_ROOT", "/tmp/root")
        .env("FILEBLADE_TARGET", "/tmp/a\nb\"c")
        .env("FILEBLADE_SELECTION_COUNT", "1")
        .env("FILEBLADE_SELECTION_JSON", r#"[{"path":"/tmp/a\nb\"c"}]"#)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = state.path().join("last.json");
    assert_eq!(mode(&path) & 0o777, 0o600);
    let document: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(document["action"], "test/quote\"slash\\line\nnext");
    assert_eq!(document["target"], "/tmp/a\nb\"c");
    assert_eq!(document["selection"][0]["path"], "/tmp/a\nb\"c");
}

#[test]
fn the_cwd_rule_places_the_child_in_the_root_the_target_or_the_plugin() {
    let fixture = Fixture::new();
    let work = fs::canonicalize(fixture.work_dir()).unwrap();
    let file = fixture.work_dir().join("one.txt");
    fs::write(&file, "one").unwrap();
    let from_root = fixture.run("in-root", "root", &["--root", &text(&fixture.work_dir())]);
    assert_eq!(from_root["ok"], true, "{from_root}");
    assert_eq!(reported(&from_root)["CWD"], text(&work));
    let on_directory = fixture.run("in-target", "dir", &["--path", &text(&fixture.work_dir())]);
    assert_eq!(on_directory["ok"], true, "{on_directory}");
    assert_eq!(reported(&on_directory)["CWD"], text(&work));
    let on_file = fixture.run("in-target", "file", &["--path", &text(&file)]);
    assert_eq!(on_file["ok"], true, "{on_file}");
    assert_eq!(reported(&on_file)["CWD"], text(&work));
    let plugin_cwd = fixture.run("dump", "none", &[]);
    assert_eq!(
        reported(&plugin_cwd)["CWD"],
        text(&fs::canonicalize(fixture.plugin_dir()).unwrap())
    );
}
