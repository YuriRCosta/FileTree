use super::storage::{self, Content};
use super::{
    DocumentKind, LegacyWriter, Preparation, Roots, Status, legacy_writer, preflight_document,
};
use crate::lease::Authority;
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
struct Entry {
    source: String,
    from: PathBuf,
    role: String,
    to: PathBuf,
    content: Content,
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
            match import(legacy_root, native_root, authority) {
                Ok(()) => Status::Ready,
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

fn import(legacy_root: &Roots, native_root: &Roots, authority: &Authority) -> AppResult<()> {
    let (native, bindings) = native_roots(native_root, authority)?;
    let state = &native["state"];
    let mut legacy = paths(legacy_root);
    legacy.insert("bin".into(), artifact_bin()?);
    let saved = storage::read(state, Path::new(RECEIPT))?;
    let (receipt, fresh) = if let Some(Content::File(bytes)) = saved {
        let receipt: Receipt = serde_json::from_slice(&bytes)?;
        if receipt.version != 1 || receipt.native != bindings || receipt.legacy != legacy {
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
                    .filter(|name| !name.as_encoded_bytes().starts_with(b"."))
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
                version: 1,
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
            && existing != entry.content
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
    authority
        .verify()
        .map_err(|error| invalid(error.to_string()))?;
    if fresh {
        storage::publish(state, Path::new(RECEIPT), &Content::File(bytes.clone()))?;
    }
    for entry in &receipt.entries {
        authority
            .verify()
            .map_err(|error| invalid(error.to_string()))?;
        storage::publish(&native[&entry.role], &entry.to, &entry.content)?;
    }
    verify_sources(&receipt, copied)?;
    match legacy_writer(legacy_root) {
        LegacyWriter::Absent | LegacyWriter::Stopped { .. } => {}
        _ => return Err(invalid("legacy writer evidence changed during migration")),
    }
    authority
        .verify()
        .map_err(|error| invalid(error.to_string()))?;
    storage::publish(state, Path::new(COPIED), &Content::File(bytes.clone()))?;
    if receipt.entries.iter().any(|entry| entry.source == "bin") {
        let bin = storage::root(&receipt.legacy["bin"])?;
        for entry in receipt
            .entries
            .iter()
            .rev()
            .filter(|entry| entry.source == "bin")
        {
            authority
                .verify()
                .map_err(|error| invalid(error.to_string()))?;
            storage::remove(&bin, &entry.from, &entry.content)?;
        }
    }
    authority
        .verify()
        .map_err(|error| invalid(error.to_string()))?;
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
    for entry in &receipt.entries {
        let root = roots
            .get(&entry.source)
            .ok_or_else(|| invalid("missing migration source root"))?;
        let current = storage::read(root, &entry.from)?;
        if copied && entry.source == "bin" && current.is_none() {
            continue;
        }
        if current.as_ref() != Some(&entry.content) {
            return Err(invalid(format!(
                "legacy source changed: {}",
                entry.from.display()
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
    if path
        .file_name()
        .is_some_and(|name| name.as_encoded_bytes().ends_with(b".lock"))
    {
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
    let mut destinations = BTreeMap::new();
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
        if let Some(previous) = destinations.insert((&entry.role, &entry.to), &entry.content)
            && previous != &entry.content
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
            if entry.source == "bin"
                && entry
                    .from
                    .file_name()
                    .is_some_and(|name| name == "manifest.json")
            {
                let manifest: Value = serde_json::from_slice(bytes)?;
                if manifest["schemaVersion"] != 1 || !manifest["items"].is_array() {
                    return Err(invalid("unsupported artifact-bin manifest"));
                }
                if let Some(id) = manifest["helperRecordId"].as_str() {
                    let module = manifest["module"].as_str().unwrap_or_default();
                    if !["hooks", "mcp"].contains(&module)
                        || id.is_empty()
                        || id.contains('/')
                        || id.contains("..")
                    {
                        return Err(invalid("invalid helper recovery identity"));
                    }
                    let expected = PathBuf::from(format!("{module}-recovery/{id}.json"));
                    let record = entries
                        .iter()
                        .find(|entry| entry.source == "recovery" && entry.from == expected)
                        .ok_or_else(|| invalid("missing paired helper recovery evidence"))?;
                    let Content::File(record) = &record.content else {
                        return Err(invalid("invalid helper recovery evidence"));
                    };
                    let record: Value = serde_json::from_slice(record)?;
                    if record["formatVersion"] != 1
                        || record["transactionId"] != id
                        || record["payload"] != manifest["payload"]
                        || record["definitionId"] != manifest["id"]
                    {
                        return Err(invalid("unsupported helper recovery schema"));
                    }
                }
                let directory = entry
                    .from
                    .parent()
                    .ok_or_else(|| invalid("invalid bin entry"))?;
                for item in manifest["items"].as_array().unwrap() {
                    let stored = item["stored"]
                        .as_str()
                        .ok_or_else(|| invalid("missing stored artifact identity"))?;
                    let path = Path::new(stored);
                    if path
                        .components()
                        .any(|part| !matches!(part, std::path::Component::Normal(_)))
                        || !path.starts_with("items")
                    {
                        return Err(invalid("unsafe stored artifact path"));
                    }
                    if !entries.iter().any(|entry| {
                        entry.source == "bin" && entry.from.starts_with(directory.join(path))
                    }) {
                        return Err(invalid("missing stored artifact recovery evidence"));
                    }
                }
            }
        }
    }
    Ok(())
}
