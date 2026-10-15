use inir_core::{
    events::Event,
    runtime::Backend,
    services::{brightness, network::{self, Mirror, Signal}, niri},
};
use inir_types::desktop::*;
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

#[test]
fn access_point_security_and_identity() {
    let props = HashMap::from([
        (
            "Ssid".into(),
            OwnedValue::try_from(zbus::zvariant::Value::from(b"caf\xc3\xa9:test".to_vec()))
                .unwrap(),
        ),
        ("Strength".into(), OwnedValue::from(72u8)),
        ("Frequency".into(), OwnedValue::from(5220u32)),
        ("RsnFlags".into(), OwnedValue::from(0x200u32)),
    ]);
    let ap = network::access_point("/ap/1", "/device/1", &props, "/ap/1");
    assert_eq!(ap.ssid, "café:test");
    assert!(ap.active);
    assert_eq!(ap.security, "enterprise");
    assert_eq!(ap.strength, 72);
    assert_eq!(ap.frequency, 5220);
    for (bits, expected) in [
        (0x2000, "enterprise"),
        (0x800, "owe"),
        (0x400, "wpa3"),
        (0x100, "wpa"),
    ] {
        let p = HashMap::from([("RsnFlags".into(), OwnedValue::from(bits as u32))]);
        assert_eq!(
            network::access_point("/ap/1", "/device/1", &p, "").security,
            expected
        );
    }
    let hidden = network::access_point("/ap/2", "/device/1", &HashMap::new(), "/ap/1");
    assert!(hidden.ssid.is_empty());
    assert!(!hidden.active);
    assert!(hidden.security.is_empty());
}

#[test]
fn backlight_reading_and_write_validation() {
    let dir = tempfile::tempdir().unwrap();
    for (name, max, raw) in [("stub", 1, 1), ("panel", 1000, 250)] {
        let path = dir.path().join(name);
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("max_brightness"), max.to_string()).unwrap();
        std::fs::write(path.join("brightness"), raw.to_string()).unwrap();
    }
    let state = brightness::snapshot(dir.path()).unwrap();
    assert_eq!(state.devices[0].id, "panel");
    assert_eq!(state.devices[0].value, 0.25);
    let (_, raw) = brightness::target(dir.path(), "panel", 0.413).unwrap();
    assert_eq!(raw, 413);
    for value in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(brightness::target(dir.path(), "panel", value).is_err());
    }
    for name in ["", "..", "../panel", "panel/brightness"] {
        assert!(brightness::target(dir.path(), name, 0.5).is_err());
    }
}

fn window(id: u64, focused: bool) -> niri_ipc::Window {
    serde_json::from_value(serde_json::json!({"id":id,"title":"Window","app_id":"app","pid":null,"workspace_id":1,"is_focused":focused,"is_floating":false,"is_urgent":false,"layout":{"pos_in_scrolling_layout":null,"tile_size":[100.0,100.0],"window_size":[100,100],"tile_pos_in_workspace_view":null,"window_offset_in_tile":[0.0,0.0]},"focus_timestamp":null})).unwrap()
}

#[test]
fn niri_focus_close_urgency_and_stale_workspace_updates() {
    let mut engine = niri::Engine::default();
    engine.apply(niri_ipc::Event::KeyboardLayoutSwitched { idx: 1 });
    engine.apply(niri_ipc::Event::WindowClosed { id: 99 });
    engine.apply(niri_ipc::Event::CastStopped { stream_id: 99 });
    engine.apply(niri_ipc::Event::WindowLayoutsChanged {
        changes: vec![(99, window(99, false).layout)],
    });
    engine.apply(niri_ipc::Event::WorkspaceActivated {
        id: 99,
        focused: true,
    });
    engine.apply(niri_ipc::Event::WindowsChanged {
        windows: vec![window(1, true), window(2, false)],
    });
    engine.apply(niri_ipc::Event::WindowFocusChanged { id: Some(2) });
    engine.apply(niri_ipc::Event::WindowUrgencyChanged {
        id: 1,
        urgent: true,
    });
    let state = engine.snapshot();
    assert_eq!(state.windows.iter().filter(|w| w.focused).count(), 1);
    assert!(state.windows[0].urgent);
    assert!(state.windows[1].focus_serial > state.windows[0].focus_serial);
    engine.apply(niri_ipc::Event::WindowClosed { id: 2 });
    assert_eq!(engine.snapshot().windows.len(), 1);
}

#[test]
fn niri_facade_state_keeps_geometry_nulls_history_and_config() {
    let mut engine = niri::Engine::default();
    let mut first = window(1, true);
    first.pid = None;
    first.workspace_id = None;
    first.title = None;
    first.layout.window_offset_in_tile = (3.5, 7.0);
    first.layout.tile_pos_in_workspace_view = Some((-40.0, 12.5));
    engine.apply(niri_ipc::Event::WindowsChanged {
        windows: vec![first, window(2, false)],
    });
    assert!(!engine.snapshot().status.ready);
    engine.apply(niri_ipc::Event::WorkspacesChanged { workspaces: vec![] });
    engine.apply(niri_ipc::Event::KeyboardLayoutsChanged {
        keyboard_layouts: niri_ipc::KeyboardLayouts {
            names: vec!["English".into(), "German".into()],
            current_idx: 1,
        },
    });
    engine.apply(niri_ipc::Event::OverviewOpenedOrClosed { is_open: true });
    engine.apply(niri_ipc::Event::ConfigLoaded { failed: false });
    engine.apply(niri_ipc::Event::ConfigLoaded { failed: false });
    let state = engine.snapshot();
    assert!(state.status.ready && state.overview_open);
    assert_eq!(state.config_generation, 2);
    assert_eq!(state.keyboard_layout_index, 1);
    assert_eq!(*state.mru_window_ids, vec![1]);
    let first = &state.windows[0];
    assert!(!first.title_present && first.workspace_id.is_empty());
    assert_eq!((first.window_offset_x, first.window_offset_y), (3.5, 7.0));
    assert!(first.tile_pos_valid);
    assert_eq!((first.tile_pos_x, first.tile_pos_y), (-40.0, 12.5));
    engine.apply(niri_ipc::Event::WindowFocusChanged { id: Some(2) });
    engine.apply(niri_ipc::Event::WindowFocusChanged { id: None });
    assert_eq!(*engine.snapshot().mru_window_ids, vec![2, 1]);
    assert!(!engine.snapshot().windows.iter().any(|w| w.focused));
    engine.apply(niri_ipc::Event::WindowClosed { id: 2 });
    assert_eq!(*engine.snapshot().mru_window_ids, vec![1]);

    let mut restored = niri::Engine::default();
    let mut older = window(1, false);
    older.focus_timestamp = Some(niri_ipc::Timestamp { secs: 10, nanos: 0 });
    let mut newer = window(2, false);
    newer.focus_timestamp = Some(niri_ipc::Timestamp { secs: 20, nanos: 0 });
    restored.apply(niri_ipc::Event::WindowsChanged {
        windows: vec![newer, older],
    });
    assert_eq!(*restored.snapshot().mru_window_ids, vec![2, 1]);
}

#[test]
fn niri_title_and_geometry_changes_preserve_unaffected_rows() {
    use inir_core::model::{diff, ModelDelta};
    let mut engine = niri::Engine::default();
    engine.apply(niri_ipc::Event::WorkspacesChanged { workspaces: vec![] });
    engine.apply(niri_ipc::Event::WindowsChanged {
        windows: (1..=100).map(|id| window(id, id == 1)).collect(),
    });
    let before = engine.frame();
    let built = engine.projected_window_updates;
    let sorts = engine.layout_sorts;
    let mut changed = window(50, false);
    changed.title = Some("title churn".into());
    engine.apply(niri_ipc::Event::WindowOpenedOrChanged { window: changed.clone() });
    let after = engine.frame();
    assert_eq!(engine.projected_window_updates - built, 1);
    assert_eq!(engine.layout_sorts, sorts);
    assert!(Arc::ptr_eq(&before.workspaces, &after.workspaces));
    for index in 0..100 {
        assert_eq!(Arc::ptr_eq(&before.windows[index], &after.windows[index]), index != 49);
    }
    assert!(matches!(diff(&before.windows, &after.windows).as_slice(),
        [ModelDelta::Change { index: 49, roles: 2, .. }]));
    assert_ne!(before.windows[49].title, "title churn");
    changed.layout.tile_size.0 += 1.0;
    engine.apply(niri_ipc::Event::WindowLayoutsChanged { changes: vec![(50, changed.layout)] });
    let geometry = engine.frame();
    assert_eq!(engine.layout_sorts, sorts);
    assert!(matches!(diff(&after.windows, &geometry.windows).as_slice(),
        [ModelDelta::Change { index: 49, roles, .. }] if *roles == 1 << 11));
}

#[test]
fn native_leases_are_independent_and_released_on_drop() {
    let backend = Backend::new().unwrap();
    let a = backend.acquire(3);
    let b = backend.acquire(3);
    assert_eq!(backend.domain_consumers(), [0, 0, 0, 2, 0]);
    drop(a);
    assert_eq!(backend.domain_consumers(), [0, 0, 0, 1, 0]);
    drop(b);
    assert_eq!(backend.domain_consumers(), [0, 0, 0, 0, 0]);
}

#[tokio::test]
async fn niri_actions_reuse_connection_and_do_not_replay_failed_actions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("niri.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        for reply in ["{\"Ok\":\"Handled\"}\n", "{\"Err\":\"rejected\"}\n"] {
            let mut line = String::new();
            assert!(stream.read_line(&mut line).await.unwrap() > 0);
            stream.get_mut().write_all(reply.as_bytes()).await.unwrap();
        }
        let mut line = String::new();
        // Accept the third command but close without a reply. It must not be replayed.
        assert!(stream.read_line(&mut line).await.unwrap() > 0);
        drop(stream);
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        line.clear();
        stream.read_line(&mut line).await.unwrap();
        assert!(line.contains("FocusWindow"));
        stream
            .get_mut()
            .write_all(b"{\"Ok\":\"Handled\"}\n")
            .await
            .unwrap();
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut client = niri::ActionClient::default();
        let action = serde_json::json!({ "FocusWindow": { "id": 1 } });
        client.action(&path, action.clone()).await.unwrap();
        assert_eq!(
            client.action(&path, action.clone()).await.unwrap_err(),
            "rejected"
        );
        assert!(client.action(&path, action.clone()).await.is_err());
        client.action(&path, action).await.unwrap();
        server.await.unwrap();
    })
    .await
    .unwrap();
}

#[test]
fn niri_layout_order_uses_output_position_before_workspace_and_column() {
    let mut engine = niri::Engine::default();
    let outputs = [("A-right", 0), ("Z-left", -1920)].into_iter().map(|(name, x)| {
        let output = serde_json::from_value(serde_json::json!({
            "name": name, "make": "test", "model": "test", "serial": null,
            "physical_size": null, "modes": [], "current_mode": null, "is_custom_mode": false,
            "vrr_supported": false, "vrr_enabled": false,
            "logical": { "x": x, "y": 0, "width": 1920, "height": 1080, "scale": 1.0, "transform": "Normal" }
        })).unwrap();
        (name.to_owned(), output)
    }).collect();
    engine.set_outputs(outputs);
    let workspaces = [(1, "A-right"), (2, "Z-left")]
        .into_iter()
        .map(|(id, output)| {
            serde_json::from_value(serde_json::json!({
                "id": id, "idx": 1, "name": null, "output": output,
                "is_urgent": false, "is_active": true, "is_focused": id == 1,
                "active_window_id": null
            }))
            .unwrap()
        })
        .collect();
    engine.apply(niri_ipc::Event::WorkspacesChanged { workspaces });
    let mut windows = vec![
        window(9, false),
        window(2, false),
        window(3, false),
        window(1, true),
    ];
    windows[0].workspace_id = None;
    windows[1].layout.pos_in_scrolling_layout = Some((3, 1));
    windows[2].workspace_id = Some(2);
    windows[3].layout.pos_in_scrolling_layout = Some((1, 1));
    engine.apply(niri_ipc::Event::WindowsChanged { windows });
    let state = engine.snapshot();
    assert_eq!(
        state
            .windows
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>(),
        vec!["3", "1", "2", "9"]
    );
    let output = serde_json::to_value(&state.outputs["Z-left"]).unwrap();
    assert_eq!(output["logical"]["x"].as_f64(), Some(-1920.0));
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.login1")]
enum LoginError {
    NoSessionForPID(String),
}

struct LoginManager(bool);
#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl LoginManager {
    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, _pid: u32) -> Result<OwnedObjectPath, LoginError> {
        if self.0 {
            Ok(OwnedObjectPath::try_from("/session/direct").unwrap())
        } else {
            Err(LoginError::NoSessionForPID("user service".into()))
        }
    }
    fn get_user(&self, uid: u32) -> OwnedObjectPath {
        assert_eq!(uid, unsafe { libc::geteuid() });
        OwnedObjectPath::try_from("/user/current").unwrap()
    }
}

struct LoginUser(bool);
#[zbus::interface(name = "org.freedesktop.login1.User")]
impl LoginUser {
    #[zbus(property)]
    fn display(&self) -> (String, OwnedObjectPath) {
        if self.0 {
            (
                "2".into(),
                OwnedObjectPath::try_from("/session/display").unwrap(),
            )
        } else {
            (String::new(), OwnedObjectPath::try_from("/").unwrap())
        }
    }
}

#[tokio::test]
async fn backlight_resolves_display_session_for_user_services() {
    for (direct, display, expected) in [
        (true, true, Some("/session/direct")),
        (false, true, Some("/session/display")),
        (false, false, None),
    ] {
        let (server, client) = UnixStream::pair().unwrap();
        let builder = zbus::connection::Builder::unix_stream(server)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at("/org/freedesktop/login1", LoginManager(direct))
            .unwrap()
            .serve_at("/user/current", LoginUser(display))
            .unwrap();
        let (server, client) = tokio::join!(
            builder.build(),
            zbus::connection::Builder::unix_stream(client).p2p().build()
        );
        let _server = server.unwrap();
        let result = brightness::logind_session(&client.unwrap()).await;
        if let Some(path) = expected {
            assert_eq!(result.unwrap().as_str(), path);
        } else {
            assert!(result.unwrap_err().contains("no display session"));
        }
    }
}

#[tokio::test]
async fn niri_stream_and_action_use_separate_sockets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("niri.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        assert_eq!(line.trim(), "\"EventStream\"");
        stream
            .get_mut()
            .write_all(b"{\"Ok\":\"Handled\"}\n")
            .await
            .unwrap();
        let event = niri_ipc::Event::WindowsChanged {
            windows: vec![window(1, true)],
        };
        stream
            .get_mut()
            .write_all(format!("{}\n", serde_json::to_string(&event).unwrap()).as_bytes())
            .await
            .unwrap();
        // Forward-compatible unknown events must not kill the connection.
        stream
            .get_mut()
            .write_all(b"{\"FutureEvent\":{}}\n")
            .await
            .unwrap();
        // The engine queries outputs on its own short-lived connection at startup.
        let (outputs, _) = listener.accept().await.unwrap();
        let mut outputs = BufReader::new(outputs);
        line.clear();
        outputs.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<niri_ipc::Request>(&line).unwrap(),
            niri_ipc::Request::Outputs
        ));
        outputs
            .get_mut()
            .write_all(b"{\"Ok\":{\"Outputs\":{}}}\n")
            .await
            .unwrap();
        let (action, _) = listener.accept().await.unwrap();
        let mut action = BufReader::new(action);
        line.clear();
        action.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<niri_ipc::Request>(&line).unwrap(),
            niri_ipc::Request::Action(niri_ipc::Action::FocusWindow { id: 1 })
        ));
        action
            .get_mut()
            .write_all(b"{\"Err\":\"test rejection\"}\n")
            .await
            .unwrap();
    });
    let (events, mut incoming) = tokio::sync::mpsc::channel(16);
    let stop = tokio_util::sync::CancellationToken::new();
    let worker_path = path.clone();
    let worker_stop = stop.clone();
    let worker =
        tokio::spawn(async move { niri::stream(&worker_path, &events, &worker_stop).await });
    let Event::NiriFrame(state) = incoming.recv().await.unwrap() else {
        panic!()
    };
    assert_eq!(state.windows[0].id, "1");
    let result = niri::request(
        &path,
        niri_ipc::Request::Action(niri_ipc::Action::FocusWindow { id: 1 }),
    )
    .await;
    assert_eq!(result.unwrap_err(), "test rejection");
    server.await.unwrap();
    stop.cancel();
    let _ = worker.await.unwrap();
}

struct Manager;
#[zbus::interface(name = "org.freedesktop.NetworkManager")]
impl Manager {
    #[zbus(property)]
    fn wireless_enabled(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn connectivity(&self) -> u32 {
        4
    }
    #[zbus(property)]
    fn devices(&self) -> Vec<OwnedObjectPath> {
        vec![OwnedObjectPath::try_from("/device/1").unwrap()]
    }
    fn get_devices(&self) -> Vec<OwnedObjectPath> {
        self.devices()
    }
}
struct Device;
#[zbus::interface(name = "org.freedesktop.NetworkManager.Device")]
impl Device {
    #[zbus(property)]
    fn device_type(&self) -> u32 {
        2
    }
    #[zbus(property)]
    fn state(&self) -> u32 {
        100
    }
}
struct Wireless(Arc<AtomicUsize>);
#[zbus::interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
impl Wireless {
    #[zbus(property)]
    fn active_access_point(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from("/ap/1").unwrap()
    }
    fn get_all_access_points(&self) -> Vec<OwnedObjectPath> {
        vec![self.active_access_point()]
    }
    fn request_scan(&self, _options: HashMap<String, OwnedValue>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
struct Ap;
#[zbus::interface(name = "org.freedesktop.NetworkManager.AccessPoint")]
impl Ap {
    #[zbus(property)]
    fn ssid(&self) -> Vec<u8> {
        b"mock wifi".to_vec()
    }
    #[zbus(property)]
    fn strength(&self) -> u8 {
        75
    }
    #[zbus(property)]
    fn frequency(&self) -> u32 {
        2412
    }
}

// NetworkManager's ObjectManager, served at /org/freedesktop as the daemon does.
struct ObjectRoot;
#[zbus::interface(name = "org.freedesktop.DBus.ObjectManager")]
impl ObjectRoot {
    fn get_managed_objects(
        &self,
    ) -> HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>> {
        managed_objects()
    }
}

fn path(value: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(value).unwrap()
}

fn props(pairs: Vec<(&str, Value<'_>)>) -> HashMap<String, OwnedValue> {
    pairs
        .into_iter()
        .map(|(name, value)| (name.to_owned(), OwnedValue::try_from(value).unwrap()))
        .collect()
}

fn ap_props(ssid: &str, strength: u8) -> HashMap<String, OwnedValue> {
    props(vec![
        ("Ssid", Value::from(ssid.as_bytes().to_vec())),
        ("Strength", Value::from(strength)),
        ("Frequency", Value::from(2412u32)),
        ("Flags", Value::from(1u32)),
        ("WpaFlags", Value::from(0u32)),
        ("RsnFlags", Value::from(0u32)),
        ("HwAddress", Value::from("00:11:22:33:44:55")),
        ("MaxBitrate", Value::from(54000u32)),
    ])
}

fn managed_objects() -> HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>> {
    HashMap::from([
        (
            path("/device/1"),
            HashMap::from([
                (
                    "org.freedesktop.NetworkManager.Device".to_owned(),
                    props(vec![
                        ("DeviceType", Value::from(2u32)),
                        ("State", Value::from(100u32)),
                    ]),
                ),
                (
                    "org.freedesktop.NetworkManager.Device.Wireless".to_owned(),
                    props(vec![
                        ("ActiveAccessPoint", Value::from(path("/ap/1"))),
                        ("AccessPoints", Value::from(vec![path("/ap/1")])),
                    ]),
                ),
            ]),
        ),
        (
            path("/ap/1"),
            HashMap::from([(
                "org.freedesktop.NetworkManager.AccessPoint".to_owned(),
                ap_props("mock wifi", 75),
            )]),
        ),
    ])
}

fn mirror_with_one_access_point() -> Mirror {
    let mut mirror = Mirror::default();
    mirror.add(
        "/device/1",
        &HashMap::from([
            (
                "org.freedesktop.NetworkManager.Device".to_owned(),
                props(vec![
                    ("DeviceType", Value::from(2u32)),
                    ("State", Value::from(100u32)),
                ]),
            ),
            (
                "org.freedesktop.NetworkManager.Device.Wireless".to_owned(),
                props(vec![
                    ("ActiveAccessPoint", Value::from(path("/ap/1"))),
                    ("AccessPoints", Value::from(vec![path("/ap/1")])),
                ]),
            ),
        ]),
    );
    mirror.add(
        "/ap/1",
        &HashMap::from([(
            "org.freedesktop.NetworkManager.AccessPoint".to_owned(),
            ap_props("home", 40),
        )]),
    );
    mirror
}

#[test]
fn network_mirror_applies_access_point_signals_without_refetch() {
    const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
    const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
    let mut mirror = mirror_with_one_access_point();
    let mut refresh = BTreeSet::new();
    let state = mirror.state();
    assert_eq!(state.access_points.len(), 1);
    assert!(state.access_points[0].active);

    // Strength is applied in place; nothing is queued for a re-read.
    mirror.apply(
        Signal::Properties {
            path: "/ap/1".into(),
            interface: AP.into(),
            changed: props(vec![("Strength", Value::from(80u8))]),
            invalidated: vec![],
        },
        &mut refresh,
    );
    assert_eq!(mirror.state().access_points[0].strength, 80);

    // A new AP arrives with its full properties, then joins the device.
    mirror.apply(
        Signal::Added {
            path: "/ap/2".into(),
            interfaces: HashMap::from([(AP.into(), ap_props("guest", 60))]),
        },
        &mut refresh,
    );
    mirror.apply(
        Signal::AccessPoint {
            device: "/device/1".into(),
            path: "/ap/2".into(),
            added: true,
        },
        &mut refresh,
    );
    let ssids: Vec<_> = mirror.state().access_points.iter().map(|ap| ap.ssid.clone()).collect();
    assert_eq!(ssids, ["home", "guest"]);

    // Moving the active AP flips only the device's pointer.
    mirror.apply(
        Signal::Properties {
            path: "/device/1".into(),
            interface: WIRELESS.into(),
            changed: props(vec![("ActiveAccessPoint", Value::from(path("/ap/2")))]),
            invalidated: vec![],
        },
        &mut refresh,
    );
    let state = mirror.state();
    assert!(state.access_points.iter().find(|ap| ap.ssid == "guest").unwrap().active);
    assert!(!state.access_points.iter().find(|ap| ap.ssid == "home").unwrap().active);

    // Removal drops the AP from the device and from the table.
    mirror.apply(
        Signal::AccessPoint {
            device: "/device/1".into(),
            path: "/ap/2".into(),
            added: false,
        },
        &mut refresh,
    );
    mirror.apply(
        Signal::Removed {
            path: "/ap/2".into(),
            interfaces: vec![AP.into()],
        },
        &mut refresh,
    );
    assert_eq!(mirror.state().access_points.len(), 1);
    assert!(refresh.is_empty(), "applied deltas must not queue re-reads");
}

#[test]
fn network_mirror_rereads_only_invalidated_or_unknown_objects() {
    const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
    let mut mirror = mirror_with_one_access_point();
    let mut refresh = BTreeSet::new();
    mirror.apply(
        Signal::Properties {
            path: "/ap/1".into(),
            interface: AP.into(),
            changed: HashMap::new(),
            invalidated: vec!["Strength".into()],
        },
        &mut refresh,
    );
    assert_eq!(refresh, BTreeSet::from([("/ap/1".to_owned(), AP.to_owned())]));
    refresh.clear();
    // An AP we never saw cannot be patched; it is re-read once instead.
    mirror.apply(
        Signal::Properties {
            path: "/ap/9".into(),
            interface: AP.into(),
            changed: props(vec![("Strength", Value::from(10u8))]),
            invalidated: vec![],
        },
        &mut refresh,
    );
    assert_eq!(refresh, BTreeSet::from([("/ap/9".to_owned(), AP.to_owned())]));
    assert_eq!(mirror.state().access_points.len(), 1);
}

#[test]
fn network_mirror_tracks_saved_wifi_profiles() {
    let mut mirror = mirror_with_one_access_point();
    let mut refresh = BTreeSet::new();
    assert!(mirror.state().known_profiles.is_empty());
    // A new profile is read before it is listed, so its name is not in the signal.
    mirror.apply(
        Signal::Profile { path: "/settings/1".into(), added: true },
        &mut refresh,
    );
    assert_eq!(
        refresh,
        BTreeSet::from([(
            "/settings/1".to_owned(),
            "org.freedesktop.NetworkManager.Settings.Connection".to_owned(),
        )])
    );
    refresh.clear();
    mirror.set_wifi_profile("/settings/1", Some("home".into()));
    mirror.set_wifi_profile("/settings/2", Some("home".into()));
    mirror.set_wifi_profile("/settings/3", Some("cafe".into()));
    assert_eq!(mirror.state().known_profiles, ["cafe", "home"]);
    mirror.apply(
        Signal::Profile { path: "/settings/1".into(), added: false },
        &mut refresh,
    );
    assert_eq!(mirror.state().known_profiles, ["cafe", "home"]);
    mirror.apply(
        Signal::Profile { path: "/settings/2".into(), added: false },
        &mut refresh,
    );
    assert_eq!(mirror.state().known_profiles, ["cafe"]);
    assert!(refresh.is_empty());
}

#[test]
fn network_mirror_ignores_signal_strength_jitter() {
    const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
    let mut mirror = mirror_with_one_access_point(); // strength 40
    let mut refresh = BTreeSet::new();
    let mut strength = |mirror: &mut Mirror, value: u8| {
        mirror.apply(
            Signal::Properties {
                path: "/ap/1".into(),
                interface: AP.into(),
                changed: props(vec![("Strength", Value::from(value))]),
                invalidated: vec![],
            },
            &mut refresh,
        );
        mirror.state().access_points[0].strength
    };
    // ±1-4 point jitter does not republish (no redraw of strength indicators).
    assert_eq!(strength(&mut mirror, 42), 40);
    assert_eq!(strength(&mut mirror, 37), 40);
    // A real move lands, and slow drift lands once it accumulates past 5 points.
    assert_eq!(strength(&mut mirror, 45), 45);
    assert_eq!(strength(&mut mirror, 48), 45);
    assert_eq!(strength(&mut mirror, 50), 50);
}

#[test]
fn network_mirror_tracks_active_connections_without_polling() {
    const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
    let mut mirror = mirror_with_one_access_point();
    let mut refresh = BTreeSet::new();
    assert!(mirror.state().active_connections.is_empty());
    // A VPN activates: InterfacesAdded carries its full properties.
    mirror.apply(
        Signal::Added {
            path: "/active/1".into(),
            interfaces: HashMap::from([(
                ACTIVE.into(),
                props(vec![
                    ("Id", Value::from("work-vpn")),
                    ("Type", Value::from("vpn")),
                    ("State", Value::from(2u32)),
                    ("Vpn", Value::from(true)),
                    ("Uuid", Value::from("uuid-vpn")),
                ]),
            )]),
        },
        &mut refresh,
    );
    let active = mirror.state().active_connections;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "work-vpn");
    assert!(active[0].vpn);
    assert_eq!(active[0].state, 2);
    // A state change arrives as a PropertiesChanged body, applied in place.
    mirror.apply(
        Signal::Properties {
            path: "/active/1".into(),
            interface: ACTIVE.into(),
            changed: props(vec![("State", Value::from(4u32))]),
            invalidated: vec![],
        },
        &mut refresh,
    );
    assert_eq!(mirror.state().active_connections[0].state, 4);
    assert!(refresh.is_empty(), "applied active-connection deltas must not refetch");
    // Deactivation removes it.
    mirror.apply(
        Signal::Removed {
            path: "/active/1".into(),
            interfaces: vec![ACTIVE.into()],
        },
        &mut refresh,
    );
    assert!(mirror.state().active_connections.is_empty());
}

// NetworkManager's Settings service; the mock has no saved profiles.
struct SettingsService;
#[zbus::interface(name = "org.freedesktop.NetworkManager.Settings")]
impl SettingsService {
    fn list_connections(&self) -> Vec<OwnedObjectPath> {
        Vec::new()
    }
}

#[tokio::test]
async fn network_manager_wire_protocol_snapshot_and_scan() {
    let scans = Arc::new(AtomicUsize::new(0));
    let (server, client) = UnixStream::pair().unwrap();
    let builder = zbus::connection::Builder::unix_stream(server)
        .server(zbus::Guid::generate())
        .unwrap()
        .p2p()
        .serve_at("/org/freedesktop/NetworkManager", Manager)
        .unwrap()
        .serve_at("/org/freedesktop", ObjectRoot)
        .unwrap()
        .serve_at("/org/freedesktop/NetworkManager/Settings", SettingsService)
        .unwrap()
        .serve_at("/device/1", Device)
        .unwrap()
        .serve_at("/device/1", Wireless(scans.clone()))
        .unwrap()
        .serve_at("/ap/1", Ap)
        .unwrap();
    let (server, client) = tokio::join!(
        builder.build(),
        zbus::connection::Builder::unix_stream(client).p2p().build()
    );
    let _server = server.unwrap();
    let client = client.unwrap();
    let state = network::snapshot(&client).await.unwrap();
    assert!(state.status.ready);
    assert!(state.wifi_enabled);
    assert_eq!(state.connectivity, 4);
    assert_eq!(state.access_points[0].ssid, "mock wifi");
    assert!(state.access_points[0].active);
    network::execute(&client, Command::WifiScan).await.unwrap();
    assert_eq!(scans.load(Ordering::Relaxed), 1);
}

#[test]
fn service_workers_are_opt_in_and_commands_fail_while_inactive() {
    let backend = Backend::new().unwrap();
    assert_eq!(backend.service_consumers(), 0);
    let backend = Arc::new(backend);
    let (tx, rx) = std::sync::mpsc::channel();
    let task_backend = backend.clone();
    let _task = backend.spawn(async move {
        tx.send(task_backend.execute(Command::WifiScan).await)
            .unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_err());
    assert!(!backend.snapshot().desktop.network.status.ready);
}
