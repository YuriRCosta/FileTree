use super::*;

impl QuarantinedEntry {
    pub fn stat(&self) -> EntryStat {
        self.stat
    }

    pub fn name(&self) -> &OsStr {
        &self.entry_name
    }

    pub fn path(&self) -> PathBuf {
        self.parent_path
            .join(&self.staging_name)
            .join(&self.entry_name)
    }

    pub fn command_directory(&self) -> PathBuf {
        PathBuf::from(format!(
            "/proc/self/fd/{}",
            self.staging_directory.as_raw_fd()
        ))
    }

    pub fn command_path(&self) -> String {
        format!("./{}", self.entry_name.to_string_lossy())
    }

    pub fn restore(&self) -> io::Result<()> {
        renameat_with(
            &self.staging_directory,
            &self.entry_name,
            &self.parent_directory,
            &self.original_name,
            RenameFlags::NOREPLACE,
        )
        .map_err(io::Error::from)?;
        fsync(&self.staging_directory).map_err(io::Error::from)?;
        fsync(&self.parent_directory).map_err(io::Error::from)?;
        self.cleanup_staging_directory();
        Ok(())
    }

    pub fn remove(self) -> io::Result<()> {
        remove_entry_from(&self.staging_directory, &self.entry_name)?;
        fsync(&self.staging_directory).map_err(io::Error::from)?;
        fsync(&self.parent_directory).map_err(io::Error::from)?;
        self.cleanup_staging_directory();
        Ok(())
    }

    pub fn finish(self) -> io::Result<()> {
        match stat_in(&self.staging_directory, &self.entry_name) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fsync(&self.staging_directory).map_err(io::Error::from)?;
                self.cleanup_staging_directory();
                Ok(())
            }
            Ok(_) => Err(invalid_data("secured entry was not moved")),
            Err(error) => Err(error),
        }
    }

    pub fn remove_empty_directory(self) -> io::Result<()> {
        match unlinkat(
            &self.staging_directory,
            &self.entry_name,
            AtFlags::REMOVEDIR,
        ) {
            Ok(()) => {
                fsync(&self.staging_directory).map_err(io::Error::from)?;
                fsync(&self.parent_directory).map_err(io::Error::from)?;
                self.cleanup_staging_directory();
                Ok(())
            }
            Err(error) => {
                let restored = self.restore();
                Err(with_restore_error(error.into(), restored, &self))
            }
        }
    }

    pub fn remove_nondirectory(self) -> io::Result<()> {
        match unlinkat(&self.staging_directory, &self.entry_name, AtFlags::empty()) {
            Ok(()) => {
                fsync(&self.staging_directory).map_err(io::Error::from)?;
                fsync(&self.parent_directory).map_err(io::Error::from)?;
                self.cleanup_staging_directory();
                Ok(())
            }
            Err(error) => {
                let restored = self.restore();
                Err(with_restore_error(error.into(), restored, &self))
            }
        }
    }

    pub fn relocate_noreplace_to(
        self,
        destination_parent: &ResolvedParent,
        cancelled: &AtomicBool,
        progress: &mut dyn FnMut(&Path),
    ) -> io::Result<()> {
        match renameat_with(
            &self.staging_directory,
            &self.entry_name,
            &destination_parent.directory,
            &destination_parent.name,
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => {
                fsync(&self.staging_directory).map_err(io::Error::from)?;
                fsync(&destination_parent.directory).map_err(io::Error::from)?;
                self.cleanup_staging_directory();
                Ok(())
            }
            Err(rustix::io::Errno::XDEV) => {
                let copied = copy_from_parent_noreplace(
                    &self.staging_directory,
                    &self.entry_name,
                    &self.path(),
                    destination_parent,
                    cancelled,
                    progress,
                );
                if let Err(error) = copied {
                    let restored = self.restore();
                    return Err(with_restore_error(error, restored, &self));
                }
                if let Err(error) = remove_entry_from(&self.staging_directory, &self.entry_name) {
                    return Err(io::Error::other(format!(
                        "destination was copied but secured source cleanup failed: {error}; source data remains at {}",
                        self.path().display()
                    )));
                }
                fsync(&self.staging_directory).map_err(io::Error::from)?;
                fsync(&destination_parent.directory).map_err(io::Error::from)?;
                self.cleanup_staging_directory();
                Ok(())
            }
            Err(error) => {
                let restored = self.restore();
                Err(with_restore_error(error.into(), restored, &self))
            }
        }
    }

    pub(super) fn cleanup_staging_directory(&self) {
        cleanup_staging_link(
            &self.parent_directory,
            &self.staging_name,
            self.staging_identity,
        );
        let _ = self.intent.clear();
    }

    pub fn finish_or_restore(self) -> io::Result<()> {
        match stat_in(&self.staging_directory, &self.entry_name) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fsync(&self.staging_directory).map_err(io::Error::from)?;
                self.cleanup_staging_directory();
                Ok(())
            }
            Ok(_) => {
                let restored = self.restore();
                Err(with_restore_error(
                    invalid_data("secured entry was not moved"),
                    restored,
                    &self,
                ))
            }
            Err(error) => Err(error),
        }
    }
}

pub fn quarantine_path(path: &Path) -> io::Result<QuarantinedEntry> {
    quarantine_resolved(resolved_parent(path)?)
}

pub fn quarantine_resolved(parent: ResolvedParent) -> io::Result<QuarantinedEntry> {
    let staging_name = OsString::from(format!(".fileblade-stage-{}", Uuid::new_v4().simple()));
    mkdirat(
        &parent.directory,
        &staging_name,
        Mode::from_raw_mode(PRIVATE_DIRECTORY_MODE),
    )
    .map_err(io::Error::from)?;
    let staging_directory = openat(
        &parent.directory,
        &staging_name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    verify_owner_and_kind(&staging_directory, EntryKind::Directory)?;
    fchmod(
        &staging_directory,
        Mode::from_raw_mode(PRIVATE_DIRECTORY_MODE),
    )
    .map_err(io::Error::from)?;
    verify_private_fd(
        &staging_directory,
        EntryKind::Directory,
        PRIVATE_DIRECTORY_MODE,
    )?;
    let staging_stat = stat_value(fstat(&staging_directory).map_err(io::Error::from)?);
    if stat_in(&parent.directory, &staging_name)?.identity() != staging_stat.identity() {
        return Err(invalid_data(
            "staging directory changed while it was opened",
        ));
    }
    let entry_name = OsString::from(format!("item-{}", Uuid::new_v4().simple()));
    let intent = match write_intent(&serde_json::json!({
        "kind": "stage",
        "stage": crate::common::path_text(&parent.path.join(&staging_name)),
        "item": entry_name.to_string_lossy(),
        "original": crate::common::path_text(&parent.path.join(&parent.name)),
        "pid": std::process::id(),
    })) {
        Ok(intent) => intent,
        Err(error) => {
            cleanup_staging_link(&parent.directory, &staging_name, staging_stat.identity());
            return Err(error);
        }
    };
    if let Err(error) = renameat_with(
        &parent.directory,
        &parent.name,
        &staging_directory,
        &entry_name,
        RenameFlags::NOREPLACE,
    ) {
        cleanup_staging_link(&parent.directory, &staging_name, staging_stat.identity());
        let _ = intent.clear();
        return Err(error.into());
    }
    let stat = match stat_in(&staging_directory, &entry_name) {
        Ok(stat) => stat,
        Err(error) => {
            let _ = renameat_with(
                &staging_directory,
                &entry_name,
                &parent.directory,
                &parent.name,
                RenameFlags::NOREPLACE,
            );
            cleanup_staging_link(&parent.directory, &staging_name, staging_stat.identity());
            return Err(error);
        }
    };
    let quarantine = QuarantinedEntry {
        parent_directory: parent.directory,
        staging_directory,
        original_name: parent.name,
        staging_name,
        entry_name,
        parent_path: parent.path,
        staging_identity: staging_stat.identity(),
        stat,
        intent,
    };
    if let Err(error) = fsync(&quarantine.staging_directory)
        .and_then(|()| fsync(&quarantine.parent_directory))
        .map_err(io::Error::from)
    {
        let restored = quarantine.restore();
        return Err(with_restore_error(error, restored, &quarantine));
    }
    Ok(quarantine)
}

pub(super) fn cleanup_staging_link(parent: &OwnedFd, name: &OsStr, expected: EntryIdentity) {
    if stat_in(parent, name)
        .map(|stat| stat.identity() == expected)
        .unwrap_or(false)
    {
        let _ = unlinkat(parent, name, AtFlags::REMOVEDIR);
    }
}

pub fn remove_path_matching(path: &Path, expected: EntryIdentity) -> io::Result<()> {
    let quarantine = quarantine_path(path)?;
    if quarantine.stat.identity() != expected {
        return Err(quarantine_mismatch(&quarantine, path));
    }
    quarantine.remove()
}

pub fn remove_empty_directory_matching(path: &Path, expected: EntryIdentity) -> io::Result<()> {
    let quarantine = quarantine_path(path)?;
    if quarantine.stat.identity() != expected || quarantine.stat.kind != EntryKind::Directory {
        return Err(quarantine_mismatch(&quarantine, path));
    }
    quarantine.remove_empty_directory()
}

pub fn remove_nondirectory_matching(path: &Path, expected: EntryIdentity) -> io::Result<()> {
    remove_nondirectory_matching_resolved(resolved_parent(path)?, expected)
}

pub fn remove_nondirectory_matching_resolved(
    target: ResolvedParent,
    expected: EntryIdentity,
) -> io::Result<()> {
    let path = target.full_path();
    let quarantine = quarantine_resolved(target)?;
    if quarantine.stat.identity() != expected || quarantine.stat.kind == EntryKind::Directory {
        return Err(quarantine_mismatch(&quarantine, &path));
    }
    quarantine.remove_nondirectory()
}

pub(super) fn quarantine_mismatch(quarantine: &QuarantinedEntry, path: &Path) -> io::Error {
    with_restore_error(
        invalid_data(format!(
            "{} changed before it could be secured",
            path.display()
        )),
        quarantine.restore(),
        quarantine,
    )
}

pub(super) fn with_restore_error(
    error: io::Error,
    restored: io::Result<()>,
    quarantine: &QuarantinedEntry,
) -> io::Error {
    match restored {
        Ok(()) => error,
        Err(restore_error) => io::Error::other(format!(
            "{error}; data remains at {} because rollback failed: {restore_error}",
            quarantine.path().display()
        )),
    }
}

pub fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    let source_parent = resolved_parent(source)?;
    let destination_parent = resolved_parent(destination)?;
    rename_noreplace_resolved(&source_parent, &destination_parent)
}

pub fn rename_noreplace_resolved(
    source: &ResolvedParent,
    destination: &ResolvedParent,
) -> io::Result<()> {
    renameat_with(
        &source.directory,
        &source.name,
        &destination.directory,
        &destination.name,
        RenameFlags::NOREPLACE,
    )
    .map_err(io::Error::from)?;
    fsync(&source.directory).map_err(io::Error::from)?;
    fsync(&destination.directory).map_err(io::Error::from)
}

pub fn relocate_noreplace(
    source: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(&Path),
) -> io::Result<()> {
    check_cancelled(cancelled)?;
    let source = resolved_parent(source)?;
    let destination = resolved_parent(destination)?;
    quarantine_resolved(source)?.relocate_noreplace_to(&destination, cancelled, progress)
}

pub fn relocate_noreplace_matching(
    source: &Path,
    destination: &Path,
    expected: EntryIdentity,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(&Path),
) -> io::Result<()> {
    check_cancelled(cancelled)?;
    let source = resolved_parent(source)?;
    let destination = resolved_parent(destination)?;
    relocate_noreplace_matching_resolved(source, &destination, expected, cancelled, progress)
}

pub fn relocate_noreplace_matching_resolved(
    source: ResolvedParent,
    destination: &ResolvedParent,
    expected: EntryIdentity,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(&Path),
) -> io::Result<()> {
    check_cancelled(cancelled)?;
    let source_path = source.full_path();
    let quarantine = quarantine_resolved(source)?;
    if quarantine.stat.identity() != expected {
        return Err(quarantine_mismatch(&quarantine, &source_path));
    }
    quarantine.relocate_noreplace_to(destination, cancelled, progress)
}
