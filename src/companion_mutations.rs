use crate::common::parse_path;
use crate::{AppError, AppResult, secure};
use base64::{Engine, prelude::BASE64_STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MAX_CONTENT: usize = 8 * 1024 * 1024;
pub const MAX_INPUT: usize = 24 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Mutation {
    Write {
        path: String,
        resolved: String,
        expected: Option<secure::FileVersion>,
        data: String,
    },
    Link {
        path: String,
        parent: String,
        target: String,
    },
    Unlink {
        path: String,
        parent: String,
        dev: u64,
        ino: u64,
    },
}

fn absolute(raw: &str) -> AppResult<PathBuf> {
    let path = parse_path(raw)?;
    if !path.is_absolute() {
        return Err(AppError::invalid("mutation paths must be absolute"));
    }
    Ok(path)
}

fn parent_for(
    path: &Path,
    expected_parent: &Path,
    create: bool,
) -> AppResult<secure::ResolvedParent> {
    if create {
        secure::ensure_directories(expected_parent, 0o700)?;
    }
    let current = std::fs::canonicalize(
        path.parent()
            .ok_or_else(|| AppError::invalid("path has no parent"))?,
    )?;
    if current != expected_parent {
        return Err(AppError::invalid(
            "source directory changed; refresh and retry",
        ));
    }
    Ok(secure::resolved_parent(
        &expected_parent.join(
            path.file_name()
                .ok_or_else(|| AppError::invalid("path has no name"))?,
        ),
    )?)
}

fn lease() -> AppResult<secure::LockedFile> {
    secure::try_open_private_lock(&crate::paths::state_dir().join("companion-mutations.lock"))?
        .ok_or_else(|| {
            AppError::invalid("another companion change is running; retry when it finishes")
        })
}

pub fn write(
    logical: &Path,
    target: &Path,
    expected: Option<&secure::FileVersion>,
    bytes: &[u8],
) -> AppResult<()> {
    let _lease = lease()?;
    write_locked(logical, target, expected, bytes)
}

fn write_locked(
    logical: &Path,
    target: &Path,
    expected: Option<&secure::FileVersion>,
    bytes: &[u8],
) -> AppResult<()> {
    if bytes.len() > MAX_CONTENT {
        return Err(AppError::invalid(
            "updated configuration exceeds its byte limit",
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| AppError::invalid("path has no parent"))?;
    if expected.is_none() {
        secure::ensure_directories(parent, 0o700)?;
    }
    let current = match std::fs::canonicalize(logical) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound && expected.is_none() => {
            let link = secure::entry_stat(logical);
            if link
                .as_ref()
                .is_ok_and(|stat| stat.kind == secure::EntryKind::Symlink)
            {
                return Err(AppError::invalid("source symlink changed or is dangling"));
            }
            let logical_parent = std::fs::canonicalize(
                logical
                    .parent()
                    .ok_or_else(|| AppError::invalid("path has no parent"))?,
            )?;
            logical_parent.join(
                logical
                    .file_name()
                    .ok_or_else(|| AppError::invalid("path has no name"))?,
            )
        }
        Err(error) => return Err(error.into()),
    };
    if current != target {
        return Err(AppError::invalid("source path changed; refresh and retry"));
    }
    let target = secure::resolved_parent(target)?;
    secure::write_config_expected(target, expected, bytes, MAX_CONTENT)?;
    Ok(())
}

pub fn link(path: &Path, parent: &Path, target: &Path) -> AppResult<()> {
    let _lease = lease()?;
    link_locked(path, parent, target)
}

fn link_locked(path: &Path, parent: &Path, target: &Path) -> AppResult<()> {
    let destination = parent_for(path, parent, true)?;
    rustix::fs::symlinkat(target, &destination.directory, &destination.name)
        .map_err(io::Error::from)?;
    rustix::fs::fsync(&destination.directory).map_err(io::Error::from)?;
    Ok(())
}

pub fn unlink(path: &Path, parent: &Path, dev: u64, ino: u64) -> AppResult<()> {
    let _lease = lease()?;
    unlink_locked(path, parent, dev, ino)
}

fn unlink_locked(path: &Path, parent: &Path, dev: u64, ino: u64) -> AppResult<()> {
    let selected = parent_for(path, parent, false)?;
    secure::remove_nondirectory_matching_resolved(
        selected,
        secure::EntryIdentity {
            dev,
            ino,
            kind: secure::EntryKind::Symlink,
        },
    )?;
    Ok(())
}

pub fn execute(raw: &[u8]) -> AppResult<Value> {
    if raw.len() > MAX_INPUT {
        return Err(AppError::invalid("mutation input exceeds its byte limit"));
    }
    let mutation: Mutation =
        serde_json::from_slice(raw).map_err(|_| AppError::invalid("invalid mutation request"))?;
    // One bounded, cross-process lease also covers different companions editing the same file.
    let _lease = lease()?;
    match mutation {
        Mutation::Write {
            path,
            resolved,
            expected,
            data,
        } => {
            let logical = absolute(&path)?;
            let target = absolute(&resolved)?;
            let bytes = BASE64_STANDARD
                .decode(data)
                .map_err(|_| AppError::invalid("invalid configuration bytes"))?;
            write_locked(&logical, &target, expected.as_ref(), &bytes)?;
        }
        Mutation::Link {
            path,
            parent,
            target,
        } => {
            link_locked(&absolute(&path)?, &absolute(&parent)?, &absolute(&target)?)?;
        }
        Mutation::Unlink {
            path,
            parent,
            dev,
            ino,
        } => {
            unlink_locked(&absolute(&path)?, &absolute(&parent)?, dev, ino)?;
        }
    }
    Ok(json!({"ok":true,"schemaVersion":1}))
}

pub fn stdin_request() -> AppResult<Value> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut bytes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let timeout = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| AppError::invalid("mutation input timed out"))?;
        let mut descriptors = [rustix::event::PollFd::new(
            &stdin,
            rustix::event::PollFlags::IN,
        )];
        if rustix::event::poll(&mut descriptors, Some(&timeout.try_into().unwrap()))
            .map_err(io::Error::from)?
            == 0
        {
            return Err(AppError::invalid("mutation input timed out"));
        }
        let mut buffer = [0u8; 65536];
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > MAX_INPUT {
            return Err(AppError::invalid("mutation input exceeds its byte limit"));
        }
    }
    execute(&bytes)
}
