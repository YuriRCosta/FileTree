use super::*;

impl EntryStat {
    pub fn identity(self) -> EntryIdentity {
        EntryIdentity {
            dev: self.dev,
            ino: self.ino,
            kind: self.kind,
        }
    }
}

impl ResolvedParent {
    pub fn full_path(&self) -> PathBuf {
        self.path.join(&self.name)
    }
}

pub fn resolved_parent(path: &Path) -> io::Result<ResolvedParent> {
    resolved_parent_with(path, true)
}

pub fn resolved_child(
    directory: &OwnedFd,
    directory_path: &Path,
    name: &OsStr,
) -> io::Result<ResolvedParent> {
    if name.as_bytes().is_empty()
        || matches!(name.as_bytes(), b"." | b"..")
        || name.as_bytes().contains(&b'/')
        || name.as_bytes().contains(&0)
    {
        return Err(invalid_input("invalid file name"));
    }
    Ok(ResolvedParent {
        directory: fcntl_dupfd_cloexec(directory, 3).map_err(io::Error::from)?,
        name: name.to_os_string(),
        path: directory_path.to_path_buf(),
    })
}

pub(super) fn resolved_parent_nofollow(path: &Path) -> io::Result<ResolvedParent> {
    resolved_parent_with(path, true)
}

pub(super) fn resolved_parent_with(path: &Path, no_symlinks: bool) -> io::Result<ResolvedParent> {
    let normalized = normalized_absolute(path)?;
    let parent = normalized
        .parent()
        .ok_or_else(|| invalid_input(format!("{} has no parent", path.display())))?;
    let name = normalized
        .file_name()
        .ok_or_else(|| invalid_input(format!("{} has no file name", path.display())))?
        .to_os_string();
    if name.as_bytes().is_empty() || name.as_bytes().contains(&0) {
        return Err(invalid_input("invalid file name"));
    }
    Ok(ResolvedParent {
        directory: open_absolute_directory(parent, no_symlinks)?,
        name,
        path: parent.to_path_buf(),
    })
}

pub fn entry_stat(path: &Path) -> io::Result<EntryStat> {
    let parent = resolved_parent(path)?;
    entry_stat_resolved(&parent)
}

pub fn entry_stat_resolved(target: &ResolvedParent) -> io::Result<EntryStat> {
    stat_in(&target.directory, &target.name)
}

pub fn entry_exists(path: &Path) -> io::Result<bool> {
    let target = resolved_parent(path)?;
    entry_exists_resolved(&target)
}

pub fn entry_exists_resolved(target: &ResolvedParent) -> io::Result<bool> {
    match entry_stat_resolved(target) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn same_resolved_target(left: &ResolvedParent, right: &ResolvedParent) -> io::Result<bool> {
    if left.name != right.name {
        return Ok(false);
    }
    let left_parent = stat_value(fstat(&left.directory).map_err(io::Error::from)?);
    let right_parent = stat_value(fstat(&right.directory).map_err(io::Error::from)?);
    Ok(left_parent.identity() == right_parent.identity())
}

pub fn stat_in(parent: &OwnedFd, name: &OsStr) -> io::Result<EntryStat> {
    statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map(stat_value)
        .map_err(io::Error::from)
}

pub fn read_link(path: &Path) -> io::Result<PathBuf> {
    let parent = resolved_parent(path)?;
    let target =
        readlinkat(&parent.directory, &parent.name, Vec::new()).map_err(io::Error::from)?;
    Ok(PathBuf::from(OsString::from_vec(target.into_bytes())))
}

pub fn directory_names(path: &Path) -> io::Result<Vec<OsString>> {
    let fd = open_absolute_directory(path, false)?;
    directory_names_from(&fd)
}

pub fn directory_names_from(fd: &OwnedFd) -> io::Result<Vec<OsString>> {
    directory_names_bounded_from(fd, usize::MAX).map(|(names, _)| names)
}

pub fn directory_names_bounded(path: &Path, limit: usize) -> io::Result<(Vec<OsString>, bool)> {
    let fd = open_absolute_directory(path, false)?;
    directory_names_bounded_from(&fd, limit)
}

pub fn directory_names_bounded_from(
    fd: &OwnedFd,
    limit: usize,
) -> io::Result<(Vec<OsString>, bool)> {
    let (mut names, truncated) = directory_names_checked_from(fd, limit, || Ok(()))?;
    names.sort();
    Ok((names, truncated))
}

pub fn directory_names_bounded_cancellable(
    path: &Path,
    limit: usize,
    cancelled: &AtomicBool,
) -> io::Result<(Vec<OsString>, bool)> {
    let fd = open_absolute_directory(path, true)?;
    let (mut names, truncated) = directory_names_checked_from(&fd, limit, || {
        if cancelled.load(Ordering::Relaxed) {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "operation cancelled",
            ))
        } else {
            Ok(())
        }
    })?;
    if !truncated {
        names.sort();
    }
    Ok((names, truncated))
}

fn directory_names_checked_from(
    fd: &OwnedFd,
    limit: usize,
    mut check: impl FnMut() -> io::Result<()>,
) -> io::Result<(Vec<OsString>, bool)> {
    check()?;
    let mut directory = Dir::read_from(fd).map_err(io::Error::from)?;
    let mut names = Vec::new();
    let mut truncated = false;
    for entry in &mut directory {
        check()?;
        let entry = entry.map_err(io::Error::from)?;
        let bytes = entry.file_name().to_bytes();
        if bytes != b"." && bytes != b".." {
            if names.len() >= limit {
                truncated = true;
                break;
            }
            names.push(OsString::from_vec(bytes.to_vec()));
        }
    }
    check()?;
    Ok((names, truncated))
}

pub fn open_directory(path: &Path) -> io::Result<OwnedFd> {
    open_absolute_directory(path, false)
}

pub fn open_directory_entry(path: &Path) -> io::Result<OwnedFd> {
    let parent = resolved_parent(path)?;
    openat(
        &parent.directory,
        &parent.name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(io::Error::from)
}

pub fn open_directory_nofollow(path: &Path) -> io::Result<OwnedFd> {
    open_absolute_directory(path, true)
}

pub fn open_file_read(path: &Path) -> io::Result<File> {
    let parent = resolved_parent(path)?;
    let fd = openat(
        &parent.directory,
        &parent.name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let stat = stat_value(fstat(&fd).map_err(io::Error::from)?);
    if stat.kind != EntryKind::File {
        return Err(invalid_data(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    Ok(fd.into())
}

pub(super) fn open_absolute_directory(path: &Path, no_symlinks: bool) -> io::Result<OwnedFd> {
    let normalized = normalized_absolute(path)?;
    let root = root_directory()?;
    let relative = normalized.strip_prefix("/").unwrap_or(&normalized);
    let relative = if relative.as_os_str().is_empty() {
        Path::new(".")
    } else {
        relative
    };
    let resolve = ResolveFlags::IN_ROOT
        | ResolveFlags::NO_MAGICLINKS
        | if no_symlinks {
            ResolveFlags::NO_SYMLINKS
        } else {
            ResolveFlags::empty()
        };
    match openat2(
        &root,
        relative,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        resolve,
    ) {
        Ok(fd) => Ok(fd),
        Err(rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL) => {
            walk_directory_nofollow(root, &normalized)
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn walk_directory_nofollow(mut current: OwnedFd, path: &Path) -> io::Result<OwnedFd> {
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        current = openat(
            &current,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
    }
    Ok(current)
}

pub(super) fn root_directory() -> io::Result<OwnedFd> {
    openat(
        rustix::fs::CWD,
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(io::Error::from)
}

pub(super) fn normalized_absolute(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid_input(format!("{} is not absolute", path.display())));
    }
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => normalized.push(name),
            Component::ParentDir => {
                if normalized != Path::new("/") {
                    normalized.pop();
                }
            }
            Component::Prefix(_) => return Err(invalid_input("unsupported path prefix")),
        }
    }
    Ok(normalized)
}

pub(super) fn verify_owner_and_kind(fd: &impl AsFd, kind: EntryKind) -> io::Result<()> {
    let stat = stat_value(fstat(fd).map_err(io::Error::from)?);
    if stat.kind != kind {
        return Err(invalid_data("private state has the wrong file type"));
    }
    if stat.uid != geteuid().as_raw() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private state is owned by another user",
        ));
    }
    Ok(())
}

pub(super) fn stat_value(stat: Stat) -> EntryStat {
    let kind = match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => EntryKind::File,
        FileType::Directory => EntryKind::Directory,
        FileType::Symlink => EntryKind::Symlink,
        _ => EntryKind::Other,
    };
    EntryStat {
        dev: stat.st_dev,
        ino: stat.st_ino,
        mode: Mode::from_raw_mode(stat.st_mode).as_raw_mode(),
        uid: stat.st_uid,
        size: stat.st_size.max(0) as u64,
        atime: stat.st_atime,
        atime_nsec: stat.st_atime_nsec as i64,
        mtime: stat.st_mtime,
        mtime_nsec: stat.st_mtime_nsec as i64,
        ctime: stat.st_ctime,
        ctime_nsec: stat.st_ctime_nsec as i64,
        kind,
    }
}

pub fn directory_mount_id(directory: &OwnedFd) -> io::Result<u64> {
    use rustix::fs::{StatxFlags, statx};
    let stat = statx(directory, "", AtFlags::EMPTY_PATH, StatxFlags::MNT_ID)?;
    if !StatxFlags::from_bits_retain(stat.stx_mask).contains(StatxFlags::MNT_ID) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "mount identity is unavailable",
        ));
    }
    Ok(stat.stx_mnt_id)
}

pub fn directory_unique_mount_id(directory: &OwnedFd) -> io::Result<u64> {
    use rustix::fs::{StatxFlags, statx};
    let unique_mount_id = StatxFlags::from_bits_retain(0x00004000);
    let stat = statx(directory, "", AtFlags::EMPTY_PATH, unique_mount_id)?;
    if !StatxFlags::from_bits_retain(stat.stx_mask).contains(unique_mount_id) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unique mount identity is unavailable",
        ));
    }
    Ok(stat.stx_mnt_id)
}

pub fn open_directory_within_mount(root: &OwnedFd, relative: &Path) -> io::Result<OwnedFd> {
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(invalid_input("location path must remain within its root"));
    }
    openat2(
        root,
        if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        },
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
    )
    .map_err(io::Error::from)
}

#[cfg(test)]
mod enumeration_tests {
    use super::*;

    #[test]
    fn enumeration_stops_at_budget_and_checks_cancellation_during_read() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..10_000 {
            std::fs::write(root.path().join(index.to_string()), b"").unwrap();
        }
        let directory = open_directory_nofollow(root.path()).unwrap();
        for limit in [0, 1, 4095] {
            let mut checks = 0;
            let (names, truncated) = directory_names_checked_from(&directory, limit, || {
                checks += 1;
                Ok(())
            })
            .unwrap();
            assert_eq!(names.len(), limit);
            assert!(truncated);
            assert!(checks <= limit + 5, "{checks}");
        }
        let mut checks = 0;
        let error = directory_names_checked_from(&directory, 4095, || {
            checks += 1;
            if checks == 32 {
                Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(checks, 32);
        assert!(
            directory_names_bounded_cancellable(root.path(), 1, &AtomicBool::new(true)).is_err()
        );
    }
}
