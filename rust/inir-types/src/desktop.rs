//! Service snapshots contain no Qt objects or subprocess output.
use serde::{Deserialize, Serialize};

/// Structured IPC extension fields; JSON decoding/encoding stays in the worker.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IpcValue {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    List(Vec<IpcValue>),
    Map(std::collections::BTreeMap<String, IpcValue>),
}

impl From<serde_json::Value> for IpcValue {
    fn from(value: serde_json::Value) -> Self {
        use serde_json::Value;
        match value {
            Value::Null => Self::Null, Value::Bool(v) => Self::Bool(v),
            Value::Number(v) => Self::Number(v.as_f64().unwrap_or_default()),
            Value::String(v) => Self::String(v),
            Value::Array(v) => Self::List(v.into_iter().map(Self::from).collect()),
            Value::Object(v) => Self::Map(v.into_iter().map(|(k,v)| (k, Self::from(v))).collect()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub ready: bool,
    pub error: String,
}

macro_rules! record {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name { $(pub $field: $ty),* }
    };
}

record!(AccessPoint {
    id: String,
    device: String,
    ssid: String,
    ssid_bytes: Vec<u8>,
    bssid: String,
    strength: u8,
    frequency: u32,
    rate: u32,
    security: String,
    active: bool
});
record!(ActiveConnection {
    id: String,
    // NetworkManager connection type, e.g. "vpn", "wireguard", "802-11-wireless".
    kind: String,
    state: u32,
    vpn: bool,
    uuid: String
});
record!(NetworkState {
    status: ServiceStatus, wifi_enabled: bool, ethernet: bool,
    connectivity: u32, access_points: Vec<AccessPoint>,
    // Names of saved Wi-Fi profiles (connection ids), for "known network" hints.
    known_profiles: Vec<String>,
    // Currently active connections (VPN/wireguard/wifi/ethernet), event-driven so
    // VPN status needs no nmcli polling.
    active_connections: Vec<ActiveConnection>
});
record!(PowerState {
    status: ServiceStatus, active_profile: String, profiles: Vec<String>, degraded: String
});
record!(Backlight {
    id: String,
    kind: String,
    raw: u32,
    maximum: u32,
    value: f64
});
record!(BrightnessState { status: ServiceStatus, devices: Vec<Backlight> });
record!(KeyboardLockState { status: ServiceStatus, caps: bool, num: bool, devices: u32 });
record!(NiriWindow {
    id: String,
    title: String,
    app_id: String,
    pid: i32,
    title_present: bool,
    app_id_present: bool,
    workspace_id: String,
    focused: bool,
    floating: bool,
    urgent: bool,
    focus_serial: u64,
    // Position in the scrolling layout (-1 when the window has no layout slot).
    // Windows are emitted already ordered by output/workspace/column/row so the
    // QML side no longer re-runs sortWindowsByLayout.
    column: i32,
    row: i32,
    // Full window geometry, carried so the QML facade can reconstruct the niri
    // `layout` object (tile_size, window_size, tile_pos_in_workspace_view)
    // consumers depend on, without losing fields vs the raw event stream.
    tile_width: f64,
    tile_height: f64,
    window_width: i32,
    window_height: i32,
    tile_pos_x: f64,
    tile_pos_y: f64,
    tile_pos_valid: bool,
    window_offset_x: f64,
    window_offset_y: f64,
    focus_timestamp: Option<(u64, u32)>
});
record!(NiriWorkspace {
    id: String,
    index: u8,
    name: String,
    name_present: bool,
    output: String,
    active: bool,
    focused: bool,
    active_window_id: String,
    urgent: bool
});
record!(NiriState {
    status: ServiceStatus,
    windows: std::sync::Arc<Vec<std::sync::Arc<NiriWindow>>>,
    workspaces: std::sync::Arc<Vec<std::sync::Arc<NiriWorkspace>>>,
    overview_open: bool, keyboard_layouts: std::sync::Arc<Vec<String>>, keyboard_layout_index: u8,
    outputs: std::sync::Arc<std::collections::BTreeMap<String, IpcValue>>,
    mru_window_ids: std::sync::Arc<Vec<u64>>, config_generation: u64, config_failed: bool
});
record!(DesktopState {
    network: NetworkState,
    power: PowerState,
    brightness: BrightnessState,
    niri: NiriState
});

/// Commands are parsed before dispatch; credentials never enter snapshots/logs.
#[derive(Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    WifiEnabled {
        enabled: bool,
    },
    WifiScan,
    WifiDisconnect {
        device: String,
    },
    WifiConnect {
        access_point: String,
        password: Option<String>,
    },
    /// Connects to the strongest visible access point with this SSID.
    WifiConnectSsid {
        ssid: String,
        password: Option<String>,
    },
    /// Deletes the saved Wi-Fi profile named `ssid`.
    ProfileForget {
        ssid: String,
    },
    /// Replaces the saved PSK of the Wi-Fi profile named `ssid`.
    ProfileSetPassword {
        ssid: String,
        password: String,
    },
    /// Reads the saved PSK of the Wi-Fi profile named `ssid` (returned as the payload).
    ProfileSecret {
        ssid: String,
    },
    PowerProfile {
        profile: String,
    },
    Brightness {
        device: String,
        value: f64,
    },
    Niri {
        action: serde_json::Value,
    },
    NiriAction {
        name: String,
        parameters: IpcValue,
    },
    NiriRefreshOutputs,
}
