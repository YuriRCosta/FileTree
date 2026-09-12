use super::*;
use base64::{Engine, prelude::BASE64_STANDARD};

pub fn open_record_lock(path: &Path) -> io::Result<LockedFile> {
    if !crate::lease::persistence::native()? {
        return open_private_lock(path);
    }
    let parent = crate::lease::persistence::record_parent(path)?;
    let fd = openat(
        &parent.directory,
        &parent.name,
        OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::from_raw_mode(PRIVATE_FILE_MODE),
    )?;
    verify_private_fd(&fd, EntryKind::File, PRIVATE_FILE_MODE)?;
    flock(&fd, FlockOperation::LockExclusive)?;
    crate::lease::persistence::check_write(path)?;
    Ok(LockedFile { file: fd.into() })
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileVersion {
    pub dev: u64,
    pub ino: u64,
    pub data: String,
}

fn matches_version(file: &mut File, expected: &FileVersion, maximum: usize) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.dev() != expected.dev
        || metadata.ino() != expected.ino
        || metadata.len() > maximum as u64
    {
        return Ok(false);
    }
    let expected_bytes = BASE64_STANDARD
        .decode(&expected.data)
        .map_err(|_| invalid_input("invalid original configuration bytes"))?;
    if expected_bytes.len() > maximum {
        return Err(invalid_input(
            "original configuration exceeds its byte limit",
        ));
    }
    let mut offset = 0usize;
    let mut remaining = maximum + 1;
    let mut buffer = [0u8; 65536];
    while remaining > 0 {
        let count = file.read(&mut buffer[..remaining.min(65536)])?;
        if count == 0 {
            return Ok(offset == expected_bytes.len());
        }
        if expected_bytes.get(offset..offset + count) != Some(&buffer[..count]) {
            return Ok(false);
        }
        offset += count;
        remaining -= count;
    }
    Ok(false)
}

pub fn write_config_expected(
    target: ResolvedParent,
    expected: Option<&FileVersion>,
    data: &[u8],
    maximum: usize,
) -> io::Result<()> {
    crate::lease::persistence::check_write(&target.full_path())?;
    let target = if crate::lease::persistence::storage_anchor(&target.full_path())?.is_some() {
        crate::lease::persistence::record_parent(&target.full_path())?
    } else {
        target
    };
    if data.len() > maximum {
        return Err(invalid_input(
            "updated configuration exceeds its byte limit",
        ));
    }
    let partial_name = OsString::from(format!(".fileblade-partial-{}", Uuid::new_v4().simple()));
    mkdirat(
        &target.directory,
        &partial_name,
        Mode::from_raw_mode(PRIVATE_DIRECTORY_MODE),
    )?;
    let partial = openat(
        &target.directory,
        &partial_name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    verify_private_fd(&partial, EntryKind::Directory, PRIVATE_DIRECTORY_MODE)?;
    fsync(&target.directory)?;
    let partial_path = target.path.join(&partial_name);
    let intent = match write_intent(
        &serde_json::json!({"kind":"partial", "partial":crate::common::path_text(&partial_path), "pid":std::process::id()}),
    ) {
        Ok(intent) => intent,
        Err(error) => {
            let _ = unlinkat(&target.directory, &partial_name, AtFlags::REMOVEDIR);
            return Err(error);
        }
    };
    let outcome = (|| {
        let new_name = OsStr::new("replacement");
        let new_fd = openat(
            &partial,
            new_name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_raw_mode(PRIVATE_FILE_MODE),
        )?;
        let mut new_file = File::from(new_fd);
        new_file.write_all(data)?;
        new_file.sync_all()?;
        fsync(&partial)?;
        let previous = match expected {
            Some(version) => {
                let resolved = resolved_child(&target.directory, &target.path, &target.name)?;
                let secured = quarantine_resolved(resolved)?;
                let checked = (|| {
                    let old_fd = openat(
                        &secured.staging_directory,
                        &secured.entry_name,
                        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                        Mode::empty(),
                    )?;
                    let mut old_file = File::from(old_fd);
                    if !matches_version(&mut old_file, version, maximum)? {
                        return Err(invalid_data(
                            "configuration changed since it was read; refresh and retry",
                        ));
                    }
                    fchmod(&new_file, Mode::from_raw_mode(secured.stat.mode & 0o777))?;
                    if let Ok(names) = old_file.list_xattr() {
                        for name in names {
                            if let Ok(Some(value)) = old_file.get_xattr(&name) {
                                let _ = new_file.set_xattr(&name, &value);
                            }
                        }
                    }
                    new_file.sync_all()
                })();
                if let Err(error) = checked {
                    return Err(with_restore_error(error, secured.restore(), &secured));
                }
                Some(secured)
            }
            None => None,
        };
        crate::lease::persistence::verify()?;
        let published = renameat_with(
            &partial,
            new_name,
            &target.directory,
            &target.name,
            RenameFlags::NOREPLACE,
        )
        .map_err(io::Error::from);
        if let Err(error) = published {
            return Err(match previous {
                Some(secured) => with_restore_error(error, secured.restore(), &secured),
                None => error,
            });
        }
        fsync(&target.directory)?;
        fsync(&partial)?;
        if let Some(previous) = previous {
            previous.remove_nondirectory()?;
        }
        Ok(())
    })();
    let cleanup = remove_entry_from(&target.directory, &partial_name)
        .and_then(|()| fsync(&target.directory).map_err(io::Error::from));
    if cleanup.is_ok() {
        let _ = intent.clear();
    }
    outcome.and(cleanup)
}
