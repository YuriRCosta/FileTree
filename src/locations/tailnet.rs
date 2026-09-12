use super::{Connection, Descriptor, Kind, disconnected};
use crate::command::{CommandSpec, which};
use crate::{AppError, AppResult};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const MAX_PEERS: usize = 4096;

#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    #[serde(flatten)]
    pub location: Descriptor,
    pub host: String,
    pub online: bool,
}

pub fn discover(cancelled: &AtomicBool) -> AppResult<Vec<Candidate>> {
    let program =
        which("tailscale").ok_or_else(|| AppError::command("tailscale is not installed"))?;
    let output = CommandSpec::new(program)
        .args(["status", "--json"])
        .timeout(Duration::from_secs(3))
        .limits(8 * 1024 * 1024, 8192)
        .stop_on_output_limit()
        .run_cancellable(cancelled)?;
    if !output.status.success() {
        return Err(AppError::command(
            "tailscale status failed; check the existing Tailscale connection",
        ));
    }
    candidates(&serde_json::from_slice(&output.stdout)?)
}

pub fn candidates(status: &Value) -> AppResult<Vec<Candidate>> {
    if status["BackendState"].as_str() != Some("Running") {
        return Err(AppError::invalid(
            "Tailscale is not running; connect it before discovering peers",
        ));
    }
    let peers = status["Peer"]
        .as_object()
        .ok_or_else(|| AppError::invalid("Tailscale returned no peer inventory"))?;
    if peers.len() > MAX_PEERS {
        return Err(AppError::invalid(
            "tailnet peer inventory exceeds 4096 peers",
        ));
    }
    let self_id = status["Self"]["ID"].as_str();
    let mut ids = BTreeSet::new();
    let mut result = Vec::new();
    for (key, peer) in peers {
        let id = peer["ID"]
            .as_str()
            .filter(|id| !id.is_empty())
            .unwrap_or(key);
        if Some(id) == self_id
            || id.len() > 256
            || id.chars().any(char::is_control)
            || !ids.insert(id.to_string())
        {
            continue;
        }
        let host = peer["DNSName"]
            .as_str()
            .map(|host| host.trim_end_matches('.'))
            .filter(|host| valid_host(host))
            .map(str::to_string)
            .or_else(|| {
                peer["TailscaleIPs"]
                    .as_array()?
                    .iter()
                    .filter_map(|value| value.as_str()?.parse::<IpAddr>().ok())
                    .next()
                    .map(|ip| ip.to_string())
            });
        let Some(host) = host else { continue };
        let authority = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.clone()
        };
        let label = peer["HostName"]
            .as_str()
            .filter(|name| !name.is_empty())
            .unwrap_or(&host)
            .chars()
            .filter(|ch| {
                !ch.is_control()
                    && !matches!(*ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
            .take(128)
            .collect::<String>();
        let location = disconnected(
            &format!("tailnet:{id}"),
            Kind::Sftp,
            &format!("sftp://{authority}/"),
            &label,
            Connection::Disconnected,
        )?;
        result.push(Candidate {
            location,
            host,
            online: peer["Online"].as_bool().unwrap_or(false),
        });
    }
    result.sort_by(|left, right| {
        left.location
            .label
            .to_lowercase()
            .cmp(&right.location.label.to_lowercase())
            .then(left.location.id.cmp(&right.location.id))
    });
    Ok(result)
}

pub(super) fn valid_host(host: &str) -> bool {
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    !host.is_empty()
        && host.len() <= 253
        && host.is_ascii()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}
