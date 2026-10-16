//! Shared production utility logic. No Qt, subprocess interpreter or private runtime.
pub mod clipboard;
pub mod dev_reload;
pub mod devices;
pub mod files;
pub mod input_locks;
pub mod lyrics;
pub mod osk;
pub mod sensors;
pub mod system;

use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::{OnceCell, Semaphore};
use tokio_util::sync::CancellationToken;

pub const PROTOCOL_VERSION: u32 = 1;

/// A watcher must not outlive a crashed/reloaded shell or wl-paste owner. Linux
/// clears this flag for children at fork, so it affects only this helper process.
pub fn attach_to_parent() -> Result<(), String> {
    let parent = unsafe { libc::getppid() };
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0) } != 0 {
        return Err("Cannot attach utility lifetime to its parent".into());
    }
    if unsafe { libc::getppid() } != parent {
        return Err("Utility parent exited".into());
    }
    Ok(())
}
pub const OPERATIONS: &[&str] = &[
    "devices.snapshot",
    "system.sensors",
    "system.memoryMaps",
    "system.networkCounters",
    "system.disk",
    "files.read",
    "files.writeJson",
    "lyrics.fetch",
    "input.layoutLabels",
];

/// One context per backend or command invocation; clients and concurrency limits
/// are shared by jobs, never initialized on the GUI thread.
pub struct Context {
    http: OnceCell<reqwest::Client>,
    blocking: Arc<Semaphore>,
    network: Semaphore,
}
impl Default for Context {
    fn default() -> Self {
        Self {
            http: OnceCell::new(),
            blocking: Arc::new(Semaphore::new(2)),
            network: Semaphore::new(4),
        }
    }
}
impl Context {
    pub async fn network_permit(&self) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        self.network
            .acquire()
            .await
            .map_err(|_| "Utility context stopped".into())
    }
    pub async fn http(&self) -> Result<&reqwest::Client, String> {
        self.http
            .get_or_try_init(|| async {
                reqwest::Client::builder()
                    .user_agent("iNiR/2.32.0 (https://github.com/snowarch/inir)")
                    .timeout(std::time::Duration::from_secs(12))
                    .build()
                    .map_err(|_| "Cannot initialize the HTTP client".into())
            })
            .await
    }

    pub async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let permit = self
            .blocking
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "Utility context stopped")?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|_| "Utility worker stopped".to_owned())?
    }

    pub async fn execute(
        &self,
        operation: &str,
        args: Value,
        stop: &CancellationToken,
    ) -> Result<Value, String> {
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err("Utility request canceled".into()),
            result = self.dispatch(operation, args) => result,
        }
    }

    async fn dispatch(&self, operation: &str, args: Value) -> Result<Value, String> {
        match operation {
            "input.layoutLabels" => {
                let description = string(&args, "description")?.to_owned();
                self.blocking(move || osk::labels(&description)).await
            }
            "devices.snapshot" => {
                let root = args["root"]
                    .as_str()
                    .unwrap_or("/sys/bus/usb/devices")
                    .to_owned();
                self.blocking(move || devices::snapshot(std::path::Path::new(&root)))
                    .await
            }
            "system.sensors" => {
                let root = args["root"].as_str().unwrap_or("/sys").to_owned();
                self.blocking(move || Ok(json!(sensors::detect(std::path::Path::new(&root)))))
                    .await
            }
            "system.memoryMaps" => {
                let pid = args["pid"]
                    .as_f64()
                    .filter(|v| {
                        v.is_finite() && v.fract() == 0.0 && *v > 0.0 && *v <= u32::MAX as f64
                    })
                    .ok_or("Invalid process ID")? as u32;
                self.blocking(move || system::memory_maps(pid)).await
            }
            "system.networkCounters" => {
                let interfaces: Vec<String> = serde_json::from_value(args["interfaces"].clone())
                    .map_err(|_| "Missing interfaces")?;
                self.blocking(move || system::network_counters(&interfaces))
                    .await
            }
            "system.disk" => {
                let path = args["path"].as_str().unwrap_or("/").to_owned();
                self.blocking(move || system::disk(&path)).await
            }
            "files.read" => {
                let paths: Vec<String> =
                    serde_json::from_value(args["paths"].clone()).map_err(|_| "Missing paths")?;
                self.blocking(move || files::read_many(&paths)).await
            }
            "files.writeJson" => {
                let path = string(&args, "path")?.to_owned();
                let value = args.get("value").ok_or("Missing JSON value")?.clone();
                self.blocking(move || {
                    let text =
                        serde_json::to_string_pretty(&value).map_err(|e| e.to_string())? + "\n";
                    files::atomic_write(std::path::Path::new(&path), text.as_bytes())?;
                    Ok(Value::Null)
                })
                .await
            }
            "lyrics.fetch" => lyrics::fetch(self, &args).await,
            _ => Err(format!("Unknown utility operation: {operation}")),
        }
    }
}

pub fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Missing {key}"))
}
