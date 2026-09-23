use super::*;
use crate::command::CommandOutput;
use selection::{
    prune_selection_files, remove_selection_file, selection_document, write_selection_file,
};
use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use uuid::Uuid;

mod selection;

pub struct RunRequest {
    pub source: String,
    pub plugin: String,
    pub plugin_dir: String,
    pub action: String,
    pub context: String,
    pub root: String,
    pub paths: Vec<String>,
    pub yes: bool,
    pub screen: String,
}

const TAIL_BYTES: usize = 4096;
const STDOUT_LIMIT: usize = 64 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;

static LIVE: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());

struct LiveGuard {
    key: String,
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(index) = live.iter().position(|entry| entry.0 == self.key) {
            live.swap_remove(index);
        }
    }
}

#[derive(Default)]
struct Completion {
    detached: bool,
    pid: u32,
    exit_code: Option<i32>,
    signal: Option<i32>,
    timed_out: bool,
    elapsed: Duration,
    stdout: (String, bool),
    stderr: (String, bool),
}

pub fn run(request: &RunRequest, cancelled: &AtomicBool) -> AppResult<Value> {
    if !request.root.is_empty() {
        parse_path(&request.root)?;
    }
    let Some(source) = Source::parse(&request.source) else {
        return Ok(refusal("source must be plugin or user"));
    };
    let Some(context) = Context::parse(&request.context) else {
        return Ok(refusal(
            "context must be file, dir, selection, root or none",
        ));
    };
    let (root, action) = match load(source, request) {
        Ok(loaded) => loaded,
        Err(error) => return Ok(refusal(&error)),
    };
    let key = key_for(source, &request.plugin, &action.id);
    if action.confirm && !request.yes {
        return Ok(refusal(&format!("{key} needs confirmation")));
    }
    if !action.accepts(context) {
        return Ok(refusal(&format!(
            "{key} does not accept the {} context",
            context.as_str()
        )));
    }
    let targets = match targets_for(context, request) {
        Ok(targets) => targets,
        Err(error) => return Ok(refusal(&error)),
    };
    let program = match resolve_program(&action, &root, source) {
        Ok(program) => program,
        Err(error) => return Ok(refusal(&error)),
    };
    let guard = match claim(&key, !action.detach) {
        Ok(guard) => guard,
        Err(error) => return Ok(refusal(&error)),
    };
    let module = match source {
        Source::Plugin => request.plugin.as_str(),
        Source::User => USER_PLUGIN_ID,
    };
    let dirs = crate::module_dirs::ensure(module)?;
    let state_dir = directory_field(&dirs, "state_dir")?;
    prune_selection_files(&state_dir);
    let selection = selection_document(&targets).to_string();
    let selection_file = if selection.len() > MAX_SELECTION_ENV_BYTES {
        Some(write_selection_file(&state_dir, &selection)?)
    } else {
        None
    };
    let mut spec = CommandSpec::new(&program)
        .args(arguments(&action, &targets))
        .cwd(working_directory(&action, &root, request, &targets))
        .timeout(Duration::from_secs(action.timeout))
        .limits(STDOUT_LIMIT, STDERR_LIMIT)
        .retain_tail(true);
    for (name, value) in environment(EnvironmentInput {
        request,
        source,
        key: &key,
        root: &root,
        state_dir: &state_dir,
        config_dir: &directory_field(&dirs, "config_dir")?,
        targets: &targets,
        selection: &selection,
        selection_file: selection_file.as_deref(),
    }) {
        spec = spec.env(name, value);
    }
    let envelope = run_envelope(request, &key, source, targets.len());
    let started = Instant::now();
    if action.detach {
        return match spec.spawn_detached() {
            Ok(pid) => Ok(finish(
                envelope,
                detached_completion(pid, started.elapsed()),
            )),
            Err(error) => {
                remove_selection_file(selection_file.as_deref());
                Err(error)
            }
        };
    }
    let outcome = spec.run_cancellable(cancelled);
    let elapsed = started.elapsed();
    remove_selection_file(selection_file.as_deref());
    drop(guard);
    match outcome {
        Ok(output) => Ok(finish(envelope, captured_completion(&output, elapsed))),
        Err(AppError::Cancelled) => Err(AppError::Cancelled),
        Err(_) if elapsed >= Duration::from_secs(action.timeout) => {
            Ok(finish(envelope, timeout_completion(elapsed)))
        }
        Err(error) => Err(error),
    }
}

fn load(source: Source, request: &RunRequest) -> Result<(PathBuf, Action), String> {
    match source {
        Source::Plugin => {
            let (root, actions) = plugin_actions(&request.plugin, &request.plugin_dir)?;
            let action = actions
                .into_iter()
                .find(|candidate| candidate.id == request.action)
                .ok_or_else(|| {
                    format!("{} declares no action {}", request.plugin, request.action)
                })?;
            Ok((root, action))
        }
        Source::User => user_action(&request.plugin_dir, &request.action),
    }
}

fn targets_for(context: Context, request: &RunRequest) -> Result<Vec<PathBuf>, String> {
    if request.paths.iter().any(String::is_empty) {
        return Err("every path must be given".to_string());
    }
    let paths: Vec<PathBuf> = request
        .paths
        .iter()
        .take(MAX_SELECTION_TARGETS + 1)
        .map(|path| parse_path(path))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    match context {
        Context::File | Context::Dir => {
            if paths.len() != 1 {
                return Err(format!("{} takes exactly one path", context.as_str()));
            }
            let wanted = context == Context::Dir;
            match resolved_kind(&paths[0]) {
                Some(is_directory) if is_directory == wanted => Ok(paths),
                Some(_) => Err(format!(
                    "{} is not a {}",
                    path_text(&paths[0]),
                    context.as_str()
                )),
                None => Err(format!("{} does not exist", path_text(&paths[0]))),
            }
        }
        Context::Selection => {
            if paths.is_empty() {
                return Err("selection takes at least one path".to_string());
            }
            if request.paths.len() > MAX_SELECTION_TARGETS {
                return Err(format!("selection too large ({MAX_SELECTION_TARGETS})"));
            }
            Ok(paths)
        }
        Context::Root => {
            let root = parse_path(&request.root).map_err(|error| error.to_string())?;
            if request.root.is_empty() || resolved_kind(&root) != Some(true) {
                return Err("root must name an existing directory".to_string());
            }
            Ok(vec![root])
        }
        Context::None => {
            if !paths.is_empty() {
                return Err("the none context takes no paths".to_string());
            }
            Ok(Vec::new())
        }
    }
}

fn claim(key: &str, captured: bool) -> Result<LiveGuard, String> {
    let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    if live.iter().any(|entry| entry.0 == key) {
        return Err("already running".to_string());
    }
    if captured && live.iter().filter(|entry| entry.1).count() >= MAX_CAPTURED_RUNS {
        return Err(format!("action limit reached ({MAX_CAPTURED_RUNS})"));
    }
    live.push((key.to_string(), captured));
    Ok(LiveGuard {
        key: key.to_string(),
    })
}

fn arguments(action: &Action, targets: &[PathBuf]) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = action.argv.iter().skip(1).map(OsString::from).collect();
    if action.paths == PathsMode::Append {
        arguments.extend(targets.iter().map(|target| target.as_os_str().to_owned()));
    }
    arguments
}

fn working_directory(
    action: &Action,
    root: &Path,
    request: &RunRequest,
    targets: &[PathBuf],
) -> PathBuf {
    let requested_root = (!request.root.is_empty())
        .then(|| parse_path(&request.root).ok())
        .flatten();
    let candidate = match action.cwd {
        CwdMode::Plugin => None,
        CwdMode::Root => requested_root,
        CwdMode::Target => targets
            .first()
            .map(|target| {
                if resolved_kind(target) == Some(true) {
                    target.clone()
                } else {
                    target
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from("/"))
                }
            })
            .or(requested_root),
    };
    candidate
        .filter(|path| open_directory_entry(path).is_ok())
        .unwrap_or_else(|| root.to_path_buf())
}

struct EnvironmentInput<'a> {
    request: &'a RunRequest,
    source: Source,
    key: &'a str,
    root: &'a Path,
    state_dir: &'a Path,
    config_dir: &'a Path,
    targets: &'a [PathBuf],
    selection: &'a str,
    selection_file: Option<&'a Path>,
}

fn environment(input: EnvironmentInput<'_>) -> Vec<(String, OsString)> {
    let plugin = match input.source {
        Source::Plugin => input.request.plugin.clone(),
        Source::User => String::new(),
    };
    let root_path = if input.request.root.is_empty() {
        String::new()
    } else {
        parse_path(&input.request.root)
            .map(|path| path_text(&path))
            .unwrap_or_default()
    };
    let mut values = vec![
        ("FILETREE_ACTION", input.key.to_string()),
        ("FILETREE_SOURCE", input.source.as_str().to_string()),
        ("FILETREE_PLUGIN_ID", plugin),
        ("FILETREE_PLUGIN_ROOT", path_text(input.root)),
        ("FILETREE_STATE_DIR", path_text(input.state_dir)),
        ("FILETREE_CONFIG_DIR", path_text(input.config_dir)),
        ("FILETREE_CONTEXT", input.request.context.clone()),
        ("FILETREE_ROOT", root_path),
        (
            "FILETREE_TARGET",
            input
                .targets
                .first()
                .map(|target| path_text(target))
                .unwrap_or_default(),
        ),
        ("FILETREE_SELECTION_COUNT", input.targets.len().to_string()),
        ("FILETREE_CLI", launcher()),
        (
            "FILETREE_HOST_VERSION",
            env!("CARGO_PKG_VERSION").to_string(),
        ),
        ("FILETREE_SCREEN", printable(&input.request.screen, 64)),
    ];
    let (document, file) = match input.selection_file {
        Some(path) => (String::new(), path_text(path)),
        None => (input.selection.to_string(), String::new()),
    };
    values.push(("FILETREE_SELECTION_JSON", document));
    values.push(("FILETREE_SELECTION_FILE", file));
    values
        .into_iter()
        .map(|(key, value)| {
            let value = value.replace('\0', "");
            let path_key = matches!(
                key,
                "FILETREE_PLUGIN_ROOT"
                    | "FILETREE_STATE_DIR"
                    | "FILETREE_CONFIG_DIR"
                    | "FILETREE_ROOT"
                    | "FILETREE_TARGET"
                    | "FILETREE_SELECTION_FILE"
                    | "FILETREE_CLI"
            );
            let native = if path_key && !value.is_empty() {
                parse_path(&value)
                    .map(PathBuf::into_os_string)
                    .unwrap_or_else(|_| value.into())
            } else {
                value.into()
            };
            (key.to_string(), native)
        })
        .collect()
}

fn run_envelope(request: &RunRequest, key: &str, source: Source, targets: usize) -> Value {
    json!({
        "key": key,
        "source": source.as_str(),
        "plugin": match source {
            Source::Plugin => request.plugin.clone(),
            Source::User => String::new(),
        },
        "context": request.context,
        "targets": targets,
    })
}

fn detached_completion(pid: u32, elapsed: Duration) -> Completion {
    Completion {
        detached: true,
        pid,
        elapsed,
        ..Completion::default()
    }
}

fn captured_completion(output: &CommandOutput, elapsed: Duration) -> Completion {
    Completion {
        exit_code: output.status.code(),
        signal: output.status.signal(),
        stdout: (tail(&output.stdout), output.stdout_truncated),
        stderr: (tail(&output.stderr), output.stderr_truncated),
        elapsed,
        ..Completion::default()
    }
}

fn timeout_completion(elapsed: Duration) -> Completion {
    Completion {
        timed_out: true,
        elapsed,
        ..Completion::default()
    }
}

fn finish(envelope: Value, completion: Completion) -> Value {
    let ok = completion.detached || (completion.exit_code == Some(0) && !completion.timed_out);
    let mut document = envelope;
    let Value::Object(fields) = &mut document else {
        return document;
    };
    fields.insert("ok".to_string(), json!(ok));
    fields.insert("detached".to_string(), json!(completion.detached));
    fields.insert("pid".to_string(), json!(completion.pid));
    fields.insert("exit_code".to_string(), json!(completion.exit_code));
    fields.insert("signal".to_string(), json!(completion.signal));
    fields.insert("timed_out".to_string(), json!(completion.timed_out));
    fields.insert(
        "elapsed_ms".to_string(),
        json!(completion.elapsed.as_millis() as u64),
    );
    fields.insert("stdout_tail".to_string(), json!(completion.stdout.0));
    fields.insert("stderr_tail".to_string(), json!(completion.stderr.0));
    fields.insert("stdout_truncated".to_string(), json!(completion.stdout.1));
    fields.insert("stderr_truncated".to_string(), json!(completion.stderr.1));
    document
}

fn refusal(error: &str) -> Value {
    json!({"ok": false, "error": error})
}

fn directory_field(dirs: &Value, key: &str) -> std::io::Result<PathBuf> {
    parse_path(dirs.get(key).and_then(Value::as_str).unwrap_or_default())
}

fn resolved_kind(path: &Path) -> Option<bool> {
    std::fs::symlink_metadata(path).ok()?;
    std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.is_dir())
}

fn launcher() -> String {
    own_binary()
        .map(|path| path_text(&path))
        .unwrap_or_else(|_| "filetree".to_string())
}

fn printable(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| renderable(*character))
        .take(limit)
        .collect()
}

fn tail(data: &[u8]) -> String {
    let mut start = data.len().saturating_sub(TAIL_BYTES);
    while start < data.len() && data[start] & 0b1100_0000 == 0b1000_0000 {
        start += 1;
    }
    String::from_utf8_lossy(&data[start..])
        .chars()
        .filter(|character| renderable(*character) || matches!(character, '\n' | '\t'))
        .collect()
}
