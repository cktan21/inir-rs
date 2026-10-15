use super::dbus::{self, read, Properties};
use crate::events::Event;
use futures_util::{stream::SelectAll, StreamExt};
use inir_types::desktop::{AccessPoint, ActiveConnection, Command, NetworkState, ServiceStatus};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zbus::{
    zvariant::{OwnedObjectPath, Value},
    Connection, MatchRule, MessageStream,
};

pub const SERVICE: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
// NetworkManager's ObjectManager sits here: device and access-point additions and
// removals arrive as InterfacesAdded/InterfacesRemoved on this path.
const OBJECT_ROOT: &str = "/org/freedesktop";
const MANAGER: &str = SERVICE;
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const SETTINGS: &str = "org.freedesktop.NetworkManager.Settings";
const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const ACTIVE_CONNECTION: &str = "org.freedesktop.NetworkManager.Connection.Active";
// Activation states from NetworkManager's NMActiveConnectionState.
const ACTIVATED: u32 = 2;
const DEACTIVATED: u32 = 4;
// How long a connect command waits for NetworkManager to finish activating.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(30);
const WIRELESS_SECURITY: &str = "802-11-wireless-security";
const DEVICE_TYPE_ETHERNET: u32 = 1;
const DEVICE_TYPE_WIFI: u32 = 2;
const DEVICE_STATE_ACTIVATED: u32 = 100;
// Bursts of scan updates are applied together and published at most this often.
const COALESCE: Duration = Duration::from_millis(50);
// Signal-strength changes smaller than this (percentage points) are not published.
const STRENGTH_HYSTERESIS: u8 = 5;

/// One connection's settings as NetworkManager returns them (`a{sa{sv}}`).
pub type Settings = HashMap<String, Properties>;

/// One decoded NetworkManager signal. Decoding is kept apart from applying so the
/// reducer can be exercised without a bus.
#[derive(Debug)]
pub enum Signal {
    Properties {
        path: String,
        interface: String,
        changed: Properties,
        invalidated: Vec<String>,
    },
    Added {
        path: String,
        interfaces: HashMap<String, Properties>,
    },
    Removed {
        path: String,
        interfaces: Vec<String>,
    },
    AccessPoint {
        device: String,
        path: String,
        added: bool,
    },
    /// A saved connection was created (`added`) or deleted. Creation is followed by a
    /// settings read, because the new profile's name is not in the signal.
    Profile { path: String, added: bool },
    /// NetworkManager's bus name changed owner: its state must be rebuilt.
    OwnerChanged,
}

#[derive(Default)]
struct Device {
    kind: u32,
    state: u32,
    active_ap: String,
    access_points: BTreeSet<String>,
}

impl Device {
    fn apply(&mut self, props: &Properties) {
        if props.contains_key("DeviceType") {
            self.kind = read(props, "DeviceType");
        }
        if props.contains_key("State") {
            self.state = read(props, "State");
        }
    }
    fn apply_wireless(&mut self, props: &Properties) {
        if props.contains_key("ActiveAccessPoint") {
            self.active_ap = read::<OwnedObjectPath>(props, "ActiveAccessPoint")
                .as_str()
                .to_owned();
        }
        if props.contains_key("AccessPoints") {
            self.access_points = read::<Vec<OwnedObjectPath>>(props, "AccessPoints")
                .into_iter()
                .map(|path| path.as_str().to_owned())
                .collect();
        }
    }
}

/// Indexed NetworkManager state. It is bootstrapped once with ObjectManager and
/// then kept current by applying signal bodies in place, so an access-point
/// change costs no D-Bus round trip. `state()` rebuilds the presentation rows.
#[derive(Default)]
pub struct Mirror {
    wifi_enabled: bool,
    connectivity: u32,
    devices: BTreeMap<String, Device>,
    aps: BTreeMap<String, Properties>,
    /// Saved Wi-Fi profiles: settings path -> connection id (the profile name).
    wifi_profiles: BTreeMap<String, String>,
    /// Active connections (VPN/wifi/ethernet) by object path, applied in place from
    /// their Connection.Active signals so VPN status needs no nmcli polling.
    active: BTreeMap<String, Properties>,
}

impl Mirror {
    pub async fn bootstrap(bus: &Connection) -> zbus::Result<Self> {
        let manager = dbus::properties(bus, SERVICE, ROOT, MANAGER).await?;
        let mut mirror = Self::default();
        mirror.set_manager(&manager);
        let objects: dbus::Objects = dbus::proxy(bus, SERVICE, OBJECT_ROOT, OBJECT_MANAGER)
            .await?
            .call("GetManagedObjects", &())
            .await?;
        for (path, interfaces) in objects {
            mirror.add(path.as_str(), &interfaces);
        }
        // Saved profiles only feed the "known network" hints. They are best-effort:
        // a refusal here must not stop the signal stream or the access-point list.
        mirror.wifi_profiles = load_wifi_profiles(bus).await.unwrap_or_default();
        Ok(mirror)
    }

    fn set_manager(&mut self, props: &Properties) {
        if props.contains_key("WirelessEnabled") {
            self.wifi_enabled = read(props, "WirelessEnabled");
        }
        if props.contains_key("Connectivity") {
            self.connectivity = read(props, "Connectivity");
        }
    }

    /// Inserts or replaces every interface of one object. Used by the bootstrap,
    /// InterfacesAdded, and re-reads of invalidated properties.
    pub fn add(&mut self, path: &str, interfaces: &HashMap<String, Properties>) {
        if path == ROOT {
            if let Some(props) = interfaces.get(MANAGER) {
                self.set_manager(props);
            }
        }
        if let Some(props) = interfaces.get(DEVICE) {
            self.devices.entry(path.into()).or_default().apply(props);
        }
        if let Some(props) = interfaces.get(WIRELESS) {
            self.devices
                .entry(path.into())
                .or_default()
                .apply_wireless(props);
        }
        if let Some(props) = interfaces.get(AP) {
            self.aps.insert(path.into(), props.clone());
        }
        if let Some(props) = interfaces.get(ACTIVE_CONNECTION) {
            self.active.insert(path.into(), props.clone());
        }
    }

    /// Records (or forgets, when `id` is None) one saved profile after it was read.
    pub fn set_wifi_profile(&mut self, path: &str, id: Option<String>) {
        match id {
            Some(id) => {
                self.wifi_profiles.insert(path.into(), id);
            }
            None => {
                self.wifi_profiles.remove(path);
            }
        }
    }

    /// Applies one decoded signal. Properties that are invalidated, or that belong to
    /// an object we do not know yet, are queued in `refresh` for a targeted re-read.
    /// A `Settings.Connection` entry in `refresh` means "re-read this profile".
    pub fn apply(&mut self, signal: Signal, refresh: &mut BTreeSet<(String, String)>) {
        match signal {
            Signal::Properties {
                path,
                interface,
                changed,
                invalidated,
            } => {
                if !invalidated.is_empty() {
                    refresh.insert((path.clone(), interface.clone()));
                }
                match interface.as_str() {
                    MANAGER if path == ROOT => self.set_manager(&changed),
                    DEVICE => match self.devices.get_mut(&path) {
                        Some(device) => device.apply(&changed),
                        None => {
                            refresh.insert((path.clone(), interface.clone()));
                        }
                    },
                    WIRELESS => match self.devices.get_mut(&path) {
                        Some(device) => device.apply_wireless(&changed),
                        None => {
                            refresh.insert((path.clone(), interface.clone()));
                        }
                    },
                    AP => match self.aps.get_mut(&path) {
                        Some(props) => {
                            let mut changed = changed;
                            // NetworkManager reports ±1-2% signal jitter every few
                            // seconds. Each one would republish the network state and
                            // redraw every strength indicator, so ignore moves smaller
                            // than the hysteresis; slow drift still lands once it
                            // accumulates past it.
                            if let Some(next) = changed.get("Strength") {
                                let next: u8 = next.try_clone().ok().and_then(|v| u8::try_from(v).ok()).unwrap_or_default();
                                let current: u8 = read(props, "Strength");
                                if props.contains_key("Strength") && next.abs_diff(current) < STRENGTH_HYSTERESIS {
                                    changed.remove("Strength");
                                }
                            }
                            props.extend(changed)
                        }
                        None => {
                            refresh.insert((path.clone(), interface.clone()));
                        }
                    },
                    ACTIVE_CONNECTION => match self.active.get_mut(&path) {
                        Some(props) => props.extend(changed),
                        None => {
                            refresh.insert((path.clone(), interface.clone()));
                        }
                    },
                    _ => {}
                }
            }
            Signal::Added { path, interfaces } => self.add(&path, &interfaces),
            Signal::Removed { path, interfaces } => {
                // Do not resurrect an object from a refresh queued before its removal.
                refresh.retain(|(object, interface)| {
                    object != &path || !interfaces.contains(interface)
                });
                if interfaces.iter().any(|name| name == AP) {
                    self.aps.remove(&path);
                    for device in self.devices.values_mut() {
                        device.access_points.remove(&path);
                    }
                }
                if interfaces
                    .iter()
                    .any(|name| name == DEVICE || name == WIRELESS)
                {
                    self.devices.remove(&path);
                }
                if interfaces.iter().any(|name| name == ACTIVE_CONNECTION) {
                    self.active.remove(&path);
                }
            }
            Signal::AccessPoint {
                device,
                path,
                added,
            } => {
                if let Some(device) = self.devices.get_mut(&device) {
                    if added {
                        device.access_points.insert(path);
                    } else {
                        device.access_points.remove(&path);
                    }
                }
            }
            Signal::Profile { path, added } => {
                if added {
                    refresh.insert((path, CONNECTION.to_owned()));
                } else {
                    self.wifi_profiles.remove(&path);
                    refresh.remove(&(path, CONNECTION.to_owned()));
                }
            }
            Signal::OwnerChanged => {}
        }
    }

    pub fn state(&self) -> NetworkState {
        let mut access_points = Vec::new();
        for (path, device) in &self.devices {
            if device.kind != DEVICE_TYPE_WIFI {
                continue;
            }
            for ap in &device.access_points {
                if let Some(props) = self.aps.get(ap) {
                    access_points.push(access_point(ap, path, props, &device.active_ap));
                }
            }
        }
        sort_access_points(&mut access_points);
        let known_profiles: BTreeSet<&String> = self.wifi_profiles.values().collect();
        let mut active_connections: Vec<_> = self
            .active
            .values()
            .map(|props| ActiveConnection {
                id: read(props, "Id"),
                kind: read(props, "Type"),
                state: read(props, "State"),
                vpn: read(props, "Vpn"),
                uuid: read(props, "Uuid"),
            })
            .collect();
        active_connections.sort_by(|a, b| a.uuid.cmp(&b.uuid));
        NetworkState {
            status: ServiceStatus {
                ready: true,
                error: String::new(),
            },
            wifi_enabled: self.wifi_enabled,
            ethernet: self.devices.values().any(|device| {
                device.kind == DEVICE_TYPE_ETHERNET && device.state == DEVICE_STATE_ACTIVATED
            }),
            connectivity: self.connectivity,
            access_points,
            known_profiles: known_profiles.into_iter().cloned().collect(),
            active_connections,
        }
    }
}

fn sort_access_points(aps: &mut [AccessPoint]) {
    // Stable keys preserve rows across rescans; do not deduplicate distinct BSSIDs.
    aps.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.strength.cmp(&a.strength))
            .then(a.id.cmp(&b.id))
    });
}

/// The profile name when `settings` describe a Wi-Fi connection.
fn wifi_profile_id(settings: &Settings) -> Option<String> {
    let connection = settings.get("connection")?;
    if read::<String>(connection, "type") != "802-11-wireless" {
        return None;
    }
    let id = read::<String>(connection, "id");
    (!id.is_empty()).then_some(id)
}

async fn settings_of(bus: &Connection, path: &str) -> zbus::Result<Settings> {
    dbus::proxy(bus, SERVICE, path, CONNECTION)
        .await?
        .call("GetSettings", &())
        .await
}

async fn load_wifi_profiles(bus: &Connection) -> zbus::Result<BTreeMap<String, String>> {
    let paths: Vec<OwnedObjectPath> = dbus::proxy(bus, SERVICE, SETTINGS_PATH, SETTINGS)
        .await?
        .call("ListConnections", &())
        .await?;
    let mut profiles = BTreeMap::new();
    for path in paths {
        // A profile can disappear between the listing and the read; skip it.
        let Ok(settings) = settings_of(bus, path.as_str()).await else {
            continue;
        };
        if let Some(id) = wifi_profile_id(&settings) {
            profiles.insert(path.as_str().to_owned(), id);
        }
    }
    Ok(profiles)
}

/// The saved Wi-Fi profile named `name`, matched the way `nmcli connection ... id <name>`
/// matches it. Returns its object path and settings.
async fn wifi_profile(bus: &Connection, name: &str) -> Result<(String, Settings), String> {
    let paths: Vec<OwnedObjectPath> = dbus::proxy(bus, SERVICE, SETTINGS_PATH, SETTINGS)
        .await
        .map_err(|e| e.to_string())?
        .call("ListConnections", &())
        .await
        .map_err(|e| e.to_string())?;
    for path in paths {
        let Ok(settings) = settings_of(bus, path.as_str()).await else {
            continue;
        };
        if wifi_profile_id(&settings).as_deref() == Some(name) {
            return Ok((path.as_str().to_owned(), settings));
        }
    }
    Err(format!("no saved Wi-Fi profile named {name}"))
}

/// Event-driven network actor. It bootstraps once with ObjectManager, then applies
/// each signal body to the mirror. Changes are published at most every
/// `COALESCE`, and only when the presented state actually changed.
pub async fn stream(
    bus: &Connection,
    events: &mpsc::Sender<Event>,
    stop: &CancellationToken,
) -> Result<(), String> {
    // Subscribe before the bootstrap read: anything that changes during the read
    // is queued and applied on top of it, so no change is lost.
    let object_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(SERVICE)
        .map_err(|e| e.to_string())?
        .path_namespace(ROOT)
        .map_err(|e| e.to_string())?
        .build();
    let objects_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(SERVICE)
        .map_err(|e| e.to_string())?
        .path(OBJECT_ROOT)
        .map_err(|e| e.to_string())?
        .interface(OBJECT_MANAGER)
        .map_err(|e| e.to_string())?
        .build();
    // Filter at the bus so only NetworkManager's own restarts wake the actor.
    let owner_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")
        .map_err(|e| e.to_string())?
        .interface("org.freedesktop.DBus")
        .map_err(|e| e.to_string())?
        .member("NameOwnerChanged")
        .map_err(|e| e.to_string())?
        .arg(0, SERVICE)
        .map_err(|e| e.to_string())?
        .build();
    let mut messages = SelectAll::new();
    for rule in [object_rule, objects_rule, owner_rule] {
        messages.push(
            MessageStream::for_match_rule(rule, bus, Some(256))
                .await
                .map_err(|e| e.to_string())?
                .boxed(),
        );
    }

    let mut mirror = Mirror::bootstrap(bus).await.map_err(|e| e.to_string())?;
    let mut published = mirror.state();
    if events
        .send(Event::Network(published.clone()))
        .await
        .is_err()
    {
        return Ok(());
    }

    let mut dirty = false;
    let mut refresh = BTreeSet::new();
    let mut deadline = tokio::time::Instant::now();
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            message = messages.next() => {
                let Some(message) = message else {
                    return Err("NetworkManager signal stream ended".into());
                };
                let message = message.map_err(|e| e.to_string())?;
                let Some(signal) = decode(&message) else { continue };
                if matches!(signal, Signal::OwnerChanged) {
                    return Err("NetworkManager restarted".into());
                }
                if !dirty {
                    dirty = true;
                    deadline = tokio::time::Instant::now() + COALESCE;
                }
                mirror.apply(signal, &mut refresh);
            }
            _ = tokio::time::sleep_until(deadline), if dirty => {
                dirty = false;
                for (path, interface) in std::mem::take(&mut refresh) {
                    if interface == CONNECTION {
                        // A profile was created or changed: read its settings. A
                        // profile that vanished is removed by its own signal.
                        match settings_of(bus, &path).await {
                            Ok(settings) => mirror.set_wifi_profile(&path, wifi_profile_id(&settings)),
                            Err(_) => mirror.set_wifi_profile(&path, None),
                        }
                        continue;
                    }
                    // A vanished object is removed by its own InterfacesRemoved signal.
                    if let Ok(props) = dbus::properties(bus, SERVICE, &path, &interface).await {
                        mirror.add(&path, &HashMap::from([(interface, props)]));
                    }
                }
                let next = mirror.state();
                if next != published {
                    published = next.clone();
                    if events.send(Event::Network(next)).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Decodes only signal bodies represented in the mirror.
fn decode(message: &zbus::Message) -> Option<Signal> {
    let header = message.header();
    let member = header.member()?.as_str();
    let interface = header.interface()?.as_str();
    let path = header
        .path()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_default();
    let relevant = |name: &str| [MANAGER, DEVICE, WIRELESS, AP, ACTIVE_CONNECTION].contains(&name);
    match (interface, member) {
        (PROPERTIES, "PropertiesChanged") => {
            let (interface, changed, invalidated): (String, Properties, Vec<String>) =
                message.body().deserialize().ok()?;
            relevant(&interface).then_some(Signal::Properties {
                path,
                interface,
                changed,
                invalidated,
            })
        }
        (OBJECT_MANAGER, "InterfacesAdded") => {
            let (path, interfaces): (OwnedObjectPath, HashMap<String, Properties>) =
                message.body().deserialize().ok()?;
            interfaces
                .keys()
                .any(|name| relevant(name))
                .then(|| Signal::Added {
                    path: path.as_str().to_owned(),
                    interfaces,
                })
        }
        (OBJECT_MANAGER, "InterfacesRemoved") => {
            let (path, interfaces): (OwnedObjectPath, Vec<String>) =
                message.body().deserialize().ok()?;
            interfaces
                .iter()
                .any(|name| relevant(name))
                .then(|| Signal::Removed {
                    path: path.as_str().to_owned(),
                    interfaces,
                })
        }
        (WIRELESS, "AccessPointAdded" | "AccessPointRemoved") => {
            let (access_point,): (OwnedObjectPath,) = message.body().deserialize().ok()?;
            Some(Signal::AccessPoint {
                device: path,
                path: access_point.as_str().to_owned(),
                added: member == "AccessPointAdded",
            })
        }
        (SETTINGS, "NewConnection" | "ConnectionRemoved") => {
            let (profile,): (OwnedObjectPath,) = message.body().deserialize().ok()?;
            Some(Signal::Profile {
                path: profile.as_str().to_owned(),
                added: member == "NewConnection",
            })
        }
        (CONNECTION, "Updated" | "Removed") => Some(Signal::Profile {
            path,
            added: member == "Updated",
        }),
        (ACTIVE_CONNECTION, "StateChanged") => {
            let (state, _reason): (u32, u32) = message.body().deserialize().ok()?;
            Some(Signal::Properties {
                path,
                interface: ACTIVE_CONNECTION.to_owned(),
                changed: HashMap::from([("State".to_owned(), state.into())]),
                invalidated: Vec::new(),
            })
        }
        ("org.freedesktop.DBus", "NameOwnerChanged") => {
            let (name, _, _): (String, String, String) = message.body().deserialize().ok()?;
            (name == SERVICE).then_some(Signal::OwnerChanged)
        }
        _ => None,
    }
}

/// One-shot full read used for connection commands and tests.
pub async fn snapshot(bus: &Connection) -> zbus::Result<NetworkState> {
    Ok(Mirror::bootstrap(bus).await?.state())
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

/// Connects to one access point. Reuses a compatible saved profile when no password
/// is given, otherwise creates and activates a profile. NetworkManager and the
/// desktop's SecretAgent keep ownership of stored secrets.
async fn connect(bus: &Connection, ap: &AccessPoint, password: Option<&str>) -> zbus::Result<()> {
    let manager = dbus::proxy(bus, SERVICE, ROOT, MANAGER).await?;
    if matches!(ap.security.as_str(), "enterprise" | "wep" | "wpa3" | "owe") {
        return Err(zbus::Error::Failure(
            "this security mode requires the legacy connection editor".into(),
        ));
    }
    let dev = dbus::properties(bus, SERVICE, &ap.device, DEVICE).await?;
    if password.is_none() {
        for profile in read::<Vec<OwnedObjectPath>>(&dev, "AvailableConnections") {
            let settings: Settings = dbus::proxy(bus, SERVICE, profile.as_str(), CONNECTION)
                .await?
                .call("GetSettings", &())
                .await?;
            if settings
                .get("802-11-wireless")
                .is_some_and(|s| read::<Vec<u8>>(s, "ssid") == ap.ssid_bytes)
            {
                let active: OwnedObjectPath = manager
                    .call(
                        "ActivateConnection",
                        &(
                            profile,
                            OwnedObjectPath::try_from(ap.device.as_str())?,
                            OwnedObjectPath::try_from(ap.id.as_str())?,
                        ),
                    )
                    .await?;
                return await_activation(bus, active.as_str())
                    .await
                    .map_err(zbus::Error::Failure);
            }
        }
    }
    let mut connection = HashMap::new();
    connection.insert("id", Value::from(ap.ssid.as_str()));
    connection.insert("type", Value::from("802-11-wireless"));
    let mut wifi = HashMap::new();
    wifi.insert("ssid", Value::from(ap.ssid_bytes.clone()));
    wifi.insert("mode", Value::from("infrastructure"));
    let mut settings = HashMap::from([("connection", connection), ("802-11-wireless", wifi)]);
    if !ap.security.is_empty() {
        let password = password.ok_or_else(|| zbus::Error::Failure("password required".into()))?;
        settings.insert(
            WIRELESS_SECURITY,
            HashMap::from([
                ("key-mgmt", Value::from("wpa-psk")),
                ("psk", Value::from(password)),
            ]),
        );
    }
    let (_, active): (OwnedObjectPath, OwnedObjectPath) = manager
        .call(
            "AddAndActivateConnection",
            &(
                settings,
                OwnedObjectPath::try_from(ap.device.as_str())?,
                OwnedObjectPath::try_from(ap.id.as_str())?,
            ),
        )
        .await?;
    await_activation(bus, active.as_str())
        .await
        .map_err(zbus::Error::Failure)
}

/// Waits for an activation to finish. `nmcli dev wifi connect` reports the outcome of
/// activation, not just that NetworkManager accepted the request, and so does this.
async fn await_activation(bus: &Connection, active: &str) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + ACTIVATION_TIMEOUT;
    loop {
        // The active-connection object disappears when the attempt is abandoned.
        let props = dbus::properties(bus, SERVICE, active, ACTIVE_CONNECTION)
            .await
            .map_err(|_| "connection attempt was abandoned".to_string())?;
        match read::<u32>(&props, "State") {
            ACTIVATED => return Ok(()),
            DEACTIVATED => return Err("connection failed".into()),
            _ => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("connection timed out".into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Executes one network command. The returned string is the command's result payload
/// (only `ProfileSecret` uses it); every other command returns an empty string.
pub async fn execute(bus: &Connection, command: Command) -> Result<String, String> {
    match command {
        Command::WifiConnectSsid { ssid, password } => {
            // Visible access points are sorted active first, then strongest first.
            let state = snapshot(bus).await.map_err(|e| e.to_string())?;
            let ap = state
                .access_points
                .iter()
                .find(|ap| ap.ssid == ssid)
                .ok_or_else(|| format!("no visible access point named {ssid}"))?;
            connect(bus, ap, password.as_deref())
                .await
                .map(|()| String::new())
                .map_err(|e| e.to_string())
        }
        Command::ProfileForget { ssid } => {
            let (path, _) = wifi_profile(bus, &ssid).await?;
            dbus::proxy(bus, SERVICE, &path, CONNECTION)
                .await
                .map_err(|e| e.to_string())?
                .call::<_, _, ()>("Delete", &())
                .await
                .map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        Command::ProfileSetPassword { ssid, password } => {
            if password.is_empty() {
                return Err("password is empty".into());
            }
            let (path, settings) = wifi_profile(bus, &ssid).await?;
            // Update replaces every setting, so send back what was read and change only
            // the security section. Secrets are not returned by GetSettings, so psk is set.
            let mut update: HashMap<String, HashMap<String, Value<'_>>> = settings
                .into_iter()
                .map(|(section, props)| {
                    (
                        section,
                        props
                            .into_iter()
                            .map(|(key, value)| (key, Value::from(value)))
                            .collect(),
                    )
                })
                .collect();
            let security = update.entry(WIRELESS_SECURITY.into()).or_default();
            security.insert("key-mgmt".into(), Value::from("wpa-psk"));
            security.insert("psk".into(), Value::from(password.as_str()));
            dbus::proxy(bus, SERVICE, &path, CONNECTION)
                .await
                .map_err(|e| e.to_string())?
                .call::<_, _, ()>("Update", &(update,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        Command::ProfileSecret { ssid } => {
            let (path, _) = wifi_profile(bus, &ssid).await?;
            let secrets: Settings = dbus::proxy(bus, SERVICE, &path, CONNECTION)
                .await
                .map_err(|e| e.to_string())?
                .call("GetSecrets", &(WIRELESS_SECURITY,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(secrets
                .get(WIRELESS_SECURITY)
                .map(|section| read::<String>(section, "psk"))
                .unwrap_or_default())
        }
        command => legacy_execute(bus, command).await.map(|()| String::new()),
    }
}

async fn legacy_execute(bus: &Connection, command: Command) -> Result<(), String> {
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
                connect(bus, ap, password.as_deref()).await?;
            }
            _ => return Err(zbus::Error::Failure("invalid network command".into())),
        }
        Ok(())
    }
    .await;
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod signal_tests {
    use super::*;

    #[test]
    fn profile_update_and_removal_decode_without_payload() {
        let mut mirror = Mirror::default();
        let mut refresh = BTreeSet::new();
        mirror.set_wifi_profile("/settings/1", Some("old-name".into()));
        let updated = zbus::Message::signal("/settings/1", CONNECTION, "Updated")
            .unwrap()
            .build(&())
            .unwrap();
        mirror.apply(
            decode(&updated).expect("decode profile Updated"),
            &mut refresh,
        );
        assert_eq!(
            refresh,
            BTreeSet::from([("/settings/1".into(), CONNECTION.into())])
        );
        // The targeted GetSettings result can rename a known profile.
        mirror.set_wifi_profile("/settings/1", Some("renamed".into()));
        assert_eq!(mirror.state().known_profiles, ["renamed"]);

        let removed = zbus::Message::signal("/settings/1", CONNECTION, "Removed")
            .unwrap()
            .build(&())
            .unwrap();
        mirror.apply(
            decode(&removed).expect("decode profile Removed"),
            &mut refresh,
        );
        assert!(mirror.state().known_profiles.is_empty());
        assert!(
            refresh.is_empty(),
            "removed profile must cancel its queued refresh"
        );
    }

    #[test]
    fn active_state_signal_updates_and_removal_cancels_refresh() {
        let mut mirror = Mirror::default();
        mirror.add(
            "/active/1",
            &HashMap::from([(
                ACTIVE_CONNECTION.into(),
                HashMap::from([
                    ("Id".into(), zbus::zvariant::Str::from("work-vpn").into()),
                    ("Type".into(), zbus::zvariant::Str::from("vpn").into()),
                    ("Uuid".into(), zbus::zvariant::Str::from("vpn-uuid").into()),
                    ("Vpn".into(), true.into()),
                    ("State".into(), ACTIVATED.into()),
                ]),
            )]),
        );
        let mut refresh = BTreeSet::new();
        let state_changed = zbus::Message::signal("/active/1", ACTIVE_CONNECTION, "StateChanged")
            .unwrap()
            .build(&(DEACTIVATED, 1u32))
            .unwrap();
        mirror.apply(
            decode(&state_changed).expect("decode activation StateChanged"),
            &mut refresh,
        );
        assert_eq!(mirror.state().active_connections[0].state, DEACTIVATED);
        assert!(
            refresh.is_empty(),
            "state body must not trigger a round trip"
        );
        refresh.insert(("/active/1".into(), ACTIVE_CONNECTION.into()));
        let removed = zbus::Message::signal(OBJECT_ROOT, OBJECT_MANAGER, "InterfacesRemoved")
            .unwrap()
            .build(&(
                OwnedObjectPath::try_from("/active/1").unwrap(),
                vec![ACTIVE_CONNECTION],
            ))
            .unwrap();
        mirror.apply(decode(&removed).expect("decode removal"), &mut refresh);
        assert!(mirror.state().active_connections.is_empty());
        assert!(
            refresh.is_empty(),
            "removed active object must not be resurrected"
        );
    }
}
