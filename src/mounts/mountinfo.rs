use crate::AppResult;
use crate::common::{display_path, parse_path, path_text};
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountRecord {
    pub mount_id: u64,
    pub device_number: String,
    pub subroot: String,
    pub mountpoint: PathBuf,
    pub filesystem: String,
    pub source: String,
    pub read_only: bool,
}

#[derive(Default)]
pub struct MountTable {
    records: Vec<MountRecord>,
    by_device: HashMap<String, Vec<usize>>,
}

fn unmangle(bytes: &[u8]) -> OsString {
    let mut text = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = bytes
            .get(index..index + 4)
            .filter(|window| window[0] == b'\\')
            .and_then(|window| std::str::from_utf8(&window[1..]).ok())
            .and_then(|digits| u8::from_str_radix(digits, 8).ok());
        match escape {
            Some(byte) => {
                text.push(byte);
                index += 4;
            }
            None => {
                text.push(bytes[index]);
                index += 1;
            }
        }
    }
    OsString::from_vec(text)
}

pub fn device_number(major: u64, minor: u64) -> String {
    format!("{major}:{minor}")
}

pub fn device_number_of(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.file_type().is_dir() || metadata.file_type().is_file() {
        return None;
    }
    let raw = metadata.rdev();
    Some(device_number(
        libc::major(raw).into(),
        libc::minor(raw).into(),
    ))
}

pub fn parse(text: &str) -> Vec<MountRecord> {
    parse_bytes(text.as_bytes())
}

fn parse_bytes(bytes: &[u8]) -> Vec<MountRecord> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &[u8]) -> Option<MountRecord> {
    let fields: Vec<&[u8]> = line
        .split(u8::is_ascii_whitespace)
        .filter(|field| !field.is_empty())
        .collect();
    let separator = fields.iter().position(|field| *field == b"-")?;
    let head = fields.get(..separator)?;
    let tail = fields.get(separator + 1..)?;
    Some(MountRecord {
        mount_id: std::str::from_utf8(head.first()?).ok()?.parse().ok()?,
        device_number: std::str::from_utf8(head.get(2)?).ok()?.to_string(),
        subroot: display_path(Path::new(&unmangle(head.get(3)?))),
        mountpoint: PathBuf::from(unmangle(head.get(4)?)),
        filesystem: std::str::from_utf8(tail.first()?).ok()?.to_string(),
        source: {
            let source = PathBuf::from(unmangle(tail.get(1)?));
            if source.is_absolute() {
                path_text(&source)
            } else {
                display_path(&source)
            }
        },
        read_only: [head.get(5), tail.get(2)]
            .into_iter()
            .flatten()
            .any(|options| {
                options
                    .split(|byte| *byte == b',')
                    .any(|option| option == b"ro")
            }),
    })
}

impl MountTable {
    pub fn read() -> AppResult<Self> {
        Ok(Self::from_records(parse_bytes(&std::fs::read(
            MOUNTINFO_PATH,
        )?)))
    }

    pub fn from_text(text: &str) -> Self {
        Self::from_records(parse(text))
    }

    fn from_records(records: Vec<MountRecord>) -> Self {
        let mut by_device: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, record) in records.iter().enumerate() {
            by_device
                .entry(record.device_number.clone())
                .or_default()
                .push(index);
            if let Some(number) = parse_path(&record.source)
                .ok()
                .filter(|path| path.starts_with("/dev"))
                .and_then(|path| device_number_of(&path))
                && number != record.device_number
            {
                by_device.entry(number).or_default().push(index);
            }
        }
        Self { records, by_device }
    }

    pub fn records(&self) -> &[MountRecord] {
        &self.records
    }

    pub fn record_for(&self, mount_id: u64, path: &Path) -> Option<&MountRecord> {
        self.records
            .iter()
            .find(|record| record.mount_id == mount_id && path.starts_with(&record.mountpoint))
    }

    pub fn primary(&self, device: &str) -> Option<&MountRecord> {
        self.mounts(device)
            .min_by_key(|record| (record.mountpoint.as_os_str().len(), record.subroot.len()))
    }

    pub fn mounts(&self, device: &str) -> impl Iterator<Item = &MountRecord> {
        self.by_device
            .get(device)
            .into_iter()
            .flatten()
            .filter_map(|index| self.records.get(*index))
    }
}
