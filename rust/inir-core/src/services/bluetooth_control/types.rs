use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub const SCAN_DURATION: Duration = Duration::from_secs(25);
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanState {
    pub adapter: String,
    pub owners: Vec<String>,
    pub owned: bool,
    pub observed: bool,
    pub pending: bool,
    pub error: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub kind: String,
    pub device: String,
    pub name: String,
    pub value: String,
    pub entered: u16,
    pub size: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub id: String,
    pub name: String,
    pub direction: String,
    pub status: String,
    pub size: u64,
    pub transferred: u64,
    pub filename: String,
    pub error: String,
}
impl crate::model::Row for Transfer {
    fn key(&self) -> &str {
        &self.id
    }
    fn diff(&self, other: &Self) -> u64 {
        let mut roles = 0;
        for (bit, changed) in [
            self.id != other.id,
            self.name != other.name,
            self.direction != other.direction,
            self.status != other.status,
            self.size != other.size,
            self.transferred != other.transferred,
            self.filename != other.filename,
            self.error != other.error,
        ]
        .into_iter()
        .enumerate()
        {
            if changed {
                roles |= 1 << bit;
            }
        }
        roles
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub id: String,
    pub uuid: String,
    pub name: String,
    pub state: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub ready: bool,
    pub agent_ready: bool,
    pub obex_ready: bool,
    pub network_ready: bool,
    pub error: String,
    pub scans: Vec<ScanState>,
    pub prompts: Vec<Prompt>,
    pub transfers: Vec<Transfer>,
    pub networks: Vec<Network>,
}

#[derive(Default, Debug)]
pub struct Leases(pub BTreeMap<String, (String, Instant)>);
impl Leases {
    pub fn request(&mut self, owner: String, adapter: String, now: Instant) {
        self.request_for(owner, adapter, now, SCAN_DURATION);
    }
    pub fn request_for(
        &mut self,
        owner: String,
        adapter: String,
        now: Instant,
        duration: Duration,
    ) {
        self.0
            .insert(owner, (adapter, now + duration.min(SCAN_DURATION)));
    }
    pub fn release(&mut self, owner: &str) {
        self.0.remove(owner);
    }
    pub fn expire(&mut self, now: Instant) {
        self.0.retain(|_, (_, deadline)| *deadline > now);
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.0.values().map(|(_, d)| *d).min()
    }
    pub fn owners(&self, adapter: &str) -> Vec<String> {
        self.0
            .iter()
            .filter(|(_, (a, _))| a == adapter)
            .map(|(o, _)| o.clone())
            .collect()
    }
}

pub fn adapter_path(adapter: &str) -> Result<String, String> {
    let suffix = adapter
        .strip_prefix("hci")
        .ok_or("Invalid Bluetooth adapter")?;
    if suffix.is_empty() || !suffix.bytes().all(|c| c.is_ascii_digit()) {
        return Err("Invalid Bluetooth adapter".into());
    }
    Ok(format!("/org/bluez/{adapter}"))
}
pub fn device_path(adapter: &str, address: &str) -> Result<String, String> {
    let path = adapter_path(adapter)?;
    let octets: Vec<_> = address.split(':').collect();
    if octets.len() != 6
        || octets
            .iter()
            .any(|p| p.len() != 2 || !p.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err("Invalid Bluetooth device address".into());
    }
    Ok(format!(
        "{path}/dev_{}",
        address.to_ascii_uppercase().replace(':', "_")
    ))
}
pub fn safe_filename(name: &str) -> String {
    // Filesystem NAME_MAX is measured in bytes, not Unicode characters.
    let mut clean = String::new();
    for character in name.rsplit(['/', '\\']).next().unwrap_or("").chars() {
        if character.is_control() {
            continue;
        }
        if clean.len() + character.len_utf8() > 180 {
            break;
        }
        clean.push(character);
    }
    let name = clean;
    if name.is_empty() || name == "." || name == ".." {
        "Bluetooth file".into()
    } else {
        name
    }
}
