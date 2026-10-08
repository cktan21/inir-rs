#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    unsafe extern "C++Qt" {
        include!("inir-qt/service_models.h");
        #[qobject]
        type ServiceModels;
    }
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = ServiceModels]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, active)]
        #[qproperty(bool, network_ready)]
        #[qproperty(QString, network_error)]
        #[qproperty(bool, bluetooth_ready)]
        #[qproperty(QString, bluetooth_error)]
        #[qproperty(bool, battery_ready)]
        #[qproperty(QString, battery_error)]
        #[qproperty(bool, power_ready)]
        #[qproperty(QString, power_error)]
        #[qproperty(bool, brightness_ready)]
        #[qproperty(QString, brightness_error)]
        #[qproperty(bool, audio_ready)]
        #[qproperty(QString, audio_error)]
        #[qproperty(bool, media_ready)]
        #[qproperty(QString, media_error)]
        #[qproperty(bool, niri_ready)]
        #[qproperty(QString, niri_error)]
        #[qproperty(bool, wifi_enabled)]
        #[qproperty(bool, ethernet)]
        #[qproperty(i32, connectivity)]
        #[qproperty(i32, bluetooth_connected_count)]
        #[qproperty(bool, bluetooth_enabled)]
        #[qproperty(bool, battery_available)]
        #[qproperty(bool, on_battery)]
        #[qproperty(i32, battery_state)]
        #[qproperty(f64, battery_percentage)]
        #[qproperty(f64, energy_rate)]
        #[qproperty(f64, time_to_empty)]
        #[qproperty(f64, time_to_full)]
        #[qproperty(QString, active_power_profile)]
        #[qproperty(QString, power_profiles_json)]
        #[qproperty(QString, default_sink)]
        #[qproperty(QString, default_source)]
        #[qproperty(bool, overview_open)]
        #[qproperty(u64, network_revision)]
        #[qproperty(u64, bluetooth_revision)]
        #[qproperty(u64, brightness_revision)]
        #[qproperty(u64, audio_revision)]
        #[qproperty(u64, media_revision)]
        #[qproperty(u64, niri_revision)]
        type DesktopServices = super::DesktopServicesRust;
        #[qinvokable]
        fn set_services_active(self: Pin<&mut DesktopServices>, active: bool);
        #[qinvokable]
        fn execute(self: Pin<&mut DesktopServices>, id: QString, command: QString) -> bool;
        #[qsignal]
        fn command_finished(
            self: Pin<&mut DesktopServices>,
            id: QString,
            success: bool,
            error: QString,
        );
    }
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[inherit]
        fn apply_collection(
            self: Pin<&mut DesktopServices>,
            name: &QString,
            json: &QString,
        ) -> bool;
    }
    impl cxx_qt::Threading for DesktopServices {}
    impl cxx_qt::Initialize for DesktopServices {}
}

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use inir_core::runtime::{Backend, ResourceConsumer, TaskGuard};
use inir_types::desktop::{Command, DesktopState};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct DesktopServicesRust {
    active: bool,
    network_ready: bool,
    network_error: QString,
    bluetooth_ready: bool,
    bluetooth_error: QString,
    battery_ready: bool,
    battery_error: QString,
    power_ready: bool,
    power_error: QString,
    brightness_ready: bool,
    brightness_error: QString,
    audio_ready: bool,
    audio_error: QString,
    media_ready: bool,
    media_error: QString,
    niri_ready: bool,
    niri_error: QString,
    wifi_enabled: bool,
    ethernet: bool,
    connectivity: i32,
    bluetooth_connected_count: i32,
    bluetooth_enabled: bool,
    battery_available: bool,
    on_battery: bool,
    battery_state: i32,
    battery_percentage: f64,
    energy_rate: f64,
    time_to_empty: f64,
    time_to_full: f64,
    active_power_profile: QString,
    power_profiles_json: QString,
    default_sink: QString,
    default_source: QString,
    overview_open: bool,
    network_revision: u64,
    bluetooth_revision: u64,
    brightness_revision: u64,
    audio_revision: u64,
    media_revision: u64,
    niri_revision: u64,
    backend: Option<Arc<Backend>>,
    commands: Option<tokio::sync::mpsc::Sender<(QString, Command)>>,
    consumer: Option<ResourceConsumer>,
    tasks: Vec<TaskGuard>,
    last: DesktopState,
}

impl cxx_qt::Initialize for qobject::DesktopServices {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        match Backend::shared() {
            Ok(backend) => {
                let qt = self.qt_thread();
                let mut states = backend.desktop.clone();
                let pending = Arc::new(Mutex::new((false, None)));
                let task = backend.spawn(async move {
                    loop {
                        let state = states.borrow_and_update().clone();
                        let queue = {
                            let mut pending = pending.lock().unwrap();
                            pending.1 = Some(state);
                            let queue = !pending.0;
                            pending.0 = true;
                            queue
                        };
                        if queue {
                            let pending = pending.clone();
                            if qt
                                .queue(move |object| {
                                    let state = {
                                        let mut pending = pending.lock().unwrap();
                                        pending.0 = false;
                                        pending.1.take()
                                    };
                                    if let Some(state) = state {
                                        object.apply(state);
                                    }
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        if states.changed().await.is_err() {
                            break;
                        }
                    }
                });
                let (commands, mut incoming) =
                    tokio::sync::mpsc::channel::<(QString, Command)>(128);
                let qt = self.qt_thread();
                let engine = backend.clone();
                let writer = backend.spawn(async move {
                    while let Some((id, command)) = incoming.recv().await {
                        let result = engine.execute(command).await;
                        if qt
                            .queue(move |mut object| {
                                object.as_mut().command_finished(
                                    id,
                                    result.is_ok(),
                                    QString::from(result.err().unwrap_or_default()),
                                );
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                });
                let mut rust = self.as_mut().rust_mut();
                rust.backend = Some(backend);
                rust.commands = Some(commands);
                rust.tasks = vec![task, writer];
            }
            Err(error) => self
                .as_mut()
                .set_network_error(QString::from(error.to_string())),
        }
    }
}

impl qobject::DesktopServices {
    fn apply(mut self: Pin<&mut Self>, state: DesktopState) {
        let apply_start = std::time::Instant::now();
        let old = self.rust().last.clone();
        if old.network != state.network {
            self.as_mut()
                .set_network_error(QString::from(state.network.status.error.as_str()));
            self.as_mut().set_wifi_enabled(state.network.wifi_enabled);
            self.as_mut().set_ethernet(state.network.ethernet);
            self.as_mut()
                .set_connectivity(state.network.connectivity as i32);
            if state.network.access_points != old.network.access_points {
                self.as_mut().apply_collection(
                    &QString::from("accessPoints"),
                    &QString::from(serde_json::to_string(&state.network.access_points).unwrap()),
                );
            }
            let revision = self.rust().network_revision.wrapping_add(1);
            self.as_mut().set_network_revision(revision);
            self.as_mut().set_network_ready(state.network.status.ready);
        }
        if old.bluetooth != state.bluetooth {
            self.as_mut()
                .set_bluetooth_error(QString::from(state.bluetooth.status.error.as_str()));
            self.as_mut().set_bluetooth_connected_count(
                state
                    .bluetooth
                    .devices
                    .iter()
                    .filter(|d| d.connected)
                    .count() as i32,
            );
            self.as_mut()
                .set_bluetooth_enabled(state.bluetooth.adapters.iter().any(|a| a.powered));
            if state.bluetooth.adapters != old.bluetooth.adapters {
                self.as_mut().apply_collection(
                    &QString::from("bluetoothAdapters"),
                    &QString::from(serde_json::to_string(&state.bluetooth.adapters).unwrap()),
                );
            }
            if state.bluetooth.devices != old.bluetooth.devices {
                self.as_mut().apply_collection(
                    &QString::from("bluetoothDevices"),
                    &QString::from(serde_json::to_string(&state.bluetooth.devices).unwrap()),
                );
            }
            let revision = self.rust().bluetooth_revision.wrapping_add(1);
            self.as_mut().set_bluetooth_revision(revision);
            self.as_mut()
                .set_bluetooth_ready(state.bluetooth.status.ready);
        }
        if old.battery != state.battery {
            // Commit the complete snapshot before notifying QML. Battery policy
            // bindings must never observe a default percentage with ready=true.
            {
                let mut rust = self.as_mut().rust_mut();
                rust.battery_ready = state.battery.status.ready;
                rust.battery_error = QString::from(state.battery.status.error.as_str());
                rust.battery_available = state.battery.available;
                rust.on_battery = state.battery.on_battery;
                rust.battery_state = state.battery.state as i32;
                rust.battery_percentage = state.battery.percentage;
                rust.energy_rate = state.battery.energy_rate;
                rust.time_to_empty = state.battery.time_to_empty as f64;
                rust.time_to_full = state.battery.time_to_full as f64;
            }
            if QString::from(state.battery.status.error.as_str())
                != QString::from(old.battery.status.error.as_str())
            {
                self.as_mut().battery_error_changed();
            }
            if state.battery.available != old.battery.available {
                self.as_mut().battery_available_changed();
            }
            if state.battery.on_battery != old.battery.on_battery {
                self.as_mut().on_battery_changed();
            }
            if state.battery.state as i32 != old.battery.state as i32 {
                self.as_mut().battery_state_changed();
            }
            if state.battery.percentage != old.battery.percentage {
                self.as_mut().battery_percentage_changed();
            }
            if state.battery.energy_rate != old.battery.energy_rate {
                self.as_mut().energy_rate_changed();
            }
            if state.battery.time_to_empty as f64 != old.battery.time_to_empty as f64 {
                self.as_mut().time_to_empty_changed();
            }
            if state.battery.time_to_full as f64 != old.battery.time_to_full as f64 {
                self.as_mut().time_to_full_changed();
            }
            if state.battery.status.ready != old.battery.status.ready {
                self.as_mut().battery_ready_changed();
            }
        }
        if old.power != state.power {
            self.as_mut()
                .set_power_error(QString::from(state.power.status.error.as_str()));
            self.as_mut()
                .set_active_power_profile(QString::from(state.power.active_profile.as_str()));
            self.as_mut().set_power_profiles_json(QString::from(
                serde_json::to_string(&state.power.profiles).unwrap(),
            ));
            self.as_mut().set_power_ready(state.power.status.ready);
        }
        if old.brightness != state.brightness {
            self.as_mut()
                .set_brightness_error(QString::from(state.brightness.status.error.as_str()));

            if state.brightness.devices != old.brightness.devices {
                self.as_mut().apply_collection(
                    &QString::from("backlights"),
                    &QString::from(serde_json::to_string(&state.brightness.devices).unwrap()),
                );
            }
            let revision = self.rust().brightness_revision.wrapping_add(1);
            self.as_mut().set_brightness_revision(revision);
            self.as_mut()
                .set_brightness_ready(state.brightness.status.ready);
        }
        if old.audio != state.audio {
            self.as_mut()
                .set_audio_error(QString::from(state.audio.status.error.as_str()));
            self.as_mut()
                .set_default_sink(QString::from(state.audio.default_sink.as_str()));
            self.as_mut()
                .set_default_source(QString::from(state.audio.default_source.as_str()));
            if state.audio.nodes != old.audio.nodes {
                self.as_mut().apply_collection(
                    &QString::from("audioNodes"),
                    &QString::from(serde_json::to_string(&state.audio.nodes).unwrap()),
                );
            }
            let revision = self.rust().audio_revision.wrapping_add(1);
            self.as_mut().set_audio_revision(revision);
            self.as_mut().set_audio_ready(state.audio.status.ready);
        }
        if old.media != state.media {
            self.as_mut()
                .set_media_error(QString::from(state.media.status.error.as_str()));

            if state.media.players != old.media.players {
                self.as_mut().apply_collection(
                    &QString::from("mediaPlayers"),
                    &QString::from(serde_json::to_string(&state.media.players).unwrap()),
                );
            }
            let revision = self.rust().media_revision.wrapping_add(1);
            self.as_mut().set_media_revision(revision);
            self.as_mut().set_media_ready(state.media.status.ready);
        }
        if old.niri != state.niri {
            self.as_mut()
                .set_niri_error(QString::from(state.niri.status.error.as_str()));
            self.as_mut().set_overview_open(state.niri.overview_open);
            if state.niri.windows != old.niri.windows {
                self.as_mut().apply_collection(
                    &QString::from("niriWindows"),
                    &QString::from(serde_json::to_string(&state.niri.windows).unwrap()),
                );
            }
            if state.niri.workspaces != old.niri.workspaces {
                self.as_mut().apply_collection(
                    &QString::from("niriWorkspaces"),
                    &QString::from(serde_json::to_string(&state.niri.workspaces).unwrap()),
                );
            }
            let revision = self.rust().niri_revision.wrapping_add(1);
            self.as_mut().set_niri_revision(revision);
            self.as_mut().set_niri_ready(state.niri.status.ready);
        }
        self.as_mut().rust_mut().last = state;
        let apply_duration = apply_start.elapsed();
        if apply_duration.as_micros() > 100 {
            tracing::warn!(apply_us = apply_duration.as_micros(), "slow apply call on Qt main thread");
        } else {
            tracing::debug!(apply_us = apply_duration.as_micros(), "apply call");
        }
    }

    pub fn set_services_active(mut self: Pin<&mut Self>, active: bool) {
        let consumer = if active && self.rust().consumer.is_none() {
            self.rust()
                .backend
                .as_ref()
                .map(|backend| backend.service_consumer())
        } else {
            None
        };
        if !active {
            self.as_mut().rust_mut().consumer = None;
        } else if let Some(consumer) = consumer {
            self.as_mut().rust_mut().consumer = Some(consumer);
        }
        let active = active && self.rust().consumer.is_some();
        self.as_mut().set_active(active);
    }

    pub fn execute(mut self: Pin<&mut Self>, id: QString, command: QString) -> bool {
        let command: Command = match serde_json::from_str(&command.to_string()) {
            Ok(command) => command,
            Err(error) => {
                self.as_mut()
                    .command_finished(id, false, QString::from(error.to_string()));
                return false;
            }
        };
        let Some(commands) = self.rust().commands.as_ref() else {
            self.as_mut()
                .command_finished(id, false, QString::from("Rust backend is unavailable"));
            return false;
        };
        if let Err(error) = commands.try_send((id, command)) {
            let (id, _) = error.into_inner();
            self.as_mut().command_finished(
                id,
                false,
                QString::from("service command queue is full or closed"),
            );
            return false;
        }
        true
    }
}
