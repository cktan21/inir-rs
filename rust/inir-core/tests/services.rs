use inir_core::{
    events::Event,
    runtime::Backend,
    services::{brightness, network, niri},
};
use inir_types::desktop::*;
use pipewire::spa::{
    self,
    pod::{Object, Property, Value, ValueArray},
};
use std::{
    collections::HashMap,
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
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

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

#[test]
fn audio_spa_props_use_cubic_volume_and_reject_invalid_channels() {
    let mut node = AudioNode::default();
    let props = |values| {
        Value::Object(Object {
            type_: spa::utils::SpaTypes::ObjectParamProps.as_raw(),
            id: spa::param::ParamType::Props.as_raw(),
            properties: vec![
                Property::new(spa::sys::SPA_PROP_mute, Value::Bool(true)),
                Property::new(
                    spa::sys::SPA_PROP_channelVolumes,
                    Value::ValueArray(ValueArray::Float(values)),
                ),
            ],
        })
    };
    inir_core::services::audio::apply_props(&mut node, props(vec![0.125, 0.064]));
    assert_eq!(node.muted, Some(true));
    assert!((node.volume.unwrap() - 0.5).abs() < 1e-6);
    inir_core::services::audio::apply_props(&mut node, props(vec![f32::NAN]));
    assert_eq!(node.channels, vec![0.125, 0.064]);
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
    let Event::Niri(state) = incoming.recv().await.unwrap() else {
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
