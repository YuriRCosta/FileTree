use crate::lease::persistence;
use rustix::fs::{AtFlags, Mode, OFlags, fstat, fsync, openat, renameat, statat, unlinkat};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

pub fn write_private_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if !persistence::native()? {
        return crate::secure::write_private_atomic(path, data);
    }
    let parent = persistence::record_parent(path)?;
    match statat(&parent.directory, &parent.name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat)
            if stat.st_mode & libc::S_IFMT == libc::S_IFREG
                && stat.st_uid == unsafe { libc::geteuid() } => {}
        Ok(_) => {
            return Err(io::Error::other(
                "record is not a regular file owned by this user",
            ));
        }
        Err(rustix::io::Errno::NOENT) => {}
        Err(error) => return Err(error.into()),
    }
    let name = format!(".filetree-{}.tmp", uuid::Uuid::new_v4().simple());
    let fd = openat(
        &parent.directory,
        &name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )?;
    let result = (|| {
        persistence::check_write(path)?;
        let mut file = File::from(fd);
        file.write_all(data)?;
        file.sync_all()?;
        persistence::check_write(path)?;
        renameat(&parent.directory, &name, &parent.directory, &parent.name)?;
        fsync(&parent.directory).map_err(io::Error::from)
    })();
    if result.is_err() {
        let _ = unlinkat(&parent.directory, &name, AtFlags::empty());
    }
    result
}

pub fn write_new_private(path: &Path, data: &[u8]) -> io::Result<()> {
    if persistence::native()? {
        let _parent = persistence::record_parent(path)?;
    }
    crate::secure::write_new_private(path, data)
}

pub fn append(path: &Path, line: &str, cap: u64) -> io::Result<()> {
    let parent = persistence::record_parent(path)?;
    let _lock = crate::secure::open_record_lock(&path.with_extension("lock"))?;
    let mut fd = openat(
        &parent.directory,
        &parent.name,
        OFlags::WRONLY
            | OFlags::APPEND
            | OFlags::CREATE
            | OFlags::CLOEXEC
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK,
        Mode::from_raw_mode(0o600),
    )?;
    let stat = fstat(&fd)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG
        || stat.st_mode & 0o077 != 0
        || stat.st_uid != unsafe { libc::geteuid() }
    {
        return Err(io::Error::other(
            "audit record is not private regular storage",
        ));
    }
    if stat.st_size as u64 >= cap {
        persistence::check_write(path)?;
        let rotated = path.with_extension("1.jsonl");
        renameat(
            &parent.directory,
            &parent.name,
            &parent.directory,
            rotated.file_name().unwrap(),
        )?;
        fd = openat(
            &parent.directory,
            &parent.name,
            OFlags::WRONLY
                | OFlags::APPEND
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::CLOEXEC
                | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o600),
        )?;
    }
    persistence::check_write(path)?;
    let mut file = File::from(fd);
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    fsync(&parent.directory).map_err(io::Error::from)
}
