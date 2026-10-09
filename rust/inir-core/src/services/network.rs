use super::dbus::{self, read, Properties};
use crate::events::Event;
use futures_util::StreamExt;
use inir_types::desktop::{AccessPoint, Command, NetworkState, ServiceStatus};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zbus::{
    zvariant::{OwnedObjectPath, Value},
    Connection, MatchRule, MessageStream,
};

pub const SERVICE: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
const AP_PREFIX: &str = "/org/freedesktop/NetworkManager/AccessPoint";
const MANAGER: &str = SERVICE;
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
// AP properties that change continuously during a scan. Applying their signal
// bodies in place avoids the full manager→device→AP refetch cascade.
const FAST_KEYS: [&str; 4] = ["Strength", "Frequency", "MaxBitrate", "LastSeen"];

fn sort_access_points(aps: &mut [AccessPoint]) {
    // Stable keys preserve rows across rescans; do not deduplicate distinct BSSIDs.
    aps.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.strength.cmp(&a.strength))
            .then(a.id.cmp(&b.id))
    });
}

/// Event-driven network actor. Bootstraps once with a full snapshot, then keeps
/// the state current by applying D-Bus signal bodies: high-frequency AP property
/// changes are applied in place, and structural changes (devices, active AP,
/// AP add/remove, manager state) trigger a scoped re-snapshot. The signal body
/// is no longer discarded, so a scan no longer costs one full refetch per tick.
pub async fn stream(
    bus: &Connection,
    events: &mpsc::Sender<Event>,
    stop: &CancellationToken,
) -> Result<(), String> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(SERVICE)
        .map_err(|e| e.to_string())?
        .path_namespace(ROOT)
        .map_err(|e| e.to_string())?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, bus, Some(256))
        .await
        .map_err(|e| e.to_string())?;

    let mut state = snapshot(bus).await.map_err(|e| e.to_string())?;
    sort_access_points(&mut state.access_points);
    if events.send(Event::Network(state.clone())).await.is_err() {
        return Ok(());
    }

    let mut full_rescan = false;
    let mut ap_updates: HashSet<String> = HashSet::new();
    let mut ap_props: HashMap<String, Properties> = HashMap::new();
    let mut deadline = tokio::time::Instant::now();
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            message = signals.next() => {
                let Some(message) = message else {
                    return Err("NetworkManager signal stream ended".into());
                };
                let message = message.map_err(|e| e.to_string())?;
                let header = message.header();
                let member = header.member().map(|m| m.as_str().to_owned());
                let path = header.path().map(|p| p.as_str().to_owned()).unwrap_or_default();
                let mut structural = true;
                if member.as_deref() == Some("PropertiesChanged")
                    && path.starts_with(AP_PREFIX)
                {
                    if let Ok((iface, changed, _invalidated)) =
                        message.body().deserialize::<(String, Properties, Vec<String>)>()
                    {
                        if iface == AP && changed.keys().all(|k| FAST_KEYS.contains(&k.as_str())) {
                            // AP property delta: apply the body, skip the refetch.
                            ap_props.entry(path.clone()).or_default().extend(changed);
                            ap_updates.insert(path);
                            structural = false;
                        }
                    }
                }
                if structural {
                    full_rescan = true;
                }
                if deadline <= tokio::time::Instant::now() {
                    deadline = tokio::time::Instant::now() + Duration::from_millis(50);
                }
            }
            _ = tokio::time::sleep_until(deadline),
                if full_rescan || !ap_updates.is_empty() =>
            {
                let previous = state.clone();
                if full_rescan {
                    full_rescan = false;
                    match snapshot(bus).await {
                        Ok(mut next) => {
                            sort_access_points(&mut next.access_points);
                            state = next;
                            tracing::debug!(
                                aps = state.access_points.len(),
                                "network full snapshot (structural change)"
                            );
                        }
                        Err(error) => {
                            if error.to_string().contains("ServiceUnknown") {
                                return Err(error.to_string());
                            }
                        }
                    }
                } else {
                    let updated = ap_updates.len();
                    let mut changed = false;
                    for id in ap_updates.drain() {
                        let Some(props) = ap_props.remove(&id) else { continue };
                        let Some(ap) = state.access_points.iter_mut().find(|ap| ap.id == id) else {
                            // AP vanished before its delta applied; resync next tick.
                            full_rescan = true;
                            continue;
                        };
                        if props.contains_key("Strength") {
                            ap.strength = read(&props, "Strength");
                            changed = true;
                        }
                        if props.contains_key("Frequency") {
                            ap.frequency = read(&props, "Frequency");
                            changed = true;
                        }
                        if props.contains_key("MaxBitrate") {
                            ap.rate = read(&props, "MaxBitrate");
                            changed = true;
                        }
                    }
                    if changed {
                        sort_access_points(&mut state.access_points);
                        tracing::debug!(updated, "network AP delta applied (no refetch)");
                    }
                }
                ap_updates.clear();
                ap_props.clear();
                // A vanished AP above leaves full_rescan set; re-arm the timer so
                // the next tick resyncs instead of spinning on a past deadline.
                if full_rescan {
                    deadline = tokio::time::Instant::now() + Duration::from_millis(50);
                }
                if state != previous && events.send(Event::Network(state.clone())).await.is_err() {
                    return Ok(());
                }
            }
        }
    }
}

pub async fn snapshot(bus: &Connection) -> zbus::Result<NetworkState> {
    let manager = dbus::properties(bus, SERVICE, ROOT, MANAGER).await?;
    let mut state = NetworkState {
        status: ServiceStatus {
            ready: true,
            error: String::new(),
        },
        wifi_enabled: read(&manager, "WirelessEnabled"),
        connectivity: read(&manager, "Connectivity"),
        ..Default::default()
    };
    for device in read::<Vec<OwnedObjectPath>>(&manager, "Devices") {
        let Ok(properties) = dbus::properties(bus, SERVICE, device.as_str(), DEVICE).await else {
            continue;
        };
        let kind: u32 = read(&properties, "DeviceType");
        if kind == 1 && read::<u32>(&properties, "State") == 100 {
            state.ethernet = true;
        }
        if kind != 2 {
            continue;
        }
        let wireless = dbus::properties(bus, SERVICE, device.as_str(), WIRELESS).await?;
        let active: OwnedObjectPath = read(&wireless, "ActiveAccessPoint");
        let proxy = dbus::proxy(bus, SERVICE, device.as_str(), WIRELESS).await?;
        let aps: Vec<OwnedObjectPath> = proxy.call("GetAllAccessPoints", &()).await?;
        for path in aps {
            let Ok(p) = dbus::properties(bus, SERVICE, path.as_str(), AP).await else {
                continue;
            };
            state.access_points.push(access_point(
                path.as_str(),
                device.as_str(),
                &p,
                active.as_str(),
            ));
        }
    }
    // Stable keys preserve rows across rescans; do not deduplicate distinct BSSIDs.
    state.access_points.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.strength.cmp(&a.strength))
            .then(a.id.cmp(&b.id))
    });
    Ok(state)
}

pub fn access_point(path: &str, device: &str, p: &Properties, active: &str) -> AccessPoint {
    let flags: u32 = read(p, "Flags");
    let wpa: u32 = read(p, "WpaFlags");
    let rsn: u32 = read(p, "RsnFlags");
    // NetworkManager NM80211ApSecurityFlags (802.1x and Suite-B, SAE, OWE).
    let security = if (wpa | rsn) & 0x2200 != 0 {
        "enterprise"
    } else if rsn & 0x1800 != 0 {
        "owe"
    } else if rsn & 0x400 != 0 {
        "wpa3"
    } else if (wpa | rsn) != 0 {
        "wpa"
    } else if flags & 1 != 0 {
        "wep"
    } else {
        ""
    };
    AccessPoint {
        id: path.into(),
        device: device.into(),
        ssid: String::from_utf8_lossy(&read::<Vec<u8>>(p, "Ssid")).into_owned(),
        ssid_bytes: read(p, "Ssid"),
        bssid: read(p, "HwAddress"),
        strength: read(p, "Strength"),
        frequency: read(p, "Frequency"),
        rate: read(p, "MaxBitrate"),
        security: security.into(),
        active: path == active,
    }
}

pub async fn execute(bus: &Connection, command: Command) -> Result<(), String> {
    let manager = dbus::proxy(bus, SERVICE, ROOT, MANAGER)
        .await
        .map_err(|e| e.to_string())?;
    let result: zbus::Result<()> = async {
        match command {
            Command::WifiEnabled { enabled } => {
                dbus::set(
                    bus,
                    SERVICE,
                    ROOT,
                    MANAGER,
                    "WirelessEnabled",
                    Value::from(enabled),
                )
                .await?
            }
            Command::WifiScan => {
                let devices: Vec<OwnedObjectPath> = manager.call("GetDevices", &()).await?;
                for device in devices {
                    let p = dbus::properties(bus, SERVICE, device.as_str(), DEVICE).await?;
                    if read::<u32>(&p, "DeviceType") == 2 {
                        dbus::proxy(bus, SERVICE, device.as_str(), WIRELESS)
                            .await?
                            .call::<_, _, ()>("RequestScan", &(HashMap::<String, Value>::new(),))
                            .await?;
                    }
                }
            }
            Command::WifiDisconnect { device } => {
                dbus::proxy(bus, SERVICE, &device, DEVICE)
                    .await?
                    .call::<_, _, ()>("Disconnect", &())
                    .await?;
            }
            Command::WifiConnect {
                access_point,
                password,
            } => {
                let state = snapshot(bus).await?;
                let ap = state
                    .access_points
                    .iter()
                    .find(|ap| ap.id == access_point)
                    .ok_or_else(|| zbus::Error::Failure("access point disappeared".into()))?;
                if matches!(ap.security.as_str(), "enterprise" | "wep" | "wpa3" | "owe") {
                    return Err(zbus::Error::Failure(
                        "this security mode requires the legacy connection editor".into(),
                    ));
                }
                // Try an existing compatible profile first. NetworkManager and
                // the desktop's SecretAgent retain ownership of stored secrets.
                let dev = dbus::properties(bus, SERVICE, &ap.device, DEVICE).await?;
                if password.is_none() {
                    for profile in read::<Vec<OwnedObjectPath>>(&dev, "AvailableConnections") {
                        let settings: HashMap<String, Properties> = dbus::proxy(
                            bus,
                            SERVICE,
                            profile.as_str(),
                            "org.freedesktop.NetworkManager.Settings.Connection",
                        )
                        .await?
                        .call("GetSettings", &())
                        .await?;
                        if settings
                            .get("802-11-wireless")
                            .is_some_and(|s| read::<Vec<u8>>(s, "ssid") == ap.ssid_bytes)
                        {
                            let _: OwnedObjectPath = manager
                                .call(
                                    "ActivateConnection",
                                    &(
                                        profile,
                                        OwnedObjectPath::try_from(ap.device.as_str())?,
                                        OwnedObjectPath::try_from(ap.id.as_str())?,
                                    ),
                                )
                                .await?;
                            return Ok(());
                        }
                    }
                }
                let mut connection = HashMap::new();
                connection.insert("id", Value::from(ap.ssid.as_str()));
                connection.insert("type", Value::from("802-11-wireless"));
                let mut wifi = HashMap::new();
                wifi.insert("ssid", Value::from(ap.ssid_bytes.clone()));
                wifi.insert("mode", Value::from("infrastructure"));
                let mut settings =
                    HashMap::from([("connection", connection), ("802-11-wireless", wifi)]);
                if !ap.security.is_empty() {
                    let password = password
                        .as_deref()
                        .ok_or_else(|| zbus::Error::Failure("password required".into()))?;
                    settings.insert(
                        "802-11-wireless-security",
                        HashMap::from([
                            ("key-mgmt", Value::from("wpa-psk")),
                            ("psk", Value::from(password)),
                        ]),
                    );
                }
                let _: (OwnedObjectPath, OwnedObjectPath) = manager
                    .call(
                        "AddAndActivateConnection",
                        &(
                            settings,
                            OwnedObjectPath::try_from(ap.device.as_str())?,
                            OwnedObjectPath::try_from(ap.id.as_str())?,
                        ),
                    )
                    .await?;
            }
            _ => return Err(zbus::Error::Failure("invalid network command".into())),
        }
        Ok(())
    }
    .await;
    result.map_err(|e| e.to_string())
}
