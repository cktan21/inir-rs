#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("inir-qt/list_model.h");
        #[cxx_name = "nullVariant"]
        fn null_variant() -> QVariant;
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
        type NiriListModelBase;
    }
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = NiriListModelBase]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, active)]
        #[qproperty(bool, ready)]
        #[qproperty(QString, error)]
        #[qproperty(i32, count)]
        #[qproperty(u64, apply_calls)]
        #[qproperty(f64, apply_total_us)]
        #[qproperty(f64, apply_max_us)]
        #[qproperty(bool, overview_open)]
        #[qproperty(f64, focused_window_id)]
        #[qproperty(f64, focused_workspace_id)]
        #[qproperty(i32, focused_workspace_index)]
        #[qproperty(QString, current_output)]
        #[qproperty(QVariant, keyboard_layouts)]
        #[qproperty(i32, keyboard_layout_index)]
        #[qproperty(QVariant, outputs)]
        #[qproperty(QVariant, mru_window_ids)]
        #[qproperty(u64, config_generation)]
        #[qproperty(bool, config_failed)]
        type NativeNiri = super::NativeNiriRust;
        #[qinvokable]
        fn fixture(
            self: Pin<&mut NativeNiri>,
            count: i32,
            changed_row: i32,
            title: QString,
        ) -> bool;
        #[qinvokable]
        fn set_enabled(self: Pin<&mut NativeNiri>, enabled: bool);
        #[qinvokable]
        fn set_update_interval(self: Pin<&mut NativeNiri>, milliseconds: i32);
        #[qsignal]
        fn rows_updated(self: Pin<&mut NativeNiri>, roles: u64);
        #[qsignal]
        fn structure_changed(self: Pin<&mut NativeNiri>);
        #[qsignal]
        fn command_finished(self: Pin<&mut NativeNiri>, id: QString, success: bool, error: QString);
        #[qinvokable]
        fn value(self: &NativeNiri, row: i32, role: QString) -> QVariant;
        #[qinvokable]
        fn action(
            self: Pin<&mut NativeNiri>,
            id: QString,
            name: QString,
            parameters: QVariant,
        ) -> bool;
        #[qinvokable]
        fn focus_window(self: Pin<&mut NativeNiri>, id: QString, window: f64) -> bool;
        #[qinvokable]
        fn close_window(self: Pin<&mut NativeNiri>, id: QString, window: f64) -> bool;
        #[qinvokable]
        fn focus_workspace(
            self: Pin<&mut NativeNiri>,
            id: QString,
            workspace: f64,
            by_index: bool,
        ) -> bool;
        #[qinvokable]
        fn move_window(
            self: Pin<&mut NativeNiri>,
            id: QString,
            window: f64,
            workspace: f64,
            by_index: bool,
            focus: bool,
        ) -> bool;
        #[qinvokable]
        fn refresh_outputs(self: Pin<&mut NativeNiri>, id: QString) -> bool;
    }
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[cxx_override]
        fn row_count_flat(self: &NativeNiri) -> i32;
        #[cxx_override]
        fn row_data(self: &NativeNiri, row: i32, role: i32) -> QVariant;
        #[inherit]
        fn set_roles(self: Pin<&mut NativeNiri>, roles: &QStringList);
        #[inherit]
        fn reset_start(self: Pin<&mut NativeNiri>);
        #[inherit]
        fn reset_end(self: Pin<&mut NativeNiri>);
        #[inherit]
        fn insert_start(self: Pin<&mut NativeNiri>, row: i32);
        #[inherit]
        fn insert_end(self: Pin<&mut NativeNiri>);
        #[inherit]
        fn remove_start(self: Pin<&mut NativeNiri>, row: i32);
        #[inherit]
        fn remove_end(self: Pin<&mut NativeNiri>);
        #[inherit]
        fn move_start(self: Pin<&mut NativeNiri>, from: i32, to: i32);
        #[inherit]
        fn move_end(self: Pin<&mut NativeNiri>);
        #[inherit]
        fn changed(self: Pin<&mut NativeNiri>, row: i32, mask: u64);
        #[inherit]
        fn changed_range(self: Pin<&mut NativeNiri>, first: i32, last: i32, mask: u64);
    }
    impl cxx_qt::Threading for NativeNiri {}
    impl cxx_qt::Initialize for NativeNiri {}
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
    overview_open: Option<bool>,
    focused_window_id: Option<f64>,
    focused_workspace_id: Option<f64>,
    focused_workspace_index: Option<i32>,
    current_output: Option<String>,
    keyboard_layouts: Option<Arc<Vec<String>>>,
    keyboard_layout_index: Option<i32>,
    outputs: Option<Arc<std::collections::BTreeMap<String, IpcValue>>>,
    mru_window_ids: Option<Arc<Vec<u64>>>,
    config_generation: Option<u64>,
    config_failed: Option<bool>,
}
impl Summary {
    fn between(old: &NiriState, state: &NiriState) -> Self {
        Self {
            ready: state.status.ready,
            error: state.status.error.clone(),
            overview_open: {
                let next = state.overview_open;
                let prior = old.overview_open;
                (next != prior).then_some(next)
            },
            focused_window_id: {
                let next = state
                    .windows
                    .iter()
                    .find(|w| w.focused)
                    .and_then(|w| w.id.parse::<f64>().ok())
                    .unwrap_or(-1.0);
                let prior = old
                    .windows
                    .iter()
                    .find(|w| w.focused)
                    .and_then(|w| w.id.parse::<f64>().ok())
                    .unwrap_or(-1.0);
                (next != prior).then_some(next)
            },
            focused_workspace_id: {
                let next = state
                    .workspaces
                    .iter()
                    .find(|w| w.focused)
                    .and_then(|w| w.id.parse::<f64>().ok())
                    .unwrap_or(-1.0);
                let prior = old
                    .workspaces
                    .iter()
                    .find(|w| w.focused)
                    .and_then(|w| w.id.parse::<f64>().ok())
                    .unwrap_or(-1.0);
                (next != prior).then_some(next)
            },
            focused_workspace_index: {
                let next = state
                    .workspaces
                    .iter()
                    .position(|w| w.focused)
                    .unwrap_or_default() as i32;
                let prior = old
                    .workspaces
                    .iter()
                    .position(|w| w.focused)
                    .unwrap_or_default() as i32;
                (next != prior).then_some(next)
            },
            current_output: {
                let next = state
                    .workspaces
                    .iter()
                    .find(|w| w.focused)
                    .map(|w| w.output.clone())
                    .unwrap_or_default();
                let prior = old
                    .workspaces
                    .iter()
                    .find(|w| w.focused)
                    .map(|w| w.output.clone())
                    .unwrap_or_default();
                (next != prior).then_some(next)
            },
            keyboard_layouts: if state.keyboard_layouts != old.keyboard_layouts {
                Some(state.keyboard_layouts.clone())
            } else {
                None
            },
            keyboard_layout_index: {
                let next = state.keyboard_layout_index as i32;
                let prior = old.keyboard_layout_index as i32;
                (next != prior).then_some(next)
            },
            outputs: if state.outputs != old.outputs {
                Some(state.outputs.clone())
            } else {
                None
            },
            mru_window_ids: if state.mru_window_ids != old.mru_window_ids {
                Some(state.mru_window_ids.clone())
            } else {
                None
            },
            config_generation: {
                let next = state.config_generation;
                let prior = old.config_generation;
                (next != prior).then_some(next)
            },
            config_failed: {
                let next = state.config_failed;
                let prior = old.config_failed;
                (next != prior).then_some(next)
            },
        }
    }
    /// True when any summary field actually moved (every field is `Some` only on
    /// change). Lets the worker skip a GUI apply that would carry no new value.
    fn has_changes(&self) -> bool {
        self.overview_open.is_some()
            || self.focused_window_id.is_some()
            || self.focused_workspace_id.is_some()
            || self.focused_workspace_index.is_some()
            || self.current_output.is_some()
            || self.keyboard_layouts.is_some()
            || self.keyboard_layout_index.is_some()
            || self.outputs.is_some()
            || self.mru_window_ids.is_some()
            || self.config_generation.is_some()
            || self.config_failed.is_some()
    }
}
#[derive(Default)]
pub struct NativeNiriRust {
    active: bool,
    ready: bool,
    error: QString,
    count: i32,
    apply_calls: u64,
    apply_total_us: f64,
    apply_max_us: f64,
    overview_open: bool,
    focused_window_id: f64,
    focused_workspace_id: f64,
    focused_workspace_index: i32,
    current_output: QString,
    keyboard_layouts: QVariant,
    keyboard_layout_index: i32,
    outputs: QVariant,
    mru_window_ids: QVariant,
    config_generation: u64,
    config_failed: bool,
    generation: u64,
    fixture_state: Arc<NiriState>,
    model: Model<Arc<NiriWindow>>,
    backend: Option<Arc<Backend>>,
    consumer: Option<DomainConsumer>,
    task: Option<TaskGuard>,
    commands: Vec<TaskGuard>,
    update_interval: Arc<std::sync::atomic::AtomicU64>,
}
impl cxx_qt::Initialize for qobject::NativeNiri {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        self.as_mut()
            .set_roles(&crate::model::roles::<NiriWindow>());
        self.as_mut().set_focused_window_id(-1.0);
        self.as_mut().set_focused_workspace_id(-1.0);
        // Role views may load before the first compositor frame. Their public
        // metadata must already have array/map shapes, never null QVariant.
        self.as_mut().set_keyboard_layouts(crate::ipc::strings(&[]));
        self.as_mut().set_mru_window_ids(crate::ipc::numbers(&[]));
        self.as_mut()
            .set_outputs(crate::ipc::outputs(&Default::default()));
        match Backend::shared() {
            Ok(backend) => self.as_mut().rust_mut().backend = Some(backend),
            Err(error) => self.as_mut().set_error(QString::from(error.to_string())),
        }
    }
}
impl qobject::NativeNiri {
    pub fn set_update_interval(self: Pin<&mut Self>, milliseconds: i32) {
        self.rust().update_interval.store(
            milliseconds.clamp(1, 1000) as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
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
        NiriWindow::role_names()
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
        let update_interval = self.rust().update_interval.clone();
        let pending = Arc::new(Mutex::new(
            Pending::<NiriState, Arc<NiriWindow>, Summary>::default(),
        ));
        let task = backend.spawn(async move {
            loop {
                let state = states.borrow_and_update().clone();
                let queue = {
                    let mut p = pending.lock().unwrap();
                    let edits = if Arc::ptr_eq(&p.baseline.windows, &state.windows) {
                        Vec::new()
                    } else {
                        inir_core::model::diff(&p.baseline.windows, &state.windows)
                    };
                    let metadata = Summary::between(&p.baseline, &state);
                    // Skip the GUI apply when nothing a consumer sees changed: no row
                    // edits, no status change, and no summary field moved. Otherwise a
                    // bare wake (e.g. an identical republish) would churn counters.
                    let changed = !edits.is_empty()
                        || state.status != p.baseline.status
                        || metadata.has_changes();
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
                let milliseconds = update_interval.load(std::sync::atomic::Ordering::Relaxed);
                tokio::time::sleep(std::time::Duration::from_millis(if milliseconds == 0 {
                    50
                } else {
                    milliseconds
                }))
                .await;
            }
        });
        self.as_mut().rust_mut().task = Some(task);
    }
    fn apply(mut self: Pin<&mut Self>, metadata: Summary, edits: Vec<ModelDelta<Arc<NiriWindow>>>) {
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
        if let Some(value) = metadata.overview_open {
            self.as_mut().set_overview_open(value);
        }
        if let Some(value) = metadata.focused_window_id {
            self.as_mut().set_focused_window_id(value);
        }
        if let Some(value) = metadata.focused_workspace_id {
            self.as_mut().set_focused_workspace_id(value);
        }
        if let Some(value) = metadata.focused_workspace_index {
            self.as_mut().set_focused_workspace_index(value);
        }
        if let Some(value) = metadata.current_output {
            self.as_mut().set_current_output(QString::from(value));
        }
        if let Some(value) = metadata.keyboard_layouts {
            self.as_mut()
                .set_keyboard_layouts(crate::ipc::strings(&value));
        }
        if let Some(value) = metadata.keyboard_layout_index {
            self.as_mut().set_keyboard_layout_index(value);
        }
        if let Some(value) = metadata.outputs {
            self.as_mut().set_outputs(crate::ipc::outputs(&value));
        }
        if let Some(value) = metadata.mru_window_ids {
            self.as_mut()
                .set_mru_window_ids(crate::ipc::numbers(&value));
        }
        if let Some(value) = metadata.config_failed {
            self.as_mut().set_config_failed(value);
        }
        if let Some(value) = metadata.config_generation {
            self.as_mut().set_config_generation(value);
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
    /// Synthetic input for bridge correctness/benchmark tests only. No worker
    /// lease or compositor command is acquired, and production rejects it.
    pub fn fixture(mut self: Pin<&mut Self>, count: i32, changed_row: i32, title: QString) -> bool {
        if std::env::var("INIR_NATIVE_TEST").as_deref() != Ok("1")
            || self.rust().consumer.is_some()
            || self.rust().task.is_some()
            || !(0..=10000).contains(&count)
        {
            return false;
        }
        let Some(backend) = self.rust().backend.clone() else {
            return false;
        };
        self.as_mut().set_active(true);
        let previous = self.rust().fixture_state.clone();
        let events = backend.events.clone();
        let qt = self.qt_thread();
        let title = title.to_string();
        let task = backend.spawn(async move {
            let mut state = (*previous).clone();
            state.status.ready = true;
            state.windows = Arc::new(
                (0..count)
                    .map(|id| {
                        previous
                            .windows
                            .iter()
                            .find(|row| row.id == id.to_string())
                            .cloned()
                            .unwrap_or_else(|| {
                                Arc::new(NiriWindow {
                                    id: id.to_string(),
                                    title_present: true,
                                    app_id_present: true,
                                    title: "seed".into(),
                                    ..Default::default()
                                })
                            })
                    })
                    .collect(),
            );
            if changed_row == -2 {
                Arc::make_mut(&mut state.windows).reverse();
            } else {
                for (index, row) in Arc::make_mut(&mut state.windows).iter_mut().enumerate() {
                    if changed_row == -1 || changed_row == index as i32 {
                        Arc::make_mut(row).title = title.clone();
                    }
                }
            }
            let edits = inir_core::model::diff(&previous.windows, &state.windows);
            let metadata = Summary::between(&previous, &state);
            let state = Arc::new(state);
            // Exercise the same watch publication as a compositor event so the
            // sibling workspace adapter's no-op behavior is covered too.
            let _ = events.send(inir_core::events::Event::NiriFrame(state.clone())).await;
            let _ = qt.queue(move |mut object| {
                object.as_mut().rust_mut().fixture_state = state;
                object.apply(metadata, edits);
            });
        });
        let mut rust = self.as_mut().rust_mut();
        rust.commands.retain(|task| !task.is_finished());
        rust.commands.push(task);
        true
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
    pub fn action(self: Pin<&mut Self>, id: QString, name: QString, parameters: QVariant) -> bool {
        self.submit(
            id,
            Command::NiriAction {
                name: name.to_string(),
                parameters: crate::ipc::from_variant(&parameters),
            },
        )
    }
    pub fn focus_window(self: Pin<&mut Self>, id: QString, window: f64) -> bool {
        self.submit(
            id,
            Command::NiriAction {
                name: "FocusWindow".into(),
                parameters: crate::ipc::map([("id", IpcValue::Number(window))]),
            },
        )
    }
    pub fn close_window(self: Pin<&mut Self>, id: QString, window: f64) -> bool {
        self.submit(
            id,
            Command::NiriAction {
                name: "CloseWindow".into(),
                parameters: crate::ipc::map([("id", IpcValue::Number(window))]),
            },
        )
    }
    pub fn focus_workspace(
        self: Pin<&mut Self>,
        id: QString,
        workspace: f64,
        by_index: bool,
    ) -> bool {
        self.submit(
            id,
            Command::NiriAction {
                name: "FocusWorkspace".into(),
                parameters: crate::ipc::map([(
                    "reference",
                    crate::ipc::map([(
                        if by_index { "Index" } else { "Id" },
                        IpcValue::Number(workspace),
                    )]),
                )]),
            },
        )
    }
    pub fn move_window(
        self: Pin<&mut Self>,
        id: QString,
        window: f64,
        workspace: f64,
        by_index: bool,
        focus: bool,
    ) -> bool {
        self.submit(
            id,
            Command::NiriAction {
                name: "MoveWindowToWorkspace".into(),
                parameters: crate::ipc::map([
                    ("window_id", IpcValue::Number(window)),
                    (
                        "reference",
                        crate::ipc::map([(
                            if by_index { "Index" } else { "Id" },
                            IpcValue::Number(workspace),
                        )]),
                    ),
                    ("focus", IpcValue::Bool(focus)),
                ]),
            },
        )
    }
    pub fn refresh_outputs(self: Pin<&mut Self>, id: QString) -> bool {
        self.submit(id, Command::NiriRefreshOutputs)
    }
}
