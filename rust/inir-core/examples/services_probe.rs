//! Read-only live integration probe; never issues device control commands.
use inir_core::runtime::Backend;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = Arc::new(Backend::new()?);
    let consumer = backend.service_consumer();
    let start = Instant::now();
    let state = loop {
        let state = backend.snapshot().desktop;
        let statuses = [
            &state.network.status,
            &state.power.status,
            &state.brightness.status,
            &state.niri.status,
        ];
        if statuses.iter().all(|s| s.ready || !s.error.is_empty())
            || start.elapsed() > Duration::from_secs(15)
        {
            break state;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "network": {"status":state.network.status,"accessPoints":state.network.access_points.len(),"connectivity":state.network.connectivity},
            "power": {"status":state.power.status,"profile":state.power.active_profile,"profiles":state.power.profiles},
            "brightness": {"status":state.brightness.status,"devices":state.brightness.devices.len()},
            "niri": {"status":state.niri.status,"windows":state.niri.windows.len()}
        }))?
    );
    drop(consumer);
    drop(backend);
    Ok(())
}
