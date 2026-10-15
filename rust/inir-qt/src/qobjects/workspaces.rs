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
        type WorkspaceListModelBase;
    }
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = WorkspaceListModelBase]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, active)]
        #[qproperty(bool, ready)]
        #[qproperty(QString, error)]
        #[qproperty(i32, count)]
        #[qproperty(u64, apply_calls)]
        #[qproperty(f64, apply_total_us)]
        #[qproperty(f64, apply_max_us)]
        type NativeWorkspaces = super::NativeWorkspacesRust;
        #[qinvokable]
        fn set_enabled(self: Pin<&mut NativeWorkspaces>, enabled: bool);
        #[qsignal]
        fn rows_updated(self: Pin<&mut NativeWorkspaces>, roles: u64);
        #[qsignal]
        fn structure_changed(self: Pin<&mut NativeWorkspaces>);
        #[qsignal]
        fn command_finished(
            self: Pin<&mut NativeWorkspaces>,
            id: QString,
            success: bool,
            error: QString,
        );
        #[qinvokable]
        fn value(self: &NativeWorkspaces, row: i32, role: QString) -> QVariant;

    }
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[cxx_override]
        fn row_count_flat(self: &NativeWorkspaces) -> i32;
        #[cxx_override]
        fn row_data(self: &NativeWorkspaces, row: i32, role: i32) -> QVariant;
        #[inherit]
        fn set_roles(self: Pin<&mut NativeWorkspaces>, roles: &QStringList);
        #[inherit]
        fn reset_start(self: Pin<&mut NativeWorkspaces>);
        #[inherit]
        fn reset_end(self: Pin<&mut NativeWorkspaces>);
        #[inherit]
        fn insert_start(self: Pin<&mut NativeWorkspaces>, row: i32);
        #[inherit]
        fn insert_end(self: Pin<&mut NativeWorkspaces>);
        #[inherit]
        fn remove_start(self: Pin<&mut NativeWorkspaces>, row: i32);
        #[inherit]
        fn remove_end(self: Pin<&mut NativeWorkspaces>);
        #[inherit]
        fn move_start(self: Pin<&mut NativeWorkspaces>, from: i32, to: i32);
        #[inherit]
        fn move_end(self: Pin<&mut NativeWorkspaces>);
        #[inherit]
        fn changed(self: Pin<&mut NativeWorkspaces>, row: i32, mask: u64);
        #[inherit]
        fn changed_range(self: Pin<&mut NativeWorkspaces>, first: i32, last: i32, mask: u64);
    }
    impl cxx_qt::Threading for NativeWorkspaces {}
    impl cxx_qt::Initialize for NativeWorkspaces {}
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
}
impl Summary {
    fn between(state: &NiriState) -> Self {
        Self {
            ready: state.status.ready,
            error: state.status.error.clone(),
        }
    }
}
#[derive(Default)]
pub struct NativeWorkspacesRust {
    active: bool,
    ready: bool,
    error: QString,
    count: i32,
    apply_calls: u64,
    apply_total_us: f64,
    apply_max_us: f64,

    generation: u64,
    model: Model<Arc<NiriWorkspace>>,
    backend: Option<Arc<Backend>>,
    consumer: Option<DomainConsumer>,
    task: Option<TaskGuard>,
    commands: Vec<TaskGuard>,
}
impl cxx_qt::Initialize for qobject::NativeWorkspaces {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        self.as_mut()
            .set_roles(&crate::model::roles::<NiriWorkspace>());
        match Backend::shared() {
            Ok(backend) => self.as_mut().rust_mut().backend = Some(backend),
            Err(error) => self.as_mut().set_error(QString::from(error.to_string())),
        }
    }
}
impl qobject::NativeWorkspaces {
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
        NiriWorkspace::role_names()
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
        self.as_mut().rust_mut().consumer = Some(backend.acquire(3));
        let qt = self.qt_thread();
        let mut states = backend.niri.clone();
        let pending = Arc::new(Mutex::new(
            Pending::<NiriState, Arc<NiriWorkspace>, Summary>::default(),
        ));
        let task = backend.spawn(async move {
            loop {
                let state = states.borrow_and_update().clone();
                let queue = {
                    let mut p = pending.lock().unwrap();
                    let edits = if Arc::ptr_eq(&p.baseline.workspaces, &state.workspaces) {
                        Vec::new()
                    } else {
                        inir_core::model::diff(&p.baseline.workspaces, &state.workspaces)
                    };
                    // A window-only Niri event (e.g. a title change) leaves the
                    // workspaces and status untouched. Skip the GUI apply entirely
                    // rather than queue an empty one that only churns counters.
                    let changed = !edits.is_empty() || state.status != p.baseline.status;
                    let metadata = Summary::between(&state);
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
    fn apply(
        mut self: Pin<&mut Self>,
        metadata: Summary,
        edits: Vec<ModelDelta<Arc<NiriWorkspace>>>,
    ) {
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
}
