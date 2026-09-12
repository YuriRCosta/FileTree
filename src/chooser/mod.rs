pub mod portal;
pub mod transport;

use crate::{AppError, AppResult, filesystem};
use rustix::fs::{Access, AtFlags, CWD, accessat};
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use url::Url;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Open,
    Save,
    Folder,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Filter(pub String, pub Vec<(u32, String)>);

impl Filter {
    pub fn allows(&self, name: &str, mime: &str) -> bool {
        self.1.iter().any(|(kind, pattern)| match kind {
            0 => CString::new(pattern.as_str())
                .ok()
                .zip(CString::new(name).ok())
                .is_some_and(|(pattern, name)| unsafe {
                    libc::fnmatch(pattern.as_ptr(), name.as_ptr(), 0) == 0
                }),
            1 => {
                pattern == mime
                    || pattern.strip_suffix("/*").is_some_and(|prefix| {
                        mime.split_once('/')
                            .is_some_and(|(family, _)| family == prefix)
                    })
            }
            _ => false,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Offer {
    pub handle: String,
    pub caller: String,
    pub parent_window: String,
    pub title: String,
    pub accept_label: String,
    pub modal: bool,
    pub current_folder: Option<PathBuf>,
    pub current_name: String,
    pub mode: Mode,
    pub multiple: bool,
    pub filters: Vec<Filter>,
    pub current_filter: Option<Filter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Accepted {
        uris: Vec<String>,
        filter: Option<Filter>,
    },
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Decision {
    Overwrite { path: PathBuf },
    Finished { outcome: Outcome },
}

pub struct Request {
    offer: Offer,
    overwrite: Option<(PathBuf, u64, u64, i64, i64)>,
    outcome: Option<Outcome>,
}

impl Request {
    pub fn new(offer: Offer) -> AppResult<Self> {
        if offer.handle.is_empty() || offer.caller.is_empty() {
            return Err(AppError::invalid("Chooser handle and caller are required"));
        }
        for filter in offer.filters.iter().chain(offer.current_filter.iter()) {
            if filter.1.is_empty()
                || filter
                    .1
                    .iter()
                    .any(|(kind, value)| *kind > 1 || value.is_empty() || value.contains('\0'))
            {
                return Err(AppError::invalid("Invalid chooser filter"));
            }
        }
        Ok(Self {
            offer,
            overwrite: None,
            outcome: None,
        })
    }

    pub fn offer(&self) -> &Offer {
        &self.offer
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    pub fn cancel(&mut self) -> bool {
        if self.outcome.is_some() {
            return false;
        }
        self.overwrite = None;
        self.outcome = Some(Outcome::Cancelled);
        true
    }

    pub fn choose(
        &mut self,
        paths: &[PathBuf],
        filter: Option<&Filter>,
        overwrite: bool,
    ) -> AppResult<Decision> {
        self.choose_cancellable(paths, filter, overwrite, &AtomicBool::new(false))
    }

    pub fn choose_cancellable(
        &mut self,
        paths: &[PathBuf],
        filter: Option<&Filter>,
        overwrite: bool,
        cancelled: &AtomicBool,
    ) -> AppResult<Decision> {
        if self.outcome.is_some() {
            return Err(AppError::invalid("Chooser request has finished"));
        }
        let approval = self.overwrite.take();
        if paths.is_empty()
            || (paths.len() != 1 && (!self.offer.multiple || self.offer.mode == Mode::Save))
        {
            return Err(AppError::invalid("Invalid chooser selection count"));
        }
        let selected_filter = filter
            .or(self.offer.current_filter.as_ref())
            .or(self.offer.filters.first());
        if selected_filter.is_some_and(|value| {
            !self.offer.filters.contains(value) && self.offer.current_filter.as_ref() != Some(value)
        }) {
            return Err(AppError::invalid("Filter was not offered by the caller"));
        }
        let mut uris = Vec::new();
        for path in paths {
            if cancelled.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            if !path.is_absolute()
                || path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return Err(AppError::invalid("Choose an absolute local path"));
            }
            let path = if self.offer.mode == Mode::Save {
                let text = path
                    .to_str()
                    .ok_or_else(|| AppError::invalid("Unsupported filename encoding"))?;
                let checked = filesystem::validate_save_target(text);
                if checked["ok"] != true {
                    return Err(AppError::invalid(
                        checked["error"]
                            .as_str()
                            .unwrap_or("Invalid save destination"),
                    ));
                }
                let name = path
                    .file_name()
                    .ok_or_else(|| AppError::invalid("Enter a file name"))?;
                path.parent()
                    .ok_or_else(|| AppError::invalid("Choose a destination folder"))?
                    .canonicalize()?
                    .join(name)
            } else {
                path.canonicalize()?
            };
            if self.offer.mode != Mode::Folder
                && selected_filter.is_some_and(|value| {
                    !value.allows(
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or_default(),
                        &filesystem::content_type_cancellable(&path.to_string_lossy(), cancelled),
                    )
                })
            {
                return Err(AppError::invalid(
                    "Selection does not match the caller's filter",
                ));
            }
            if self.offer.mode == Mode::Save {
                match fs::symlink_metadata(&path) {
                    Ok(metadata) => {
                        if !metadata.is_file() {
                            return Err(AppError::invalid("Save target must be a regular file"));
                        }
                        let identity = (
                            path.clone(),
                            metadata.dev(),
                            metadata.ino(),
                            metadata.ctime(),
                            metadata.ctime_nsec(),
                        );
                        if !overwrite || approval.as_ref() != Some(&identity) {
                            self.overwrite = Some(identity);
                            return Ok(Decision::Overwrite { path });
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            } else {
                let metadata = fs::metadata(&path)?;
                let valid = if self.offer.mode == Mode::Folder {
                    metadata.is_dir()
                } else {
                    metadata.is_file()
                };
                if !valid {
                    return Err(AppError::invalid("Selection has the wrong file type"));
                }
                let access = if self.offer.mode == Mode::Folder {
                    Access::READ_OK | Access::EXEC_OK
                } else {
                    Access::READ_OK
                };
                accessat(CWD, &path, access, AtFlags::EACCESS).map_err(std::io::Error::from)?;
            }
            let uri = Url::from_file_path(&path)
                .map_err(|()| AppError::invalid("Cannot return a local file URI"))?
                .to_string();
            if !uris.contains(&uri) {
                uris.push(uri);
            }
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let outcome = Outcome::Accepted {
            uris,
            filter: selected_filter.cloned(),
        };
        self.outcome = Some(outcome.clone());
        Ok(Decision::Finished { outcome })
    }
}
