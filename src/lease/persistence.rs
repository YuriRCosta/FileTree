use super::{Authority, WriteMode};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

static ACTIVE: Mutex<Weak<Authority>> = Mutex::new(Weak::new());

pub struct PersistenceSession {
    authority: Arc<Authority>,
}

impl PersistenceSession {
    pub fn open(authority: Arc<Authority>) -> io::Result<Self> {
        authority.verify().map_err(io::Error::other)?;
        let mut active = ACTIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.upgrade().is_some() {
            return Err(io::Error::other(
                "persistence authority is already registered",
            ));
        }
        *active = Arc::downgrade(&authority);
        Ok(Self { authority })
    }
}

impl Drop for PersistenceSession {
    fn drop(&mut self) {
        let mut active = ACTIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.ptr_eq(&Arc::downgrade(&self.authority)) {
            *active = Weak::new();
        }
    }
}

fn authority() -> io::Result<Option<Arc<Authority>>> {
    let authority = ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .upgrade();
    if authority.is_none() && std::env::var_os("FILEBLADE_NATIVE_STATE_ROOT").is_some() {
        return Err(io::Error::other(
            "owner-unavailable: native persistence authority is not registered",
        ));
    }
    Ok(authority)
}

pub fn storage_anchor(path: &Path) -> io::Result<Option<(File, PathBuf)>> {
    match authority()? {
        Some(authority) => authority.storage_anchor(path),
        None => Ok(None),
    }
}

pub fn check_write(path: &Path) -> io::Result<()> {
    if let Some(authority) = authority()? {
        authority.persistence_anchor(path)?;
    }
    Ok(())
}

pub fn write_mode() -> io::Result<WriteMode> {
    Ok(authority()?.map_or(WriteMode::Full, |authority| authority.write_mode()))
}
