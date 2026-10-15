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
        type BrightnessListModelBase;
    }
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = BrightnessListModelBase]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, active)]
        #[qproperty(bool, ready)]
        #[qproperty(QString, error)]
        #[qproperty(i32, count)]
        #[qproperty(u64, apply_calls)]
        #[qproperty(f64, apply_total_us)]
        #[qproperty(f64, apply_max_us)]
        type NativeBrightness = super::NativeBrightnessRust;
        #[qinvokable]
        fn set_enabled(self: Pin<&mut NativeBrightness>, enabled: bool);
        #[qsignal]
        fn rows_updated(self: Pin<&mut NativeBrightness>, roles: u64);
        #[qsignal]
        fn structure_changed(self: Pin<&mut NativeBrightness>);
        #[qsignal]
        fn command_finished(
            self: Pin<&mut NativeBrightness>,
            id: QString,
            success: bool,
            error: QString,
        );
        #[qinvokable]
        fn value(self: &NativeBrightness, row: i32, role: QString) -> QVariant;
        #[qinvokable]
        fn set_brightness(
            self: Pin<&mut NativeBrightness>,
            id: QString,
            device: QString,
            value: f64,
        ) -> bool;
    }
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[cxx_override]
        fn row_count_flat(self: &NativeBrightness) -> i32;
        #[cxx_override]
        fn row_data(self: &NativeBrightness, row: i32, role: i32) -> QVariant;
        #[inherit]
        fn set_roles(self: Pin<&mut NativeBrightness>, roles: &QStringList);
        #[inherit]
        fn reset_start(self: Pin<&mut NativeBrightness>);
        #[inherit]
        fn reset_end(self: Pin<&mut NativeBrightness>);
        #[inherit]
        fn insert_start(self: Pin<&mut NativeBrightness>, row: i32);
        #[inherit]
        fn insert_end(self: Pin<&mut NativeBrightness>);
        #[inherit]
        fn remove_start(self: Pin<&mut NativeBrightness>, row: i32);
        #[inherit]
        fn remove_end(self: Pin<&mut NativeBrightness>);
        #[inherit]
        fn move_start(self: Pin<&mut NativeBrightness>, from: i32, to: i32);
        #[inherit]
        fn move_end(self: Pin<&mut NativeBrightness>);
        #[inherit]
        fn changed(self: Pin<&mut NativeBrightness>, row: i32, mask: u64);
        #[inherit]
        fn changed_range(self: Pin<&mut NativeBrightness>, first: i32, last: i32, mask: u64);
    }
    impl cxx_qt::Threading for NativeBrightness {}
    impl cxx_qt::Initialize for NativeBrightness {}
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
    fn between(_old: &BrightnessState, state: &BrightnessState) -> Self {
        Self {
            ready: state.status.ready,
            error: state.status.error.clone(),
        }
    }
}
#[derive(Default)]
pub struct NativeBrightnessRust {
    active: bool,
    ready: bool,
    error: QString,
    count: i32,
    apply_calls: u64,
    apply_total_us: f64,
    apply_max_us: f64,

    generation: u64,
    model: Model<Backlight>,
    backend: Option<Arc<Backend>>,
    consumer: Option<DomainConsumer>,
    task: Option<TaskGuard>,
    commands: Vec<TaskGuard>,
}
impl cxx_qt::Initialize for qobject::NativeBrightness {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        self.as_mut().set_roles(&crate::model::roles::<Backlight>());
        match Backend::shared() {
            Ok(backend) => self.as_mut().rust_mut().backend = Some(backend),
            Err(error) => self.as_mut().set_error(QString::from(error.to_string())),
        }
    }
}
impl qobject::NativeBrightness {
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
        Backlight::role_names()
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
        self.as_mut().rust_mut().consumer = Some(backend.acquire(2));
        let qt = self.qt_thread();
        let mut states = backend.brightness.clone();
        let pending = Arc::new(Mutex::new(
            Pending::<BrightnessState, Backlight, Summary>::default(),
        ));
        let task = backend.spawn(async move {
            loop {
                let state = states.borrow_and_update().clone();
                let queue = {
                    let mut p = pending.lock().unwrap();
                    p.edits = inir_core::model::diff(&p.baseline.devices, &state.devices);
                    p.metadata = Some(Summary::between(&p.baseline, &state));
                    p.target = Some(state);
                    let queue = !p.scheduled;
                    p.scheduled = true;
                    queue
                };
                if queue {
                    let pending = pending.clone();
                    if qt
                        .queue(move |object| {
                            if object.rust().generation != generation {
                                return;
                            }
                            let (metadata, edits) = {
                                let mut p = pending.lock().unwrap();
                                let state = p.target.take().unwrap();
                                p.baseline = state.clone();
                                p.scheduled = false;
                                (p.metadata.take().unwrap(), std::mem::take(&mut p.edits))
                            };
                            object.apply(metadata, edits);
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
    fn apply(mut self: Pin<&mut Self>, metadata: Summary, edits: Vec<ModelDelta<Backlight>>) {
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
    pub fn set_brightness(self: Pin<&mut Self>, id: QString, device: QString, value: f64) -> bool {
        self.submit(
            id,
            Command::Brightness {
                device: device.to_string(),
                value,
            },
        )
    }
}
