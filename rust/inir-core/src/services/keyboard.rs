//! Native caps/num lock state from evdev, replacing the Python daemon. Device LED
//! state is read directly, changes arrive as evdev events, and the device set is
//! refreshed on udev hotplug instead of a periodic re-enumeration.
use crate::events::Event;
use evdev::{Device, EventSummary, KeyCode, LedCode};
use inir_types::desktop::{KeyboardLockState, ServiceStatus};
use std::collections::HashMap;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Virtual/synthetic keyboards would double-count or report stale LEDs.
const IGNORED_NAMES: [&str; 2] = ["ydotool", "virtual"];

/// A keyboard exposes both the lock keys and their LEDs (matches the Python
/// daemon's candidate rule).
fn is_candidate(device: &Device) -> bool {
    let name = device.name().unwrap_or_default().to_lowercase();
    if IGNORED_NAMES.iter().any(|part| name.contains(part)) {
        return false;
    }
    let has_key = device.supported_keys().is_some_and(|keys| {
        keys.contains(KeyCode::KEY_CAPSLOCK) || keys.contains(KeyCode::KEY_NUMLOCK)
    });
    let has_led = device
        .supported_leds()
        .is_some_and(|leds| leds.contains(LedCode::LED_CAPSL) || leds.contains(LedCode::LED_NUML));
    has_key && has_led
}

fn led_state(device: &Device) -> io::Result<(bool, bool)> {
    let leds = device.get_led_state()?;
    Ok((
        leds.contains(LedCode::LED_CAPSL),
        leds.contains(LedCode::LED_NUML),
    ))
}

/// Majority vote across keyboards, keeping the previous value on a tie or when no
/// device reports (identical to the Python `_aggregate`).
fn aggregate<I: Iterator<Item = bool>>(values: I, previous: bool) -> bool {
    let (mut on, mut off) = (0usize, 0usize);
    for value in values {
        if value {
            on += 1;
        } else {
            off += 1;
        }
    }
    match on.cmp(&off) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => previous,
    }
}

enum Sample {
    State { index: usize, caps: bool, num: bool },
    Gone { index: usize },
}

/// Reads one device: a fresh sample whenever a lock LED
/// changes or a lock key is released (the LED settles shortly after the release).
async fn read_device(index: usize, mut stream: evdev::EventStream, tx: mpsc::Sender<Sample>) {
    loop {
        match stream.next_event().await {
            Ok(event) => {
                let resample = match event.destructure() {
                    EventSummary::Led(_, code, _) => {
                        code == LedCode::LED_CAPSL || code == LedCode::LED_NUML
                    }
                    EventSummary::Key(_, code, value) => {
                        value == 0
                            && (code == KeyCode::KEY_CAPSLOCK || code == KeyCode::KEY_NUMLOCK)
                    }
                    _ => false,
                };
                if resample {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    let Ok((caps, num)) = led_state(stream.device()) else {
                        let _ = tx.send(Sample::Gone { index }).await;
                        return;
                    };
                    if tx.send(Sample::State { index, caps, num }).await.is_err() {
                        return;
                    }
                }
            }
            Err(_) => {
                let _ = tx.send(Sample::Gone { index }).await;
                return;
            }
        }
    }
}

/// The udev socket stays on its own thread because it is not Send. Dropping this
/// guard wakes an eventfd, including when the async worker is aborted. With udev
/// available the thread blocks until hotplug or shutdown, with no idle timer.
struct Hotplug {
    receiver: mpsc::Receiver<()>,
    shutdown: OwnedFd,
}
impl Drop for Hotplug {
    fn drop(&mut self) {
        let value = 1u64.to_ne_bytes();
        unsafe {
            libc::write(
                self.shutdown.as_raw_fd(),
                value.as_ptr().cast(),
                value.len(),
            );
        }
    }
}
fn spawn_hotplug() -> io::Result<Hotplug> {
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let shutdown = unsafe { OwnedFd::from_raw_fd(fd) };
    let thread_shutdown = shutdown.try_clone()?;
    let (tx, rx) = mpsc::channel(8);
    std::thread::Builder::new()
        .name("inir-kbd-udev".into())
        .spawn(move || {
            let socket = udev::MonitorBuilder::new()
                .and_then(|builder| builder.match_subsystem("input"))
                .and_then(|builder| builder.listen())
                .ok();
            // Failed udev setup retains the old slow discovery fallback. It is
            // interruptible too, so releasing the lease never strands a thread.
            let timeout = if socket.is_some() { -1 } else { 5000 };
            loop {
                let mut poll_fds = [
                    libc::pollfd {
                        fd: thread_shutdown.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: socket.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                let result = unsafe { libc::poll(poll_fds.as_mut_ptr(), 2, timeout) };
                if result < 0 {
                    if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return;
                }
                if poll_fds[0].revents != 0 {
                    return;
                }
                if poll_fds[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                    return;
                }
                let changed = socket.as_ref().map_or(true, |socket| {
                    socket.iter().any(|event| {
                        matches!(
                            event.event_type(),
                            udev::EventType::Add
                                | udev::EventType::Remove
                                | udev::EventType::Change
                        )
                    })
                });
                if changed {
                    match tx.try_send(()) {
                        Ok(()) | Err(mpsc::error::TrySendError::Full(())) => {}
                        Err(mpsc::error::TrySendError::Closed(())) => return,
                    }
                }
            }
        })?;
    Ok(Hotplug {
        receiver: rx,
        shutdown,
    })
}

fn snapshot(states: &HashMap<usize, (bool, bool)>, last: &KeyboardLockState) -> KeyboardLockState {
    let devices = states.len() as u32;
    KeyboardLockState {
        status: ServiceStatus {
            ready: devices > 0,
            error: if devices > 0 {
                String::new()
            } else {
                "no readable keyboard LED devices".into()
            },
        },
        caps: aggregate(states.values().map(|value| value.0), last.caps),
        num: aggregate(states.values().map(|value| value.1), last.num),
        devices,
    }
}

async fn publish(
    events: &mpsc::Sender<Event>,
    states: &HashMap<usize, (bool, bool)>,
    last: &mut KeyboardLockState,
) {
    let state = snapshot(states, last);
    if state != *last {
        *last = state.clone();
        let _ = events.send(Event::Keyboard(state)).await;
    }
}

pub async fn stream(events: &mpsc::Sender<Event>, stop: &CancellationToken) -> Result<(), String> {
    let mut last = KeyboardLockState::default();
    let mut hotplug = spawn_hotplug().map_err(|error| error.to_string())?;
    loop {
        let candidates: Vec<Device> = evdev::enumerate()
            .map(|(_, device)| device)
            .filter(is_candidate)
            .collect();
        let (tx, mut rx) = mpsc::channel::<Sample>(64);
        let mut readers = tokio::task::JoinSet::new();
        let mut states: HashMap<usize, (bool, bool)> = HashMap::new();
        for (index, device) in candidates.into_iter().enumerate() {
            // Read the complete initial set before publishing ready, so consumers
            // cannot observe a fabricated all-off state or partial majority.
            let Ok(state) = led_state(&device) else {
                continue;
            };
            match device.into_event_stream() {
                Ok(stream) => {
                    states.insert(index, state);
                    let tx = tx.clone();
                    readers.spawn(read_device(index, stream, tx));
                }
                Err(error) => tracing::debug!(%error, "keyboard device stream unavailable"),
            }
        }
        publish(events, &states, &mut last).await;
        // The original `tx` is held here so `rx.recv()` never closes while the
        // inner loop runs, even when there are no devices.
        let rebuild = loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => {
                    readers.shutdown().await;
                    return Ok(());
                }
                sample = rx.recv() => {
                    if let Some(sample) = sample {
                        match sample {
                            Sample::State { index, caps, num } => { states.insert(index, (caps, num)); }
                            Sample::Gone { index } => { states.remove(&index); }
                        }
                        publish(events, &states, &mut last).await;
                    }
                }
                signal = hotplug.receiver.recv() => {
                    match signal { Some(()) => break true, None => break false }
                }
            }
        };
        drop(tx);
        readers.shutdown().await;
        if !rebuild {
            return Err("keyboard hotplug monitor stopped".into());
        }
        tokio::select! {
            _ = stop.cancelled() => return Ok(()),
            _ = tokio::time::sleep(Duration::from_millis(100)) => {},
        }
        while hotplug.receiver.try_recv().is_ok() {}
    }
}

#[cfg(test)]
mod tests {
    use super::{aggregate, snapshot};
    use inir_types::desktop::KeyboardLockState;
    use std::collections::HashMap;
    #[test]
    fn majority_vote_keeps_previous_on_tie_or_empty() {
        assert!(aggregate([true, true, false].into_iter(), false));
        assert!(!aggregate([false, false, true].into_iter(), true));
        // Tie keeps previous.
        assert!(aggregate([true, false].into_iter(), true));
        assert!(!aggregate([true, false].into_iter(), false));
        // No devices keeps previous.
        assert!(aggregate(std::iter::empty(), true));
    }
    #[test]
    fn readiness_and_count_follow_valid_device_samples() {
        let mut states = HashMap::new();
        let unavailable = snapshot(&states, &KeyboardLockState::default());
        assert!(!unavailable.status.ready);
        assert_eq!(unavailable.devices, 0);
        states.insert(0, (true, false));
        let ready = snapshot(&states, &unavailable);
        assert!(ready.status.ready && ready.caps && !ready.num);
        assert_eq!(ready.devices, 1);
        states.remove(&0);
        let gone = snapshot(&states, &ready);
        assert!(!gone.status.ready && gone.caps);
        assert_eq!(gone.devices, 0);
    }
    #[tokio::test]
    async fn dropping_hotplug_guard_stops_thread_without_waiting_for_a_timer() {
        let mut hotplug = super::spawn_hotplug().unwrap();
        let (_, empty) = tokio::sync::mpsc::channel(1);
        let mut receiver = std::mem::replace(&mut hotplug.receiver, empty);
        drop(hotplug);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while receiver.recv().await.is_some() {}
        })
        .await
        .expect("hotplug thread did not stop after dropping its guard");
    }
}
