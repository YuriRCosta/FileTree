use super::sftp::Saved;
use crate::{AppError, AppResult, secure};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: u8,
    entries: Vec<Saved>,
}

pub fn read() -> AppResult<Vec<Saved>> {
    let path = crate::paths::config_dir().join("locations.json");
    let bytes = match secure::read_private_bounded(&path, 128 * 1024) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(Vec::new()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let document: Document = serde_json::from_slice(&bytes)?;
    if document.schema != 1 || document.entries.len() > 64 {
        return Err(AppError::invalid(
            "unsupported or oversized saved location document",
        ));
    }
    for entry in &document.entries {
        entry.uri()?;
    }
    Ok(document.entries)
}

pub fn remember(entry: &Saved) -> AppResult<()> {
    entry.uri()?;
    let path = crate::paths::config_dir().join("locations.json");
    let _lock = secure::open_private_lock(&path.with_extension("lock"))?;
    let mut entries = read()?;
    if entries.contains(entry) {
        return Ok(());
    }
    if entries.len() >= 64 {
        return Err(AppError::invalid("at most 64 SFTP locations can be saved"));
    }
    entries.push(entry.clone());
    let document = serde_json::to_vec(&Document { schema: 1, entries })?;
    if document.len() > 128 * 1024 {
        return Err(AppError::invalid("saved locations exceed 128 KiB"));
    }
    secure::write_private_atomic(&path, &document)?;
    Ok(())
}
