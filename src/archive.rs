use crate::command::{CommandOutput, CommandSpec, which};
use crate::common::{parse_path, path_error, path_text};
use crate::secure;
use crate::{AppError, AppResult};
use serde_json::{Value, json};
use std::ffi::{OsStr, OsString};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

mod budget;
mod create;
pub use create::create;

pub const ARCHIVE_EXTENSIONS: &[&str] = &[
    "zip", "tar", "tgz", "tbz2", "txz", "tzst", "gz", "bz2", "xz", "zst", "7z", "rar",
];
const ARCHIVE_MEMBER_CAP: usize = 50_000;
const ARCHIVE_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(300);

pub fn is_archive(name: &str) -> bool {
    let lower = name.to_lowercase();
    ARCHIVE_EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(&format!(".{extension}")))
}

fn bsdtar() -> AppResult<PathBuf> {
    which("bsdtar").ok_or_else(|| AppError::command("bsdtar is not installed"))
}

fn run(arguments: Vec<String>, cwd: &Path, cancelled: &AtomicBool) -> AppResult<CommandOutput> {
    let output = CommandSpec::new(bsdtar()?)
        .args(arguments)
        .cwd(cwd)
        .env("LC_ALL", "C.UTF-8")
        .timeout(ARCHIVE_TIMEOUT)
        .limits(ARCHIVE_OUTPUT_BYTES, 64 * 1024)
        .resource_limits(budget::EXPANDED_BYTES, 1024 * 1024 * 1024)
        .run_cancellable(cancelled)?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::command(if detail.is_empty() {
            format!("bsdtar exited with {}", output.status.code().unwrap_or(-1))
        } else {
            detail
        }));
    }
    Ok(output)
}

fn stem(path: &Path) -> OsString {
    let name = path.file_name().unwrap_or_else(|| OsStr::new("archive"));
    let lower = name.as_bytes().to_ascii_lowercase();
    for extension in ["tar.gz", "tar.bz2", "tar.xz", "tar.zst"]
        .iter()
        .chain(ARCHIVE_EXTENSIONS)
    {
        if let Some(base) = lower
            .strip_suffix(format!(".{extension}").as_bytes())
            .filter(|base| !base.is_empty())
        {
            return OsStr::from_bytes(&name.as_bytes()[..base.len()]).to_os_string();
        }
    }
    name.to_os_string()
}

pub fn list(raw_path: &str, cancelled: &AtomicBool) -> Value {
    let path = match parse_path(raw_path) {
        Ok(path) => path,
        Err(error) => return path_error(raw_path, &error),
    };
    let parent = path.parent().unwrap_or(Path::new("/"));
    let listed = (|| -> AppResult<_> {
        let input = secure::open_file_read(&path)?;
        let pinned = format!("/proc/{}/fd/{}", std::process::id(), input.as_raw_fd());
        let names = run(vec!["-tf".to_string(), pinned.clone()], parent, cancelled)?;
        let details = run(
            vec![
                "-tvf".to_string(),
                pinned,
                "--uid=0".into(),
                "--gid=0".into(),
                "--uname=0".into(),
                "--gname=0".into(),
            ],
            parent,
            cancelled,
        )?;
        Ok((names, details))
    })();
    match listed {
        Ok((names, details)) => {
            let text = String::from_utf8_lossy(&names.stdout);
            let metadata = String::from_utf8_lossy(&details.stdout);
            let mut lines = metadata.lines();
            let entries = text
                .lines()
                .take(ARCHIVE_MEMBER_CAP)
                .map(|name| member(lines.next().unwrap_or_default(), &unescape(name)))
                .collect::<Vec<_>>();
            json!({
                "ok": true,
                "archive": path_text(&path),
                "count": entries.len(),
                "truncated": names.stdout_truncated || details.stdout_truncated || text.lines().count() > ARCHIVE_MEMBER_CAP,
                "entries": entries
            })
        }
        Err(error) => {
            json!({"ok": false, "archive": path_text(&path), "entries": [], "error": error.to_string()})
        }
    }
}

fn member(line: &str, name: &str) -> Value {
    let fields = line.split_whitespace().take(8).collect::<Vec<_>>();
    json!({
        "name": name.trim_end_matches('/'),
        "is_dir": fields.first().is_some_and(|mode| mode.starts_with('d')) || name.ends_with('/'),
        "is_symlink": fields.first().is_some_and(|mode| mode.starts_with('l')),
        "size": fields.get(4).and_then(|size| size.parse::<u64>().ok()).unwrap_or(0),
        "modified": fields.get(5..8).map(|date| date.join(" ")).unwrap_or_default()
    })
}

fn unescape(text: &str) -> String {
    let mut bytes = text.bytes();
    let mut decoded = Vec::with_capacity(text.len());
    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            decoded.push(byte);
            continue;
        }
        match bytes.next() {
            Some(b'a') => decoded.push(7),
            Some(b'b') => decoded.push(8),
            Some(b'f') => decoded.push(12),
            Some(b'n') => decoded.push(b'\n'),
            Some(b'r') => decoded.push(b'\r'),
            Some(b't') => decoded.push(b'\t'),
            Some(b'v') => decoded.push(11),
            Some(b'\\') => decoded.push(b'\\'),
            Some(first @ b'0'..=b'3') => {
                let digits: Vec<_> = bytes.by_ref().take(2).collect();
                if digits.len() == 2 && digits.iter().all(|byte| matches!(byte, b'0'..=b'7')) {
                    decoded.push((first - b'0') * 64 + (digits[0] - b'0') * 8 + digits[1] - b'0');
                } else {
                    decoded.extend([b'\\', first]);
                    decoded.extend(digits);
                }
            }
            Some(other) => decoded.extend([b'\\', other]),
            None => decoded.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

pub fn extract(
    raw_path: &str,
    destination: Option<&str>,
    merge: bool,
    cancelled: &AtomicBool,
) -> Value {
    let path = match parse_path(raw_path) {
        Ok(path) => path,
        Err(error) => return path_error(raw_path, &error),
    };
    let parent = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    let target = match destination.map(parse_path).transpose() {
        Ok(target) => target.unwrap_or_else(|| parent.join(stem(&path))),
        Err(error) => return path_error(destination.unwrap_or_default(), &error),
    };
    let mut changed = false;
    let mut published_path = target.clone();
    let mut journal_id = None;
    let mut journal_warning = String::new();
    let mut durability_warning = String::new();
    let outcome = (|| -> AppResult<()> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let input = secure::open_file_read(&path)?;
        let pinned = format!("/proc/{}/fd/{}", std::process::id(), input.as_raw_fd());
        let target_parent = loop {
            match secure::resolved_parent(&published_path) {
                Ok(parent) => break parent,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    published_path = published_path
                        .parent()
                        .ok_or_else(|| AppError::invalid("extraction destination has no parent"))?
                        .to_path_buf();
                }
                Err(error) => return Err(error.into()),
            }
        };
        let existed = secure::entry_exists_resolved(&target_parent)?;
        if existed && published_path != target {
            return Err(AppError::invalid(
                "extraction destination changed during preparation",
            ));
        }
        if destination.is_none() && existed {
            return Err(AppError::invalid(format!(
                "{} already exists",
                target.display()
            )));
        }
        if existed && !merge {
            let occupied = !secure::directory_names(&target)?.is_empty();
            if occupied {
                return Err(AppError::invalid(format!(
                    "{} exists and is not empty; pass --merge to extract into it",
                    target.display()
                )));
            }
        }
        let stage = target_parent.path.join(format!(
            ".fileblade-partial-{}",
            uuid::Uuid::new_v4().simple()
        ));
        secure::create_directory_noreplace(&stage, 0o700)?;
        let intent = match secure::write_intent(
            &json!({"kind": "partial", "partial": stage, "pid": std::process::id()}),
        ) {
            Ok(intent) => intent,
            Err(error) => {
                let _ = secure::remove_empty_directory(&stage);
                return Err(error.into());
            }
        };
        let extracted = (|| -> AppResult<()> {
            let snapshot = secure::create_file_noreplace(&stage.join("archive.tar"), 0o600)?;
            let normalized = format!("/proc/{}/fd/{}", std::process::id(), snapshot.as_raw_fd());
            run(
                vec![
                    "-cf".into(),
                    normalized.clone(),
                    "--format=pax".into(),
                    format!("@{pinned}"),
                ],
                &stage,
                cancelled,
            )?;
            budget::check(&mut std::fs::File::open(&normalized)?)?;
            if existed && merge {
                let directory = secure::open_directory_nofollow(&target)?;
                let cwd = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
                changed = true;
                run(vec!["-xf".into(), normalized], &cwd, cancelled)?;
                return Ok(());
            }
            let item = stage.join("item");
            secure::create_directory_noreplace(&item, 0o777)?;
            let extraction_root = item.join(target.strip_prefix(&published_path).unwrap());
            secure::ensure_directories(&extraction_root, 0o777)?;
            run(vec!["-xf".into(), normalized], &extraction_root, cancelled)?;
            if cancelled.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let original = if existed {
                let original = secure::quarantine_path(&target)?;
                if !secure::directory_names(&original.path()).is_ok_and(|names| names.is_empty()) {
                    original.restore()?;
                    return Err(AppError::invalid(
                        "extraction destination is no longer empty",
                    ));
                }
                Some(original)
            } else {
                None
            };
            let source = secure::resolved_parent(&item)?;
            if let Err(error) = rustix::fs::renameat_with(
                &source.directory,
                &source.name,
                &target_parent.directory,
                &target_parent.name,
                rustix::fs::RenameFlags::NOREPLACE,
            ) {
                if let Some(original) = original {
                    original.restore()?;
                }
                return Err(std::io::Error::from(error).into());
            }
            changed = true;
            if let Err(error) = secure::fsync_path_parent(&published_path) {
                durability_warning = error.to_string();
            }
            if let Some(original) = original
                && let Err(error) = original.remove_empty_directory()
            {
                durability_warning = error.to_string();
            }
            Ok(())
        })();
        if secure::remove_path(&stage).is_ok() {
            let _ = intent.clear();
        }
        extracted?;
        if !existed {
            match crate::journal::record_create(&path_text(&published_path), true, "") {
                Ok(id) => journal_id = Some(id),
                Err(error) => journal_warning = error.to_string(),
            }
        }
        Ok(())
    })();
    let mut result = match outcome {
        Ok(()) => json!({
            "ok": true,
            "operation": "archive-extract",
            "archive": path_text(&path),
            "destination": path_text(&target),
            "paths": [path_text(&target)]
        }),
        Err(error) => json!({
            "ok": false,
            "operation": "archive-extract",
            "archive": path_text(&path),
            "destination": path_text(&target),
            "paths": if changed { vec![path_text(&target)] } else { vec![] },
            "partial": changed,
            "error": error.to_string()
        }),
    };
    if !journal_warning.is_empty() {
        result["journal_warning"] = json!(journal_warning);
    }
    if !durability_warning.is_empty() {
        result["durability_warning"] = json!(durability_warning);
    }
    result["undoable"] = json!(journal_id.is_some());
    if let Some(id) = journal_id {
        result["journal_id"] = json!(id);
    }
    result
}
