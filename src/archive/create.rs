use super::*;
use std::collections::BTreeSet;

pub fn create(
    sources: &[String],
    raw_destination: &str,
    format: &str,
    cancelled: &AtomicBool,
) -> Value {
    let mut published = false;
    let mut durability_warning = String::new();
    let mut destination = PathBuf::new();
    let outcome = (|| -> AppResult<()> {
        if sources.is_empty() || sources.len() > 4096 {
            return Err(AppError::invalid(
                "archive requires between 1 and 4096 sources",
            ));
        }
        let format_arguments: &[&str] = match format {
            "tar" => &["--format=pax"],
            "tar.gz" => &["--format=pax", "-z"],
            "tar.zst" => &["--format=pax", "--zstd"],
            "zip" => &["--format=zip"],
            _ => {
                return Err(AppError::invalid(
                    "archive format must be tar, tar.gz, tar.zst or zip",
                ));
            }
        };
        destination = local_path(raw_destination)?;
        let target = secure::resolved_parent(&destination)?;
        if secure::entry_exists_resolved(&target)? {
            return Err(AppError::invalid("archive destination already exists"));
        }
        let canonical_parent = std::fs::canonicalize(&target.path)?;
        let mut parents = Vec::<secure::ResolvedParent>::new();
        let mut identities = Vec::new();
        let mut inputs = Vec::new();
        let mut names = BTreeSet::new();
        let mut arguments = Vec::<OsString>::new();
        for raw_source in sources {
            if cancelled.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let source = local_path(raw_source)?;
            let resolved = secure::resolved_parent(&source)?;
            let stat = secure::entry_stat_resolved(&resolved)?;
            if !matches!(
                stat.kind,
                secure::EntryKind::File | secure::EntryKind::Directory | secure::EntryKind::Symlink
            ) {
                return Err(AppError::invalid("special files cannot be archive sources"));
            }
            if stat.kind == secure::EntryKind::Directory
                && canonical_parent.starts_with(std::fs::canonicalize(&source)?)
            {
                return Err(AppError::invalid(
                    "archive destination cannot be inside a selected source folder",
                ));
            }
            if !names.insert(resolved.name.clone()) {
                return Err(AppError::invalid(
                    "archive sources have duplicate top-level names",
                ));
            }
            let identity = secure::stat_in(&resolved.directory, OsStr::new("."))?.identity();
            let index = match identities.iter().position(|value| *value == identity) {
                Some(index) => index,
                None => {
                    if parents.len() >= 64 {
                        return Err(AppError::invalid(
                            "archive selection exceeds 64 source folders",
                        ));
                    }
                    identities.push(identity);
                    parents.push(secure::resolved_child(
                        &resolved.directory,
                        &resolved.path,
                        &resolved.name,
                    )?);
                    parents.len() - 1
                }
            };
            arguments.extend([
                OsString::from("-C"),
                OsString::from(format!(
                    "/proc/{}/fd/{}",
                    std::process::id(),
                    parents[index].directory.as_raw_fd()
                )),
            ]);
            let mut name = OsString::from("./");
            name.push(&resolved.name);
            arguments.push(name);
            inputs.push((index, resolved.name, stat));
        }
        let name = OsString::from(format!(
            ".filetree-partial-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let stage = secure::resolved_child(&target.directory, &target.path, &name)?;
        secure::create_directory_noreplace_resolved(&stage, 0o700)?;
        let stage_identity = secure::entry_stat_resolved(&stage)?.identity();
        let stage_path = stage.full_path();
        let intent = match secure::write_intent(
            &json!({"kind":"partial","partial":path_text(&stage_path),"dev":stage_identity.dev,"ino":stage_identity.ino,"pid":std::process::id()}),
        ) {
            Ok(intent) => intent,
            Err(error) => {
                let _ = secure::remove_path_matching(&stage_path, stage_identity);
                return Err(error.into());
            }
        };
        let packed = (|| -> AppResult<()> {
            let directory = secure::open_directory_nofollow(&stage_path)?;
            if secure::stat_in(&directory, OsStr::new("."))?.identity() != stage_identity {
                return Err(AppError::invalid("archive staging directory changed"));
            }
            let staged = secure::resolved_child(&directory, &stage_path, OsStr::new("archive"))?;
            let output_file = secure::create_file_noreplace_resolved(&staged, 0o600)?;
            let mut command = vec![
                OsString::from("-c"),
                OsString::from("-f"),
                OsString::from(format!(
                    "/proc/{}/fd/{}",
                    std::process::id(),
                    output_file.as_raw_fd()
                )),
            ];
            command.extend(format_arguments.iter().map(OsString::from));
            command.extend(arguments);
            let output = CommandSpec::new(bsdtar()?)
                .args(command)
                .cwd("/")
                .env("LC_ALL", "C.UTF-8")
                .timeout(ARCHIVE_TIMEOUT)
                .limits(64 * 1024, 64 * 1024)
                .resource_limits(budget::EXPANDED_BYTES, 1024 * 1024 * 1024)
                .run_cancellable(cancelled)?;
            if !output.status.success() {
                if rustix::fs::fstatvfs(&output_file).is_ok_and(|space| space.f_bavail == 0) {
                    return Err(AppError::command(
                        "archive destination is full (No space left on device)",
                    ));
                }
                let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(AppError::command(if message.is_empty() {
                    format!(
                        "archive creation exited with {}",
                        output.status.code().unwrap_or(-1)
                    )
                } else {
                    message
                }));
            }
            if cancelled.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            for (index, name, before) in &inputs {
                let after = secure::stat_in(&parents[*index].directory, name)?;
                if after.identity() != before.identity()
                    || after.size != before.size
                    || after.mtime != before.mtime
                    || after.mtime_nsec != before.mtime_nsec
                    || after.ctime != before.ctime
                    || after.ctime_nsec != before.ctime_nsec
                {
                    return Err(AppError::invalid(
                        "an archive source changed during creation; retry with a stable selection",
                    ));
                }
            }
            output_file.sync_all()?;
            rustix::fs::renameat_with(
                &staged.directory,
                &staged.name,
                &target.directory,
                &target.name,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(std::io::Error::from)?;
            published = true;
            if let Err(error) = rustix::fs::fsync(&target.directory) {
                durability_warning = error.to_string();
            }
            Ok(())
        })();
        if secure::remove_path_matching(&stage_path, stage_identity).is_ok() {
            let _ = intent.clear();
        }
        packed
    })();
    let recorded = if published {
        Some(crate::journal::record_create(
            &path_text(&destination),
            false,
            "",
        ))
    } else {
        None
    };
    let journal_id = recorded.as_ref().and_then(|result| result.as_ref().ok());
    let journal_warning = recorded
        .as_ref()
        .and_then(|result| result.as_ref().err())
        .map(ToString::to_string);
    json!({"ok":outcome.is_ok(),"operation":"archive-create","destination":path_text(&destination),"format":format,
        "paths":if published {vec![path_text(&destination)]} else {vec![]},"partial":published && outcome.is_err(),
        "cancelled":matches!(&outcome,Err(AppError::Cancelled)),"error":outcome.err().map(|error| error.to_string()),
        "undoable":journal_id.is_some(),"journal_id":journal_id,"journal_warning":journal_warning,"durability_warning":durability_warning})
}

fn local_path(raw: &str) -> AppResult<PathBuf> {
    if !raw.starts_with('/')
        && raw.contains("://")
        && !raw
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
    {
        return Err(AppError::invalid(
            "archives require local paths or validated local representations",
        ));
    }
    Ok(parse_path(raw)?)
}
