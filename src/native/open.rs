use super::drain::qml;
use crate::{AppError, AppResult};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const START_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Target {
    pub path: PathBuf,
    pub select: Option<PathBuf>,
}

pub fn resolve(raw: &str) -> AppResult<Target> {
    let path = crate::common::parse_path(raw)
        .map_err(|_| AppError::invalid(format!("not a local path: {raw}")))?;
    let metadata = std::fs::metadata(&path)
        .map_err(|error| AppError::invalid(format!("{}: {error}", path.display())))?;
    if metadata.is_dir() {
        return Ok(Target { path, select: None });
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| AppError::invalid(format!("{}: no parent directory", path.display())))?
        .to_path_buf();
    Ok(Target {
        path: parent,
        select: Some(path),
    })
}

pub fn run(arguments: &[String], properties: bool) -> AppResult<()> {
    let targets = arguments
        .iter()
        .map(|raw| resolve(raw))
        .collect::<AppResult<Vec<_>>>()?;
    let payload = crate::paths::app_root()?;
    for target in targets {
        deliver(&payload, &target, properties)?;
    }
    Ok(())
}

fn deliver(payload: &Path, target: &Target, properties: bool) -> AppResult<()> {
    let deadline = Instant::now() + START_TIMEOUT;
    let arguments = [
        target.path.to_string_lossy().into_owned(),
        target
            .select
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ];
    let method = if properties { "openProperties" } else { "open" };
    match qml(payload, deadline, method, &arguments)? {
        Some(response) if response["ok"] == true => Ok(()),
        Some(response) => Err(AppError::command(format!(
            "FileBlade refused the request: {}",
            response["error"].as_str().unwrap_or("invalid response")
        ))),
        None => start(payload, target, properties, deadline),
    }
}

fn start(payload: &Path, target: &Target, properties: bool, deadline: Instant) -> AppResult<()> {
    let mut command = Command::new(payload.join("app/launch"));
    command
        .env("FILEBLADE_OPEN", &target.path)
        .env_remove("FILEBLADE_SELECT")
        .env_remove("FILEBLADE_PROPERTIES")
        .stdin(Stdio::null());
    if let Some(select) = &target.select {
        command.env("FILEBLADE_SELECT", select);
    }
    if properties {
        command.env("FILEBLADE_PROPERTIES", "1");
    }
    let mut child = command.spawn().map_err(|error| {
        AppError::command(format!(
            "could not start {}: {error}",
            payload.join("app/launch").display()
        ))
    })?;
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(AppError::command(format!(
                    "app/launch exited with {status}"
                )))
            };
        }
        if Instant::now() >= deadline {
            return Err(AppError::command(
                "FileBlade did not answer within 30 s; inspect view.log under the native state root",
            ));
        }
        if qml(payload, deadline, "status", &[])?.is_some_and(|status| status["loaded"] == true) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
