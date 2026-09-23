use super::*;

const MAX_ACTION_ROWS: usize = 64;
const MAX_ACTION_TEXT: usize = 200;
const MAX_TAIL_LINES: usize = 64;
const MAX_ACTION_PATHS: usize = 256;
const MAX_ACTION_PATH_DOCUMENT_BYTES: usize = 64 * 1024;
const MAX_ACTION_WAIT: f64 = 905.0;
const ACTION_WAIT_GRACE: f64 = 5.0;

#[derive(Clone, Debug, Args)]
pub struct ModuleDirsArgs {
    pub id: String,
}

#[derive(Clone, Debug, Args)]
pub struct ActionArgs {
    pub key: String,
    pub paths: Vec<String>,
    #[arg(long)]
    pub yes: bool,
    #[arg(long, allow_hyphen_values = true)]
    pub wait: Option<f64>,
}

struct ActionRow {
    key: String,
    title: String,
    confirm: bool,
    timeout: f64,
}

pub(super) fn module_dirs(options: ModuleDirsArgs) -> AppResult<PublicResult> {
    let document = backend_json(&[
        "module-dirs".to_string(),
        "--module".to_string(),
        options.id,
    ])?;
    if !document["ok"].as_bool().unwrap_or(false) {
        return Err(AppError::command(error_text(
            &document,
            "module directories were not created",
        )));
    }
    let lines = ["state_dir", "config_dir"]
        .iter()
        .map(|key| value_text(&document, key))
        .collect();
    Ok(PublicResult::lines(lines, document))
}

pub(super) fn actions() -> AppResult<PublicResult> {
    let document = Value::Object(object_response("actions", &[])?);
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for action in action_rows(&document) {
        let group = match value_text(action, "source").as_str() {
            "user" => "user".to_string(),
            _ => value_text(action, "plugin"),
        };
        let line = action_line(action);
        match groups.iter_mut().find(|(known, _)| *known == group) {
            Some((_, rows)) => rows.push(line),
            None => groups.push((group, vec![line])),
        }
    }
    let mut lines = Vec::new();
    for (group, rows) in groups {
        lines.push(plain(&group).to_uppercase());
        lines.extend(rows);
    }
    if lines.is_empty() {
        lines.push("no script actions".to_string());
    }
    Ok(PublicResult::lines(lines, document))
}

pub(super) fn action(options: ActionArgs) -> AppResult<PublicResult> {
    let document = Value::Object(object_response("actions", &[])?);
    let chosen = resolve_action(&document, &options.key)?;
    if chosen.confirm && !options.yes {
        return Err(AppError::command(format!(
            "{} asks for confirmation; pass --yes to run it",
            chosen.key
        )));
    }
    let wait = options.wait.unwrap_or(chosen.timeout + ACTION_WAIT_GRACE);
    if !wait.is_finite() || !(1.0..=MAX_ACTION_WAIT).contains(&wait) {
        return Err(AppError::invalid(format!(
            "action wait must be between 1 and {MAX_ACTION_WAIT} seconds"
        )));
    }
    if options.paths.len() > MAX_ACTION_PATHS {
        return Err(AppError::invalid(format!(
            "selection too large ({MAX_ACTION_PATHS})"
        )));
    }
    if options.paths.iter().any(String::is_empty) {
        return Err(AppError::invalid("every path must be given"));
    }
    let paths: Vec<Value> = options
        .paths
        .iter()
        .map(|path| Value::String(absolute_path(path)))
        .collect();
    let path_document = Value::Array(paths);
    if serde_json::to_vec(&path_document)?.len() > MAX_ACTION_PATH_DOCUMENT_BYTES {
        return Err(AppError::invalid("path list is too large for shell IPC"));
    }
    let encoded_paths = encoded_document(&path_document)?;
    let queued = response_value(&ipc(
        "runAction",
        &[chosen.key.clone(), encoded_paths, options.yes.to_string()],
    )?);
    if !queued.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(AppError::command(error_text(
            &queued,
            "the action was not started",
        )));
    }
    let request_id = value_text(&queued, "request_id");
    if request_id.is_empty() {
        return Err(AppError::command("the action returned no request id"));
    }
    let result = wait_for_action(&request_id, wait)?;
    let succeeded = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let mut report = PublicResult::lines(action_report(&chosen, &result), result);
    if !succeeded {
        report.error = Some(format!("{} failed", chosen.key));
    }
    Ok(report)
}

fn wait_for_action(request_id: &str, wait: f64) -> AppResult<Value> {
    poll(
        wait,
        || {
            let state = Value::Object(object_response("actionResult", &[request_id.to_string()])?);
            match value_text(&state, "status").as_str() {
                "done" => Ok(Some(state.get("result").cloned().unwrap_or(Value::Null))),
                "running" => Ok(None),
                _ => Err(AppError::command(format!(
                    "the shell forgot action run {request_id}"
                ))),
            }
        },
        &format!("Timed out waiting for action run {request_id}"),
    )
}

fn action_report(chosen: &ActionRow, result: &Value) -> Vec<String> {
    if !result.is_object() {
        return vec![format!("{}: no result", chosen.key)];
    }
    if let Some(error) = result
        .get("error")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        return vec![format!("{}: {}", chosen.title, plain(error))];
    }
    let mut lines = vec![format!(
        "{}: {} in {} ms",
        chosen.title,
        action_outcome(result),
        value_i64(result, "elapsed_ms")
    )];
    for line in value_text(result, "stdout_tail")
        .lines()
        .take(MAX_TAIL_LINES)
    {
        lines.push(plain(line));
    }
    lines
}

fn action_outcome(result: &Value) -> String {
    if result.get("timed_out").and_then(Value::as_bool) == Some(true) {
        return "timed out".to_string();
    }
    if result.get("detached").and_then(Value::as_bool) == Some(true) {
        return format!("detached as pid {}", value_i64(result, "pid"));
    }
    match result.get("exit_code").and_then(Value::as_i64) {
        Some(code) => format!("exit {code}"),
        None => "killed".to_string(),
    }
}

fn action_rows(document: &Value) -> impl Iterator<Item = &Value> {
    document["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_ACTION_ROWS)
}

fn action_line(action: &Value) -> String {
    let contexts = action["contexts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "  {}  [{}]  {}",
        plain(&value_text(action, "key")),
        plain(&contexts),
        plain(&value_text(action, "title"))
    )
}

fn resolve_action(document: &Value, wanted: &str) -> AppResult<ActionRow> {
    let mut bare: Vec<ActionRow> = Vec::new();
    for action in action_rows(document) {
        let row = ActionRow {
            key: value_text(action, "key"),
            title: plain(&value_text(action, "title")),
            confirm: action
                .get("confirm")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            timeout: (value_i64(action, "timeout") as f64).clamp(1.0, 900.0),
        };
        if row.key == wanted {
            return Ok(row);
        }
        if value_text(action, "id") == wanted {
            bare.push(row);
        }
    }
    match bare.len() {
        1 => Ok(bare.remove(0)),
        0 => Err(AppError::command(format!(
            "no script action {wanted}; filetree actions lists them"
        ))),
        _ => Err(AppError::command(format!(
            "{wanted} is ambiguous: {}",
            bare.iter()
                .map(|row| row.key.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn plain(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_ACTION_TEXT)
        .collect()
}
