use rustix::fs::{AtFlags, Mode, OFlags, openat};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::os::fd::AsRawFd;
use xattr::FileExt;
pub type Attributes = BTreeMap<String, Vec<u8>>;
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
    publish_snapshot(root, path, content, &Attributes::new())
}

pub fn publish_snapshot(
    root: &File,
    path: &Path,
    content: &Content,
    attrs: &Attributes,
) -> io::Result<()> {
    if let Some(existing) = read(root, path)? {
        return if existing == *content && attributes(root, path)? == *attrs {
            Ok(())
        } else {
            Err(io::Error::other("migration destination conflict"))
        };
    }
    let parent = directory(root, path.parent().unwrap_or(Path::new("")), true)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing migration filename"))?;
    let temporary = format!(".migration-{}", uuid::Uuid::new_v4());
    let created = (|| -> io::Result<()> {
        match content {
            Content::Directory => {
                rustix::fs::mkdirat(&parent, temporary.as_str(), Mode::from_raw_mode(0o700))?;
                let file = directory(&parent, Path::new(&temporary), false)?;
                set_attributes(&file, attrs)?;
                file.sync_all()?;
            }
            Content::File(bytes) => {
                let mut file = File::from(openat(
                    &parent,
                    temporary.as_str(),
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(0o600),
                )?);
                file.write_all(bytes)?;
                set_attributes(&file, attrs)?;
                file.sync_all()?;
            }
            Content::Link(bytes) => {
                if !attrs.is_empty() {
                    return Err(io::Error::other(
                        "symbolic-link attributes cannot be preserved",
                    ));
                }
                use std::os::unix::ffi::OsStringExt;
                rustix::fs::symlinkat(
                    OsString::from_vec(bytes.clone()),
                    &parent,
                    temporary.as_str(),
                )?;
            }
        }
        Ok(())
    })();
    if let Err(error) = created {
        let _ = rustix::fs::unlinkat(
            &parent,
            temporary.as_str(),
            if *content == Content::Directory {
                AtFlags::REMOVEDIR
            } else {
                AtFlags::empty()
            },
        );
        return Err(error);
    }
    let result = rustix::fs::renameat_with(
        &parent,
        temporary.as_str(),
        &parent,
        name,
        rustix::fs::RenameFlags::NOREPLACE,
    );
    if let Err(error) = result {
        let _ = rustix::fs::unlinkat(
            &parent,
            temporary.as_str(),
            if *content == Content::Directory {
                AtFlags::REMOVEDIR
            } else {
                AtFlags::empty()
            },
        );
        return Err(error.into());
    }
    parent.sync_all()
}

pub fn attributes(root: &File, path: &Path) -> io::Result<Attributes> {
    let parent = directory(root, path.parent().unwrap_or(Path::new("")), false)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing attribute filename"))?;
    let stat = rustix::fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Symlink {
        let path =
            std::path::PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd())).join(name);
        if xattr::list(&path)?.next().is_some() {
            return Err(io::Error::other(
                "symbolic-link attributes cannot be preserved",
            ));
        }
        return Ok(Attributes::new());
    }
    let file = File::from(openat(
        &parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let mut result = Attributes::new();
    let mut total = 0usize;
    for name in file.list_xattr()? {
        let name = name
            .into_string()
            .map_err(|_| io::Error::other("non-UTF-8 attribute name cannot be preserved"))?;
        if !name.starts_with("user.") {
            return Err(io::Error::other(format!(
                "attribute {name} cannot be preserved under private storage permissions"
            )));
        }
        let value = file
            .get_xattr(&name)?
            .ok_or_else(|| io::Error::other("attribute disappeared during migration"))?;
        total = total.saturating_add(name.len()).saturating_add(value.len());
        if total > 256 * 1024 || result.len() >= 256 {
            return Err(io::Error::other("migration attributes exceed bound"));
        }
        result.insert(name, value);
    }
    Ok(result)
}

fn set_attributes(file: &File, attrs: &Attributes) -> io::Result<()> {
    for (name, value) in attrs {
        if !name.starts_with("user.") || name.contains('\0') {
            return Err(io::Error::other("unsupported migration attribute"));
        }
        file.set_xattr(name, value)?;
    }
    Ok(())
}

pub fn retire(
    root: &File,
    path: &Path,
    index: usize,
    expected: &Content,
    attrs: &Attributes,
) -> io::Result<()> {
    let staged = std::path::PathBuf::from(format!(".migration-020-retired/{index}"));
    let original = read(root, path)?;
    if let Some(content) = read(root, &staged)? {
        if original.is_some()
            || content != *expected
            || attributes(root, &staged)? != *attrs
            || content == Content::Directory
                && !names(&directory(root, &staged, false)?)?.is_empty()
        {
            return Err(io::Error::other(
                "retired migration object conflicts; both locations preserved",
            ));
        }
        return Ok(());
    }
    let Some(original) = original else {
        return Err(io::Error::other(
            "original and retired artifact both missing",
        ));
    };
    if original != *expected || attributes(root, path)? != *attrs {
        return Err(io::Error::other(
            "legacy artifact changed before retirement",
        ));
    }
    let parent = directory(root, path.parent().unwrap_or(Path::new("")), false)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing retirement filename"))?;
    let destination = directory(root, Path::new(".migration-020-retired"), true)?;
    let target = index.to_string();
    rustix::fs::renameat_with(
        &parent,
        name,
        &destination,
        target.as_str(),
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    parent.sync_all()?;
    destination.sync_all()?;
    let valid = read(&destination, Path::new(&target))?.as_ref() == Some(expected)
        && attributes(&destination, Path::new(&target))? == *attrs
        && (*expected != Content::Directory
            || names(&directory(&destination, Path::new(&target), false)?)?.is_empty());
    if !valid {
        let _ = rustix::fs::renameat_with(
            &destination,
            target.as_str(),
            &parent,
            name,
            rustix::fs::RenameFlags::NOREPLACE,
        );
        parent.sync_all()?;
        destination.sync_all()?;
        return Err(io::Error::other(
            "artifact changed during retirement; original or retired object retained",
        ));
    }
    Ok(())
}
