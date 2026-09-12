use super::storage::{self, Content};
use super::{
    DocumentKind, LegacyWriter, Preparation, Roots, Status, legacy_writer, preflight_document,
};
use crate::lease::Authority;
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const RECEIPT: &str = "migration-020/receipt.json";
const COPIED: &str = "migration-020/copied.json";
const COMPLETE: &str = "migration-020/complete.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Binding {
    path: PathBuf,
    device: u64,
    inode: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub(super) source: String,
    pub(super) from: PathBuf,
    pub(super) role: String,
    pub(super) to: PathBuf,
    pub(super) content: Content,
    pub(super) attributes: storage::Attributes,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    native: BTreeMap<String, Binding>,
    legacy: BTreeMap<String, PathBuf>,
    sources: BTreeMap<String, Option<Binding>>,
    entries: Vec<Entry>,
}

fn paths(roots: &Roots) -> BTreeMap<String, PathBuf> {
    [
        ("config", &roots.config),
        ("state", &roots.state),
        ("recovery", &roots.recovery),
    ]
    .into_iter()
    .map(|(role, path)| (role.to_owned(), path.clone()))
    .collect()
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::invalid(message)
}

pub fn prepare(
    legacy_root: &Roots,
    native_root: &Roots,
    authority: &Authority,
) -> AppResult<Preparation> {
    let receipt_path = native_root.state.join(RECEIPT);
    let status = match legacy_writer(legacy_root) {
        LegacyWriter::Active { evidence, .. } => Status::ReadOnly {
            reason: format!("legacy writer is active: {}", evidence.join("; ")),
        },
        LegacyWriter::Unknown { reason } => Status::ReadOnly {
            reason: format!("legacy writer status is unknown: {reason}"),
        },
        LegacyWriter::Absent | LegacyWriter::Stopped { .. } => {
            match stopped(legacy_root, native_root, authority) {
                Ok(status) => status,
                Err(error) => Status::Refused {
                    reason: format!("{error}; originals and migration receipt retained"),
                },
            }
        }
    };
    Ok(Preparation {
        status,
        receipt_path,
    })
}

fn stopped(legacy: &Roots, native: &Roots, authority: &Authority) -> AppResult<Status> {
    native_roots(native, authority)?;
    let guard = match super::writer::hold(legacy, &artifact_bin()?) {
        Ok(guard) => guard,
        Err(reason) => return Ok(Status::ReadOnly { reason }),
    };
    match legacy_writer(legacy) {
        LegacyWriter::Absent | LegacyWriter::Stopped { .. } => {}
        evidence => {
            return Ok(Status::ReadOnly {
                reason: format!("legacy writer evidence changed: {evidence:?}"),
            });
        }
    }
    import(legacy, native, authority, &guard)?;
    Ok(Status::Ready)
}

fn barrier(authority: &Authority, guard: &super::writer::Guard) -> AppResult<()> {
    authority
        .verify()
        .map_err(|error| invalid(error.to_string()))?;
    guard.verify().map_err(invalid)
}

fn native_roots(
    roots: &Roots,
    authority: &Authority,
) -> AppResult<(BTreeMap<String, File>, BTreeMap<String, Binding>)> {
    authority
        .verify()
        .map_err(|error| invalid(error.to_string()))?;
    let mut files = BTreeMap::new();
    let mut bindings = BTreeMap::new();
    for (role, path) in paths(roots) {
        let identity = authority
            .roots()
            .get(&role)
            .ok_or_else(|| invalid("missing authority root role"))?;
        let (file, relative) = authority
            .storage_anchor(&path)?
            .ok_or_else(|| invalid("migration root is outside authority"))?;
        let metadata = file.metadata()?;
        if !relative.as_os_str().is_empty()
            || (metadata.dev(), metadata.ino()) != (identity.device, identity.inode)
        {
            return Err(invalid("migration root does not match authority role"));
        }
        bindings.insert(
            role.clone(),
            Binding {
                path: identity.path.clone(),
                device: metadata.dev(),
                inode: metadata.ino(),
            },
        );
        files.insert(role, file);
    }
    Ok((files, bindings))
}

fn artifact_bin() -> AppResult<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::common::expanded_path("~/.local/share"));
    if !data.is_absolute() {
        return Err(invalid("legacy data home must be absolute"));
    }
    Ok(data.join("fileblade/bin"))
}

fn import(
    legacy_root: &Roots,
    native_root: &Roots,
    authority: &Authority,
    guard: &super::writer::Guard,
) -> AppResult<()> {
    let (native, bindings) = native_roots(native_root, authority)?;
    for (role, path, kind) in [
        ("state", "state.json", DocumentKind::State),
        ("config", "settings.json", DocumentKind::Settings),
        ("config", "keybindings.json", DocumentKind::Keybindings),
        ("config", "blades.json", DocumentKind::Layout),
    ] {
        match storage::read(&native[role], Path::new(path))? {
            Some(Content::File(bytes)) => {
                preflight_document(kind, &bytes)?;
            }
            None => {}
            Some(_) => return Err(invalid("native document is not a regular file")),
        }
    }
    let state = &native["state"];
    let mut legacy = paths(legacy_root);
    legacy.insert("bin".into(), artifact_bin()?);
    let saved = storage::read(state, Path::new(RECEIPT))?;
    let (receipt, fresh) = if let Some(Content::File(bytes)) = saved {
        let receipt: Receipt = serde_json::from_slice(&bytes)?;
        if receipt.version != 2 || receipt.native != bindings || receipt.legacy != legacy {
            return Err(invalid("migration receipt schema or root binding mismatch"));
        }
        if let Some(completed) = storage::read(state, Path::new(COMPLETE))? {
            if completed != Content::File(completion(&bytes)) {
                return Err(invalid("migration completion receipt mismatch"));
            }
            return Ok(());
        }
        (receipt, false)
    } else if saved.is_some() {
        return Err(invalid("migration receipt must be a regular file"));
    } else {
        let mut entries = Vec::new();
        let mut sources = BTreeMap::new();
        for (source, root_path) in &legacy {
            let root = match storage::root(root_path) {
                Ok(root) => root,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    sources.insert(source.clone(), None);
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let metadata = root.metadata()?;
            sources.insert(
                source.clone(),
                Some(Binding {
                    path: root_path.clone(),
                    device: metadata.dev(),
                    inode: metadata.ino(),
                }),
            );
            let selected: Vec<PathBuf> = match source.as_str() {
                "config" => [
                    "settings.json",
                    "keybindings.json",
                    "blades.json",
                    "colors.json",
                    "hooks",
                    "config",
                ]
                .into_iter()
                .map(PathBuf::from)
                .collect(),
                "state" => ["state.json", "modules"]
                    .into_iter()
                    .map(PathBuf::from)
                    .collect(),
                "recovery" => ["hooks-recovery", "mcp-recovery"]
                    .into_iter()
                    .map(PathBuf::from)
                    .collect(),
                _ => storage::names(&root)?
                    .into_iter()
                    .filter(|name| name != ".mutation.lock")
                    .map(PathBuf::from)
                    .collect(),
            };
            for path in selected {
                collect(&root, source, &path, &mut entries, 0)?;
            }
            let current = storage::root(root_path)?.metadata()?;
            let held = root.metadata()?;
            if (current.dev(), current.ino()) != (held.dev(), held.ino()) {
                return Err(invalid("legacy source root changed during inventory"));
            }
        }
        validate(&entries)?;
        (
            Receipt {
                version: 2,
                native: bindings,
                legacy,
                sources,
                entries,
            },
            true,
        )
    };
    validate(&receipt.entries)?;
    for entry in &receipt.entries {
        if let Some(existing) = storage::read(&native[&entry.role], &entry.to)?
            && (existing != entry.content
                || storage::attributes(&native[&entry.role], &entry.to)? != entry.attributes)
        {
            return Err(invalid(format!(
                "migration destination conflict: {}",
                entry.to.display()
            )));
        }
    }
    let bytes = serde_json::to_vec(&receipt)?;
    let copied = match storage::read(state, Path::new(COPIED))? {
        None => false,
        Some(content) if content == Content::File(bytes.clone()) => true,
        Some(_) => return Err(invalid("migration copied checkpoint mismatch")),
    };
    verify_sources(&receipt, copied)?;
    if bytes.len() > storage::MAX_BYTES {
        return Err(invalid("migration receipt exceeds byte bound"));
    }
    barrier(authority, guard)?;
    if fresh {
        storage::publish(state, Path::new(RECEIPT), &Content::File(bytes.clone()))?;
    }
    for entry in &receipt.entries {
        barrier(authority, guard)?;
        storage::publish_snapshot(
            &native[&entry.role],
            &entry.to,
            &entry.content,
            &entry.attributes,
        )?;
    }
    verify_sources(&receipt, copied)?;
    match legacy_writer(legacy_root) {
        LegacyWriter::Absent | LegacyWriter::Stopped { .. } => {}
        _ => return Err(invalid("legacy writer evidence changed during migration")),
    }
    barrier(authority, guard)?;
    storage::publish(state, Path::new(COPIED), &Content::File(bytes.clone()))?;
    if receipt.entries.iter().any(|entry| entry.source == "bin") {
        let bin = guard.directory(&receipt.legacy["bin"]).map_err(invalid)?;
        verify_sources(&receipt, true)?;
        for (index, entry) in receipt
            .entries
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, entry)| entry.source == "bin")
        {
            barrier(authority, guard)?;
            storage::retire(&bin, &entry.from, index, &entry.content, &entry.attributes)?;
        }
    }
    barrier(authority, guard)?;
    verify_sources(&receipt, true)?;
    storage::publish(
        state,
        Path::new(COMPLETE),
        &Content::File(completion(&bytes)),
    )?;
    Ok(())
}

fn completion(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
}

fn verify_sources(receipt: &Receipt, copied: bool) -> AppResult<()> {
    let mut roots = BTreeMap::new();
    for (role, path) in &receipt.legacy {
        let expected = receipt
            .sources
            .get(role)
            .ok_or_else(|| invalid("missing migration source binding"))?;
        match (expected, storage::root(path)) {
            (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => {}
            (Some(binding), Ok(root)) => {
                let metadata = root.metadata()?;
                if binding.path != *path
                    || (binding.device, binding.inode) != (metadata.dev(), metadata.ino())
                {
                    return Err(invalid("migration source root identity changed"));
                }
                roots.insert(role.clone(), root);
            }
            _ => {
                return Err(invalid(
                    "migration source root appeared, disappeared or became unsafe",
                ));
            }
        }
    }
    if let Some(bin) = roots.get("bin") {
        verify_children(receipt, "bin", Path::new(""), bin, copied)?;
    }
    if copied && let Some(bin) = roots.get("bin") {
        match storage::directory(bin, Path::new(".migration-020-retired"), false) {
            Ok(directory) => {
                for name in storage::names(&directory)? {
                    let entry = name
                        .to_str()
                        .and_then(|name| name.parse::<usize>().ok())
                        .and_then(|index| receipt.entries.get(index))
                        .filter(|entry| entry.source == "bin")
                        .ok_or_else(|| invalid("unrecorded retired artifact"))?;
                    if storage::read(bin, &entry.from)?.is_some() {
                        return Err(invalid("original and retired artifact both exist"));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    for (index, entry) in receipt.entries.iter().enumerate() {
        let root = roots
            .get(&entry.source)
            .ok_or_else(|| invalid("missing migration source root"))?;
        let current = storage::read(root, &entry.from)?;
        if copied && entry.source == "bin" && current.is_none() {
            let retired = PathBuf::from(format!(".migration-020-retired/{index}"));
            if storage::read(root, &retired)?.as_ref() != Some(&entry.content)
                || storage::attributes(root, &retired)? != entry.attributes
                || entry.content == Content::Directory
                    && !storage::names(&storage::directory(root, &retired, false)?)?.is_empty()
            {
                return Err(invalid("retired artifact evidence is missing or changed"));
            }
            continue;
        }
        if current == Some(Content::Directory) {
            verify_children(
                receipt,
                &entry.source,
                &entry.from,
                &storage::directory(root, &entry.from, false)?,
                copied,
            )?;
        }
        if current.as_ref() != Some(&entry.content)
            || entry.source == "bin" && storage::attributes(root, &entry.from)? != entry.attributes
        {
            return Err(invalid(format!(
                "legacy source changed: {}",
                entry.from.display()
            )));
        }
    }
    Ok(())
}

fn verify_children(
    receipt: &Receipt,
    role: &str,
    path: &Path,
    directory: &File,
    copied: bool,
) -> AppResult<()> {
    let shared = receipt
        .sources
        .get(role)
        .and_then(|binding| binding.as_ref())
        .zip(receipt.native.get(role))
        .is_some_and(|(a, b)| (a.device, a.inode) == (b.device, b.inode));
    for name in storage::names(directory)? {
        if role == "bin"
            && path.as_os_str().is_empty()
            && (name == ".mutation.lock" || copied && name == ".migration-020-retired")
        {
            continue;
        }
        if role == "recovery" && path.components().count() == 1 && name == ".lock" {
            continue;
        }
        let child = path.join(name);
        if !receipt.entries.iter().any(|entry| {
            entry.source == role && entry.from == child
                || shared && entry.role == role && entry.to == child
        }) {
            return Err(invalid(format!(
                "legacy inventory gained an unrecorded entry: {}",
                child.display()
            )));
        }
    }
    Ok(())
}

fn collect(
    root: &File,
    source: &str,
    path: &Path,
    entries: &mut Vec<Entry>,
    depth: usize,
) -> AppResult<()> {
    if depth > 16 || entries.len() >= 10_000 {
        return Err(invalid("migration inventory exceeds bound"));
    }
    if path.file_name().is_some_and(|name| {
        name == ".lock" && source == "recovery" && path.components().count() == 2
    }) {
        return Ok(());
    }
    match storage::directory(root, path, false) {
        Ok(directory) => {
            entries.push(Entry {
                source: source.into(),
                from: path.to_owned(),
                role: if source == "bin" { "state" } else { source }.into(),
                to: if source == "bin" {
                    Path::new("artifact-bin").join(path)
                } else {
                    alias_path(path)
                },
                content: Content::Directory,
                attributes: if source == "bin" {
                    storage::attributes(root, path)?
                } else {
                    storage::Attributes::new()
                },
            });
            for name in storage::names(&directory)? {
                collect(root, source, &path.join(name), entries, depth + 1)?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOTDIR | libc::ELOOP)) => {
            let content =
                storage::read(root, path)?.ok_or_else(|| invalid("legacy source disappeared"))?;
            if matches!(content, Content::Link(_))
                && !(source == "bin" && path.components().any(|part| part.as_os_str() == "items"))
            {
                return Err(invalid(
                    "symbolic links are supported only as stored artifact items",
                ));
            }
            let role = if source == "bin" { "state" } else { source };
            let to = if source == "bin" {
                Path::new("artifact-bin").join(path)
            } else {
                alias_path(path)
            };
            let total: usize = entries
                .iter()
                .map(|entry| match &entry.content {
                    Content::File(bytes) | Content::Link(bytes) => bytes.len(),
                    Content::Directory => 0,
                })
                .sum();
            let size = match &content {
                Content::File(bytes) | Content::Link(bytes) => bytes.len(),
                Content::Directory => 0,
            };
            if total.saturating_add(size) > 24 * 1024 * 1024 {
                return Err(invalid("migration snapshot exceeds byte bound"));
            }
            entries.push(Entry {
                source: source.into(),
                from: path.to_owned(),
                role: role.into(),
                to,
                content,
                attributes: if source == "bin" {
                    storage::attributes(root, path)?
                } else {
                    storage::Attributes::new()
                },
            });
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn alias_path(path: &Path) -> PathBuf {
    let mut parts: Vec<_> = path.iter().map(|part| part.to_os_string()).collect();
    if parts.len() > 1 && (parts[0] == "modules" || parts[0] == "config") {
        for module in ["skills", "memory", "hooks", "mcp"] {
            if parts[1] == format!("data-goblin.fileblade-{module}+{module}").as_str()
                || parts[1] == format!("kurt.agent-{module}+{module}").as_str()
            {
                parts[1] = module.into();
            }
        }
    }
    parts.into_iter().collect()
}

fn validate(entries: &[Entry]) -> AppResult<()> {
    if entries.len() > 10_000 {
        return Err(invalid("migration receipt exceeds entry bound"));
    }
    let mut destinations = BTreeMap::new();
    let attribute_bytes: usize = entries
        .iter()
        .flat_map(|entry| &entry.attributes)
        .map(|(key, value)| key.len().saturating_add(value.len()))
        .sum();
    if attribute_bytes > 4 * 1024 * 1024
        || entries.iter().any(|entry| {
            entry.attributes.len() > 256
                || entry
                    .attributes
                    .keys()
                    .any(|name| !name.starts_with("user.") || name.contains('\0'))
        })
    {
        return Err(invalid("migration attributes exceed supported bounds"));
    }
    for entry in entries {
        if !["config", "state", "recovery"].contains(&entry.role.as_str())
            || !["config", "state", "recovery", "bin"].contains(&entry.source.as_str())
        {
            return Err(invalid("invalid migration receipt role"));
        }
        let expected_role = if entry.source == "bin" {
            "state"
        } else {
            &entry.source
        };
        let expected_to = if entry.source == "bin" {
            Path::new("artifact-bin").join(&entry.from)
        } else {
            alias_path(&entry.from)
        };
        if entry.role != expected_role
            || entry.to != expected_to
            || entry.from.starts_with("migration-020")
        {
            return Err(invalid("migration receipt destination mapping mismatch"));
        }
        for path in [&entry.from, &entry.to] {
            if path.as_os_str().is_empty()
                || path
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err(invalid("invalid migration receipt path"));
            }
        }
        if let Some(previous) = destinations.insert(
            (&entry.role, &entry.to),
            (&entry.content, &entry.attributes),
        ) && previous != (&entry.content, &entry.attributes)
        {
            return Err(invalid(
                "conflicting legacy module aliases; original states preserved",
            ));
        }
        if let Content::File(bytes) = &entry.content {
            let kind = match (entry.source.as_str(), entry.from.to_str()) {
                ("state", Some("state.json")) => Some(DocumentKind::State),
                ("config", Some("blades.json")) => Some(DocumentKind::Layout),
                ("config", Some("settings.json")) => Some(DocumentKind::Settings),
                ("config", Some("keybindings.json")) => Some(DocumentKind::Keybindings),
                _ => None,
            };
            if let Some(kind) = kind {
                preflight_document(kind, bytes)?;
            }
        }
    }
    super::artifacts::validate(entries)?;
    Ok(())
}
