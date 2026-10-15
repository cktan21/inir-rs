//! Bluetooth control only. Quickshell remains the sole device-list observer.
//! Discovery and agents share one persistent client connection, not CLI calls.
pub mod agent;
pub mod network;
pub mod transfers;
pub mod types;

use crate::services::dbus::{properties, read};
use agent::Broker;
use futures_util::{stream::SelectAll, StreamExt};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use types::{Leases, ScanState, State, Transfer};
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue},
    Connection, MatchRule, MessageStream, Proxy,
};

pub enum Request {
    Scan {
        owner: String,
        adapter: String,
        deadline: Instant,
    },
    Release {
        owner: String,
    },
    ReleaseAll,
    Locked(bool),
    Command {
        id: String,
        operation: String,
        args: Value,
    },
    Respond {
        id: String,
        accepted: bool,
        value: String,
    },
}
#[derive(Debug)]
pub struct Completion {
    pub id: String,
    pub result: Result<(), String>,
}
#[derive(Default)]
pub struct Outcome {
    session: Option<String>,
    transfers: Vec<Transfer>,
}
pub enum Event {
    Idle,
    Incoming(Transfer),
    Scan {
        adapter: String,
        start: bool,
        result: Result<(), String>,
        observed: Option<bool>,
    },
    Command {
        id: String,
        device: String,
        result: Result<Outcome, String>,
    },
    Transfer {
        id: String,
        props: HashMap<String, OwnedValue>,
    },
    Finalized {
        id: String,
        result: Result<String, String>,
    },
    Networks(Result<Vec<types::Network>, String>),
}

pub struct Control {
    pub requests: mpsc::Sender<Request>,
    pub state: watch::Receiver<State>,
    pub stop: CancellationToken,
}
pub struct Worker {
    incoming: mpsc::Receiver<Request>,
    state: watch::Sender<State>,
    stop: CancellationToken,
    output: mpsc::Sender<Completion>,
}
pub fn channel() -> (Control, Worker, mpsc::Receiver<Completion>) {
    let (requests, incoming) = mpsc::channel(64);
    let (state, receiver) = watch::channel(State::default());
    let (output, finished) = mpsc::channel(64);
    let stop = CancellationToken::new();
    (
        Control {
            requests,
            state: receiver,
            stop: stop.clone(),
        },
        Worker {
            incoming,
            state,
            stop,
            output,
        },
        finished,
    )
}
impl Drop for Control {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Missing {key}"))
}
fn update(state: &watch::Sender<State>, apply: impl FnOnce(&mut State)) {
    state.send_if_modified(|s| {
        let before = s.clone();
        apply(s);
        *s != before
    });
}
async fn owner(bus: &Connection, name: &str) -> bool {
    let Ok(proxy) = Proxy::new(
        bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await
    else {
        return false;
    };
    proxy
        .call::<_, _, bool>("NameHasOwner", &(name,))
        .await
        .unwrap_or(false)
}
async fn rule(
    bus: &Connection,
    sender: &str,
    namespace: &str,
    interface: &str,
) -> Result<MessageStream, String> {
    let mut builder = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(sender)
        .map_err(|e| e.to_string())?
        .path_namespace(namespace)
        .map_err(|e| e.to_string())?
        .interface(interface)
        .map_err(|e| e.to_string())?;
    if sender == "org.bluez" {
        builder = builder
            .arg(0, "org.bluez.Adapter1")
            .map_err(|e| e.to_string())?;
    }
    let rule = builder.build();
    MessageStream::for_match_rule(rule, bus, Some(128))
        .await
        .map_err(|e| e.to_string())
}

async fn scan(bus: Connection, adapter: String, start: bool) -> Event {
    let result = async {
        let path = types::adapter_path(&adapter)?;
        Proxy::new(&bus, "org.bluez", path.as_str(), "org.bluez.Adapter1")
            .await
            .map_err(|e| e.to_string())?
            .call::<_, _, ()>(
                if start {
                    "StartDiscovery"
                } else {
                    "StopDiscovery"
                },
                &(),
            )
            .await
            .map_err(|e| e.to_string())
    };
    let result = tokio::time::timeout(Duration::from_secs(8), result)
        .await
        .unwrap_or_else(|_| Err("Bluetooth discovery request timed out".into()));
    let observed = if result.is_ok() {
        let path = types::adapter_path(&adapter).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            Proxy::new(&bus, "org.bluez", path.as_str(), "org.bluez.Adapter1")
                .await
                .ok()?
                .get_property::<bool>("Discovering")
                .await
                .ok()
        })
        .await
        .ok()
        .flatten()
    } else {
        None
    };
    Event::Scan {
        adapter,
        start,
        result,
        observed,
    }
}

async fn command(
    bus: Connection,
    obex: Option<Connection>,
    integrated: bool,
    operation: String,
    args: Value,
) -> Result<Outcome, String> {
    match operation.as_str() {
        "sendFiles" => {
            let obex = obex.ok_or("Bluetooth file transfer service is unavailable")?;
            let (session, rows) = transfers::send(&obex, &bus, &args).await?;
            Ok(Outcome {
                session: Some(session),
                transfers: rows,
            })
        }
        "cancelTransfer" => {
            let path = text(&args, "transfer")?;
            if !path.starts_with("/org/bluez/obex/") {
                return Err("Invalid transfer".into());
            }
            let obex = obex.ok_or("Bluetooth file transfer service is unavailable")?;
            Proxy::new(&obex, "org.bluez.obex", path, "org.bluez.obex.Transfer1")
                .await
                .map_err(|e| e.to_string())?
                .call::<_, _, ()>("Cancel", &())
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::default())
        }
        "connectTether" | "startSharing" | "disconnectTether" => {
            network::execute(&bus, &operation, &args).await?;
            Ok(Outcome::default())
        }
        "power" => {
            let path = types::adapter_path(text(&args, "adapter")?)?;
            let enabled = args["enabled"].as_bool().ok_or("Missing power state")?;
            Proxy::new(&bus, "org.bluez", path.as_str(), "org.bluez.Adapter1")
                .await
                .map_err(|e| e.to_string())?
                .set_property("Powered", enabled)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::default())
        }
        "discoverable" => {
            let path = types::adapter_path(text(&args, "adapter")?)?;
            let enabled = args["enabled"]
                .as_bool()
                .ok_or("Missing discoverability state")?;
            let proxy = Proxy::new(&bus, "org.bluez", path.as_str(), "org.bluez.Adapter1")
                .await
                .map_err(|e| e.to_string())?;
            proxy
                .set_property("DiscoverableTimeout", 180u32)
                .await
                .map_err(|e| e.to_string())?;
            proxy
                .set_property("Discoverable", enabled)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::default())
        }
        "pair" | "connect" | "disconnect" | "forget" | "trust" | "cancelPair" => {
            let adapter = text(&args, "adapter")?;
            let path = types::device_path(adapter, text(&args, "address")?)?;
            let proxy = Proxy::new(&bus, "org.bluez", path.as_str(), "org.bluez.Device1")
                .await
                .map_err(|e| e.to_string())?;
            match operation.as_str() {
                "pair" => {
                    if !integrated {
                        return Err("Enable the integrated pairing agent first".into());
                    }
                    proxy
                        .call::<_, _, ()>("Pair", &())
                        .await
                        .map_err(|e| e.to_string())?;
                    proxy
                        .set_property("Trusted", true)
                        .await
                        .map_err(|e| e.to_string())?;
                    proxy
                        .call::<_, _, ()>("Connect", &())
                        .await
                        .map_err(|e| e.to_string())?;
                }
                "connect" => proxy
                    .call::<_, _, ()>("Connect", &())
                    .await
                    .map_err(|e| e.to_string())?,
                "disconnect" => proxy
                    .call::<_, _, ()>("Disconnect", &())
                    .await
                    .map_err(|e| e.to_string())?,
                "cancelPair" => proxy
                    .call::<_, _, ()>("CancelPairing", &())
                    .await
                    .map_err(|e| e.to_string())?,
                "trust" => proxy
                    .set_property(
                        "Trusted",
                        args["trusted"].as_bool().ok_or("Missing trust state")?,
                    )
                    .await
                    .map_err(|e| e.to_string())?,
                "forget" => {
                    let adapter_path = types::adapter_path(adapter)?;
                    Proxy::new(
                        &bus,
                        "org.bluez",
                        adapter_path.as_str(),
                        "org.bluez.Adapter1",
                    )
                    .await
                    .map_err(|e| e.to_string())?
                    .call::<_, _, ()>("RemoveDevice", &(OwnedObjectPath::try_from(path).unwrap(),))
                    .await
                    .map_err(|e| e.to_string())?;
                }
                _ => unreachable!(),
            }
            Ok(Outcome::default())
        }
        _ => Err("Unknown Bluetooth operation".into()),
    }
}

impl Worker {
    pub async fn run(self, integrated: bool, receive_root: PathBuf, locked: bool) {
        match Connection::system().await {
            Ok(bus) => {
                let session = if integrated {
                    Connection::session().await.ok()
                } else {
                    None
                };
                self.run_on(bus, session, integrated, receive_root, locked)
                    .await;
            }
            Err(error) => update(&self.state, |s| s.error = error.to_string()),
        }
    }

    /// Explicit connections keep protocol tests isolated from the desktop buses.
    pub async fn run_on(
        mut self,
        bus: Connection,
        session: Option<Connection>,
        integrated: bool,
        receive_root: PathBuf,
        locked: bool,
    ) {
        if self.stop.is_cancelled() {
            return;
        }
        let broker = Broker::new(self.state.clone());
        broker.set_locked(locked);
        let result = self
            .cycle(
                bus.clone(),
                session.clone(),
                integrated,
                receive_root,
                broker.clone(),
            )
            .await;
        broker.cancel_device("");
        let _ = bus
            .object_server()
            .remove::<agent::Agent, _>("/org/inir/BluetoothAgent")
            .await;
        let _ = bus.close().await;
        if let Some(session) = session {
            let _ = session
                .object_server()
                .remove::<transfers::Agent, _>("/org/inir/ObexAgent")
                .await;
            let _ = session.close().await;
        }
        update(&self.state, |s| {
            s.ready = false;
            s.agent_ready = false;
            s.obex_ready = false;
            s.scans.clear();
            if let Err(error) = result {
                s.error = error;
            }
            for row in &mut s.transfers {
                if !matches!(row.status.as_str(), "complete" | "error") {
                    row.status = "error".into();
                    row.error = "Bluetooth controller disconnected".into();
                }
            }
        });
    }

    async fn cycle(
        &mut self,
        bus: Connection,
        session: Option<Connection>,
        integrated: bool,
        receive_root: PathBuf,
        broker: Broker,
    ) -> Result<(), String> {
        let mut streams = SelectAll::new();
        streams.push(
            rule(
                &bus,
                "org.bluez",
                "/org/bluez",
                "org.freedesktop.DBus.Properties",
            )
            .await?
            .boxed(),
        );
        for name in ["org.bluez", "org.freedesktop.NetworkManager"] {
            let rule = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.DBus")
                .unwrap()
                .interface("org.freedesktop.DBus")
                .unwrap()
                .member("NameOwnerChanged")
                .unwrap()
                .arg(0, name)
                .unwrap()
                .build();
            streams.push(
                MessageStream::for_match_rule(rule, &bus, Some(16))
                    .await
                    .map_err(|e| e.to_string())?
                    .boxed(),
            );
        }
        if integrated {
            streams.push(
                rule(
                    &bus,
                    "org.freedesktop.NetworkManager",
                    "/org/freedesktop/NetworkManager/ActiveConnection",
                    "org.freedesktop.DBus.Properties",
                )
                .await?
                .boxed(),
            );
            let r = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.NetworkManager")
                .unwrap()
                .path("/org/freedesktop/NetworkManager")
                .unwrap()
                .interface("org.freedesktop.DBus.Properties")
                .unwrap()
                .arg(0, "org.freedesktop.NetworkManager")
                .unwrap()
                .build();
            streams.push(
                MessageStream::for_match_rule(r, &bus, Some(32))
                    .await
                    .map_err(|e| e.to_string())?
                    .boxed(),
            );
        }
        let (events, mut incoming_events) = mpsc::channel(64);
        let mut obex = None;
        let mut agent_error = String::new();
        if integrated && owner(&bus, "org.bluez").await {
            match agent::register(&bus, broker.clone()).await {
                Ok(()) => update(&self.state, |s| s.agent_ready = true),
                Err(e) => agent_error = e,
            }
            match session {
                Some(connection) => {
                    let r = MatchRule::builder()
                        .msg_type(zbus::message::Type::Signal)
                        .sender("org.freedesktop.DBus")
                        .unwrap()
                        .interface("org.freedesktop.DBus")
                        .unwrap()
                        .member("NameOwnerChanged")
                        .unwrap()
                        .arg(0, "org.bluez.obex")
                        .unwrap()
                        .build();
                    streams.push(
                        MessageStream::for_match_rule(r, &connection, Some(16))
                            .await
                            .map_err(|e| e.to_string())?
                            .boxed(),
                    );
                    streams.push(
                        rule(
                            &connection,
                            "org.bluez.obex",
                            "/org/bluez/obex",
                            "org.freedesktop.DBus.Properties",
                        )
                        .await?
                        .boxed(),
                    );
                    match transfers::register(
                        &connection,
                        broker.clone(),
                        receive_root.clone(),
                        events.clone(),
                    )
                    .await
                    {
                        Ok(()) => {
                            update(&self.state, |s| s.obex_ready = true);
                            obex = Some(connection);
                        }
                        Err(e) => {
                            agent_error = format!("{agent_error} File transfers: {e}").trim().into()
                        }
                    }
                }
                None => {
                    agent_error = format!("{agent_error} Session bus is unavailable")
                        .trim()
                        .into()
                }
            }
        }
        let bluez_ready = owner(&bus, "org.bluez").await;
        let network_ready = integrated && owner(&bus, "org.freedesktop.NetworkManager").await;
        update(&self.state, |s| {
            s.ready = bluez_ready;
            s.network_ready = network_ready;
            s.error = agent_error;
        });
        let mut jobs = JoinSet::new();
        if network_ready {
            let c = bus.clone();
            jobs.spawn(async move { Event::Networks(network::active(&c).await) });
        }
        let mut leases = Leases::default();
        let mut scans = BTreeMap::<String, ScanState>::new();
        let mut rows = BTreeMap::<String, Transfer>::new();
        let mut sessions = BTreeMap::<String, Vec<String>>::new();
        let mut finalizing = std::collections::BTreeSet::new();
        let mut stop_retries = BTreeMap::<String, (Instant, u32)>::new();
        let mut progress_deadline = None::<Instant>;
        let mut network_refresh = false;
        let mut locked = broker.is_locked();
        loop {
            // Reconcile desired leases with acknowledged sessions. Pending calls
            // are never duplicated, including close-before-start-reply races.
            leases.expire(Instant::now());
            for (adapter, (deadline, _)) in &stop_retries {
                if *deadline <= Instant::now() {
                    if let Some(scan) = scans.get_mut(adapter) {
                        scan.error.clear();
                    }
                }
            }
            for scan_state in scans.values_mut() {
                scan_state.owners = leases.owners(&scan_state.adapter);
                let wanted = !scan_state.owners.is_empty();
                if !scan_state.pending && scan_state.error.is_empty() && wanted != scan_state.owned
                {
                    scan_state.pending = true;
                    jobs.spawn(scan(bus.clone(), scan_state.adapter.clone(), wanted));
                }
            }
            update(&self.state, |s| s.scans = scans.values().cloned().collect());
            let deadline = leases
                .deadline()
                .into_iter()
                .chain(progress_deadline)
                .chain(
                    stop_retries
                        .values()
                        .filter(|(d, _)| *d > Instant::now())
                        .map(|(d, _)| *d),
                )
                .min()
                .unwrap_or_else(|| Instant::now() + Duration::from_secs(86400));
            let event = tokio::select! {
                biased;
                _ = self.stop.cancelled() => break,
                request = self.incoming.recv() => {
                    match request {
                        Some(Request::Scan { owner, adapter, deadline }) => {
                            if !locked && types::adapter_path(&adapter).is_ok() && !owner.is_empty() {
                                let now = Instant::now();
                                if deadline <= now { leases.release(&owner); continue; }
                                leases.request_for(owner, adapter.clone(), now, deadline.duration_since(now));
                                scans.entry(adapter.clone()).or_insert_with(|| ScanState { adapter, ..Default::default() }).error.clear();
                            }
                        }
                        Some(Request::Release { owner }) => leases.release(&owner),
                        Some(Request::ReleaseAll) => { leases.0.clear(); broker.cancel_device(""); }
                        Some(Request::Locked(value)) => { locked = value; if locked { leases.0.clear(); } broker.set_locked(locked); }
                        Some(Request::Respond { id, accepted, value }) => {
                            let display = self.state.borrow().prompts.iter().find(|p| p.id == id && p.kind.starts_with("display")).cloned();
                            let _ = broker.respond(&id, accepted, value);
                            if !accepted {
                                if let Some(prompt) = display {
                                    let c = bus.clone(); jobs.spawn(async move {
                                        if let Ok(proxy) = Proxy::new(&c,"org.bluez",prompt.device.as_str(),"org.bluez.Device1").await {
                                            let _ = proxy.call::<_,_,()>("CancelPairing", &()).await;
                                        }
                                        Event::Idle
                                    });
                                }
                            }
                        }
                        Some(Request::Command { id, operation, args }) => {
                            // A transfer ID must be ours before cancellation.
                            if operation == "cancelTransfer" && !rows.contains_key(args["transfer"].as_str().unwrap_or("")) {
                                let _ = self.output.send(Completion { id, result: Err("This transfer does not belong to the shell".into()) }).await;
                                continue;
                            }
                            let c = bus.clone(); let o = obex.clone();
                            let agent_ready = integrated && self.state.borrow().agent_ready;
                            let device = if matches!(operation.as_str(), "pair" | "cancelPair") {
                                args.get("adapter").and_then(Value::as_str).zip(args.get("address").and_then(Value::as_str))
                                    .and_then(|(a,d)| types::device_path(a,d).ok()).unwrap_or_default()
                            } else { String::new() };
                            jobs.spawn(async move {
                                let pair = operation == "pair";
                                let result = match tokio::time::timeout(Duration::from_secs(90), command(c.clone(), o, agent_ready, operation, args)).await {
                                    Ok(result) => result,
                                    Err(_) => {
                                        if pair {
                                            if let Ok(proxy) = Proxy::new(&c,"org.bluez",device.as_str(),"org.bluez.Device1").await {
                                                let _ = proxy.call::<_,_,()>("CancelPairing", &()).await;
                                            }
                                        }
                                        Err("Bluetooth operation timed out".into())
                                    }
                                };
                                Event::Command { id, device, result }
                            });
                        }
                        None => break,
                    }
                    continue;
                }
                event = incoming_events.recv() => event,
                result = jobs.join_next(), if !jobs.is_empty() => result.and_then(Result::ok),
                _ = tokio::time::sleep_until(deadline.into()) => {
                    if progress_deadline.is_some_and(|d| d <= Instant::now()) {
                        progress_deadline = None;
                        update(&self.state, |s| s.transfers = rows.values().cloned().collect());
                        if network_refresh {
                            network_refresh = false;
                            let c = bus.clone(); jobs.spawn(async move { Event::Networks(network::active(&c).await) });
                        }
                    }
                    continue;
                }
                message = streams.next() => {
                    let Some(Ok(message)) = message else { return Err("Bluetooth service stream disconnected".into()); };
                    let header = message.header();
                    let path = header.path().map(|p| p.to_string()).unwrap_or_default();
                    let member = header.member().map(|m| m.as_str().to_string()).unwrap_or_default();
                    if member == "NameOwnerChanged" {
                        if let Ok((name, old, new)) = message.body().deserialize::<(String,String,String)>() {
                            if name == "org.bluez" {
                                // Disconnecting this client releases all its discovery
                                // and agent registrations. The QML provider reconnects.
                                return Err(if new.is_empty() { "Bluetooth service stopped" } else { "Bluetooth service restarted" }.into());
                            }
                            if name == "org.freedesktop.NetworkManager" {
                                update(&self.state, |s| s.network_ready = !new.is_empty());
                                network_refresh = true;
                                progress_deadline.get_or_insert(Instant::now() + Duration::from_millis(200));
                            }
                            if name == "org.bluez.obex" && (!old.is_empty() || obex.is_none()) {
                                return Err("Bluetooth file transfer service restarted".into());
                            }
                        }
                        None
                    } else if let Ok((interface, changed, invalidated)) = message.body().deserialize::<(String,HashMap<String,OwnedValue>,Vec<String>)>() {
                        if interface == "org.bluez.Adapter1" {
                            if let Some(scan_state) = path.rsplit('/').next().and_then(|a| scans.get_mut(a)) {
                                if changed.contains_key("Discovering") { scan_state.observed = read(&changed, "Discovering"); }
                                if changed.contains_key("Powered") && !read::<bool>(&changed, "Powered") {
                                    let adapter = scan_state.adapter.clone();
                                    leases.0.retain(|_, (a,_)| *a != adapter);
                                    scan_state.owned = false; scan_state.error.clear();
                                }
                            }
                            None
                        } else if interface == "org.bluez.obex.Transfer1" && rows.contains_key(&path) {
                            if !invalidated.is_empty() {
                                if let Some(c) = obex.clone() { let id = path.clone(); jobs.spawn(async move {
                                    Event::Transfer { props: properties(&c,"org.bluez.obex",&id,"org.bluez.obex.Transfer1").await.unwrap_or_default(), id }
                                }); }
                            }
                            // Reduce deltas without rereading every progress signal.
                            if let Some(row) = rows.get_mut(&path) {
                                if changed.contains_key("Transferred") { row.transferred = read(&changed,"Transferred"); }
                                if changed.contains_key("Size") { row.size = read(&changed,"Size"); }
                                if changed.contains_key("Status") { row.status = read(&changed,"Status"); }
                            }
                            Some(Event::Transfer { id: path, props: changed })
                        } else if interface.starts_with("org.freedesktop.NetworkManager") {
                            network_refresh = true;
                            progress_deadline.get_or_insert(Instant::now() + Duration::from_millis(200));
                            None
                        } else { None }
                    } else { None }
                }
            };
            let Some(event) = event else {
                continue;
            };
            match event {
                Event::Idle => {}
                Event::Scan {
                    adapter,
                    start,
                    result,
                    observed,
                } => {
                    if let Some(s) = scans.get_mut(&adapter) {
                        s.pending = false;
                        if let Some(observed) = observed {
                            s.observed = observed;
                        }
                        match result {
                            Ok(()) => {
                                s.owned = start;
                                s.error.clear();
                                stop_retries.remove(&adapter);
                            }
                            Err(error) => {
                                // NotReady after powering off means BlueZ already
                                // discarded the discovery session.
                                if !start && error.contains("NotReady") {
                                    s.owned = false;
                                } else {
                                    if error.contains("timed out") {
                                        return Err(error);
                                    }
                                    s.error = error;
                                    if !start {
                                        let attempt = stop_retries
                                            .get(&adapter)
                                            .map(|(_, n)| n + 1)
                                            .unwrap_or(1);
                                        if attempt > 3 {
                                            return Err("Failed to release Bluetooth scan; closing the client connection".into());
                                        }
                                        stop_retries.insert(
                                            adapter.clone(),
                                            (
                                                Instant::now()
                                                    + Duration::from_secs(1 << (attempt - 1)),
                                                attempt,
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Event::Incoming(row) => {
                    let id = row.id.clone();
                    rows.insert(id.clone(), row);
                    if let Some(c) = obex.clone() {
                        jobs.spawn(async move {
                            Event::Transfer {
                                props: properties(
                                    &c,
                                    "org.bluez.obex",
                                    &id,
                                    "org.bluez.obex.Transfer1",
                                )
                                .await
                                .unwrap_or_default(),
                                id,
                            }
                        });
                    }
                    update(&self.state, |s| {
                        s.transfers = rows.values().cloned().collect()
                    });
                }
                Event::Command { id, device, result } => {
                    if !device.is_empty() {
                        broker.cancel_device(&device);
                    }
                    let result = result.map(|outcome| {
                        if let Some(session) = outcome.session {
                            sessions.insert(
                                session,
                                outcome.transfers.iter().map(|r| r.id.clone()).collect(),
                            );
                        }
                        for row in outcome.transfers {
                            let transfer_id = row.id.clone();
                            rows.insert(transfer_id.clone(), row);
                            if let Some(c) = obex.clone() {
                                jobs.spawn(async move {
                                    Event::Transfer {
                                        props: properties(
                                            &c,
                                            "org.bluez.obex",
                                            &transfer_id,
                                            "org.bluez.obex.Transfer1",
                                        )
                                        .await
                                        .unwrap_or_default(),
                                        id: transfer_id,
                                    }
                                });
                            }
                        }
                    });
                    update(&self.state, |s| {
                        s.transfers = rows.values().cloned().collect()
                    });
                    let _ = self.output.send(Completion { id, result }).await;
                    network_refresh = true;
                    progress_deadline.get_or_insert(Instant::now() + Duration::from_millis(200));
                }
                Event::Transfer { id, props } => {
                    if let Some(row) = rows.get_mut(&id) {
                        if props.contains_key("Transferred") {
                            row.transferred = read(&props, "Transferred");
                        }
                        if props.contains_key("Size") {
                            row.size = read(&props, "Size");
                        }
                        if props.contains_key("Status") {
                            row.status = read(&props, "Status");
                        }
                        if row.status == "complete"
                            && row.direction == "receive"
                            && row.filename.contains("/.incoming-")
                        {
                            row.status = "saving".into();
                            if finalizing.insert(id.clone()) {
                                let path = row.filename.clone();
                                let name = row.name.clone();
                                let root = receive_root.clone();
                                jobs.spawn(async move {
                                    Event::Finalized {
                                        id,
                                        result: transfers::finalize(&path, &root, &name).await,
                                    }
                                });
                            }
                        } else if matches!(row.status.as_str(), "complete" | "error") {
                            if row.status == "error" {
                                row.error = "Transfer failed or was canceled".into();
                            }
                            update(&self.state, |s| {
                                s.transfers = rows.values().cloned().collect()
                            });
                        }
                    }
                    progress_deadline.get_or_insert(Instant::now() + Duration::from_millis(200));
                }
                Event::Finalized { id, result } => {
                    finalizing.remove(&id);
                    if let Some(row) = rows.get_mut(&id) {
                        match result {
                            Ok(path) => {
                                row.filename = path;
                                row.status = "complete".into();
                            }
                            Err(error) => {
                                row.status = "error".into();
                                row.error = error;
                            }
                        }
                    }
                    update(&self.state, |s| {
                        s.transfers = rows.values().cloned().collect()
                    });
                }
                Event::Networks(result) => match result {
                    Ok(networks) => update(&self.state, |s| {
                        s.networks = networks;
                        s.network_ready = true;
                    }),
                    Err(_) => update(&self.state, |s| s.network_ready = false),
                },
            }
            let done: Vec<_> = sessions
                .iter()
                .filter(|(_, ids)| {
                    ids.iter().all(|id| {
                        rows.get(id)
                            .is_some_and(|r| matches!(r.status.as_str(), "complete" | "error"))
                    })
                })
                .map(|(session, _)| session.clone())
                .collect();
            for session in done {
                sessions.remove(&session);
                if let Some(c) = obex.clone() {
                    jobs.spawn(async move {
                        if let Ok(proxy) = Proxy::new(
                            &c,
                            "org.bluez.obex",
                            "/org/bluez/obex",
                            "org.bluez.obex.Client1",
                        )
                        .await
                        {
                            let _ = proxy
                                .call::<_, _, ()>(
                                    "RemoveSession",
                                    &(OwnedObjectPath::try_from(session).unwrap(),),
                                )
                                .await;
                        }
                        Event::Idle
                    });
                }
            }
            while rows.len() > 128 {
                let old = rows
                    .iter()
                    .find(|(id, row)| {
                        matches!(row.status.as_str(), "complete" | "error")
                            && !sessions.values().any(|ids| ids.contains(id))
                    })
                    .map(|(id, _)| id.clone());
                if let Some(id) = old {
                    rows.remove(&id);
                } else {
                    break;
                }
            }
        }
        jobs.abort_all();
        for s in scans.values().filter(|s| s.owned) {
            let _ = scan(bus.clone(), s.adapter.clone(), false).await;
        }
        if integrated {
            if let Ok(proxy) =
                Proxy::new(&bus, "org.bluez", "/org/bluez", "org.bluez.AgentManager1").await
            {
                let _ = proxy
                    .call::<_, _, ()>(
                        "UnregisterAgent",
                        &(OwnedObjectPath::try_from("/org/inir/BluetoothAgent").unwrap(),),
                    )
                    .await;
            }
            if let Some(connection) = &obex {
                if let Ok(proxy) = Proxy::new(
                    connection,
                    "org.bluez.obex",
                    "/org/bluez/obex",
                    "org.bluez.obex.AgentManager1",
                )
                .await
                {
                    let _ = proxy
                        .call::<_, _, ()>(
                            "UnregisterAgent",
                            &(OwnedObjectPath::try_from("/org/inir/ObexAgent").unwrap(),),
                        )
                        .await;
                }
            }
        }
        // Connection teardown releases any late-starting session and OBEX agent.
        Ok(())
    }
}
