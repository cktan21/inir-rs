use inir_core::services::bluetooth_control::{
    self as control,
    agent::{Agent, Broker},
    network, transfers,
    types::{Leases, Prompt, State},
    Request,
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;
use zbus::{zvariant::OwnedObjectPath, Connection, Proxy};

struct Bus {
    child: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            child,
            address: address.trim().into(),
        }
    }
    async fn connect(&self) -> Connection {
        zbus::connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Clone, Default)]
struct Adapter {
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
    external: Arc<AtomicBool>,
    delay: u64,
}
#[zbus::interface(name = "org.bluez.Adapter1")]
impl Adapter {
    async fn start_discovery(&self) {
        self.starts.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(self.delay)).await;
    }
    fn stop_discovery(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
    }
    #[zbus(property)]
    fn discovering(&self) -> bool {
        self.external.load(Ordering::SeqCst)
    }
    #[zbus(property)]
    fn powered(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn address(&self) -> &str {
        "11:22:33:44:55:66"
    }
}
struct Device;
#[zbus::interface(name = "org.bluez.Device1")]
impl Device {
    #[zbus(property)]
    fn alias(&self) -> &str {
        "Test keyboard"
    }
    #[zbus(property)]
    fn trusted(&self) -> bool {
        false
    }
}

async fn wait_for(state: &mut watch::Receiver<State>, predicate: impl Fn(&State) -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if predicate(&state.borrow()) {
                break;
            }
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[test]
fn scan_leases_expire_independently_and_reconnect_cannot_extend_them() {
    let now = Instant::now();
    let mut leases = Leases::default();
    leases.request("a".into(), "hci0".into(), now);
    leases.request("b".into(), "hci0".into(), now + Duration::from_secs(10));
    leases.expire(now + Duration::from_secs(25));
    assert_eq!(leases.owners("hci0"), vec!["b"]);
    leases.request_for(
        "b".into(),
        "hci0".into(),
        now + Duration::from_secs(30),
        Duration::from_secs(5),
    );
    leases.expire(now + Duration::from_secs(35));
    assert!(leases.0.is_empty());
    leases.request_for("a".into(), "hci0".into(), now, Duration::from_secs(300));
    assert_eq!(leases.deadline(), Some(now + Duration::from_secs(25)));
}

#[test]
fn addressing_and_received_filenames_cannot_escape_their_scope() {
    use control::types::{adapter_path, device_path, safe_filename};
    assert_eq!(
        device_path("hci0", "aa:bb:cc:dd:ee:ff").unwrap(),
        "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"
    );
    for adapter in ["", "hci", "hci0/../hci1", "../hci0"] {
        assert!(adapter_path(adapter).is_err());
    }
    for address in ["", "../", "aa:bb:cc:dd:ee:ff/evil", "aa:bb:cc:dd:ee:gg"] {
        assert!(device_path("hci0", address).is_err());
    }
    assert_eq!(safe_filename("../../outside.txt"), "outside.txt");
    assert_eq!(safe_filename("..\\outside.txt"), "outside.txt");
    assert_eq!(safe_filename(".."), "Bluetooth file");
    assert_eq!(safe_filename("bad\0name"), "badname");
    assert!(safe_filename(&"🦀".repeat(180)).len() <= 180);
}

#[tokio::test]
async fn acknowledged_scans_handle_close_before_start_and_multiple_views() {
    let bus = Bus::new();
    let adapter = Adapter {
        delay: 80,
        ..Default::default()
    };
    adapter.external.store(true, Ordering::SeqCst);
    let server = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.bluez")
        .unwrap()
        .serve_at("/org/bluez/hci0", adapter.clone())
        .unwrap()
        .build()
        .await
        .unwrap();
    let client = bus.connect().await;
    let (controller, worker, _output) = control::channel();
    let mut state = controller.state.clone();
    let dir = tempfile::tempdir().unwrap();
    let task = tokio::spawn(worker.run_on(client, None, false, dir.path().into(), false));
    wait_for(&mut state, |s| s.ready).await;
    let request = |owner: &str, ms| Request::Scan {
        owner: owner.into(),
        adapter: "hci0".into(),
        deadline: Instant::now() + Duration::from_millis(ms),
    };
    controller
        .requests
        .send(request("closed", 1000))
        .await
        .unwrap();
    wait_for(&mut state, |s| s.scans.iter().any(|s| s.pending)).await;
    controller
        .requests
        .send(Request::Release {
            owner: "closed".into(),
        })
        .await
        .unwrap();
    wait_for(&mut state, |s| {
        s.scans
            .iter()
            .any(|s| !s.pending && !s.owned && s.owners.is_empty())
    })
    .await;
    assert_eq!(adapter.starts.load(Ordering::SeqCst), 1);
    assert_eq!(adapter.stops.load(Ordering::SeqCst), 1);
    controller.requests.send(request("one", 500)).await.unwrap();
    controller.requests.send(request("two", 700)).await.unwrap();
    wait_for(&mut state, |s| {
        s.scans.iter().any(|s| s.owned && s.owners.len() == 2)
    })
    .await;
    controller
        .requests
        .send(Request::Release {
            owner: "one".into(),
        })
        .await
        .unwrap();
    wait_for(&mut state, |s| {
        s.scans.iter().any(|s| s.owned && s.owners == ["two"])
    })
    .await;
    assert_eq!(adapter.stops.load(Ordering::SeqCst), 1);
    wait_for(&mut state, |s| {
        s.scans
            .iter()
            .any(|s| !s.owned && !s.pending && s.owners.is_empty())
    })
    .await;
    assert_eq!(adapter.starts.load(Ordering::SeqCst), 2);
    assert_eq!(adapter.stops.load(Ordering::SeqCst), 2);
    assert!(
        adapter.external.load(Ordering::SeqCst),
        "our stop must not change another client's session"
    );
    controller.stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    drop(server);
}

#[tokio::test]
async fn private_bus_agent_returns_typed_passkeys_and_cancels_pending_calls() {
    let bus = Bus::new();
    let server = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.bluez")
        .unwrap()
        .serve_at("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF", Device)
        .unwrap()
        .build()
        .await
        .unwrap();
    let client = bus.connect().await;
    let (sender, mut state) = watch::channel(State::default());
    let broker = Broker::new(sender);
    client
        .object_server()
        .at(
            "/org/inir/BluetoothAgent",
            Agent {
                broker: broker.clone(),
            },
        )
        .await
        .unwrap();
    let proxy = Proxy::new_owned(
        server.clone(),
        client.unique_name().unwrap().to_string(),
        "/org/inir/BluetoothAgent",
        "org.bluez.Agent1",
    )
    .await
    .unwrap();
    let device = OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap();
    let p = proxy.clone();
    let d = device.clone();
    let call = tokio::spawn(async move { p.call::<_, _, u32>("RequestPasskey", &(d,)).await });
    wait_for(&mut state, |s| !s.prompts.is_empty()).await;
    let prompt = state.borrow().prompts[0].clone();
    assert_eq!(prompt.name, "Test keyboard");
    broker.respond(&prompt.id, true, "000123".into()).unwrap();
    assert_eq!(call.await.unwrap().unwrap(), 123);

    let p = proxy.clone();
    let d = device.clone();
    let call = tokio::spawn(async move { p.call::<_, _, String>("RequestPinCode", &(d,)).await });
    wait_for(&mut state, |s| !s.prompts.is_empty()).await;
    proxy.call::<_, _, ()>("Cancel", &()).await.unwrap();
    assert!(call
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("Canceled"));
    assert!(state.borrow().prompts.is_empty());

    broker.set_locked(true);
    assert!(proxy
        .call::<_, _, ()>("RequestConfirmation", &(device.clone(), 123u32))
        .await
        .unwrap_err()
        .to_string()
        .contains("Canceled"));
    broker.set_locked(false);
    proxy
        .call::<_, _, ()>("DisplayPasskey", &(device.clone(), 7u32, 2u16))
        .await
        .unwrap();
    assert_eq!(state.borrow().prompts[0].value, "000007");
    proxy
        .call::<_, _, ()>("DisplayPasskey", &(device, 7u32, 3u16))
        .await
        .unwrap();
    assert_eq!(state.borrow().prompts.len(), 1);
    assert_eq!(state.borrow().prompts[0].entered, 3);
    broker.cancel_device("");
}

#[tokio::test]
async fn prompt_rejection_and_display_cleanup_do_not_leave_a_broker_alive() {
    let (tx, mut state) = watch::channel(State::default());
    let broker = Broker::new(tx);
    let b = broker.clone();
    let call = tokio::spawn(async move {
        b.ask(Prompt {
            kind: "confirmation".into(),
            device: "test".into(),
            ..Default::default()
        })
        .await
    });
    wait_for(&mut state, |s| !s.prompts.is_empty()).await;
    let id = state.borrow().prompts[0].id.clone();
    broker.respond(&id, false, String::new()).unwrap();
    assert!(call.await.unwrap().is_err());
    assert!(broker.respond(&id, true, "old response".into()).is_err());
    broker
        .display(Prompt {
            kind: "displayPin".into(),
            device: "test".into(),
            ..Default::default()
        })
        .unwrap();
    broker.set_locked(true);
    assert!(state.borrow().prompts.is_empty());
}

#[tokio::test]
async fn received_files_finalize_without_overwriting_or_escaping() {
    let root = tempfile::tempdir().unwrap();
    let staging = tempfile::Builder::new()
        .prefix(".incoming-")
        .tempdir_in(root.path())
        .unwrap();
    let path = staging.path().join("hello.txt");
    tokio::fs::write(&path, b"received").await.unwrap();
    tokio::fs::write(root.path().join("hello.txt"), b"existing")
        .await
        .unwrap();
    let destination = transfers::finalize(path.to_str().unwrap(), root.path(), "../../hello.txt")
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(root.path().join("hello.txt"))
            .await
            .unwrap(),
        b"existing"
    );
    assert_eq!(tokio::fs::read(destination).await.unwrap(), b"received");
    assert!(!path.exists());
    assert!(transfers::finalize("/tmp/unowned.txt", root.path(), "file")
        .await
        .is_err());
}

#[test]
fn tethering_profiles_have_correct_roles_and_do_not_autoconnect() {
    use inir_core::services::dbus::read;
    for (address, kind, method) in [
        (Some("AA:BB:CC:DD:EE:FF"), "panu", "auto"),
        (None, "nap", "shared"),
    ] {
        let profile = network::profile(address, "hci0").unwrap();
        assert_eq!(read::<String>(&profile["bluetooth"], "type"), kind);
        assert_eq!(read::<String>(&profile["ipv4"], "method"), method);
        assert!(!read::<bool>(&profile["connection"], "autoconnect"));
        assert_eq!(profile.contains_key("bridge"), address.is_none());
    }
}

struct AgentManager;
#[zbus::interface(name = "org.bluez.AgentManager1")]
impl AgentManager {
    fn register_agent(&self, _path: OwnedObjectPath, capability: &str) {
        assert_eq!(capability, "KeyboardDisplay");
    }
    fn request_default_agent(&self, _path: OwnedObjectPath) {}
    fn unregister_agent(&self, _path: OwnedObjectPath) {}
}
struct ObexManager;
#[zbus::interface(name = "org.bluez.obex.AgentManager1")]
impl ObexManager {
    fn register_agent(&self, _path: OwnedObjectPath) {}
    fn unregister_agent(&self, _path: OwnedObjectPath) {}
}
#[derive(Clone, Default)]
struct ObexClient {
    removed: Arc<AtomicUsize>,
}
#[zbus::interface(name = "org.bluez.obex.Client1")]
impl ObexClient {
    fn create_session(
        &self,
        destination: &str,
        options: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> OwnedObjectPath {
        assert_eq!(destination, "AA:BB:CC:DD:EE:FF");
        assert_eq!(
            inir_core::services::dbus::read::<String>(&options, "Target"),
            "opp"
        );
        assert_eq!(
            inir_core::services::dbus::read::<String>(&options, "Source"),
            "11:22:33:44:55:66"
        );
        OwnedObjectPath::try_from("/org/bluez/obex/session1").unwrap()
    }
    fn remove_session(&self, _path: OwnedObjectPath) {
        self.removed.fetch_add(1, Ordering::SeqCst);
    }
}
struct ObjectPush;
#[zbus::interface(name = "org.bluez.obex.ObjectPush1")]
impl ObjectPush {
    fn send_file(
        &self,
        file: &str,
    ) -> (
        OwnedObjectPath,
        std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    ) {
        assert!(std::path::Path::new(file).is_absolute());
        (
            OwnedObjectPath::try_from("/org/bluez/obex/session1/transfer1").unwrap(),
            transfer_properties(file),
        )
    }
}
fn transfer_properties(
    file: &str,
) -> std::collections::HashMap<String, zbus::zvariant::OwnedValue> {
    use zbus::zvariant::{OwnedValue, Value};
    std::collections::HashMap::from([
        (
            "Name".into(),
            OwnedValue::try_from(Value::from("sample.txt")).unwrap(),
        ),
        (
            "Filename".into(),
            OwnedValue::try_from(Value::from(file)).unwrap(),
        ),
        (
            "Status".into(),
            OwnedValue::try_from(Value::from("queued")).unwrap(),
        ),
        ("Size".into(), OwnedValue::from(4u64)),
        ("Transferred".into(), OwnedValue::from(0u64)),
    ])
}
#[derive(Clone, Default)]
struct ObexTransfer {
    canceled: Arc<AtomicUsize>,
}
#[zbus::interface(name = "org.bluez.obex.Transfer1")]
impl ObexTransfer {
    fn cancel(&self) {
        self.canceled.fetch_add(1, Ordering::SeqCst);
    }
    #[zbus(property)]
    fn name(&self) -> &str {
        "sample.txt"
    }
    #[zbus(property)]
    fn filename(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "queued"
    }
    #[zbus(property)]
    fn size(&self) -> u64 {
        4
    }
    #[zbus(property)]
    fn transferred(&self) -> u64 {
        0
    }
}

#[tokio::test]
async fn private_bus_obex_send_receive_cancel_and_progress() {
    use zbus::zvariant::{OwnedValue, Value};
    let private = Bus::new();
    let client_mock = ObexClient::default();
    let transfer_mock = ObexTransfer::default();
    let server = zbus::connection::Builder::address(private.address.as_str())
        .unwrap()
        .name("org.bluez")
        .unwrap()
        .name("org.bluez.obex")
        .unwrap()
        .serve_at("/org/bluez", AgentManager)
        .unwrap()
        .serve_at("/org/bluez/hci0", Adapter::default())
        .unwrap()
        .serve_at("/org/bluez/obex", ObexManager)
        .unwrap()
        .serve_at("/org/bluez/obex", client_mock.clone())
        .unwrap()
        .serve_at("/org/bluez/obex/session1", ObjectPush)
        .unwrap()
        .serve_at("/org/bluez/obex/session1/transfer1", transfer_mock.clone())
        .unwrap()
        .serve_at("/org/bluez/obex/incoming", ObexTransfer::default())
        .unwrap()
        .build()
        .await
        .unwrap();
    let system = private.connect().await;
    let session = private.connect().await;
    let destination = session.unique_name().unwrap().to_string();
    let (controller, worker, mut output) = control::channel();
    let mut state = controller.state.clone();
    let root = tempfile::tempdir().unwrap();
    let task = tokio::spawn(worker.run_on(system, Some(session), true, root.path().into(), false));
    wait_for(&mut state, |s| s.ready && s.agent_ready && s.obex_ready).await;
    let proxy = Proxy::new_owned(
        server.clone(),
        destination,
        "/org/inir/ObexAgent",
        "org.bluez.obex.Agent1",
    )
    .await
    .unwrap();
    let call = tokio::spawn(async move {
        proxy
            .call::<_, _, String>(
                "AuthorizePush",
                &(OwnedObjectPath::try_from("/org/bluez/obex/incoming").unwrap(),),
            )
            .await
    });
    wait_for(&mut state, |s| {
        s.prompts.iter().any(|p| p.kind == "incomingFile")
    })
    .await;
    let id = state.borrow().prompts[0].id.clone();
    controller
        .requests
        .send(Request::Respond {
            id,
            accepted: true,
            value: String::new(),
        })
        .await
        .unwrap();
    let incoming_path = call.await.unwrap().unwrap();
    assert!(std::path::Path::new(&incoming_path).starts_with(root.path()));
    tokio::fs::write(&incoming_path, b"test").await.unwrap();
    wait_for(&mut state, |s| {
        s.transfers.iter().any(|t| t.direction == "receive")
    })
    .await;
    let changed = std::collections::HashMap::from([
        (
            "Status",
            OwnedValue::try_from(Value::from("complete")).unwrap(),
        ),
        ("Transferred", OwnedValue::from(4u64)),
    ]);
    server
        .emit_signal(
            None::<&str>,
            "/org/bluez/obex/incoming",
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &("org.bluez.obex.Transfer1", changed, Vec::<String>::new()),
        )
        .await
        .unwrap();
    wait_for(&mut state, |s| {
        s.transfers
            .iter()
            .any(|t| t.direction == "receive" && t.status == "complete")
    })
    .await;
    assert_eq!(
        tokio::fs::read(root.path().join("sample.txt"))
            .await
            .unwrap(),
        b"test"
    );
    let file = root.path().join("sample.txt");
    controller.requests.send(Request::Command { id: "send".into(), operation: "sendFiles".into(),
        args: serde_json::json!({"adapter":"hci0","address":"AA:BB:CC:DD:EE:FF","files":[file.to_str().unwrap()]}) }).await.unwrap();
    let completion = tokio::time::timeout(Duration::from_secs(3), output.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(completion.result.is_ok(), "{:?}", completion);
    wait_for(&mut state, |s| {
        s.transfers.iter().any(|t| t.direction == "send")
    })
    .await;
    controller
        .requests
        .send(Request::Command {
            id: "cancel".into(),
            operation: "cancelTransfer".into(),
            args: serde_json::json!({"transfer":"/org/bluez/obex/session1/transfer1"}),
        })
        .await
        .unwrap();
    assert!(output.recv().await.unwrap().result.is_ok());
    assert_eq!(transfer_mock.canceled.load(Ordering::SeqCst), 1);
    controller
        .requests
        .send(Request::Command {
            id: "unknown".into(),
            operation: "cancelTransfer".into(),
            args: serde_json::json!({"transfer":"/org/bluez/obex/not-ours"}),
        })
        .await
        .unwrap();
    assert!(output.recv().await.unwrap().result.is_err());
    let changed = std::collections::HashMap::from([(
        "Status",
        OwnedValue::try_from(Value::from("error")).unwrap(),
    )]);
    server
        .emit_signal(
            None::<&str>,
            "/org/bluez/obex/session1/transfer1",
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &("org.bluez.obex.Transfer1", changed, Vec::<String>::new()),
        )
        .await
        .unwrap();
    wait_for(&mut state, |s| {
        s.transfers
            .iter()
            .any(|t| t.direction == "send" && t.status == "error")
    })
    .await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while client_mock.removed.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    controller.stop.cancel();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
}
