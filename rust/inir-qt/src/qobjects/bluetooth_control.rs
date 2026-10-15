#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("inir-qt/list_model.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        #[cxx_name = "commandVariant"]
        fn command_variant(value: &QVariant) -> QVariant;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }
    unsafe extern "C++Qt" {
        include!("inir-qt/list_model.h");
        #[qobject]
        type BluetoothControlModelBase;
    }
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = BluetoothControlModelBase]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, active)]
        #[qproperty(bool, ready)]
        #[qproperty(QVariant, state)]
        #[qproperty(i32, count)]
        type NativeBluetoothController = super::ControllerRust;
        #[qinvokable]
        fn set_enabled(
            self: Pin<&mut NativeBluetoothController>,
            enabled: bool,
            integrated: bool,
            receive_directory: QString,
            locked: bool,
        );
        #[qinvokable]
        fn set_locked(self: Pin<&mut NativeBluetoothController>, locked: bool);
        #[qinvokable]
        fn scan(
            self: Pin<&mut NativeBluetoothController>,
            owner: QString,
            adapter: QString,
            remaining_ms: i32,
        ) -> bool;
        #[qinvokable]
        fn release_scan(self: Pin<&mut NativeBluetoothController>, owner: QString);
        #[qinvokable]
        fn release_all(self: Pin<&mut NativeBluetoothController>);
        #[qinvokable]
        fn execute(
            self: Pin<&mut NativeBluetoothController>,
            id: QString,
            operation: QString,
            parameters: QVariant,
        ) -> bool;
        #[qinvokable]
        fn respond(
            self: Pin<&mut NativeBluetoothController>,
            id: QString,
            accepted: bool,
            value: QString,
        ) -> bool;
        #[qsignal]
        fn command_finished(
            self: Pin<&mut NativeBluetoothController>,
            id: QString,
            success: bool,
            error: QString,
        );
        #[qsignal]
        fn state_updated(self: Pin<&mut NativeBluetoothController>);
    }
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[cxx_override]
        fn row_count_flat(self: &NativeBluetoothController) -> i32;
        #[cxx_override]
        fn row_data(self: &NativeBluetoothController, row: i32, role: i32) -> QVariant;
        #[inherit]
        fn set_roles(self: Pin<&mut NativeBluetoothController>, roles: &QStringList);
        #[inherit]
        fn insert_start(self: Pin<&mut NativeBluetoothController>, row: i32);
        #[inherit]
        fn insert_end(self: Pin<&mut NativeBluetoothController>);
        #[inherit]
        fn remove_start(self: Pin<&mut NativeBluetoothController>, row: i32);
        #[inherit]
        fn remove_end(self: Pin<&mut NativeBluetoothController>);
        #[inherit]
        fn move_start(self: Pin<&mut NativeBluetoothController>, from: i32, to: i32);
        #[inherit]
        fn move_end(self: Pin<&mut NativeBluetoothController>);
        #[inherit]
        fn changed(self: Pin<&mut NativeBluetoothController>, row: i32, mask: u64);
        #[inherit]
        fn changed_range(
            self: Pin<&mut NativeBluetoothController>,
            first: i32,
            last: i32,
            mask: u64,
        );
    }
    impl cxx_qt::Threading for NativeBluetoothController {}
    impl cxx_qt::Initialize for NativeBluetoothController {}
}
use crate::model::{Model, Row};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QVariant};
use inir_core::{
    model::ModelDelta,
    runtime::{Backend, TaskGuard},
    services::bluetooth_control::{
        self,
        types::{State, Transfer},
        Control, Request,
    },
};
use inir_types::desktop::IpcValue;
use std::{
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
};

impl Row for Transfer {
    fn role_names() -> &'static [&'static str] {
        &[
            "transferId",
            "name",
            "direction",
            "status",
            "size",
            "transferred",
            "filename",
            "error",
        ]
    }
    fn data(&self, role: i32) -> QVariant {
        match role - 257 {
            0 => crate::model::text(&self.id),
            1 => crate::model::text(&self.name),
            2 => crate::model::text(&self.direction),
            3 => crate::model::text(&self.status),
            4 => QVariant::from(&(self.size as f64)),
            5 => QVariant::from(&(self.transferred as f64)),
            6 => crate::model::text(&self.filename),
            7 => crate::model::text(&self.error),
            _ => QVariant::default(),
        }
    }
}
#[derive(Default)]
pub struct ControllerRust {
    active: bool,
    ready: bool,
    state: QVariant,
    count: i32,
    integrated: bool,
    directory: String,
    generation: u64,
    model: Model<Transfer>,
    backend: Option<Arc<Backend>>,
    control: Option<Control>,
    tasks: Vec<TaskGuard>,
    handover: Option<tokio::sync::oneshot::Receiver<()>>,
}
impl cxx_qt::Initialize for qobject::NativeBluetoothController {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        self.as_mut().set_roles(&crate::model::roles::<Transfer>());
        if let Ok(backend) = Backend::shared() {
            self.as_mut().rust_mut().backend = Some(backend);
        }
    }
}
impl qobject::NativeBluetoothController {
    fn row_count_flat(&self) -> i32 {
        self.rust().model.rows.len() as i32
    }
    fn row_data(&self, row: i32, role: i32) -> QVariant {
        self.rust()
            .model
            .rows
            .get(row as usize)
            .map(|r| r.data(role))
            .unwrap_or_default()
    }
    pub fn set_enabled(
        mut self: Pin<&mut Self>,
        enabled: bool,
        integrated: bool,
        receive_directory: QString,
        locked: bool,
    ) {
        let directory = receive_directory.to_string();
        if enabled == *self.active()
            && integrated == self.rust().integrated
            && directory == self.rust().directory
            && self
                .rust()
                .control
                .as_ref()
                .is_some_and(|c| !c.requests.is_closed())
        {
            return;
        }
        let generation = self.rust().generation.wrapping_add(1);
        {
            let mut rust = self.as_mut().rust_mut();
            rust.generation = generation;
            rust.control = None; // cancellation; worker performs asynchronous cleanup
            rust.integrated = integrated;
            rust.directory = directory.clone();
            rust.tasks.retain(|t| !t.is_finished());
        }
        self.as_mut().set_active(enabled);
        self.as_mut().set_ready(false);
        if !enabled {
            self.apply(State::default());
            return;
        }
        let Some(backend) = self.rust().backend.clone() else {
            return;
        };
        let (control, worker, mut output) = bluetooth_control::channel();
        let mut states = control.state.clone();
        let prior = self.as_mut().rust_mut().handover.take();
        let (done, finished) = tokio::sync::oneshot::channel();
        let task = backend.spawn(async move {
            if let Some(prior) = prior {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(10), prior).await;
            }
            worker
                .run(integrated, PathBuf::from(directory), locked)
                .await;
            let _ = done.send(());
        });
        self.as_mut().rust_mut().handover = Some(finished);
        let qt = self.qt_thread();
        let pending = Arc::new(Mutex::new((false, None::<State>)));
        let state_task = backend.spawn(async move {
            loop {
                let state = states.borrow_and_update().clone();
                let queue = {
                    let mut p = pending.lock().unwrap();
                    p.1 = Some(state);
                    let queue = !p.0;
                    p.0 = true;
                    queue
                };
                if queue {
                    let pending = pending.clone();
                    if qt
                        .queue(move |mut object| {
                            if object.rust().generation != generation {
                                return;
                            }
                            let state = {
                                let mut p = pending.lock().unwrap();
                                p.0 = false;
                                p.1.take().unwrap()
                            };
                            object.as_mut().apply(state);
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
        let qt = self.qt_thread();
        let output_task = backend.spawn(async move {
            while let Some(completion) = output.recv().await {
                let _ = qt.queue(move |mut object| {
                    if object.rust().generation == generation {
                        object.as_mut().command_finished(
                            QString::from(completion.id),
                            completion.result.is_ok(),
                            QString::from(completion.result.err().unwrap_or_default()),
                        );
                    }
                });
            }
        });
        let mut rust = self.as_mut().rust_mut();
        rust.control = Some(control);
        rust.tasks.extend([task, state_task, output_task]);
    }
    fn submit(&self, request: Request) -> bool {
        self.rust()
            .control
            .as_ref()
            .is_some_and(|c| c.requests.try_send(request).is_ok())
    }
    pub fn scan(self: Pin<&mut Self>, owner: QString, adapter: QString, remaining_ms: i32) -> bool {
        if !(1..=25000).contains(&remaining_ms) {
            return false;
        }
        self.submit(Request::Scan {
            owner: owner.to_string(),
            adapter: adapter.to_string(),
            deadline: std::time::Instant::now()
                + std::time::Duration::from_millis(remaining_ms as u64),
        })
    }
    pub fn release_scan(self: Pin<&mut Self>, owner: QString) {
        self.submit(Request::Release {
            owner: owner.to_string(),
        });
    }
    pub fn release_all(self: Pin<&mut Self>) {
        self.submit(Request::ReleaseAll);
    }
    pub fn set_locked(self: Pin<&mut Self>, locked: bool) {
        self.submit(Request::Locked(locked));
    }
    pub fn execute(
        self: Pin<&mut Self>,
        id: QString,
        operation: QString,
        parameters: QVariant,
    ) -> bool {
        let parameters = qobject::command_variant(&parameters);
        let args = serde_json::to_value(crate::ipc::from_variant(&parameters)).unwrap_or_default();
        self.submit(Request::Command {
            id: id.to_string(),
            operation: operation.to_string(),
            args,
        })
    }
    pub fn respond(self: Pin<&mut Self>, id: QString, accepted: bool, value: QString) -> bool {
        self.submit(Request::Respond {
            id: id.to_string(),
            accepted,
            value: value.to_string(),
        })
    }
    fn apply(mut self: Pin<&mut Self>, state: State) {
        let edits = inir_core::model::diff(&self.rust().model.rows, &state.transfers);
        for edit in edits {
            match edit {
                ModelDelta::Insert { index, row } => {
                    self.as_mut().insert_start(index as i32);
                    self.as_mut().rust_mut().model.rows.insert(index, row);
                    self.as_mut().insert_end();
                }
                ModelDelta::Remove { index } => {
                    self.as_mut().remove_start(index as i32);
                    self.as_mut().rust_mut().model.rows.remove(index);
                    self.as_mut().remove_end();
                }
                ModelDelta::Move { from, to } => {
                    self.as_mut().move_start(from as i32, to as i32);
                    let row = self.as_mut().rust_mut().model.rows.remove(from);
                    self.as_mut().rust_mut().model.rows.insert(to, row);
                    self.as_mut().move_end();
                }
                ModelDelta::Change { index, row, roles } => {
                    self.as_mut().rust_mut().model.rows[index] = row;
                    self.as_mut().changed(index as i32, roles);
                }
                ModelDelta::ChangeRange { index, rows, roles } => {
                    let last = index + rows.len() - 1;
                    for (offset, row) in rows.into_iter().enumerate() {
                        self.as_mut().rust_mut().model.rows[index + offset] = row;
                    }
                    self.as_mut()
                        .changed_range(index as i32, last as i32, roles);
                }
            }
        }
        self.as_mut().set_count(state.transfers.len() as i32);
        self.as_mut().set_ready(state.ready);
        let mut metadata = serde_json::to_value(&state).unwrap();
        metadata.as_object_mut().unwrap().remove("transfers");
        self.as_mut()
            .set_state(crate::ipc::variant(&IpcValue::from(metadata)));
        self.as_mut().state_updated();
    }
}
