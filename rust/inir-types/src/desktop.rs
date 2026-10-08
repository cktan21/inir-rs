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
record!(BluetoothAdapter {
    id: String,
    address: String,
    name: String,
    powered: bool,
    discovering: bool
});
record!(BluetoothDevice {
    id: String, adapter: String, address: String, name: String, icon: String,
    paired: bool, trusted: bool, connected: bool, battery: Option<f64>
});
record!(BluetoothState {
    status: ServiceStatus, adapters: Vec<BluetoothAdapter>, devices: Vec<BluetoothDevice>
});
record!(BatteryState {
    status: ServiceStatus,
    available: bool,
    on_battery: bool,
    state: u32,
    percentage: f64,
    energy_rate: f64,
    time_to_empty: i64,
    time_to_full: i64
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
record!(AudioNode {
    id: String, serial: String, name: String, description: String, media_class: String,
    volume: Option<f64>, muted: Option<bool>, channels: Vec<f32>
});
record!(AudioState {
    status: ServiceStatus, default_sink: String, default_source: String, nodes: Vec<AudioNode>
});
record!(MediaPlayer {
    id: String,
    identity: String,
    playback_status: String,
    title: String,
    artist: String,
    art_url: String,
    length: i64,
    can_control: bool,
    can_seek: bool
});
record!(MediaState { status: ServiceStatus, players: Vec<MediaPlayer> });
record!(NiriWindow {
    id: String,
    title: String,
    app_id: String,
    workspace_id: String,
    focused: bool,
    floating: bool,
    urgent: bool,
    focus_serial: u64
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
    bluetooth: BluetoothState,
    battery: BatteryState,
    power: PowerState,
    brightness: BrightnessState,
    audio: AudioState,
    media: MediaState,
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
    BluetoothEnabled {
        adapter: String,
        enabled: bool,
    },
    BluetoothDiscovery {
        adapter: String,
        enabled: bool,
    },
    BluetoothConnect {
        device: String,
    },
    BluetoothDisconnect {
        device: String,
    },
    BluetoothPair {
        device: String,
    },
    BluetoothForget {
        adapter: String,
        device: String,
    },
    PowerProfile {
        profile: String,
    },
    Brightness {
        device: String,
        value: f64,
    },
    AudioVolume {
        node: String,
        value: f64,
    },
    AudioMute {
        node: String,
        muted: bool,
    },
    Media {
        player: String,
        method: String,
    },
    Niri {
        action: serde_json::Value,
    },
}
