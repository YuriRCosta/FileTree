mod listing;
pub mod saved;
pub mod sftp;
pub mod tailnet;
pub use listing::list;

use crate::common::{parse_path, path_text};
use crate::mounts::{Volume, mountinfo::MountTable};
use crate::secure::{self, EntryIdentity};
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Local,
    Usb,
    Mtp,
    Sftp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Connection {
    Connected,
    Disconnected,
    Locked,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    List,
    Read,
    Write,
    Mkdir,
    Rename,
    AtomicRename,
    Trash,
    Permissions,
    Mount,
    Unmount,
    Eject,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalRepresentation {
    pub path: String,
    pub device: u64,
    pub inode: u64,
    pub mount_id: u64,
    pub filesystem: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Descriptor {
    pub schema: u8,
    pub id: String,
    pub kind: Kind,
    pub canonical_uri: String,
    pub label: String,
    pub connection: Connection,
    pub session_generation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_representation: Option<LocalRepresentation>,
    pub capabilities: BTreeSet<Capability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Proof {
    path: PathBuf,
    identity: EntryIdentity,
    mount_id: u64,
    unique_mount_id: u64,
}

#[derive(Clone)]
struct Session {
    proof: Proof,
    generation: String,
    capabilities: BTreeSet<Capability>,
}

static SESSIONS: Mutex<Option<HashMap<String, Session>>> = Mutex::new(None);

pub fn payload(cancelled: &AtomicBool) -> AppResult<Value> {
    let table = MountTable::read()?;
    let volumes = crate::mounts::list_with(&table);
    let mut locations = Vec::new();
    for (path, label) in [
        (crate::common::expanded_path("~"), "Home"),
        (PathBuf::from("/"), "Computer"),
    ] {
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let id = format!("local:{}", path_text(&path));
        locations.push(local(&id, Kind::Local, &path_text(&path), label, &table)?);
    }
    for volume in &volumes {
        if cancelled.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        locations.push(volume_descriptor(volume, &table)?);
    }
    let ids: BTreeSet<_> = locations
        .iter()
        .map(|location| location.id.as_str())
        .collect();
    if let Ok(mut sessions) = SESSIONS.lock()
        && let Some(sessions) = sessions.as_mut()
    {
        sessions.retain(|id, _| ids.contains(id.as_str()));
    }
    let tailnet = match tailnet::discover(cancelled) {
        Ok(candidates) => {
            sftp::retain_candidates(&candidates);
            locations.extend(candidates.iter().map(|candidate| {
                sftp::snapshot(&candidate.location.id).unwrap_or_else(|| candidate.location.clone())
            }));
            json!({"available":true,"candidates":candidates})
        }
        Err(error) => {
            sftp::retain_candidates(&[]);
            json!({"available":false,"candidates":[],"error":error.to_string()})
        }
    };
    if cancelled.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    for cleanup in sftp::cleanup_locations() {
        if !locations.iter().any(|location| location.id == cleanup.id) {
            locations.push(cleanup);
        }
    }
    let (saved, saved_error) = match saved::read() {
        Ok(entries) => (entries, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    Ok(
        json!({"ok":true,"schema":1,"locations":locations,"tailnet":tailnet,"saved":saved,"saved_error":saved_error,
        "volumes":volumes.iter().map(Volume::json).collect::<Vec<_>>(),"actions":crate::mounts::actions::available()}),
    )
}

pub fn local(
    id: &str,
    kind: Kind,
    raw_path: &str,
    label: &str,
    table: &MountTable,
) -> AppResult<Descriptor> {
    if !matches!(kind, Kind::Local | Kind::Usb) {
        return Err(AppError::invalid(
            "remote capabilities require a provider validation",
        ));
    }
    if !raw_path.starts_with('/')
        && raw_path.contains("://")
        && !raw_path
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
    {
        return Err(AppError::invalid(
            "a remote URI is not a local representation",
        ));
    }
    let path = parse_path(raw_path)?;
    let uri = url::Url::from_file_path(&path)
        .map_err(|_| AppError::invalid("invalid local location"))?
        .to_string();
    let mut descriptor = disconnected(id, kind, &uri, label, Connection::Unavailable)?;
    let Some(mount) = table
        .records()
        .iter()
        .filter(|record| path.starts_with(&record.mountpoint))
        .max_by_key(|record| record.mountpoint.components().count())
    else {
        invalidate(id);
        descriptor.error = Some("location has no validated mount".into());
        return Ok(descriptor);
    };
    if mount.filesystem.starts_with("fuse.")
        || matches!(
            mount.filesystem.as_str(),
            "fuse" | "nfs" | "nfs4" | "cifs" | "smb3"
        )
    {
        invalidate(id);
        descriptor.error =
            Some("this mounted provider needs explicit capability validation".into());
        return Ok(descriptor);
    }
    let directory = match secure::open_directory_nofollow(&path) {
        Ok(directory) => directory,
        Err(error) => {
            invalidate(id);
            descriptor.error = Some(error.to_string());
            return Ok(descriptor);
        }
    };
    let identity = secure::stat_in(&directory, OsStr::new("."))?.identity();
    let mount_id = secure::directory_mount_id(&directory)?;
    let observed = MountTable::read()?;
    let current = observed
        .records()
        .iter()
        .find(|record| record.mount_id == mount_id && path.starts_with(&record.mountpoint));
    let Some(current) = current.filter(|current| *current == mount) else {
        invalidate(id);
        descriptor.error =
            Some("location mount changed during validation; refresh Locations".into());
        return Ok(descriptor);
    };
    let mount = current;
    let proof = Proof {
        path: path.clone(),
        identity,
        mount_id: mount.mount_id,
        unique_mount_id: secure::directory_unique_mount_id(&directory)?,
    };
    let still_selected = secure::open_directory_nofollow(&path).is_ok_and(|selected| {
        secure::stat_in(&selected, OsStr::new(".")).is_ok_and(|stat| stat.identity() == identity)
            && secure::directory_unique_mount_id(&selected)
                .is_ok_and(|mount| mount == proof.unique_mount_id)
    });
    if !still_selected {
        invalidate(id);
        descriptor.error = Some("location changed during validation; refresh Locations".into());
        return Ok(descriptor);
    }
    descriptor.connection = Connection::Connected;
    descriptor.local_representation = Some(LocalRepresentation {
        path: path_text(&path),
        device: identity.dev,
        inode: identity.ino,
        mount_id: mount.mount_id,
        filesystem: mount.filesystem.clone(),
    });
    use rustix::fs::{Access, AtFlags, accessat};
    if accessat(
        &directory,
        ".",
        Access::READ_OK | Access::EXEC_OK,
        AtFlags::EACCESS,
    )
    .is_ok()
    {
        descriptor
            .capabilities
            .extend([Capability::List, Capability::Read]);
    }
    if !mount.read_only
        && accessat(
            &directory,
            ".",
            Access::WRITE_OK | Access::EXEC_OK,
            AtFlags::EACCESS,
        )
        .is_ok()
    {
        descriptor.capabilities.extend([
            Capability::Write,
            Capability::Mkdir,
            Capability::Rename,
            Capability::AtomicRename,
        ]);
        if crate::command::which("gio").is_some() {
            descriptor.capabilities.insert(Capability::Trash);
        }
        if matches!(
            mount.filesystem.as_str(),
            "ext2" | "ext3" | "ext4" | "btrfs" | "xfs" | "tmpfs" | "overlay" | "f2fs" | "zfs"
        ) {
            descriptor.capabilities.insert(Capability::Permissions);
        }
    }
    {
        let mut sessions = SESSIONS
            .lock()
            .map_err(|_| AppError::command("location sessions unavailable"))?;
        let sessions = sessions.get_or_insert_with(HashMap::new);
        if sessions.len() >= 1024 && !sessions.contains_key(id) {
            return Err(AppError::command("too many validated location sessions"));
        }
        let session = sessions.entry(id.into()).or_insert_with(|| Session {
            proof: proof.clone(),
            generation: uuid::Uuid::new_v4().to_string(),
            capabilities: descriptor.capabilities.clone(),
        });
        if session.proof != proof || session.capabilities != descriptor.capabilities {
            session.proof = proof;
            session.capabilities = descriptor.capabilities.clone();
            session.generation = uuid::Uuid::new_v4().to_string();
        }
        descriptor.session_generation = session.generation.clone();
    }
    Ok(descriptor)
}

pub fn disconnected(
    id: &str,
    kind: Kind,
    uri: &str,
    label: &str,
    connection: Connection,
) -> AppResult<Descriptor> {
    if connection == Connection::Connected {
        return Err(AppError::invalid(
            "a connected location requires provider validation",
        ));
    }
    let uri = canonical_uri(kind, uri)?;
    Ok(Descriptor {
        schema: 1,
        id: id.into(),
        kind,
        canonical_uri: uri,
        label: label.into(),
        connection,
        session_generation: String::new(),
        local_representation: None,
        capabilities: BTreeSet::new(),
        error: None,
    })
}

pub fn invalidate(id: &str) {
    if let Ok(mut sessions) = SESSIONS.lock()
        && let Some(sessions) = sessions.as_mut()
    {
        sessions.remove(id);
    }
}

fn session(id: &str, generation: &str) -> AppResult<Session> {
    SESSIONS
        .lock()
        .map_err(|_| AppError::command("location sessions unavailable"))?
        .as_ref()
        .and_then(|sessions| sessions.get(id))
        .filter(|session| !generation.is_empty() && session.generation == generation)
        .cloned()
        .ok_or_else(|| AppError::invalid("location session is disconnected or stale"))
}

fn validated_directory(id: &str, generation: &str) -> AppResult<(Session, rustix::fd::OwnedFd)> {
    let session = session(id, generation)?;
    let directory = secure::open_directory_nofollow(&session.proof.path)?;
    let identity = secure::stat_in(&directory, OsStr::new("."))?.identity();
    if identity != session.proof.identity
        || secure::directory_mount_id(&directory)? != session.proof.mount_id
        || secure::directory_unique_mount_id(&directory)? != session.proof.unique_mount_id
    {
        return Err(AppError::invalid(
            "location representation changed since validation",
        ));
    }
    self::session(id, generation)?;
    Ok((session, directory))
}

pub fn validate_local(id: &str, generation: &str) -> AppResult<PathBuf> {
    validated_directory(id, generation).map(|(session, _)| session.proof.path)
}

fn volume_descriptor(volume: &Volume, table: &MountTable) -> AppResult<Descriptor> {
    let properties = crate::mounts::udev::UdevProperties::read(&volume.device_number);
    let id = properties
        .get("ID_FS_UUID")
        .map(|uuid| format!("volume:uuid:{uuid}"))
        .unwrap_or_else(|| format!("volume:{}:{}", volume.device_number, volume.source));
    let kind = if volume.bus == "usb" {
        Kind::Usb
    } else {
        Kind::Local
    };
    let mut descriptor = if let Some(path) = &volume.mountpoint {
        local(&id, kind, path, &volume.name, table)?
    } else {
        invalidate(&id);
        let uri = url::Url::from_file_path(&volume.source)
            .map_err(|_| AppError::invalid("invalid volume source"))?;
        disconnected(
            &id,
            kind,
            uri.as_str(),
            &volume.name,
            Connection::Disconnected,
        )?
    };
    if volume.read_only {
        descriptor.capabilities.retain(|capability| {
            !matches!(
                capability,
                Capability::Write
                    | Capability::Mkdir
                    | Capability::Rename
                    | Capability::AtomicRename
                    | Capability::Trash
                    | Capability::Permissions
            )
        });
    }
    if crate::mounts::actions::available() {
        descriptor
            .capabilities
            .insert(if volume.mountpoint.is_some() {
                if volume.external {
                    Capability::Eject
                } else {
                    Capability::Unmount
                }
            } else {
                Capability::Mount
            });
    }
    Ok(descriptor)
}

pub fn canonical_uri(kind: Kind, raw: &str) -> AppResult<String> {
    if kind == Kind::Mtp && raw.starts_with("mtp://[usb:") {
        let (device, tail) = raw[11..]
            .split_once(']')
            .ok_or_else(|| AppError::invalid("invalid MTP device URI"))?;
        let (bus, device_number) = device
            .split_once(',')
            .ok_or_else(|| AppError::invalid("invalid MTP device URI"))?;
        let number = |value: &str| {
            value
                .parse::<u16>()
                .ok()
                .filter(|value| *value > 0 && *value <= 999)
        };
        let bus = number(bus).ok_or_else(|| AppError::invalid("invalid MTP bus"))?;
        let device =
            number(device_number).ok_or_else(|| AppError::invalid("invalid MTP device"))?;
        if !tail.is_empty() && !tail.starts_with('/') {
            return Err(AppError::invalid("invalid MTP path"));
        }
        let normalized = canonical_uri(
            Kind::Mtp,
            &format!(
                "mtp://device.invalid{}",
                if tail.is_empty() { "/" } else { tail }
            ),
        )?;
        let path = normalized
            .strip_prefix("mtp://device.invalid")
            .ok_or_else(|| AppError::invalid("invalid MTP path"))?;
        return Ok(format!("mtp://[usb:{bus:03},{device:03}]{path}"));
    }
    let mut uri = url::Url::parse(raw).map_err(|error| AppError::invalid(error.to_string()))?;
    let expected = match kind {
        Kind::Local | Kind::Usb => "file",
        Kind::Mtp => "mtp",
        Kind::Sftp => "sftp",
    };
    if uri.scheme() != expected
        || uri.password().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
    {
        return Err(AppError::invalid(
            "location URI has an unsupported scheme, credential, query or fragment",
        ));
    }
    if expected == "file" {
        if uri.to_file_path().is_err() {
            return Err(AppError::invalid("location is not a local file URI"));
        }
    } else if uri.host_str().is_none_or(str::is_empty) {
        return Err(AppError::invalid(
            "remote location requires an explicit host",
        ));
    }
    if uri.path().is_empty() {
        uri.set_path("/");
    }
    Ok(uri.to_string())
}

pub fn connect(
    options: &crate::backend::LocationConnectArgs,
    cancelled: &AtomicBool,
) -> AppResult<Value> {
    let candidate = tailnet::discover(cancelled)?
        .into_iter()
        .find(|candidate| candidate.location.id == options.location)
        .ok_or_else(|| {
            AppError::invalid("peer is no longer in the tailnet inventory; refresh Locations")
        })?;
    let saved = sftp::Saved {
        host: options.expected_host.clone(),
        user: options.user.clone(),
        path: options.path.clone(),
    };
    let location = sftp::connect(&candidate, &saved, cancelled)?;
    let connected = location.connection == Connection::Connected;
    let save_error = if connected && options.save {
        saved::remember(&saved).err().map(|error| error.to_string())
    } else {
        None
    };
    Ok(
        json!({"ok":connected,"error":location.error,"location":location,"saved":connected && options.save && save_error.is_none(),"save_error":save_error}),
    )
}
