use super::*;

pub struct RecoveryIntent {
    path: PathBuf,
    parent: ResolvedParent,
    _lease: LockedFile,
}

impl RecoveryIntent {
    pub fn clear(&self) -> io::Result<()> {
        crate::lease::persistence::check_write(&self.path)?;
        unlinkat(&self.parent.directory, &self.parent.name, AtFlags::empty())?;
        fsync(&self.parent.directory).map_err(io::Error::from)
    }
}

pub fn write_intent(record: &serde_json::Value) -> io::Result<RecoveryIntent> {
    let directory = crate::recovery::inflight_dir();
    let path = directory.join(format!("{}.json", Uuid::new_v4().simple()));
    let temporary = path.with_extension("tmp");
    let parent = crate::lease::persistence::record_parent(&path)?;
    let temporary = resolved_child(
        &parent.directory,
        &parent.path,
        temporary.file_name().unwrap(),
    )?;
    let mut file = create_file_noreplace_resolved(&temporary, PRIVATE_FILE_MODE)?;
    let result = (|| {
        flock(&file, FlockOperation::LockExclusive).map_err(io::Error::from)?;
        file.write_all(record.to_string().as_bytes())?;
        file.sync_all()?;
        crate::lease::persistence::check_write(&path)?;
        renameat_with(
            &parent.directory,
            &temporary.name,
            &parent.directory,
            &parent.name,
            RenameFlags::NOREPLACE,
        )?;
        fsync(&parent.directory).map_err(io::Error::from)
    })();
    if let Err(error) = result {
        let _ = unlinkat(&parent.directory, &temporary.name, AtFlags::empty());
        let _ = unlinkat(&parent.directory, &parent.name, AtFlags::empty());
        return Err(error);
    }
    Ok(RecoveryIntent {
        path,
        parent,
        _lease: LockedFile { file },
    })
}

pub fn try_lock_private(path: &Path) -> io::Result<Option<LockedFile>> {
    let parent = match resolved_parent_nofollow(path) {
        Ok(parent) => parent,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let fd = match openat(
        &parent.directory,
        &parent.name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    verify_private_fd(&fd, EntryKind::File, PRIVATE_FILE_MODE)?;
    match flock(&fd, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(Some(LockedFile { file: fd.into() })),
        Err(rustix::io::Errno::WOULDBLOCK) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
