#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("inir-qt/list_model.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        #[cxx_name = "commandVariant"]
        fn utility_command_variant(value: &QVariant) -> QVariant;
    }
    unsafe extern "C++Qt" {
        include!("inir-qt/list_model.h");
        #[qobject]
        type UtilityJobsBase;
    }
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = UtilityJobsBase]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, ready)]
        #[qproperty(QString, error)]
        #[qproperty(i32, pending_count)]
        #[qproperty(bool, device_events_ready)]
        type NativeUtilityJobs = super::UtilityJobsRust;
        #[qinvokable]
        fn submit(
            self: Pin<&mut NativeUtilityJobs>,
            id: QString,
            operation: QString,
            parameters: QVariant,
        ) -> bool;
        #[qinvokable]
        fn cancel(self: Pin<&mut NativeUtilityJobs>, id: QString) -> bool;
        #[qinvokable]
        fn set_device_events_enabled(self: Pin<&mut NativeUtilityJobs>, enabled: bool);
        #[qsignal]
        fn finished(
            self: Pin<&mut NativeUtilityJobs>,
            id: QString,
            success: bool,
            result: QVariant,
            error: QString,
        );
        #[qsignal]
        fn devices_changed(self: Pin<&mut NativeUtilityJobs>, kind: QString);
    }
    impl cxx_qt::Threading for NativeUtilityJobs {}
    impl cxx_qt::Initialize for NativeUtilityJobs {}
}

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QVariant};
use inir_core::runtime::{Backend, TaskGuard};
use inir_types::desktop::IpcValue;
use std::{collections::HashMap, pin::Pin, sync::Arc};
use tokio_util::sync::CancellationToken;

struct Job {
    stop: CancellationToken,
    _task: TaskGuard,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[derive(Default)]
pub struct UtilityJobsRust {
    ready: bool,
    error: QString,
    pending_count: i32,
    device_events_ready: bool,
    backend: Option<Arc<Backend>>,
    context: Arc<inir_utils::Context>,
    jobs: HashMap<String, Job>,
    device_job: Option<Job>,
    device_generation: u64,
}
impl cxx_qt::Initialize for qobject::NativeUtilityJobs {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        match Backend::shared() {
            Ok(backend) => {
                self.as_mut().rust_mut().backend = Some(backend);
                self.as_mut().set_ready(true);
            }
            Err(error) => self.set_error(QString::from(error.to_string())),
        }
    }
}
impl qobject::NativeUtilityJobs {
    pub fn set_device_events_enabled(mut self: Pin<&mut Self>, enabled: bool) {
        if enabled == self.rust().device_job.is_some() {
            return;
        }
        self.as_mut().rust_mut().device_generation += 1;
        self.as_mut().rust_mut().device_job = None;
        self.as_mut().set_device_events_ready(false);
        if !enabled {
            return;
        }
        let Some(backend) = self.rust().backend.clone() else {
            return;
        };
        let generation = self.rust().device_generation;
        let stop = CancellationToken::new();
        let worker_stop = stop.clone();
        let qt = self.qt_thread();
        let task = backend.spawn(async move {
            let result = inir_utils::devices::watch(&worker_stop, |kind| {
                let _ = qt.queue(move |object| {
                    if object.rust().device_generation != generation {
                        return;
                    }
                    if kind == "ready" {
                        object.set_device_events_ready(true);
                    } else {
                        object.devices_changed(QString::from(kind));
                    }
                });
            })
            .await;
            if let Err(error) = result {
                let _ = qt.queue(move |mut object| {
                    if object.rust().device_generation != generation {
                        return;
                    }
                    object.as_mut().set_device_events_ready(false);
                    object.set_error(QString::from(error));
                });
            }
        });
        self.as_mut().rust_mut().device_job = Some(Job { stop, _task: task });
    }
    pub fn submit(
        mut self: Pin<&mut Self>,
        id: QString,
        operation: QString,
        parameters: QVariant,
    ) -> bool {
        let id = id.to_string();
        let operation = operation.to_string();
        if id.is_empty()
            || self.rust().jobs.len() >= 64
            || self.rust().jobs.contains_key(&id)
            || !inir_utils::OPERATIONS.contains(&operation.as_str())
        {
            return false;
        }
        let Some(backend) = self.rust().backend.clone() else {
            return false;
        };
        let parameters = qobject::utility_command_variant(&parameters);
        let args = serde_json::to_value(crate::ipc::from_variant(&parameters)).unwrap_or_default();
        let context = self.rust().context.clone();
        let stop = CancellationToken::new();
        let worker_stop = stop.clone();
        let key = id.clone();
        let qt = self.qt_thread();
        let task = backend.spawn(async move {
            let result = context.execute(&operation, args, &worker_stop).await;
            let _ = qt.queue(move |mut object| {
                // A canceled or completed job can only finish once. Qt thread
                // serialization also prevents reuse of an outstanding request ID.
                let Some(_job) = object.as_mut().rust_mut().jobs.remove(&key) else {
                    return;
                };
                let count = object.rust().jobs.len() as i32;
                object.as_mut().set_pending_count(count);
                let result = if worker_stop.is_cancelled() {
                    Err("Utility request canceled".into())
                } else {
                    result
                };
                let (success, value, error) = match result {
                    Ok(value) => (true, value, String::new()),
                    Err(error) => (false, serde_json::Value::Null, error),
                };
                object.finished(
                    QString::from(key),
                    success,
                    crate::ipc::variant(&IpcValue::from(value)),
                    QString::from(error),
                );
            });
        });
        self.as_mut()
            .rust_mut()
            .jobs
            .insert(id, Job { stop, _task: task });
        let count = self.rust().jobs.len() as i32;
        self.set_pending_count(count);
        true
    }
    pub fn cancel(self: Pin<&mut Self>, id: QString) -> bool {
        if let Some(job) = self.rust().jobs.get(&id.to_string()) {
            job.stop.cancel();
            true
        } else {
            false
        }
    }
}
