use crate::command::{CommandSpec, owner::OwnerHandle, which};
use crate::{AppError, AppResult};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

struct Slot {
    owner: Option<OwnerHandle>,
}

static CLIPBOARD: Mutex<Option<Slot>> = Mutex::new(None);

pub struct Session {
    _private: (),
}

impl Session {
    pub fn open() -> AppResult<Self> {
        let mut slot = CLIPBOARD
            .lock()
            .map_err(|_| AppError::command("clipboard slot unavailable"))?;
        if slot.is_some() {
            return Err(AppError::command("clipboard session already active"));
        }
        *slot = Some(Slot { owner: None });
        Ok(Self { _private: () })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let mut slot = CLIPBOARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = None;
    }
}

pub fn write(mime: &str, input: Vec<u8>, cancelled: &AtomicBool) -> AppResult<()> {
    let mut resident = CLIPBOARD
        .lock()
        .map_err(|_| AppError::command("clipboard slot unavailable"))?;
    let slot = resident.as_mut().ok_or_else(|| {
        AppError::command("clipboard writes require the resident FileTree server")
    })?;
    let program = which("wl-copy").ok_or_else(|| AppError::command("wl-copy is not installed"))?;
    let owner = CommandSpec::new(program)
        .args(["--foreground", "--type", mime])
        .timeout(Duration::from_secs(3))
        .limits(64 * 1024, 64 * 1024)
        .spawn_owner_cancellable(input, cancelled)?;
    slot.owner = Some(owner);
    Ok(())
}
