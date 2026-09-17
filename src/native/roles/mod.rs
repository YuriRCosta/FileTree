use crate::{AppError, AppResult};
use clap::{Args as ClapArgs, Subcommand, ValueEnum};
use fileblade_output::Output;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

mod autostart;
mod bindings;
mod chooser;
mod engine;
mod entry;
mod folder;
mod receipt;
mod reveal;

const AUTHORITY_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Role {
    Autostart,
    Bindings,
    Chooser,
    Folder,
    Reveal,
}

impl Role {
    pub const ALL: [Role; 5] = [
        Role::Autostart,
        Role::Bindings,
        Role::Chooser,
        Role::Folder,
        Role::Reveal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Role::Autostart => "autostart",
            Role::Bindings => "bindings",
            Role::Chooser => "chooser",
            Role::Folder => "folder",
            Role::Reveal => "reveal",
        }
    }

    pub fn parse(name: &str) -> AppResult<Self> {
        Self::ALL
            .into_iter()
            .find(|role| role.name() == name)
            .ok_or_else(|| AppError::invalid(format!("unknown desktop role {name}")))
    }

    fn plan(self, launcher: &Path) -> Vec<entry::Planned> {
        let config = crate::paths::xdg_home("XDG_CONFIG_HOME", "~/.config");
        let data = crate::paths::xdg_home("XDG_DATA_HOME", "~/.local/share");
        match self {
            Role::Autostart => autostart::plan(launcher, &config),
            Role::Bindings => bindings::plan(&config),
            Role::Chooser => chooser::plan(launcher, &config, &data),
            Role::Folder => folder::plan(launcher, &config, &data),
            Role::Reveal => reveal::plan(launcher, &data),
        }
    }

    fn conflict(self) -> String {
        match self {
            Role::Reveal => reveal::conflict(),
            _ => String::new(),
        }
    }

    fn after(self) -> String {
        match self {
            Role::Bindings => bindings::after(),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug, ClapArgs)]
pub struct Args {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Debug, Subcommand)]
enum Command {
    Status,
    Enable {
        #[arg(long, value_enum)]
        role: Role,
    },
    Disable {
        #[arg(
            long,
            value_enum,
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        role: Option<Role>,
        #[arg(long)]
        all: bool,
    },
}

#[derive(Clone, Copy, Debug)]
enum Request {
    Status,
    Set(Role, bool),
}

pub fn run(args: Args, output: Arc<Output>) -> ExitCode {
    let (action, document) = match args.command {
        Command::Status => ("roles_status", execute(Request::Status)),
        Command::Enable { role } => ("roles_enable", execute(Request::Set(role, true))),
        Command::Disable { role, .. } => (
            "roles_disable",
            disable_roles(role.map_or_else(|| Role::ALL.to_vec(), |role| vec![role])),
        ),
    };
    let document = document.unwrap_or_else(|error| refused(action, &error.to_string()));
    let code = match document["status"].as_str() {
        Some("partial" | "refused") => 1,
        _ => 0,
    };
    if output.machine(&document).is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::from(code)
}

fn disable_roles(roles: Vec<Role>) -> AppResult<Value> {
    let mut results = serde_json::Map::new();
    for role in roles {
        let document = execute(Request::Set(role, false))?;
        if document["status"] == "refused" {
            return Ok(document);
        }
        results.insert(role.name().into(), document["roles"][role.name()].clone());
    }
    Ok(compose("roles_disable", results))
}

fn refused(action: &str, error: &str) -> Value {
    json!({"schema": 1, "action": action, "status": "refused", "error": error,
        "remaining_owned_entries": [], "roles": {}})
}

fn compose(action: &str, roles: serde_json::Map<String, Value>) -> Value {
    let statuses: Vec<&str> = roles
        .values()
        .filter_map(|role| role["status"].as_str())
        .collect();
    let status = if statuses
        .iter()
        .any(|status| matches!(*status, "partial" | "refused"))
    {
        "partial"
    } else if action == "roles_enable" && statuses.iter().all(|status| *status == "already_on") {
        "already_on"
    } else {
        "complete"
    };
    let remaining: Vec<Value> = roles
        .values()
        .filter_map(|role| role["remaining_owned_entries"].as_array())
        .flatten()
        .cloned()
        .collect();
    let error = roles
        .values()
        .filter_map(|role| role["error"].as_str())
        .find(|error| !error.is_empty())
        .unwrap_or("");
    json!({"schema": 1, "action": action, "status": status, "error": error,
        "remaining_owned_entries": remaining, "roles": roles})
}

fn perform(request: Request) -> Value {
    let action = match request {
        Request::Status => "roles_status",
        Request::Set(_, true) => "roles_enable",
        Request::Set(_, false) => "roles_disable",
    };
    let mut receipt = match receipt::load() {
        Ok(receipt) => receipt,
        Err(error) => return refused(action, &error.to_string()),
    };
    match request {
        Request::Status => engine::status(&receipt),
        Request::Set(role, on) => {
            let outcome = if on {
                engine::enable(&mut receipt, role)
            } else {
                engine::disable(&mut receipt, role)
            };
            let mut roles = serde_json::Map::new();
            roles.insert(role.name().into(), outcome.to_json());
            let mut document = compose(action, roles);
            if outcome.status == "refused" {
                document["status"] = json!("refused");
                document["error"] = json!(outcome.error);
            }
            document
        }
    }
}

pub fn status_document() -> AppResult<Value> {
    Ok(perform(Request::Status))
}

pub fn set_document(role: &str, on: bool) -> AppResult<Value> {
    Ok(perform(Request::Set(Role::parse(role)?, on)))
}

fn execute(request: Request) -> AppResult<Value> {
    let Some(root) = crate::lease::selected_root()? else {
        return Ok(perform(request));
    };
    match crate::lease::transport::connect(&root) {
        Ok(socket) => forward(socket, request),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            let authority = Arc::new(
                crate::lease::Authority::acquire_bound(
                    &root,
                    crate::lease::native_config_root(),
                    crate::lease::native_recovery_root(),
                )
                .map_err(|error| AppError::command(error.to_string()))?,
            );
            authority
                .set_write_mode(crate::lease::WriteMode::Full)
                .map_err(|error| AppError::command(error.to_string()))?;
            let _session =
                crate::lease::persistence::PersistenceSession::open(Arc::clone(&authority))?;
            Ok(perform(request))
        }
        Err(error) => Err(error.into()),
    }
}

fn forward(mut socket: std::os::unix::net::UnixStream, request: Request) -> AppResult<Value> {
    socket.set_read_timeout(Some(AUTHORITY_TIMEOUT))?;
    socket.set_write_timeout(Some(AUTHORITY_TIMEOUT))?;
    let mut reader = BufReader::new(socket.try_clone()?);
    let mut exchange = |frame: Value, reply: &str| -> AppResult<Value> {
        serde_json::to_writer(&mut socket, &frame)?;
        socket.write_all(b"\n")?;
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                return Err(AppError::command("native authority closed the connection"));
            }
            let value: Value = serde_json::from_str(&line)?;
            if value["type"] == reply && value["id"] == frame["id"] {
                return Ok(value);
            }
        }
    };
    let hello = exchange(json!({"v": 1, "type": "hello"}), "hello")?;
    if hello["ok"] != true {
        return Err(AppError::command("native authority handshake failed"));
    }
    let (command, arguments): (&str, Vec<String>) = match request {
        Request::Status => ("roles-status", Vec::new()),
        Request::Set(role, on) => (
            "roles-set",
            ["--role", role.name(), if on { "--on" } else { "" }]
                .into_iter()
                .filter(|argument| !argument.is_empty())
                .map(str::to_string)
                .collect(),
        ),
    };
    let response = exchange(
        json!({
            "v": 1, "type": "request", "id": uuid::Uuid::new_v4().to_string(), "generation": 1,
            "command": command, "arguments": arguments,
            "deadline_ms": AUTHORITY_TIMEOUT.as_millis() as u64,
        }),
        "response",
    )?;
    if response["ok"] != true {
        return Err(AppError::command(
            response["error"]
                .as_str()
                .unwrap_or("native authority refused the desktop role request"),
        ));
    }
    Ok(response["payload"].clone())
}
