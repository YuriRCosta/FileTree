use super::*;

pub const SOCKET_KEY: &str = "data-goblin.fileblade/action";
pub const MAX_ACTIONS_PER_SOURCE: usize = 16;
pub const MAX_ARGV: usize = 32;
pub const MAX_TIMEOUT_SECONDS: u64 = 900;
pub const MAX_SELECTION_ENV_BYTES: usize = 65536;
pub const MIN_TIMEOUT_SECONDS: u64 = 1;
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
pub const MAX_ARGV_ITEM_BYTES: usize = 1024;
pub const MAX_ARGV_BYTES: usize = 8 * 1024;
pub const MAX_SELECTION_TARGETS: usize = 256;
pub const MAX_USER_ACTION_FILES: usize = 32;
pub const MAX_ACTIONS_TOTAL: usize = 64;
pub const MAX_PROVIDERS: usize = 128;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_USER_ACTION_BYTES: usize = 16 * 1024;
pub const MAX_CAPTURED_RUNS: usize = 4;
pub const MAX_ERRORS_PER_SOURCE: usize = 16;
pub const MAX_ERRORS_TOTAL: usize = 64;
pub const MAX_USER_ACTION_CANDIDATES: usize = 256;
pub const USER_PLUGIN_ID: &str = "user";
const MAX_ID_BYTES: usize = 64;
const MAX_TITLE_CHARS: usize = 64;
const MAX_GLYPH_CHARS: usize = 16;
const MAX_DESCRIPTION_CHARS: usize = 160;
const DEFAULT_GLYPH: &str = "󰑮";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Plugin,
    User,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::User => "user",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "plugin" => Some(Self::Plugin),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Context {
    File,
    Dir,
    Selection,
    Root,
    None,
}

impl Context {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Dir => "dir",
            Self::Selection => "selection",
            Self::Root => "root",
            Self::None => "none",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "file" => Some(Self::File),
            "dir" => Some(Self::Dir),
            "selection" => Some(Self::Selection),
            "root" => Some(Self::Root),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathsMode {
    Env,
    Append,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CwdMode {
    Plugin,
    Root,
    Target,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    Notice,
    Silent,
}

#[derive(Clone, Debug)]
pub struct Action {
    pub id: String,
    pub title: String,
    pub glyph: String,
    pub description: String,
    pub contexts: Vec<Context>,
    pub argv: Vec<String>,
    pub paths: PathsMode,
    pub cwd: CwdMode,
    pub confirm: bool,
    pub timeout: u64,
    pub detach: bool,
    pub output: OutputMode,
}

impl Action {
    pub fn accepts(&self, context: Context) -> bool {
        self.contexts.contains(&context)
    }

    pub fn program(&self) -> &str {
        self.argv.first().map_or("", String::as_str)
    }
}

pub fn socket_entries(manifest: &Value) -> Vec<Value> {
    manifest
        .get("extensions")
        .and_then(|extensions| extensions.get(SOCKET_KEY))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

pub fn normalize(raw: &Value) -> Result<Action, String> {
    let object = raw.as_object().ok_or("action must be an object")?;
    let id = text_field(object, "id").trim().to_string();
    if !valid_id(&id) {
        return Err(rejected_id(&id));
    }
    let title = plain(text_field(object, "title"), MAX_TITLE_CHARS);
    if title.is_empty() {
        return Err(format!("action {id} has no title"));
    }
    let contexts = normalize_contexts(object.get("contexts"));
    if contexts.is_empty() {
        return Err(format!("action {id} declares no usable context"));
    }
    let argv =
        normalize_argv(object.get("argv")).map_err(|error| format!("action {id}: {error}"))?;
    let glyph = {
        let candidate = plain(text_field(object, "glyph"), MAX_GLYPH_CHARS);
        if candidate.is_empty() {
            DEFAULT_GLYPH.to_string()
        } else {
            candidate
        }
    };
    Ok(Action {
        id,
        title,
        glyph,
        description: plain(text_field(object, "description"), MAX_DESCRIPTION_CHARS),
        contexts,
        argv,
        paths: match text_field(object, "paths") {
            "append" => PathsMode::Append,
            _ => PathsMode::Env,
        },
        cwd: match text_field(object, "cwd") {
            "root" => CwdMode::Root,
            "target" => CwdMode::Target,
            _ => CwdMode::Plugin,
        },
        confirm: object
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        timeout: object
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .clamp(MIN_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS),
        detach: object
            .get("detach")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        output: match text_field(object, "output") {
            "silent" => OutputMode::Silent,
            _ => OutputMode::Notice,
        },
    })
}

pub fn normalize_source(entries: &[Value]) -> (Vec<Action>, Vec<String>, bool) {
    let mut actions = Vec::new();
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    let mut truncated = false;
    for raw in entries {
        if actions.len() >= MAX_ACTIONS_PER_SOURCE || errors.len() >= MAX_ERRORS_PER_SOURCE {
            truncated = true;
            break;
        }
        match normalize(raw) {
            Ok(action) if seen.insert(action.id.clone()) => actions.push(action),
            Ok(action) => errors.push(format!("duplicate action id {}", action.id)),
            Err(error) => errors.push(error),
        }
    }
    (actions, errors, truncated)
}

pub fn resolve_program(action: &Action, root: &Path, source: Source) -> Result<PathBuf, String> {
    let first = action.program();
    if first.is_empty() || first.chars().any(char::is_control) {
        return Err("argv[0] is empty or holds control characters".to_string());
    }
    let candidate = Path::new(first);
    if candidate.is_absolute() {
        if source == Source::User && executable_file(candidate) {
            return Ok(candidate.to_path_buf());
        }
        return Err(format!("argv[0] {first} must be a file inside the plugin"));
    }
    if source == Source::User && !first.contains('/') {
        return which(first).ok_or_else(|| format!("argv[0] {first} is not on PATH"));
    }
    resolve_plugin_program(first, root)
}

pub(crate) fn resolve_plugin_program(first: &str, root: &Path) -> Result<PathBuf, String> {
    if first.is_empty() || first.chars().any(char::is_control) {
        return Err("argv[0] is empty or holds control characters".to_string());
    }
    let candidate = Path::new(first);
    if candidate
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "argv[0] {first} must be a plain relative path inside the plugin"
        ));
    }
    let resolved = root.join(candidate);
    let stat = std::fs::symlink_metadata(&resolved)
        .map_err(|error| format!("argv[0] {first}: {error}"))?;
    if !stat.file_type().is_file() {
        return Err(format!("argv[0] {first} is not a regular file"));
    }
    if stat.permissions().mode() & 0o100 == 0 {
        return Err(format!("argv[0] {first} lacks the owner exec bit"));
    }
    if !confined(&resolved, root) {
        return Err(format!("argv[0] {first} must be a file inside the plugin"));
    }
    Ok(resolved)
}

pub fn rejected_id(id: &str) -> String {
    format!(
        "action id {:?} is not [A-Za-z0-9][A-Za-z0-9._-]*",
        plain(id, MAX_ID_BYTES)
    )
}

fn confined(resolved: &Path, root: &Path) -> bool {
    let real = std::fs::canonicalize(resolved);
    let base = std::fs::canonicalize(root);
    match (real, base) {
        (Ok(real), Ok(base)) => real.starts_with(&base),
        _ => false,
    }
}

pub fn key_for(source: Source, plugin: &str, id: &str) -> String {
    match source {
        Source::Plugin => format!("{plugin}/{id}"),
        Source::User => format!("{USER_PLUGIN_ID}/{id}"),
    }
}

pub fn valid_id(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_ID_BYTES {
        return false;
    }
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
}

fn normalize_contexts(raw: Option<&Value>) -> Vec<Context> {
    let mut contexts = Vec::new();
    for value in raw
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let Some(context) = value.as_str().and_then(Context::parse) else {
            continue;
        };
        if !contexts.contains(&context) {
            contexts.push(context);
        }
    }
    contexts
}

fn normalize_argv(raw: Option<&Value>) -> Result<Vec<String>, String> {
    let entries = raw
        .and_then(Value::as_array)
        .ok_or("argv must be an array of strings")?;
    if entries.is_empty() {
        return Err("argv must hold at least one item".to_string());
    }
    if entries.len() > MAX_ARGV {
        return Err(format!("argv holds more than {MAX_ARGV} items"));
    }
    let mut argv = Vec::with_capacity(entries.len());
    let mut total = 0_usize;
    for entry in entries {
        let item = entry.as_str().ok_or("argv items must be strings")?;
        if item.is_empty() || item.len() > MAX_ARGV_ITEM_BYTES {
            return Err(format!(
                "argv items must be 1 to {MAX_ARGV_ITEM_BYTES} bytes"
            ));
        }
        if item.chars().any(char::is_control) {
            return Err("argv items must not hold control characters".to_string());
        }
        total += item.len();
        if total > MAX_ARGV_BYTES {
            return Err(format!("argv is larger than {MAX_ARGV_BYTES} bytes"));
        }
        argv.push(item.to_string());
    }
    Ok(argv)
}

fn text_field<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> &'a str {
    object.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn plain(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| renderable(*character))
        .take(limit)
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn renderable(character: char) -> bool {
    !character.is_control()
        && !matches!(
            character,
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}'
        )
}

fn executable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
