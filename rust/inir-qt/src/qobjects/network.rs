#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("inir-qt/list_model.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }
    unsafe extern "C++Qt" {
        include!("inir-qt/list_model.h");
        #[qobject]
        type NetworkListModelBase;
    }
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = NetworkListModelBase]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, active)]
        #[qproperty(bool, ready)]
        #[qproperty(QString, error)]
        #[qproperty(i32, count)]
        #[qproperty(u64, apply_calls)]
        #[qproperty(f64, apply_total_us)]
        #[qproperty(f64, apply_max_us)]
        #[qproperty(bool, wifi_enabled)]
        #[qproperty(bool, ethernet)]
        #[qproperty(i32, connectivity)]
        #[qproperty(QString, active_ssid)]
        #[qproperty(QVariant, known_profiles)]
        #[qproperty(QVariant, active_connections)]
        #[qproperty(i32, active_strength)]
        type NativeNetwork = super::NativeNetworkRust;
        #[qinvokable]
        fn set_enabled(self: Pin<&mut NativeNetwork>, enabled: bool);
        #[qsignal]
        fn rows_updated(self: Pin<&mut NativeNetwork>, roles: u64);
        #[qsignal]
        fn structure_changed(self: Pin<&mut NativeNetwork>);
        #[qsignal]
        fn command_finished(
            self: Pin<&mut NativeNetwork>,
            id: QString,
            success: bool,
            error: QString,
        );
        #[qsignal]
        fn profile_revealed(
            self: Pin<&mut NativeNetwork>,
            id: QString,
            success: bool,
            value: QString,
            error: QString,
        );
        #[qinvokable]
        fn value(self: &NativeNetwork, row: i32, role: QString) -> QVariant;
        #[qinvokable]
        fn enable_wifi(self: Pin<&mut NativeNetwork>, id: QString, enabled: bool) -> bool;
        #[qinvokable]
        fn scan(self: Pin<&mut NativeNetwork>, id: QString) -> bool;
        #[qinvokable]
        fn disconnect_device(self: Pin<&mut NativeNetwork>, id: QString, device: QString) -> bool;
        #[qinvokable]
        fn connect_access_point(
            self: Pin<&mut NativeNetwork>,
            id: QString,
            access_point: QString,
            password: QString,
        ) -> bool;
        #[qinvokable]
        fn connect_ssid(
            self: Pin<&mut NativeNetwork>,
            id: QString,
            ssid: QString,
            password: QString,
        ) -> bool;
        #[qinvokable]
        fn forget_profile(self: Pin<&mut NativeNetwork>, id: QString, ssid: QString) -> bool;
        #[qinvokable]
        fn set_profile_password(
            self: Pin<&mut NativeNetwork>,
            id: QString,
            ssid: QString,
            password: QString,
        ) -> bool;
        #[qinvokable]
        fn reveal_profile(self: Pin<&mut NativeNetwork>, id: QString, ssid: QString) -> bool;
    }
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[cxx_override]
        fn row_count_flat(self: &NativeNetwork) -> i32;
        #[cxx_override]
        fn row_data(self: &NativeNetwork, row: i32, role: i32) -> QVariant;
        #[inherit]
        fn set_roles(self: Pin<&mut NativeNetwork>, roles: &QStringList);
        #[inherit]
        fn reset_start(self: Pin<&mut NativeNetwork>);
        #[inherit]
        fn reset_end(self: Pin<&mut NativeNetwork>);
        #[inherit]
        fn insert_start(self: Pin<&mut NativeNetwork>, row: i32);
        #[inherit]
        fn insert_end(self: Pin<&mut NativeNetwork>);
        #[inherit]
        fn remove_start(self: Pin<&mut NativeNetwork>, row: i32);
        #[inherit]
        fn remove_end(self: Pin<&mut NativeNetwork>);
        #[inherit]
        fn move_start(self: Pin<&mut NativeNetwork>, from: i32, to: i32);
        #[inherit]
        fn move_end(self: Pin<&mut NativeNetwork>);
        #[inherit]
        fn changed(self: Pin<&mut NativeNetwork>, row: i32, mask: u64);
        #[inherit]
        fn changed_range(self: Pin<&mut NativeNetwork>, first: i32, last: i32, mask: u64);
    }
    impl cxx_qt::Threading for NativeNetwork {}
    impl cxx_qt::Initialize for NativeNetwork {}
}
use crate::model::{Model, Pending, Row};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QVariant};
use inir_core::{
    model::ModelDelta,
    runtime::{Backend, DomainConsumer, TaskGuard},
};
use inir_types::desktop::*;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};
struct Summary {
    ready: bool,
    error: String,
    wifi_enabled: Option<bool>,
    ethernet: Option<bool>,
    connectivity: Option<i32>,
    active_ssid: Option<String>,
    active_strength: Option<i32>,
    known_profiles: Option<Vec<String>>,
    active_connections: Option<Vec<ActiveConnection>>,
}
impl Summary {
    fn between(old: &NetworkState, state: &NetworkState) -> Self {
        Self {
            ready: state.status.ready,
            error: state.status.error.clone(),
            wifi_enabled: {
                let next = state.wifi_enabled;
                let prior = old.wifi_enabled;
                (next != prior).then_some(next)
            },
            ethernet: {
                let next = state.ethernet;
                let prior = old.ethernet;
                (next != prior).then_some(next)
            },
            connectivity: {
                let next = state.connectivity as i32;
                let prior = old.connectivity as i32;
                (next != prior).then_some(next)
            },
            active_ssid: {
                let next = state
                    .access_points
                    .iter()
                    .find(|ap| ap.active)
                    .map(|ap| ap.ssid.clone())
                    .unwrap_or_default();
                let prior = old
                    .access_points
                    .iter()
                    .find(|ap| ap.active)
                    .map(|ap| ap.ssid.clone())
                    .unwrap_or_default();
                (next != prior).then_some(next)
            },
            active_strength: {
                let next = state
                    .access_points
                    .iter()
                    .find(|ap| ap.active)
                    .map(|ap| ap.strength as i32)
                    .unwrap_or_default();
                let prior = old
                    .access_points
                    .iter()
                    .find(|ap| ap.active)
                    .map(|ap| ap.strength as i32)
                    .unwrap_or_default();
                (next != prior).then_some(next)
            },
            known_profiles: if state.known_profiles != old.known_profiles {
                Some(state.known_profiles.clone())
            } else {
                None
            },
            active_connections: if state.active_connections != old.active_connections {
                Some(state.active_connections.clone())
            } else {
                None
            },
        }
    }
    fn has_changes(&self) -> bool {
        self.wifi_enabled.is_some()
            || self.ethernet.is_some()
            || self.connectivity.is_some()
            || self.active_ssid.is_some()
            || self.active_strength.is_some()
            || self.known_profiles.is_some()
            || self.active_connections.is_some()
    }
}
#[derive(Default)]
pub struct NativeNetworkRust {
    active: bool,
    ready: bool,
    error: QString,
    count: i32,
    apply_calls: u64,
    apply_total_us: f64,
    apply_max_us: f64,
    known_profiles: QVariant,
    active_connections: QVariant,
    wifi_enabled: bool,
    ethernet: bool,
    connectivity: i32,
    active_ssid: QString,
    active_strength: i32,
    generation: u64,
    model: Model<AccessPoint>,
    backend: Option<Arc<Backend>>,
    consumer: Option<DomainConsumer>,
    task: Option<TaskGuard>,
    commands: Vec<TaskGuard>,
}
impl cxx_qt::Initialize for qobject::NativeNetwork {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        self.as_mut()
            .set_roles(&crate::model::roles::<AccessPoint>());
        match Backend::shared() {
            Ok(backend) => self.as_mut().rust_mut().backend = Some(backend),
            Err(error) => self.as_mut().set_error(QString::from(error.to_string())),
        }
    }
}
impl qobject::NativeNetwork {
    fn row_count_flat(&self) -> i32 {
        self.rust().model.rows.len() as i32
    }
    fn row_data(&self, row: i32, role: i32) -> QVariant {
        self.rust()
            .model
            .rows
            .get(row as usize)
            .map(|row| row.data(role))
            .unwrap_or_default()
    }
    pub fn value(&self, row: i32, role: QString) -> QVariant {
        AccessPoint::role_names()
            .iter()
            .position(|name| *name == role.to_string())
            .map(|role| self.row_data(row, role as i32 + 257))
            .unwrap_or_default()
    }
    pub fn set_enabled(mut self: Pin<&mut Self>, enabled: bool) {
        if *self.active() == enabled {
            return;
        }
        let generation = self.rust().generation.wrapping_add(1);
        self.as_mut().rust_mut().generation = generation;
        self.as_mut().set_active(enabled);
        if !enabled {
            self.as_mut().rust_mut().task = None;
            self.as_mut().rust_mut().consumer = None;
            self.as_mut().set_ready(false);
            // The next lease starts with a default comparison baseline. Clear
            // summary properties too, or an empty reconnect could expose a VPN
            // that belonged to the previous lease without producing a diff.
            self.as_mut().set_known_profiles(crate::ipc::strings(&[]));
            self.as_mut()
                .set_active_connections(crate::ipc::active_connections(&[]));
            self.as_mut().set_wifi_enabled(false);
            self.as_mut().set_ethernet(false);
            self.as_mut().set_connectivity(0);
            self.as_mut().set_active_ssid(QString::default());
            self.as_mut().set_active_strength(0);
            self.as_mut().reset_start();
            self.as_mut().rust_mut().model.rows.clear();
            self.as_mut().reset_end();
            self.as_mut().set_count(0);
            self.as_mut().structure_changed();
            return;
        }
        let Some(backend) = self.rust().backend.clone() else {
            return;
        };
        self.as_mut().rust_mut().consumer = Some(backend.acquire(0));
        let qt = self.qt_thread();
        let mut states = backend.network.clone();
        let pending = Arc::new(Mutex::new(
            Pending::<NetworkState, AccessPoint, Summary>::default(),
        ));
        let task = backend.spawn(async move {
            loop {
                let state = states.borrow_and_update().clone();
                let queue = {
                    let mut p = pending.lock().unwrap();
                    p.edits =
                        inir_core::model::diff(&p.baseline.access_points, &state.access_points);
                    let metadata = Summary::between(&p.baseline, &state);
                    let changed = !p.edits.is_empty()
                        || state.status != p.baseline.status
                        || metadata.has_changes();
                    let edits = std::mem::take(&mut p.edits);
                    p.stage(state, edits, metadata, changed)
                };
                if queue {
                    let pending = pending.clone();
                    if qt
                        .queue(move |object| {
                            if object.rust().generation != generation {
                                return;
                            }
                            let payload = pending.lock().unwrap().drain();
                            if let Some((metadata, edits)) = payload {
                                object.apply(metadata, edits);
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
                // Same presentation cadence as the QML fallback; bursts are
                // reduced off-thread before they touch any GUI bindings.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        });
        self.as_mut().rust_mut().task = Some(task);
    }
    fn apply(mut self: Pin<&mut Self>, metadata: Summary, edits: Vec<ModelDelta<AccessPoint>>) {
        if !*self.active() {
            return;
        }
        let start = std::time::Instant::now();
        let mut structure = false;
        let mut roles = 0u64;
        for edit in edits {
            match edit {
                ModelDelta::Insert { index, row } => {
                    self.as_mut().insert_start(index as i32);
                    self.as_mut().rust_mut().model.rows.insert(index, row);
                    self.as_mut().insert_end();
                    structure = true;
                }
                ModelDelta::Remove { index } => {
                    self.as_mut().remove_start(index as i32);
                    self.as_mut().rust_mut().model.rows.remove(index);
                    self.as_mut().remove_end();
                    structure = true;
                }
                ModelDelta::Move { from, to } => {
                    self.as_mut().move_start(from as i32, to as i32);
                    let row = self.as_mut().rust_mut().model.rows.remove(from);
                    self.as_mut().rust_mut().model.rows.insert(to, row);
                    self.as_mut().move_end();
                    structure = true;
                }
                ModelDelta::Change {
                    index,
                    row,
                    roles: mask,
                } => {
                    self.as_mut().rust_mut().model.rows[index] = row;
                    self.as_mut().changed(index as i32, mask);
                    roles |= mask;
                }

                ModelDelta::ChangeRange {
                    index,
                    rows,
                    roles: mask,
                } => {
                    let last = index + rows.len() - 1;
                    {
                        let mut rust = self.as_mut().rust_mut();
                        for (offset, row) in rows.into_iter().enumerate() {
                            rust.model.rows[index + offset] = row;
                        }
                    }
                    self.as_mut().changed_range(index as i32, last as i32, mask);
                    roles |= mask;
                }
            }
        }
        let count = self.rust().model.rows.len() as i32;
        self.as_mut().set_count(count);
        self.as_mut().set_error(QString::from(metadata.error));
        if let Some(value) = metadata.wifi_enabled {
            self.as_mut().set_wifi_enabled(value);
        }
        if let Some(value) = metadata.ethernet {
            self.as_mut().set_ethernet(value);
        }
        if let Some(value) = metadata.connectivity {
            self.as_mut().set_connectivity(value);
        }
        if let Some(value) = metadata.active_ssid {
            self.as_mut().set_active_ssid(QString::from(value));
        }
        if let Some(value) = metadata.active_strength {
            self.as_mut().set_active_strength(value);
        }
        if let Some(value) = metadata.known_profiles {
            self.as_mut()
                .set_known_profiles(crate::ipc::strings(&value));
        }
        if let Some(value) = metadata.active_connections {
            self.as_mut()
                .set_active_connections(crate::ipc::active_connections(&value));
        }
        self.as_mut().set_ready(metadata.ready);
        if structure {
            self.as_mut().structure_changed();
        }
        if structure || roles != 0 {
            self.as_mut()
                .rows_updated(if structure { u64::MAX } else { roles });
        }
        let elapsed = start.elapsed().as_secs_f64() * 1e6;
        let calls = self.rust().apply_calls + 1;
        let total = self.rust().apply_total_us + elapsed;
        let max = self.rust().apply_max_us.max(elapsed);
        self.as_mut().set_apply_calls(calls);
        self.as_mut().set_apply_total_us(total);
        self.as_mut().set_apply_max_us(max);
    }
    fn submit(mut self: Pin<&mut Self>, id: QString, command: Command) -> bool {
        if !*self.active() {
            return false;
        }
        let Some(backend) = self.rust().backend.clone() else {
            return false;
        };
        let qt = self.qt_thread();
        let id = id.to_string();
        let task = backend.clone().spawn(async move {
            let result = backend.execute(command).await;
            let _ = qt.queue(move |mut object| {
                object.as_mut().command_finished(
                    QString::from(id),
                    result.is_ok(),
                    QString::from(result.err().unwrap_or_default()),
                );
            });
        });
        let mut rust = self.as_mut().rust_mut();
        rust.commands.retain(|task| !task.is_finished());
        rust.commands.push(task);
        true
    }
    pub fn enable_wifi(self: Pin<&mut Self>, id: QString, enabled: bool) -> bool {
        self.submit(id, Command::WifiEnabled { enabled })
    }
    pub fn scan(self: Pin<&mut Self>, id: QString) -> bool {
        self.submit(id, Command::WifiScan)
    }
    pub fn disconnect_device(self: Pin<&mut Self>, id: QString, device: QString) -> bool {
        self.submit(
            id,
            Command::WifiDisconnect {
                device: device.to_string(),
            },
        )
    }
    pub fn connect_access_point(
        self: Pin<&mut Self>,
        id: QString,
        access_point: QString,
        password: QString,
    ) -> bool {
        self.submit(
            id,
            Command::WifiConnect {
                access_point: access_point.to_string(),
                password: (!password.is_empty()).then(|| password.to_string()),
            },
        )
    }
    pub fn connect_ssid(
        self: Pin<&mut Self>,
        id: QString,
        ssid: QString,
        password: QString,
    ) -> bool {
        self.submit(
            id,
            Command::WifiConnectSsid {
                ssid: ssid.to_string(),
                password: (!password.is_empty()).then(|| password.to_string()),
            },
        )
    }
    pub fn forget_profile(self: Pin<&mut Self>, id: QString, ssid: QString) -> bool {
        self.submit(
            id,
            Command::ProfileForget {
                ssid: ssid.to_string(),
            },
        )
    }
    pub fn set_profile_password(
        self: Pin<&mut Self>,
        id: QString,
        ssid: QString,
        password: QString,
    ) -> bool {
        self.submit(
            id,
            Command::ProfileSetPassword {
                ssid: ssid.to_string(),
                password: password.to_string(),
            },
        )
    }
    /// The saved secret is returned through `profile_revealed`, never through a
    /// command_finished error string, so it is only ever delivered to the requester.
    pub fn reveal_profile(mut self: Pin<&mut Self>, id: QString, ssid: QString) -> bool {
        if !*self.active() {
            return false;
        }
        let Some(backend) = self.rust().backend.clone() else {
            return false;
        };
        let qt = self.qt_thread();
        let id = id.to_string();
        let command = Command::ProfileSecret {
            ssid: ssid.to_string(),
        };
        let task = backend.clone().spawn(async move {
            let (success, value, error) = match backend.query(command).await {
                Ok(value) => (true, value, String::new()),
                Err(error) => (false, String::new(), error),
            };
            let _ = qt.queue(move |mut object| {
                object.as_mut().profile_revealed(
                    QString::from(id),
                    success,
                    QString::from(value),
                    QString::from(error),
                );
            });
        });
        let mut rust = self.as_mut().rust_mut();
        rust.commands.retain(|task| !task.is_finished());
        rust.commands.push(task);
        true
    }
}
