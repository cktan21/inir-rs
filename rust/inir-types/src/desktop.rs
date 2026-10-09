//! Service snapshots contain no Qt objects or subprocess output.
use serde::{Deserialize, Serialize};

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
record!(NetworkState {
    status: ServiceStatus, wifi_enabled: bool, ethernet: bool,
    connectivity: u32, access_points: Vec<AccessPoint>
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
record!(NiriWindow {
    id: String,
    title: String,
    app_id: String,
    workspace_id: String,
    focused: bool,
    floating: bool,
    urgent: bool,
    focus_serial: u64,
    // Position in the scrolling layout (-1 when the window has no layout slot).
    // Windows are emitted already ordered by output/workspace/column/row so the
    // QML side no longer re-runs sortWindowsByLayout.
    column: i32,
    row: i32
});
record!(NiriWorkspace {
    id: String,
    index: u8,
    name: String,
    output: String,
    active: bool,
    focused: bool,
    active_window_id: String,
    urgent: bool
});
record!(NiriState {
    status: ServiceStatus, windows: Vec<NiriWindow>, workspaces: Vec<NiriWorkspace>,
    overview_open: bool, keyboard_layouts: Vec<String>, keyboard_layout_index: u8
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
}
