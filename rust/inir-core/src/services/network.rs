use super::dbus::{self, read, Properties};
use inir_types::desktop::{AccessPoint, Command, NetworkState, ServiceStatus};
use std::collections::HashMap;
use zbus::{
    zvariant::{OwnedObjectPath, Value},
    Connection,
};

pub const SERVICE: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
const MANAGER: &str = SERVICE;
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";

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
