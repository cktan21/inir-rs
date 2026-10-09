// Phase 1 (efficiency revamp): only Network, Power and Brightness run natively.
// Audio, Battery, Bluetooth, Media and Niri are served by Quickshell's existing
// native C++/QML to avoid duplicate subscriptions. The disabled modules remain
// compiled so later phases can re-enable them with the delta architecture.
#![allow(dead_code, unused_imports)]
use super::{brightness, network, niri, power};
use crate::events::Event;
use futures_util::{stream::SelectAll, StreamExt};
use inir_types::desktop::*;
use std::{path::Path, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use zbus::{Connection, MatchRule, MessageStream};

pub struct Request {
    pub command: Command,
    pub reply: oneshot::Sender<Result<(), String>>,
}

async fn update(bus: &Connection, domain: &str, events: &mpsc::Sender<Event>) {
    macro_rules! snapshot {
        ($function:expr, $variant:ident, $state:ty) => {{
            let start = std::time::Instant::now();
            let result = tokio::time::timeout(Duration::from_secs(8), $function).await;
            let elapsed = start.elapsed();
            tracing::debug!(domain, elapsed_ms = elapsed.as_millis(), "snapshot update");
            match result {
                Ok(Ok(state)) => Event::$variant(state),
                result => {
                    let mut state = <$state>::default();
                    state.status.error = match result {
                        Ok(Err(e)) => e.to_string(),
                        _ => "D-Bus snapshot timed out".into(),
                    };
                    Event::$variant(state)
                }
            }
        }};
    }
    let event = match domain {
        "network" => snapshot!(network::snapshot(bus), Network, NetworkState),
        "power" => snapshot!(power::profiles(bus), Power, PowerState),
        _ => return,
    };
    let _ = events.send(event).await;
}

async fn bus_cycle(
    session: bool,
    events: &mpsc::Sender<Event>,
    stop: &CancellationToken,
) -> zbus::Result<()> {
    let bus = if session {
        Connection::session().await?
    } else {
        Connection::system().await?
    };
    let mut streams = SelectAll::new();
    // Subscribe before initial reads. Paths also cover PropertiesChanged and
    // ObjectManager additions/removals without a per-object subscription storm.
    let paths: &[(&str, &str)] = if session {
        &[] // Phase 1: media handled by Quickshell.Services.Mpris
    } else {
        &[
            ("network", "/org/freedesktop/NetworkManager"),
            ("power", "/net/hadess/PowerProfiles"),
            // Phase 1: bluetooth/battery handled by Quickshell C++
        ]
    };
    for (domain, path) in paths {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .path_namespace(*path)?
            .build();
        let stream = MessageStream::for_match_rule(rule, &bus, Some(128)).await?;
        streams.push(stream.map(move |message| (*domain, message)).boxed());
    }
    // Phase 1/4: filter NameOwnerChanged at the bus with arg0 so only our own
    // services' restarts wake the worker, not every client on the bus.
    let owner_names: &[&str] = if session {
        &[]
    } else {
        &[network::SERVICE, power::PROFILES, "net.hadess.PowerProfiles"]
    };
    for name in owner_names {
        let owner_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg(0, *name)?
            .build();
        streams.push(
            MessageStream::for_match_rule(owner_rule, &bus, Some(16))
                .await?
                .map(|message| ("owner", message))
                .boxed(),
        );
    }
    let domains: &[&str] = if session {
        &[] // Phase 1: no session-bus domains
    } else {
        &["network", "power"] // Phase 1: dropped bluetooth, battery
    };
    for domain in domains {
        update(&bus, domain, events).await;
    }
    let mut pending = std::collections::BTreeSet::new();
    let mut deadline = tokio::time::Instant::now();
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            message = streams.next() => {
                let Some((mut domain, message)) = message else { return Err(zbus::Error::Failure("D-Bus stream ended".into())); };
                let message = message?;
                if domain == "owner" {
                    let (name, _, _): (String, String, String) = message.body().deserialize()?;
                    // Phase 1: only react to network and power name changes.
                    domain = if name == network::SERVICE { "network" }
                        else if name == power::PROFILES || name == "net.hadess.PowerProfiles" { "power" }
                        else { continue };
                }
                if pending.is_empty() { deadline = tokio::time::Instant::now() + Duration::from_millis(50); }
                pending.insert(domain);
            }
            _ = tokio::time::sleep_until(deadline), if !pending.is_empty() => {
                let pending_count = pending.len();
                let domains: Vec<_> = std::mem::take(&mut pending).into_iter().collect();
                tracing::debug!(pending_count, domains_str = domains.join(","), "coalesced signal batch");
                for domain in domains { update(&bus, domain, events).await; }
            }
        }
    }
}

async fn bus_worker(session: bool, events: mpsc::Sender<Event>, stop: CancellationToken) {
    loop {
        let result = tokio::select! { biased; _ = stop.cancelled() => break, result = bus_cycle(session, &events, &stop) => result };
        if let Err(error) = result {
            tracing::warn!(%error, session, "desktop bus disconnected");
            macro_rules! failed {
                ($variant:ident, $state:ty) => {{
                    let mut state = <$state>::default();
                    state.status.error = error.to_string();
                    let _ = events.send(Event::$variant(state)).await;
                }};
            }
            // Phase 1: only network and power are served natively.
            if !session {
                failed!(Network, NetworkState);
                failed!(Power, PowerState);
            }
        }
        tokio::select! { _ = stop.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    }
}

async fn brightness_worker(events: mpsc::Sender<Event>, stop: CancellationToken) {
    loop {
        let snapshot =
            tokio::task::spawn_blocking(|| brightness::snapshot(Path::new("/sys/class/backlight")))
                .await;
        let state = match snapshot {
            Ok(Ok(state)) => state,
            result => {
                let mut state = BrightnessState::default();
                state.status.error = format!("{result:?}");
                state
            }
        };
        if events.send(Event::Brightness(state)).await.is_err() {
            break;
        }
        // sysfs does not emit reliable inotify notifications for brightness.
        // This low-frequency read only runs while a desktop consumer exists.
        tokio::select! { _ = stop.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    }
}

async fn niri_worker(events: mpsc::Sender<Event>, stop: CancellationToken) {
    let Some(path) = std::env::var_os("NIRI_SOCKET") else {
        let mut state = NiriState::default();
        state.status.error = "NIRI_SOCKET is unset".into();
        let _ = events.send(Event::Niri(state)).await;
        return;
    };
    loop {
        let result = tokio::select! { biased; _ = stop.cancelled() => break, result = niri::stream(Path::new(&path), &events, &stop) => result };
        if let Err(error) = result {
            let mut state = NiriState::default();
            state.status.error = error;
            let _ = events.send(Event::Niri(state)).await;
        }
        tokio::select! { _ = stop.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    }
}

async fn execute(command: Command) -> Result<(), String> {
    match command {
        // Niri actions remain served by the QML frontend until Phase 3 cuts the
        // native Niri domain over to the Rust backend.
        Command::Niri { .. } => Err("command served by the QML frontend".into()),
        command => {
            let bus = Connection::system().await.map_err(|e| e.to_string())?;
            match command {
                command @ (Command::WifiEnabled { .. }
                | Command::WifiScan
                | Command::WifiDisconnect { .. }
                | Command::WifiConnect { .. }) => network::execute(&bus, command).await,
                Command::PowerProfile { profile } => power::set_profile(&bus, &profile).await,
                Command::Brightness { device, value } => {
                    brightness::set(&bus, &device, value).await
                }
                _ => Err("invalid system command".into()),
            }
        }
    }
}

pub async fn run(
    events: mpsc::Sender<Event>,
    mut requests: mpsc::Receiver<Request>,
    mut consumers: watch::Receiver<usize>,
    stop: CancellationToken,
) {
    loop {
        if *consumers.borrow_and_update() == 0 {
            tokio::select! { biased; _ = stop.cancelled() => break, changed = consumers.changed() => { if changed.is_err() { break; } }, request = requests.recv() => { if let Some(request) = request { let _ = request.reply.send(Err("desktop services are inactive".into())); } else { break; } } }
            continue;
        }
        let cycle = stop.child_token();
        let mut tasks = JoinSet::new();
        // Phase 1: only the system bus (network + power) and the backlight poll
        // run natively. The session bus (media), Niri stream and PipeWire driver
        // are intentionally not spawned to avoid duplicating Quickshell.
        tasks.spawn(bus_worker(false, events.clone(), cycle.clone()));
        tasks.spawn(brightness_worker(events.clone(), cycle.clone()));
        loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                changed = consumers.changed() => { if changed.is_err() || *consumers.borrow_and_update() == 0 { break; } }
                request = requests.recv() => {
                    let Some(request) = request else { break; };
                    let result = tokio::select! {
                        _ = stop.cancelled() => Err("backend stopped".into()),
                        result = tokio::time::timeout(Duration::from_secs(10), execute(request.command)) => result.unwrap_or_else(|_| Err("service command timed out".into()))
                    };
                    let _ = request.reply.send(result);
                }
            }
        }
        cycle.cancel();
        tasks.abort_all();
        if stop.is_cancelled() {
            break;
        }
    }
}
