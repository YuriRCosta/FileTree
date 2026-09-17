use crate::AppResult;
use serde_json::{Value, json};
use std::path::Path;

pub mod actions;
pub mod block;
pub mod mountinfo;
pub mod udev;

pub use actions::{eject, mount, unmount};

const SKIPPED_FILESYSTEMS: [&str; 6] = [
    "crypto_LUKS",
    "swap",
    "linux_raid_member",
    "LVM2_member",
    "zfs_member",
    "bcache",
];
const SYSTEM_MOUNTPOINTS: [&str; 4] = ["/", "/boot", "/boot/efi", "/efi"];
const SYSTEM_PREFIXES: [&str; 5] = ["/var/", "/usr/", "/etc/", "/nix/", "/snap/"];

#[derive(Clone, Debug)]
pub struct Volume {
    pub name: String,
    pub device: String,
    pub source: String,
    pub device_number: String,
    pub mountpoint: Option<String>,
    pub filesystem: Option<String>,
    pub label: Option<String>,
    pub bus: String,
    pub size: u64,
    pub used: Option<u64>,
    pub available: Option<u64>,
    pub fraction: Option<f64>,
    pub percent: Option<u64>,
    pub removable: bool,
    pub external: bool,
    pub read_only: bool,
    pub image: Option<String>,
    pub tier: &'static str,
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

fn system_mountpoint(mountpoint: &str) -> bool {
    SYSTEM_MOUNTPOINTS.contains(&mountpoint)
        || SYSTEM_PREFIXES
            .iter()
            .any(|prefix| mountpoint.starts_with(prefix))
}

fn display_name(
    label: Option<&str>,
    model: Option<&str>,
    mountpoint: Option<&str>,
    size: u64,
    device: &block::BlockDevice,
) -> String {
    if let Some(label) = label {
        return label.to_string();
    }
    if let Some(mountpoint) = mountpoint.filter(|value| system_mountpoint(value)) {
        return mountpoint.to_string();
    }
    if let Some(image) = device.loop_backing.as_deref() {
        return Path::new(image).file_name().map_or_else(
            || image.to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
    }
    if let Some(model) = model.filter(|_| !device.partition) {
        return model.replace('_', " ");
    }
    if size > 0 {
        return format!("{} Volume", human_size(size));
    }
    device.name.clone()
}

pub fn list() -> AppResult<Vec<Volume>> {
    Ok(list_with(&mountinfo::MountTable::read()?))
}

pub fn list_with(table: &mountinfo::MountTable) -> Vec<Volume> {
    block::list()
        .into_iter()
        .filter_map(|device| volume(&device, table))
        .collect()
}

fn volume(device: &block::BlockDevice, table: &mountinfo::MountTable) -> Option<Volume> {
    let properties = udev::UdevProperties::read(&device.device_number);
    if properties.ignored() {
        return None;
    }
    let record = table.primary(&device.device_number);
    let filesystem = record
        .map(|record| record.filesystem.clone())
        .or_else(|| properties.filesystem().map(str::to_string));
    if filesystem
        .as_deref()
        .is_some_and(|value| SKIPPED_FILESYSTEMS.contains(&value))
    {
        return None;
    }
    if filesystem.is_none() || (record.is_none() && device.virtual_device()) {
        return None;
    }
    let mountpoint = record.map(|record| crate::common::path_text(&record.mountpoint));
    let external = device.hotplug || device.removable || device.loop_backing.is_some();
    let tier = match (&mountpoint, external) {
        (_, true) => "external",
        (Some(mountpoint), false) if system_mountpoint(mountpoint) => "system",
        (Some(_), false) => "internal",
        (None, false) => "unmounted",
    };
    let usage = mountpoint.as_deref().and_then(crate::capacity::usage_at);
    let label = properties.label().map(str::to_string);
    Some(Volume {
        name: display_name(
            label.as_deref(),
            properties.model(),
            mountpoint.as_deref(),
            device.size,
            device,
        ),
        device: device.name.clone(),
        source: device.path(),
        device_number: device.device_number.clone(),
        mountpoint,
        filesystem,
        label,
        bus: properties.bus().unwrap_or(&device.bus).to_string(),
        size: device.size,
        used: usage.map(|usage| usage.used),
        available: usage.map(|usage| usage.available),
        fraction: usage.and_then(|usage| usage.fraction),
        percent: usage.and_then(|usage| usage.percent),
        removable: device.removable,
        external,
        read_only: device.read_only || record.is_some_and(|record| record.read_only),
        image: device.loop_backing.clone(),
        tier,
    })
}

impl Volume {
    pub fn json(&self) -> Value {
        json!({
            "name": self.name,
            "device": self.device,
            "source": self.source,
            "device_number": self.device_number,
            "mountpoint": self.mountpoint,
            "mounted": self.mountpoint.is_some(),
            "filesystem": self.filesystem,
            "label": self.label,
            "bus": self.bus,
            "size": self.size,
            "size_label": human_size(self.size),
            "used": self.used,
            "available": self.available,
            "fraction": self.fraction,
            "percent": self.percent,
            "removable": self.removable,
            "external": self.external,
            "read_only": self.read_only,
            "image": self.image,
            "tier": self.tier,
            "needs_authorization": !self.external,
        })
    }
}

pub fn payload() -> AppResult<Value> {
    Ok(payload_with(&list()?))
}

pub fn payload_with(volumes: &[Volume]) -> Value {
    json!({
        "volumes": volumes.iter().map(Volume::json).collect::<Vec<Value>>(),
        "actions": actions::available(),
    })
}
