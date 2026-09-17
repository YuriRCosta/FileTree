use crate::command::{CommandSpec, which};
use crate::common::{CONTROL_TIMEOUT, display_path, expanded_path, parse_path, path_text};
use crate::hyprland::{
    cursor_position, has_window_at_point, hypr_dispatch, hypr_query, launch_command,
    validate_desktop_id, window_selector, window_under_cursor,
};
use crate::{AppError, AppResult};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use url::Url;

mod actions;
pub mod config;
mod context;
mod editors;
mod entries;
mod herdr;
mod paths;
mod processes;
mod run;
use actions::*;
pub use context::*;
use editors::*;
use entries::*;
use herdr::*;
use paths::*;
use processes::*;
pub use run::*;
const MAX_MIME_PROBES: usize = 12;
const MAX_PROCESSES: usize = 8192;
const MAX_PROC_FILE_BYTES: u64 = 64 * 1024;
const MAX_DESKTOP_FILES: usize = 4096;
const MAX_DESKTOP_BYTES: u64 = 64 * 1024;
const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_APPLICATIONS: usize = 8;
const HERDR_RUN_ATTEMPTS: usize = 12;

type Placement = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

const HERDR_PLACEMENTS: [Placement; 4] = [
    (
        "right",
        "Vertical split",
        "v",
        "󰯌",
        "Open in a new pane to the right of the focused herdr pane",
    ),
    (
        "down",
        "Horizontal split",
        "h",
        "󰯋",
        "Open in a new pane below the focused herdr pane",
    ),
    (
        "tab",
        "New tab",
        "t",
        "󰓩",
        "Open in a new tab of the focused herdr workspace",
    ),
    (
        "workspace",
        "New space",
        "s",
        "󰖲",
        "Open in a new herdr space",
    ),
];

const TMUX_PLACEMENTS: [Placement; 4] = [
    (
        "right",
        "Vertical split",
        "v",
        "󰯌",
        "Open in a new pane to the right of the active tmux pane",
    ),
    (
        "down",
        "Horizontal split",
        "h",
        "󰯋",
        "Open in a new pane below the active tmux pane",
    ),
    (
        "window",
        "New window",
        "w",
        "󰓩",
        "Open in a new tmux window",
    ),
    (
        "session",
        "New session",
        "s",
        "󰖲",
        "Open in a new tmux session",
    ),
];

const NVIM_PLACEMENTS: [Placement; 3] = [
    ("tab", "Tab", "t", "󰓩", "One tab per file"),
    ("buffer", "Buffer", "b", "󰈔", "Edit in the current window"),
    ("split", "Split", "s", "󰤼", "One vertical split per file"),
];

const REVIEW_PLACEMENTS: [Placement; 5] = [
    (
        "right",
        "Vertical split",
        "v",
        "󰯌",
        "Review in a new pane to the right of the focused pane",
    ),
    (
        "down",
        "Horizontal split",
        "h",
        "󰯋",
        "Review in a new pane below the focused pane",
    ),
    (
        "tab",
        "New tab",
        "t",
        "󰓩",
        "Review in a new herdr tab or tmux window",
    ),
    (
        "workspace",
        "New space",
        "s",
        "󰖲",
        "Review in a new herdr space or tmux session",
    ),
    (
        "window",
        "New window",
        "w",
        "󰆍",
        "Review in a separate terminal window",
    ),
];

#[derive(Clone, Debug)]
pub struct DropRunOptions {
    pub action: String,
    pub placement: String,
    pub paths: Vec<String>,
    pub target_json: String,
    pub desktop_id: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct DropPasteOptions {
    pub x: Option<i64>,
    pub y: Option<i64>,
    pub form: String,
    pub paths: Vec<String>,
    pub blade_titles: Vec<String>,
    pub dry_run: bool,
}

#[derive(Debug)]
struct Runner {
    dry_run: bool,
    commands: Vec<Vec<String>>,
}

impl Runner {
    fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            commands: Vec::new(),
        }
    }

    fn detached<S: AsRef<OsStr>>(&mut self, command: Vec<S>, cwd: Option<&str>) -> AppResult<()> {
        self.commands.push(
            command
                .iter()
                .map(|value| command_text(value.as_ref()))
                .collect(),
        );
        if self.dry_run {
            return Ok(());
        }
        let program = command
            .first()
            .ok_or_else(|| AppError::invalid("empty command"))?
            .as_ref();
        let mut spec = CommandSpec::new(command_binary(program)?)
            .args(command.iter().skip(1).map(AsRef::as_ref));
        if let Some(directory) = cwd.filter(|value| !value.is_empty()) {
            spec = spec.cwd(parse_path(directory)?);
        }
        spec.spawn_detached()?;
        Ok(())
    }

    fn capture<S: AsRef<OsStr>>(
        &mut self,
        command: Vec<S>,
        environment: Option<BTreeMap<OsString, OsString>>,
    ) -> AppResult<(i32, String, String)> {
        self.capture_input(command, environment, None)
    }

    fn capture_input<S: AsRef<OsStr>>(
        &mut self,
        command: Vec<S>,
        environment: Option<BTreeMap<OsString, OsString>>,
        input: Option<Vec<u8>>,
    ) -> AppResult<(i32, String, String)> {
        self.commands.push(
            command
                .iter()
                .map(|value| command_text(value.as_ref()))
                .collect(),
        );
        if self.dry_run {
            return Ok((0, String::new(), String::new()));
        }
        let program = command
            .first()
            .ok_or_else(|| AppError::invalid("empty command"))?
            .as_ref();
        let mut spec = CommandSpec::new(command_binary(program)?)
            .args(command.iter().skip(1).map(AsRef::as_ref))
            .timeout(CONTROL_TIMEOUT)
            .limits(1024 * 1024, 256 * 1024);
        if let Some(environment) = environment {
            spec = spec.env_clear();
            for (key, value) in environment {
                spec = spec.env(key, value);
            }
        }
        if let Some(input) = input {
            spec = spec.stdin(input);
        }
        let output = spec.run()?;
        if output.stdout_truncated || output.stderr_truncated {
            return Err(AppError::command("command output exceeded its byte limit"));
        }
        Ok((
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }

    fn dispatch(&mut self, expression: String) -> AppResult<()> {
        self.commands.push(vec![
            "hyprctl".to_string(),
            "dispatch".to_string(),
            expression.clone(),
        ]);
        if self.dry_run {
            Ok(())
        } else {
            hypr_dispatch(&expression)
        }
    }

    fn pause(&self, duration: Duration) {
        if !self.dry_run {
            thread::sleep(duration);
        }
    }
}

fn focus_target(runner: &mut Runner, target: &Value) -> AppResult<bool> {
    let address = text_field(target, "address");
    if address.is_empty() {
        return Ok(false);
    }
    runner.dispatch(format!(
        "hl.dsp.focus({{ window = \"{}\" }})",
        window_selector(&address)?
    ))?;
    Ok(true)
}

fn focused_row(value: Option<&Value>) -> Option<Value> {
    value
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("focused").and_then(Value::as_bool).unwrap_or(false))
        })
        .cloned()
}

fn multiplexer_command(facts: &Value) -> Vec<String> {
    if has_files(facts) {
        editor_command(facts)
    } else {
        Vec::new()
    }
}

fn editor_command(facts: &Value) -> Vec<String> {
    let mut command = vec!["nvim".to_string(), "--".to_string()];
    command.extend(string_array(facts, "files"));
    command
}

fn review_command(facts: &Value) -> Vec<String> {
    let mut command = vec!["hunk".to_string(), "diff".to_string(), "--".to_string()];
    if text_field(facts, "git_root").is_empty() {
        command.extend(string_array(facts, "files"));
    } else {
        command.extend(string_array(facts, "paths"));
    }
    command
}

fn review_cwd(facts: &Value) -> String {
    let root = text_field(facts, "git_root");
    if root.is_empty() {
        text_field(facts, "folder")
    } else {
        root
    }
}

fn tmux_command(socket: &str) -> Vec<String> {
    let binary = which("tmux")
        .map(|path| path_text(&path))
        .unwrap_or_else(|| "tmux".to_string());
    if socket.is_empty() {
        vec![binary]
    } else {
        vec![binary, "-S".to_string(), socket.to_string()]
    }
}

fn wrapped<S: AsRef<OsStr>>(command: Vec<S>) -> Vec<OsString> {
    let mut result = vec![OsString::from("setsid")];
    if let Some(launcher) = which("uwsm-app") {
        result.push(launcher.into_os_string());
        result.push(OsString::from("--"));
    }
    result.extend(command.into_iter().map(|value| value.as_ref().to_owned()));
    result
}

fn resolve_point(x: Option<i64>, y: Option<i64>) -> AppResult<(i64, i64)> {
    match (x, y) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => cursor_position(),
    }
}

fn parse_target(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return json!({"kind": "desktop"});
    }
    serde_json::from_str(raw)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({"kind": "desktop"}))
}

fn accepts_files(value: &str) -> bool {
    ["%f", "%F", "%u", "%U"]
        .iter()
        .any(|token| value.contains(token))
}

fn terminal_class(value: &str) -> bool {
    matches!(
        value,
        "com.mitchellh.ghostty"
            | "Alacritty"
            | "kitty"
            | "foot"
            | "footclient"
            | "herdr"
            | "org.wezfurlong.wezterm"
            | "org.omarchy.terminal"
    ) || value.starts_with("org.omarchy.")
}

fn app_kind(value: &str) -> &'static str {
    let lower = value.to_lowercase();
    if matches!(
        lower.as_str(),
        "chromium"
            | "firefox"
            | "brave-browser"
            | "google-chrome"
            | "zen"
            | "org.mozilla.firefox"
            | "vivaldi-stable"
    ) || lower.starts_with("chrome-")
    {
        "browser"
    } else {
        "app"
    }
}

fn is_text_like(value: &str) -> bool {
    value.starts_with("text/")
        || matches!(
            value,
            "application/json"
                | "application/xml"
                | "application/x-shellscript"
                | "application/toml"
                | "application/x-yaml"
                | "application/javascript"
                | "inode/x-empty"
                | "application/x-empty"
        )
}

fn bounded_file_bytes(path: &Path) -> Vec<u8> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    let _ = file.take(MAX_PROC_FILE_BYTES).read_to_end(&mut bytes);
    bytes
}

fn bounded_file_text(path: &Path) -> String {
    String::from_utf8_lossy(&bounded_file_bytes(path)).into_owned()
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn text_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn extend_result(mut left: Value, right: Value) -> Value {
    if let (Some(left), Some(right)) = (left.as_object_mut(), right.as_object()) {
        left.extend(right.clone());
    }
    left
}
