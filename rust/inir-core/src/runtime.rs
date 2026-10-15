use crate::{events::Event, services::system::{self, Sampler}, state::AppState};
use inir_types::desktop::*;
use inir_types::{config::Config, ResourceState, SystemInfo};
use std::{io, sync::{Arc, Mutex, OnceLock, Weak}, time::Duration};
use tokio::{runtime::Handle, sync::{mpsc, watch}, task::JoinHandle};
use tokio_util::sync::CancellationToken;

/// One current-thread executor, driven continuously on its own OS thread.
/// Domain state never crosses a global clone/diff boundary.
pub struct Backend {
    runtime: Handle,
    cancellation: CancellationToken,
    pub events: mpsc::Sender<Event>,
    pub system: watch::Receiver<SystemInfo>,
    pub resources: watch::Receiver<ResourceState>,
    pub config: watch::Receiver<Config>,
    pub network: watch::Receiver<Arc<NetworkState>>,
    pub power: watch::Receiver<Arc<PowerState>>,
    pub brightness: watch::Receiver<Arc<BrightnessState>>,
    pub niri: watch::Receiver<Arc<NiriState>>,
    pub keyboard: watch::Receiver<Arc<KeyboardLockState>>,
    domains: watch::Sender<[usize; 5]>,
    requests: mpsc::Sender<crate::services::Request>,
    consumers: watch::Sender<usize>,
}
pub struct TaskGuard(JoinHandle<()>);
impl TaskGuard { pub fn is_finished(&self) -> bool { self.0.is_finished() } }
impl Drop for TaskGuard { fn drop(&mut self) { self.0.abort(); } }
pub struct ResourceConsumer { consumers: watch::Sender<usize> }
impl Drop for ResourceConsumer {
    fn drop(&mut self) { self.consumers.send_modify(|count| *count = count.saturating_sub(1)); }
}
pub struct DomainConsumer { domains: watch::Sender<[usize; 5]>, index: usize }
impl Drop for DomainConsumer {
    fn drop(&mut self) { self.domains.send_modify(|counts| counts[self.index] = counts[self.index].saturating_sub(1)); }
}
impl Backend {
    pub fn shared() -> io::Result<Arc<Self>> {
        static SHARED: OnceLock<Mutex<Weak<Backend>>> = OnceLock::new();
        let mut weak = SHARED.get_or_init(|| Mutex::new(Weak::new())).lock().unwrap();
        if let Some(backend) = weak.upgrade() { return Ok(backend); }
        let backend = Arc::new(Self::new()?); *weak = Arc::downgrade(&backend); Ok(backend)
    }
    pub fn new() -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let handle = runtime.handle().clone();
        let cancellation = CancellationToken::new();
        let thread_stop = cancellation.clone();
        std::thread::Builder::new().name("inir-core".into()).spawn(move || {
            runtime.block_on(thread_stop.cancelled());
            runtime.shutdown_background();
        })?;
        let (events, mut incoming) = mpsc::channel(128);
        let (system_tx, system) = watch::channel(SystemInfo::default());
        let (resource_tx, resources) = watch::channel(ResourceState::default());
        let (config_tx, config) = watch::channel(Config::default());
        let (network_tx, network) = watch::channel(Arc::new(NetworkState::default()));
        let (power_tx, power) = watch::channel(Arc::new(PowerState::default()));
        let (brightness_tx, brightness) = watch::channel(Arc::new(BrightnessState::default()));
        let (niri_tx, niri) = watch::channel(Arc::new(NiriState::default()));
        let (keyboard_tx, keyboard) = watch::channel(Arc::new(KeyboardLockState::default()));
        let (domains, domain_rx) = watch::channel([0usize; 5]);
        let (requests, request_rx) = mpsc::channel(128);
        handle.spawn(crate::services::run(events.clone(), request_rx, domain_rx, cancellation.clone()));
        let (consumers, mut consumer_rx) = watch::channel(0usize);
        let stop = cancellation.clone();
        handle.spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    event = incoming.recv() => {
                        let Some(event) = event else { break; };
                        macro_rules! publish {
                            ($tx:ident, $value:ident) => {
                                if **$tx.borrow() != $value { $tx.send_replace(Arc::new($value)); }
                            };
                        }
                        match event {
                            Event::System(next) => { system_tx.send_if_modified(|old| { if *old == next { false } else { *old = next; true } }); }
                            Event::Resource(next) => { resource_tx.send_if_modified(|old| { if *old == next { false } else { *old = next; true } }); }
                            Event::Config(next) => { config_tx.send_if_modified(|old| { if *old == next { false } else { *old = next; true } }); }
                            Event::Network(mut next) => {
                                // Publish one representative per SSID; the actor
                                // retains all BSSIDs for signal reduction/commands.
                                let mut grouped = std::collections::BTreeMap::<String,AccessPoint>::new();
                                for ap in std::mem::take(&mut next.access_points) {
                                    if ap.ssid.is_empty() { continue; }
                                    let replace = grouped.get(&ap.ssid).is_none_or(|old|
                                        (ap.active && !old.active) || (ap.active == old.active && ap.strength > old.strength));
                                    if replace { grouped.insert(ap.ssid.clone(),ap); }
                                }
                                next.access_points = grouped.into_values().collect();
                                next.access_points.sort_by(|a,b| b.active.cmp(&a.active).then(b.strength.cmp(&a.strength)).then(a.ssid.cmp(&b.ssid)).then(a.id.cmp(&b.id)));
                                publish!(network_tx, next);
                            }
                            Event::Power(next) => publish!(power_tx, next),
                            Event::Brightness(next) => publish!(brightness_tx, next),
                            Event::Niri(next) => publish!(niri_tx, next),
                            Event::NiriFrame(next) => {
                                if !Arc::ptr_eq(&niri_tx.borrow(), &next) { niri_tx.send_replace(next); }
                            }
                            Event::Keyboard(next) => publish!(keyboard_tx, next),
                        }
                    }
                }
            }
        });
        let seed_events = events.clone();
        handle.spawn(async move {
            if let Ok((identity, sample)) = tokio::task::spawn_blocking(|| (system::identity(), Sampler::default().sample())).await {
                let _ = seed_events.send(Event::System(identity)).await;
                if let Ok(sample) = sample { let _ = seed_events.send(Event::Resource(sample)).await; }
            }
        });
        let stop = cancellation.clone();
        let sample_events = events.clone();
        handle.spawn(async move {
            let mut sampler = Sampler::default();
            loop {
                if *consumer_rx.borrow_and_update() == 0 {
                    sampler = Sampler::default();
                    tokio::select! { biased; _ = stop.cancelled() => break, changed = consumer_rx.changed() => { if changed.is_err() { break; } } }
                    continue;
                }
                match tokio::task::spawn_blocking(move || { let result = sampler.sample(); (sampler, result) }).await {
                    Ok((next, sample)) => {
                        sampler = next;
                        match sample { Ok(sample) => { let _ = sample_events.send(Event::Resource(sample)).await; }, Err(err) => tracing::warn!(%err, "resource sampling failed") }
                    }
                    Err(_) => break,
                }
                tokio::select! { biased; _ = stop.cancelled() => break, changed = consumer_rx.changed() => { if changed.is_err() { break; } }, _ = tokio::time::sleep(Duration::from_secs(1)) => {} }
            }
        });
        Ok(Self { runtime: handle, cancellation, events, system, resources, config, network, power, brightness, niri, keyboard, domains, requests, consumers })
    }
    /// Explicit diagnostic snapshot only; never used to publish domain changes.
    pub fn snapshot(&self) -> AppState {
        AppState { system: self.system.borrow().clone(), resources: self.resources.borrow().clone(), config: self.config.borrow().clone(),
            desktop: DesktopState { network: (**self.network.borrow()).clone(), power: (**self.power.borrow()).clone(),
                brightness: (**self.brightness.borrow()).clone(), niri: (**self.niri.borrow()).clone() } }
    }
    pub fn resource_consumer(&self) -> ResourceConsumer {
        self.consumers.send_modify(|count| *count += 1);
        ResourceConsumer { consumers: self.consumers.clone() }
    }
    pub fn resource_consumers(&self) -> usize { *self.consumers.borrow() }
    pub fn acquire(&self, index: usize) -> DomainConsumer {
        assert!(index < 5);
        self.domains.send_modify(|counts| counts[index] += 1);
        DomainConsumer { domains: self.domains.clone(), index }
    }
    pub fn service_consumer(&self) -> Vec<DomainConsumer> { (0..5).map(|index| self.acquire(index)).collect() }
    pub fn service_consumers(&self) -> usize { self.domains.borrow().iter().sum() }
    pub fn domain_consumers(&self) -> [usize; 5] { *self.domains.borrow() }
    pub async fn execute(&self, command: Command) -> Result<(), String> {
        self.query(command).await.map(|_| ())
    }
    /// Runs a command and returns its result payload (empty unless the command yields data).
    pub async fn query(&self, command: Command) -> Result<String, String> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.requests.send(crate::services::Request { command, reply }).await.map_err(|_| "service worker stopped")?;
        response.await.map_err(|_| "service command cancelled".to_string())?
    }
    pub fn spawn(&self, future: impl std::future::Future<Output = ()> + Send + 'static) -> TaskGuard {
        TaskGuard(self.runtime.spawn(future))
    }
}
impl Drop for Backend { fn drop(&mut self) { self.cancellation.cancel(); } }
