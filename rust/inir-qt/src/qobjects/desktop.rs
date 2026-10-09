#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
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
        #[qproperty(bool, power_ready)]
        #[qproperty(QString, power_error)]
        #[qproperty(bool, brightness_ready)]
        #[qproperty(QString, brightness_error)]
        #[qproperty(bool, niri_ready)]
        #[qproperty(QString, niri_error)]
        #[qproperty(bool, wifi_enabled)]
        #[qproperty(bool, ethernet)]
        #[qproperty(i32, connectivity)]
        #[qproperty(QString, active_power_profile)]
        #[qproperty(QString, power_profiles_json)]
        #[qproperty(bool, overview_open)]
        #[qproperty(u64, network_revision)]
        #[qproperty(u64, brightness_revision)]
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
            rows: &QVariant,
        ) -> bool;
    }
    impl cxx_qt::Threading for DesktopServices {}
    impl cxx_qt::Initialize for DesktopServices {}
}

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use inir_core::runtime::{Backend, ResourceConsumer, TaskGuard};
use inir_types::desktop::{AccessPoint, Backlight, Command, DesktopState, NiriWindow, NiriWorkspace};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};

type VariantMap = QMap<QMapPair_QString_QVariant>;

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

/// Build a `QVariantList` (wrapped in a `QVariant`) from typed rows with no
/// JSON serialization. Each row is a `QVariantMap`; the C++ model applies it
/// directly, so no encoding or `QJsonDocument` parse runs on the GUI thread.
fn rows(items: impl ExactSizeIterator<Item = VariantMap>) -> QVariant {
    let mut list = QList::<QVariant>::default();
    list.reserve(items.len() as isize);
    for item in items {
        list.append(QVariant::from(&item));
    }
    QVariant::from(&list)
}

fn access_point_row(ap: &AccessPoint) -> VariantMap {
    let mut m = VariantMap::default();
    m.insert(QString::from("id"), qstr(&ap.id));
    m.insert(QString::from("device"), qstr(&ap.device));
    m.insert(QString::from("ssid"), qstr(&ap.ssid));
    m.insert(QString::from("bssid"), qstr(&ap.bssid));
    m.insert(QString::from("strength"), QVariant::from(&(ap.strength as i32)));
    m.insert(QString::from("frequency"), QVariant::from(&(ap.frequency as i32)));
    m.insert(QString::from("rate"), QVariant::from(&(ap.rate as i32)));
    m.insert(QString::from("security"), qstr(&ap.security));
    m.insert(QString::from("active"), QVariant::from(&ap.active));
    m
}

fn backlight_row(b: &Backlight) -> VariantMap {
    let mut m = VariantMap::default();
    m.insert(QString::from("id"), qstr(&b.id));
    m.insert(QString::from("kind"), qstr(&b.kind));
    m.insert(QString::from("raw"), QVariant::from(&(b.raw as i32)));
    m.insert(QString::from("maximum"), QVariant::from(&(b.maximum as i32)));
    m.insert(QString::from("value"), QVariant::from(&b.value));
    m
}

fn niri_window_row(w: &NiriWindow) -> VariantMap {
    let mut m = VariantMap::default();
    m.insert(QString::from("id"), qstr(&w.id));
    m.insert(QString::from("title"), qstr(&w.title));
    m.insert(QString::from("appId"), qstr(&w.app_id));
    m.insert(QString::from("workspaceId"), qstr(&w.workspace_id));
    m.insert(QString::from("focused"), QVariant::from(&w.focused));
    m.insert(QString::from("floating"), QVariant::from(&w.floating));
    m.insert(QString::from("urgent"), QVariant::from(&w.urgent));
    m.insert(
        QString::from("focusSerial"),
        QVariant::from(&(w.focus_serial as i64)),
    );
    m.insert(QString::from("column"), QVariant::from(&w.column));
    m.insert(QString::from("row"), QVariant::from(&w.row));
    m
}

fn niri_workspace_row(w: &NiriWorkspace) -> VariantMap {
    let mut m = VariantMap::default();
    m.insert(QString::from("id"), qstr(&w.id));
    m.insert(QString::from("index"), QVariant::from(&(w.index as i32)));
    m.insert(QString::from("name"), qstr(&w.name));
    m.insert(QString::from("output"), qstr(&w.output));
    m.insert(QString::from("active"), QVariant::from(&w.active));
    m.insert(QString::from("focused"), QVariant::from(&w.focused));
    m.insert(QString::from("activeWindowId"), qstr(&w.active_window_id));
    m.insert(QString::from("urgent"), QVariant::from(&w.urgent));
    m
}

#[derive(Default)]
pub struct DesktopServicesRust {
    active: bool,
    network_ready: bool,
    network_error: QString,
    power_ready: bool,
    power_error: QString,
    brightness_ready: bool,
    brightness_error: QString,
    niri_ready: bool,
    niri_error: QString,
    wifi_enabled: bool,
    ethernet: bool,
    connectivity: i32,
    active_power_profile: QString,
    power_profiles_json: QString,
    overview_open: bool,
    network_revision: u64,
    brightness_revision: u64,
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
                let rows = rows(state.network.access_points.iter().map(access_point_row));
                self.as_mut()
                    .apply_collection(&QString::from("accessPoints"), &rows);
            }
            let revision = self.rust().network_revision.wrapping_add(1);
            self.as_mut().set_network_revision(revision);
            self.as_mut().set_network_ready(state.network.status.ready);
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
                let rows = rows(state.brightness.devices.iter().map(backlight_row));
                self.as_mut()
                    .apply_collection(&QString::from("backlights"), &rows);
            }
            let revision = self.rust().brightness_revision.wrapping_add(1);
            self.as_mut().set_brightness_revision(revision);
            self.as_mut()
                .set_brightness_ready(state.brightness.status.ready);
        }
        if old.niri != state.niri {
            self.as_mut()
                .set_niri_error(QString::from(state.niri.status.error.as_str()));
            self.as_mut().set_overview_open(state.niri.overview_open);
            if state.niri.windows != old.niri.windows {
                let rows = rows(state.niri.windows.iter().map(niri_window_row));
                self.as_mut()
                    .apply_collection(&QString::from("niriWindows"), &rows);
            }
            if state.niri.workspaces != old.niri.workspaces {
                let rows = rows(state.niri.workspaces.iter().map(niri_workspace_row));
                self.as_mut()
                    .apply_collection(&QString::from("niriWorkspaces"), &rows);
            }
            let revision = self.rust().niri_revision.wrapping_add(1);
            self.as_mut().set_niri_revision(revision);
            self.as_mut().set_niri_ready(state.niri.status.ready);
        }
        self.as_mut().rust_mut().last = state;
        let apply_duration = apply_start.elapsed();
        if apply_duration.as_micros() > 100 {
            tracing::warn!(
                apply_us = apply_duration.as_micros(),
                "slow apply call on Qt main thread"
            );
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
