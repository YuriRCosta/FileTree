use rustix::fd::{AsFd, OwnedFd};
use rustix::fs::{
    AtFlags, Dir, FileType, FlockOperation, Mode, OFlags, RenameFlags, ResolveFlags, Stat,
    Timespec, Timestamps, fchmod, flock, fstat, fsync, mkdirat, openat, openat2, readlinkat,
    renameat, renameat_with, statat, symlinkat, unlinkat,
};
use rustix::io::fcntl_dupfd_cloexec;
use rustix::process::geteuid;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;
use xattr::FileExt;

mod config_write;
mod copy;
mod intent;
mod permissions;
mod private;
mod quarantine;
mod remove;
mod resolve;
pub use config_write::*;
pub use copy::*;
pub use intent::*;
pub use permissions::*;
pub use private::*;
pub use quarantine::*;
pub use remove::*;
pub use resolve::*;
pub const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
pub const PRIVATE_FILE_MODE: u32 = 0o600;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryStat {
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub uid: u32,
    pub size: u64,
    pub atime: i64,
    pub atime_nsec: i64,
    pub mtime: i64,
    pub mtime_nsec: i64,
    pub ctime: i64,
    pub ctime_nsec: i64,
    pub kind: EntryKind,
}

pub struct ResolvedParent {
    pub directory: OwnedFd,
    pub name: OsString,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryIdentity {
    pub dev: u64,
    pub ino: u64,
    pub kind: EntryKind,
}

pub struct QuarantinedEntry {
    parent_directory: OwnedFd,
    staging_directory: OwnedFd,
    original_name: OsString,
    staging_name: OsString,
    entry_name: OsString,
    parent_path: PathBuf,
    staging_identity: EntryIdentity,
    stat: EntryStat,
    intent: RecoveryIntent,
}

pub struct LockedFile {
    file: File,
}

fn check_cancelled(cancelled: &AtomicBool) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "operation cancelled",
        ))
    } else {
        Ok(())
    }
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
