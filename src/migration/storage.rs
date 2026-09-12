use rustix::fs::{AtFlags, Mode, OFlags, openat};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

pub const MAX_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Content {
    Directory,
    File(Vec<u8>),
    Link(Vec<u8>),
}

pub fn directory(root: &File, path: &Path, create: bool) -> io::Result<File> {
    let mut current = root.try_clone()?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(io::Error::other("invalid migration relative path"));
        };
        if create {
            match rustix::fs::mkdirat(&current, name, Mode::from_raw_mode(0o700)) {
                Ok(()) => current.sync_all()?,
                Err(rustix::io::Errno::EXIST) => {}
                Err(error) => return Err(error.into()),
            }
        }
        current = File::from(openat(
            &current,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        safe(&current)?;
    }
    Ok(current)
}

pub fn root(path: &Path) -> io::Result<File> {
    let relative = path.strip_prefix("/").map_err(io::Error::other)?;
    let base = File::open("/")?;
    let mut current = base;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(io::Error::other("invalid migration root"));
        };
        current = File::from(openat(
            &current,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
    }
    safe(&current)?;
    Ok(current)
}

pub fn safe(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
        return Err(io::Error::other(
            "unsafe migration storage ownership or permissions",
        ));
    }
    Ok(())
}

pub fn names(root: &File) -> io::Result<Vec<OsString>> {
    let fd = rustix::io::dup(root)?;
    let (mut names, truncated) = crate::secure::directory_names_bounded_from(&fd, 10_001)?;
    if truncated || names.len() > 10_000 {
        return Err(io::Error::other("migration directory exceeds entry bound"));
    }
    names.sort();
    Ok(names)
}

pub fn read(root: &File, path: &Path) -> io::Result<Option<Content>> {
    let parent = match directory(root, path.parent().unwrap_or(Path::new("")), false) {
        Ok(parent) => parent,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing migration filename"))?;
    let stat = match rustix::fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Directory {
        directory(root, path, false)?;
        return Ok(Some(Content::Directory));
    }
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Symlink {
        return Ok(Some(Content::Link(
            rustix::fs::readlinkat(&parent, name, Vec::new())?.into_bytes(),
        )));
    }
    let file = File::from(openat(
        &parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    safe(&file)?;
    let before = file.metadata()?;
    if !before.is_file() || before.nlink() != 1 || before.len() > MAX_BYTES as u64 {
        return Err(io::Error::other("unsupported migration file or size"));
    }
    let mut bytes = Vec::new();
    (&file).take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() > MAX_BYTES
        || (
            before.dev(),
            before.ino(),
            before.len(),
            before.mtime_nsec(),
            before.ctime_nsec(),
        ) != (
            after.dev(),
            after.ino(),
            after.len(),
            after.mtime_nsec(),
            after.ctime_nsec(),
        )
    {
        return Err(io::Error::other("migration source changed while reading"));
    }
    Ok(Some(Content::File(bytes)))
}

pub fn publish(root: &File, path: &Path, content: &Content) -> io::Result<()> {
    if let Some(existing) = read(root, path)? {
        return if existing == *content {
            Ok(())
        } else {
            Err(io::Error::other("migration destination conflict"))
        };
    }
    if *content == Content::Directory {
        directory(root, path, true)?.sync_all()?;
        return Ok(());
    }
    let parent = directory(root, path.parent().unwrap_or(Path::new("")), true)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing migration filename"))?;
    let temporary = format!(".migration-{}", uuid::Uuid::new_v4());
    match content {
        Content::Directory => unreachable!(),
        Content::File(bytes) => {
            let mut file = File::from(openat(
                &parent,
                temporary.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Content::Link(bytes) => {
            use std::os::unix::ffi::OsStringExt;
            rustix::fs::symlinkat(
                OsString::from_vec(bytes.clone()),
                &parent,
                temporary.as_str(),
            )?;
        }
    }
    let result = rustix::fs::renameat_with(
        &parent,
        temporary.as_str(),
        &parent,
        name,
        rustix::fs::RenameFlags::NOREPLACE,
    );
    if let Err(error) = result {
        let _ = rustix::fs::unlinkat(&parent, temporary.as_str(), AtFlags::empty());
        return Err(error.into());
    }
    parent.sync_all()
}

pub fn remove(root: &File, path: &Path, expected: &Content) -> io::Result<()> {
    let Some(current) = read(root, path)? else {
        return Ok(());
    };
    if current != *expected {
        return Err(io::Error::other(
            "legacy artifact changed before migration removal",
        ));
    }
    let parent = directory(root, path.parent().unwrap_or(Path::new("")), false)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing migration filename"))?;
    rustix::fs::unlinkat(
        &parent,
        name,
        if *expected == Content::Directory {
            AtFlags::REMOVEDIR
        } else {
            AtFlags::empty()
        },
    )?;
    parent.sync_all()
}
