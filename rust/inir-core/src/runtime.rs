use crate::{
    events::{Domain, Event},
    services::system::{self, Sampler},
    state::AppState,
};
use inir_types::desktop::{Command, DesktopState};
use inir_types::{config::Config, ResourceState, SystemInfo};
use std::{
    io,
    sync::{Arc, Mutex, OnceLock, RwLock, Weak},
    time::Duration,
};
use tokio::{
    runtime::Runtime,
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

/// One worker pool per shell process, shared by QObjects. A weak cache permits
/// complete teardown when the last QObject disappears on a Quickshell reload.
pub struct Backend {
    runtime: Option<Runtime>,
    cancellation: CancellationToken,
    state: Arc<RwLock<AppState>>,
    pub events: mpsc::Sender<Event>,
    pub system: watch::Receiver<SystemInfo>,
    pub resources: watch::Receiver<ResourceState>,
    pub config: watch::Receiver<Config>,
    pub desktop: watch::Receiver<DesktopState>,
    service_consumers: watch::Sender<usize>,
    requests: mpsc::Sender<crate::services::Request>,
    consumers: watch::Sender<usize>,
}

pub struct TaskGuard(JoinHandle<()>);
impl TaskGuard {
    pub fn is_finished(&self) -> bool {
        self.0.is_finished()
    }
}
impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct ResourceConsumer {
    consumers: watch::Sender<usize>,
}
impl Drop for ResourceConsumer {
    fn drop(&mut self) {
        self.consumers
            .send_modify(|count| *count = count.saturating_sub(1));
    }
}

impl Backend {
    pub fn shared() -> io::Result<Arc<Self>> {
        static SHARED: OnceLock<Mutex<Weak<Backend>>> = OnceLock::new();
        let mut weak = SHARED
            .get_or_init(|| Mutex::new(Weak::new()))
            .lock()
            .unwrap();
        if let Some(backend) = weak.upgrade() {
            return Ok(backend);
        }
        let backend = Arc::new(Self::new()?);
        *weak = Arc::downgrade(&backend);
        Ok(backend)
    }

    pub fn new() -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("inir-core")
            .enable_all()
            .build()?;
        let cancellation = CancellationToken::new();
        let state = Arc::new(RwLock::new(AppState::default()));
        let (events, mut incoming) = mpsc::channel(128);
        let (system_tx, system) = watch::channel(SystemInfo::default());
        let (resource_tx, resources) = watch::channel(ResourceState::default());
        let (config_tx, config) = watch::channel(Config::default());
        let (desktop_tx, desktop) = watch::channel(DesktopState::default());
        let (service_consumers, service_rx) = watch::channel(0usize);
        let (requests, request_rx) = mpsc::channel(128);
        runtime.spawn(crate::services::run(
            events.clone(),
            request_rx,
            service_rx,
            cancellation.clone(),
        ));
        let (consumers, mut consumer_rx) = watch::channel(0usize);
        let reducer_state = state.clone();
        let stop = cancellation.clone();
        runtime.spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    event = incoming.recv() => {
                        let Some(event) = event else { break; };
                        let mut state = reducer_state.write().unwrap();
                        match state.reduce(event) {
                            Some(Domain::System) => { system_tx.send_replace(state.system.clone()); }
                            Some(Domain::Resource) => { resource_tx.send_replace(state.resources.clone()); }
                            Some(Domain::Config) => { config_tx.send_replace(state.config.clone()); }
                            Some(Domain::Desktop) => { desktop_tx.send_replace(state.desktop.clone()); }
                            None => {}
                        }
                    }
                }
            }
        });
        let seed_events = events.clone();
        runtime.spawn(async move {
            if let Ok((identity, sample)) =
                tokio::task::spawn_blocking(|| (system::identity(), Sampler::default().sample()))
                    .await
            {
                let _ = seed_events.send(Event::System(identity)).await;
                if let Ok(sample) = sample {
                    let _ = seed_events.send(Event::Resource(sample)).await;
                }
            }
        });
        let stop = cancellation.clone();
        let sample_events = events.clone();
        runtime.spawn(async move {
            let mut sampler = Sampler::default();
            loop {
                // Zero timers and zero procfs reads without consumers. Reset CPU
                // history after suspension so the first sample isn't stale.
                if *consumer_rx.borrow_and_update() == 0 {
                    sampler = Sampler::default();
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => break,
                        changed = consumer_rx.changed() => { if changed.is_err() { break; } }
                    }
                    continue;
                }
                let result = tokio::task::spawn_blocking(move || {
                    let result = sampler.sample();
                    (sampler, result)
                })
                .await;
                match result {
                    Ok((next, sample)) => {
                        sampler = next;
                        match sample {
                            Ok(sample) => {
                                let _ = sample_events.send(Event::Resource(sample)).await;
                            }
                            Err(err) => tracing::warn!(%err, "resource sampling failed"),
                        }
                    }
                    Err(_) => break,
                }
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    changed = consumer_rx.changed() => { if changed.is_err() { break; } }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
        });
        Ok(Self {
            runtime: Some(runtime),
            cancellation,
            state,
            events,
            system,
            resources,
            config,
            desktop,
            service_consumers,
            requests,
            consumers,
        })
    }

    pub fn snapshot(&self) -> AppState {
        self.state.read().unwrap().clone()
    }

    pub fn resource_consumer(&self) -> ResourceConsumer {
        self.consumers.send_modify(|count| *count += 1);
        ResourceConsumer {
            consumers: self.consumers.clone(),
        }
    }

    pub fn resource_consumers(&self) -> usize {
        *self.consumers.borrow()
    }

    pub fn service_consumer(&self) -> ResourceConsumer {
        self.service_consumers.send_modify(|count| *count += 1);
        ResourceConsumer {
            consumers: self.service_consumers.clone(),
        }
    }

    pub fn service_consumers(&self) -> usize {
        *self.service_consumers.borrow()
    }

    pub async fn execute(&self, command: Command) -> Result<(), String> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.requests
            .send(crate::services::Request { command, reply })
            .await
            .map_err(|_| "service worker stopped")?;
        response
            .await
            .map_err(|_| "service command cancelled".to_string())?
    }

    pub fn spawn(
        &self,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> TaskGuard {
        TaskGuard(self.runtime.as_ref().unwrap().spawn(future))
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.cancellation.cancel();
        // Qt must not wait for a worker or queued QObject callback at destruction.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}
