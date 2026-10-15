// Network, power, brightness and Niri run natively. Audio, battery, Bluetooth and
// media stay with Quickshell's native C++/QML services so each has one owner.
use super::{brightness, dbus::SystemBus, keyboard, network, niri, power};
use crate::events::Event;
use futures_util::{stream::SelectAll, StreamExt};
use inir_types::desktop::*;
use std::{path::Path, time::Duration};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use zbus::{Connection, MatchRule, MessageStream};

// Network connects wait for NetworkManager to finish activating (up to 30 s).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(40);

pub struct Request {
    pub command: Command,
    pub reply: oneshot::Sender<Result<String, String>>,
}

async fn update_power(bus: &Connection, events: &mpsc::Sender<Event>) {
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(8), power::profiles(bus)).await;
    tracing::debug!(
        domain = "power",
        elapsed_ms = start.elapsed().as_millis(),
        "snapshot update"
    );
    let state = match result {
        Ok(Ok(state)) => state,
        result => {
            let mut state = PowerState::default();
            state.status.error = match result {
                Ok(Err(e)) => e.to_string(),
                _ => "D-Bus snapshot timed out".into(),
            };
            state
        }
    };
    let _ = events.send(Event::Power(state)).await;
}

async fn power_cycle(
    system: &SystemBus,
    events: &mpsc::Sender<Event>,
    stop: &CancellationToken,
) -> zbus::Result<()> {
    let bus = system.connection().await?;
    let mut messages = SelectAll::new();
    // Subscribe before the first read so a change during the read is not lost.
    // Both the current UPower path and the legacy net.hadess path are watched.
    for path in [power::PROFILE_PATH, power::OLD_PATH] {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .path_namespace(path)?
            .build();
        messages.push(MessageStream::for_match_rule(rule, &bus, Some(128)).await?.boxed());
    }
    // Filter NameOwnerChanged at the bus so only the profiles service restarting
    // wakes this worker, not every client that connects to the bus.
    for name in [power::PROFILES, power::OLD_PROFILES] {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg(0, name)?
            .build();
        messages.push(MessageStream::for_match_rule(rule, &bus, Some(16)).await?.boxed());
    }
    update_power(&bus, events).await;
    let mut pending = false;
    let mut deadline = tokio::time::Instant::now();
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            message = messages.next() => {
                let Some(message) = message else {
                    return Err(zbus::Error::Failure("D-Bus stream ended".into()));
                };
                message?;
                if !pending {
                    pending = true;
                    deadline = tokio::time::Instant::now() + Duration::from_millis(50);
                }
            }
            _ = tokio::time::sleep_until(deadline), if pending => {
                pending = false;
                update_power(&bus, events).await;
            }
        }
    }
}

async fn power_worker(system: SystemBus, events: mpsc::Sender<Event>, stop: CancellationToken) {
    loop {
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            result = power_cycle(&system, &events, &stop) => result
        };
        if let Err(error) = result {
            tracing::warn!(%error, "power profiles stream disconnected");
            system.reset().await;
            let mut state = PowerState::default();
            state.status.error = error.to_string();
            let _ = events.send(Event::Power(state)).await;
        }
        tokio::select! { _ = stop.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    }
}

async fn network_worker(system: SystemBus, events: mpsc::Sender<Event>, stop: CancellationToken) {
    loop {
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            result = async {
                let bus = system.connection().await.map_err(|e| e.to_string())?;
                network::stream(&bus, &events, &stop).await
            } => result,
        };
        if let Err(error) = result {
            tracing::warn!(%error, "network actor disconnected");
            system.reset().await;
            let mut state = NetworkState::default();
            state.status.error = error;
            let _ = events.send(Event::Network(state)).await;
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

async fn niri_worker(
    events: mpsc::Sender<Event>,
    stop: CancellationToken,
    refresh: watch::Receiver<u64>,
) {
    let Some(path) = std::env::var_os("NIRI_SOCKET") else {
        let mut state = NiriState::default();
        state.status.error = "NIRI_SOCKET is unset".into();
        let _ = events.send(Event::Niri(state)).await;
        return;
    };
    loop {
        let result = tokio::select! { biased; _ = stop.cancelled() => break, result = niri::stream_with_refresh(Path::new(&path), &events, &stop, refresh.clone()) => result };
        if let Err(error) = result {
            let mut state = NiriState::default();
            state.status.error = error;
            let _ = events.send(Event::Niri(state)).await;
        }
        tokio::select! { _ = stop.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    }
}

async fn keyboard_worker(events: mpsc::Sender<Event>, stop: CancellationToken) {
    loop {
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            result = keyboard::stream(&events, &stop) => result,
        };
        if let Err(error) = result {
            let mut state = KeyboardLockState::default();
            state.status.error = error;
            let _ = events.send(Event::Keyboard(state)).await;
        }
        tokio::select! { _ = stop.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    }
}

async fn execute(
    command: Command,
    system: &SystemBus,
    niri: &mut niri::ActionClient,
    refresh: &watch::Sender<u64>,
) -> Result<String, String> {
    match command {
        // Niri actions go through the native backend's persistent action socket.
        Command::Niri { action } => {
            let path = std::env::var_os("NIRI_SOCKET").ok_or("NIRI_SOCKET is unset")?;
            niri.action(Path::new(&path), action).await.map(|()| String::new())
        }
        Command::NiriAction { name, parameters } => {
            let path = std::env::var_os("NIRI_SOCKET").ok_or("NIRI_SOCKET is unset")?;
            let action = serde_json::json!({ name: parameters });
            niri.action(Path::new(&path), action).await.map(|()| String::new())
        }
        Command::NiriRefreshOutputs => {
            refresh.send_modify(|generation| *generation = generation.wrapping_add(1));
            Ok(String::new())
        }
        command => {
            let bus = system.connection().await.map_err(|e| e.to_string())?;
            match command {
                command @ (Command::WifiEnabled { .. }
                | Command::WifiScan
                | Command::WifiDisconnect { .. }
                | Command::WifiConnect { .. }
                | Command::WifiConnectSsid { .. }
                | Command::ProfileForget { .. }
                | Command::ProfileSetPassword { .. }
                | Command::ProfileSecret { .. }) => network::execute(&bus, command).await,
                Command::PowerProfile { profile } => {
                    power::set_profile(&bus, &profile).await.map(|()| String::new())
                }
                Command::Brightness { device, value } => {
                    brightness::set(&bus, &device, value).await.map(|()| String::new())
                }
                _ => Err("invalid system command".into()),
            }
        }
    }
}

pub async fn run(
    events: mpsc::Sender<Event>,
    mut requests: mpsc::Receiver<Request>,
    mut consumers: watch::Receiver<[usize; 5]>,
    stop: CancellationToken,
) {
    let (refresh, refresh_rx) = watch::channel(0);
    let mut actions = niri::ActionClient::default();
    let system = SystemBus::default();
    let domains = std::env::var("INIR_NATIVE_DOMAINS").unwrap_or_else(|_| "network,brightness,niri,keyboard".into());
    let names = ["network", "power", "brightness", "niri", "keyboard"];
    let enabled: [bool; 5] = std::array::from_fn(|i| domains.split(',').any(|s| s.trim() == names[i]));
    let mut tasks: [Option<(CancellationToken, tokio::task::JoinHandle<()>)>; 5] = std::array::from_fn(|_| None);
    loop {
        let counts = *consumers.borrow_and_update();
        for i in 0..5 {
            if enabled[i] && counts[i] > 0 && tasks[i].is_none() {
                let cycle = stop.child_token();
                let worker_stop = cycle.clone();
                let sender = events.clone();
                let refresh_rx = refresh_rx.clone();
                let system = system.clone();
                let task = tokio::spawn(async move {
                    match i {
                        0 => network_worker(system, sender, worker_stop).await,
                        1 => power_worker(system, sender, worker_stop).await,
                        2 => brightness_worker(sender, worker_stop).await,
                        3 => niri_worker(sender, worker_stop, refresh_rx).await,
                        4 => keyboard_worker(sender, worker_stop).await,
                        _ => unreachable!(),
                    }
                });
                tasks[i] = Some((cycle, task));
            } else if counts[i] == 0 {
                if let Some((cycle, task)) = tasks[i].take() {
                    cycle.cancel(); task.abort();
                    let reset = match i {
                        0 => Event::Network(NetworkState::default()),
                        1 => Event::Power(PowerState::default()),
                        2 => Event::Brightness(BrightnessState::default()),
                        3 => Event::Niri(NiriState::default()),
                        4 => Event::Keyboard(KeyboardLockState::default()),
                        _ => unreachable!(),
                    };
                    let _ = events.send(reset).await;
                }
            }
        }
        tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            changed = consumers.changed() => { if changed.is_err() { break; } }
            request = requests.recv() => {
                let Some(request) = request else { break; };
                let domain = match request.command {
                    Command::WifiEnabled { .. } | Command::WifiScan | Command::WifiDisconnect { .. } | Command::WifiConnect { .. }
                    | Command::WifiConnectSsid { .. } | Command::ProfileForget { .. } | Command::ProfileSetPassword { .. }
                    | Command::ProfileSecret { .. } => 0,
                    Command::PowerProfile { .. } => 1,
                    Command::Brightness { .. } => 2,
                    Command::Niri { .. } | Command::NiriAction { .. } | Command::NiriRefreshOutputs => 3,
                };
                let result = if tasks[domain].is_none() { Err("domain is inactive".into()) } else {
                    tokio::select! {
                        _ = stop.cancelled() => Err("backend stopped".into()),
                        result = tokio::time::timeout(COMMAND_TIMEOUT, execute(request.command, &system, &mut actions, &refresh)) => result.unwrap_or_else(|_| Err("service command timed out".into()))
                    }
                };
                let _ = request.reply.send(result);
            }
        }
    }
    for (cycle, task) in tasks.into_iter().flatten() { cycle.cancel(); task.abort(); }
}
