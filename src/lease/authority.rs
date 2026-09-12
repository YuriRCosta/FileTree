use super::storage::Storage;
use super::{File, LeaseError, Path, PathBuf, io};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum WriteMode {
    Full,
    ReadOnly { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RootIdentity {
    pub path: PathBuf,
    pub device: u64,
    pub inode: u64,
}

#[derive(Debug)]
pub struct Authority {
    storage: Vec<Storage>,
    roots: BTreeMap<String, RootIdentity>,
    selected: BTreeMap<String, PathBuf>,
    lost: AtomicBool,
    mode: OnceLock<WriteMode>,
}

impl Authority {
    pub fn acquire(state_root: impl AsRef<Path>) -> Result<Self, LeaseError> {
        let selected = BTreeMap::from([("state".into(), state_root.as_ref().to_path_buf())]);
        let state = Storage::acquire(state_root)?;
        let (device, inode) = state.identity()?;
        let roots = BTreeMap::from([(
            "state".into(),
            RootIdentity {
                path: state.root().to_path_buf(),
                device,
                inode,
            },
        )]);
        Ok(Self {
            storage: vec![state],
            roots,
            selected,
            lost: AtomicBool::new(false),
            mode: OnceLock::new(),
        })
    }

    pub fn acquire_bound(
        state: impl AsRef<Path>,
        config: impl AsRef<Path>,
        recovery: impl AsRef<Path>,
    ) -> Result<Self, LeaseError> {
        let mut authority = Self::acquire(state)?;
        for (role, path) in [("config", config.as_ref()), ("recovery", recovery.as_ref())] {
            authority.selected.insert(role.into(), path.to_path_buf());
            let canonical = std::fs::canonicalize(path).ok();
            let storage = if let Some(existing) = authority
                .storage
                .iter()
                .find(|storage| canonical.as_deref() == Some(storage.root()))
            {
                existing
            } else {
                authority.storage.push(Storage::acquire(path)?);
                authority.storage.last().unwrap()
            };
            let (device, inode) = storage.identity()?;
            authority.roots.insert(
                role.into(),
                RootIdentity {
                    path: storage.root().to_path_buf(),
                    device,
                    inode,
                },
            );
        }
        authority.storage[0]
            .record(&serde_json::to_value(&authority.roots).map_err(io::Error::other)?)?;
        authority.verify()?;
        Ok(authority)
    }

    pub fn root(&self) -> &Path {
        self.storage[0].root()
    }

    pub fn identity(&self) -> io::Result<(u64, u64)> {
        self.storage[0].identity()
    }

    pub fn roots(&self) -> &BTreeMap<String, RootIdentity> {
        &self.roots
    }

    pub fn verify(&self) -> Result<(), LeaseError> {
        if self.lost.load(Ordering::Acquire) {
            return Err(LeaseError::Unsafe("authority-lost".into()));
        }
        for storage in &self.storage {
            if let Err(error) = storage.verify() {
                self.lost.store(true, Ordering::Release);
                return Err(LeaseError::Unsafe(format!("authority-lost: {error}")));
            }
        }
        for (role, selected) in &self.selected {
            if std::fs::canonicalize(selected).ok().as_deref()
                != Some(self.roots[role].path.as_path())
            {
                self.lost.store(true, Ordering::Release);
                return Err(LeaseError::Unsafe(
                    "authority-lost: selected root changed".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn set_write_mode(&self, mode: WriteMode) -> Result<(), LeaseError> {
        self.verify()?;
        self.mode
            .set(mode)
            .map_err(|_| LeaseError::Unsafe("migration write mode is already set".into()))
    }

    pub fn write_mode(&self) -> WriteMode {
        self.mode
            .get()
            .cloned()
            .unwrap_or_else(|| WriteMode::ReadOnly {
                reason: "migration has not been prepared".into(),
            })
    }

    pub fn persistence_anchor(&self, path: &Path) -> io::Result<Option<(File, PathBuf)>> {
        let anchor = self.storage_anchor(path)?;
        if let WriteMode::ReadOnly { reason } = self.write_mode() {
            return Err(io::Error::other(format!("migration-refused: {reason}")));
        }
        Ok(anchor)
    }

    pub fn storage_anchor(&self, path: &Path) -> io::Result<Option<(File, PathBuf)>> {
        self.verify().map_err(io::Error::other)?;
        if !path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(io::Error::other(
                "persistence path must be absolute without parent traversal",
            ));
        }
        let matched = self
            .roots
            .iter()
            .flat_map(|(role, identity)| {
                let storage = self
                    .storage
                    .iter()
                    .find(|storage| storage.root() == identity.path)
                    .unwrap();
                [identity.path.as_path(), self.selected[role].as_path()]
                    .into_iter()
                    .filter_map(move |root| {
                        path.strip_prefix(root)
                            .ok()
                            .map(|relative| (storage, root, relative))
                    })
            })
            .max_by_key(|(_, root, _)| root.components().count());
        let Some((storage, _, relative)) = matched else {
            return Ok(None);
        };
        Ok(Some((storage.duplicate()?, relative.to_path_buf())))
    }
}
