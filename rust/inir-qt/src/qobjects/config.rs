#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(QString, document_json)]
        #[qproperty(QString, schema_json)]
        #[qproperty(QString, file_path)]
        #[qproperty(QString, error_string)]
        #[qproperty(bool, ready)]
        type ConfigService = super::ConfigServiceRust;

        #[qinvokable]
        fn open(self: Pin<&mut ConfigService>, path: QString);
        #[qinvokable]
        fn set_value(self: Pin<&mut ConfigService>, key: QString, json_value: QString);
        #[qsignal]
        fn config_changed(self: Pin<&mut ConfigService>);
        #[qsignal]
        fn write_finished(self: Pin<&mut ConfigService>, key: QString, success: bool);
    }
    impl cxx_qt::Threading for ConfigService {}
    impl cxx_qt::Initialize for ConfigService {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use inir_core::{
    config::ConfigStore,
    events::Event,
    runtime::{Backend, TaskGuard},
};
use std::sync::Arc;
use tokio::sync::mpsc;

struct Patch {
    key: String,
    value: serde_json::Value,
}

#[derive(Default)]
pub struct ConfigServiceRust {
    document_json: QString,
    schema_json: QString,
    file_path: QString,
    error_string: QString,
    ready: bool,
    backend: Option<Arc<Backend>>,
    store: Option<ConfigStore>,
    watcher: Option<TaskGuard>,
    writer: Option<TaskGuard>,
    patches: Option<mpsc::Sender<Patch>>,
    generation: u64,
}

impl cxx_qt::Initialize for qobject::ConfigService {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        self.as_mut().set_schema_json(QString::from(serde_json::json!({
            "categories": inir_types::schema::CATEGORIES.iter().map(|(id, title)| serde_json::json!({"id": id, "title": title})).collect::<Vec<_>>(),
            "fields": inir_types::schema::fields(),
        }).to_string()));
        match Backend::shared() {
            Ok(backend) => self.as_mut().rust_mut().backend = Some(backend),
            Err(err) => self
                .as_mut()
                .set_error_string(QString::from(err.to_string())),
        }
    }
}

impl qobject::ConfigService {
    pub fn open(mut self: Pin<&mut Self>, path: QString) {
        self.as_mut().set_ready(false);
        self.as_mut().set_error_string(QString::default());
        {
            let mut rust = self.as_mut().rust_mut();
            rust.generation += 1;
            rust.watcher = None;
            rust.store = None;
            rust.writer = None;
            rust.patches = None;
        }
        let store = match ConfigStore::new(path.to_string()) {
            Ok(store) => store,
            Err(err) => {
                self.as_mut()
                    .set_error_string(QString::from(err.to_string()));
                return;
            }
        };
        self.as_mut()
            .set_file_path(QString::from(store.path().to_string_lossy().as_ref()));
        let generation = self.rust().generation;
        let qt = self.qt_thread();
        let Some(ref backend) = self.rust().backend else {
            return;
        };
        let events = backend.events.clone();
        let writer_events = events.clone();
        let writer_qt = self.qt_thread();
        let writer_store = store.clone();
        let (patches, mut pending) = mpsc::channel::<Patch>(128);
        // One FIFO writer per QObject: quick slider/toggle changes cannot be
        // reordered by spawn_blocking scheduling or flock acquisition.
        let writer = backend.spawn(async move {
            while let Some(Patch { key, value }) = pending.recv().await {
                let store = writer_store.clone();
                let patch_key = key.clone();
                let result =
                    tokio::task::spawn_blocking(move || store.set(&patch_key, value)).await;
                if let Ok(Ok(ref config)) = result {
                    let _ = writer_events.send(Event::Config(config.clone())).await;
                }
                if writer_qt
                    .queue(move |mut object| {
                        if object.rust().generation != generation {
                            return;
                        }
                        match result {
                            Ok(Ok(config)) => {
                                object.as_mut().apply_config(generation, config);
                                object.as_mut().write_finished(QString::from(key), true);
                            }
                            result => {
                                let error = match result {
                                    Ok(Err(err)) => err.to_string(),
                                    Err(err) => err.to_string(),
                                    _ => unreachable!(),
                                };
                                object.as_mut().set_error_string(QString::from(error));
                                object.as_mut().write_finished(QString::from(key), false);
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let task_store = store.clone();
        let watcher = backend.spawn(async move {
            let initialization = tokio::task::spawn_blocking(move || {
                // Establish the directory/watch before loading to cover renames
                // racing with initialization. load_and_migrate holds the lock.
                if let Some(parent) = task_store.path().parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let watch = task_store.watch().map_err(std::io::Error::other)?;
                let config = task_store.load_and_migrate()?;
                Ok::<_, std::io::Error>((task_store, watch, config))
            })
            .await;
            let (store, (_watcher, mut changes), config) = match initialization {
                Ok(Ok(value)) => value,
                result => {
                    let error = match result {
                        Ok(Err(err)) => err.to_string(),
                        Err(err) => err.to_string(),
                        _ => unreachable!(),
                    };
                    let _ = qt.queue(move |mut object| {
                        if object.rust().generation == generation {
                            object.as_mut().set_error_string(QString::from(error));
                        }
                    });
                    return;
                }
            };
            let _ = events.send(Event::Config(config.clone())).await;
            let _ = qt.queue(move |object| {
                object.apply_config(generation, config);
            });
            while changes.recv().await.is_some() {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                while changes.try_recv().is_ok() {}
                let reader = store.clone();
                match tokio::task::spawn_blocking(move || reader.read()).await {
                    Ok(Ok(config)) => {
                        let _ = events.send(Event::Config(config.clone())).await;
                        if qt
                            .queue(move |object| {
                                object.apply_config(generation, config);
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    result => {
                        let error = match result {
                            Ok(Err(err)) => err.to_string(),
                            Err(err) => err.to_string(),
                            _ => unreachable!(),
                        };
                        if qt
                            .queue(move |mut object| {
                                if object.rust().generation == generation {
                                    object.as_mut().set_error_string(QString::from(error));
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        let mut rust = self.as_mut().rust_mut();
        rust.store = Some(store);
        rust.watcher = Some(watcher);
        rust.writer = Some(writer);
        rust.patches = Some(patches);
    }

    fn apply_config(mut self: Pin<&mut Self>, generation: u64, config: inir_types::config::Config) {
        if self.rust().generation != generation {
            return;
        }
        if let Ok(json) = serde_json::to_string(&config) {
            let changed = self.document_json().to_string() != json;
            self.as_mut().set_document_json(QString::from(json));
            self.as_mut().set_error_string(QString::default());
            self.as_mut().set_ready(true);
            if changed {
                self.as_mut().config_changed();
            }
        }
    }

    pub fn set_value(mut self: Pin<&mut Self>, key: QString, json_value: QString) {
        let value = match serde_json::from_str(&json_value.to_string()) {
            Ok(value) => value,
            Err(err) => {
                self.as_mut()
                    .set_error_string(QString::from(err.to_string()));
                self.as_mut().write_finished(key, false);
                return;
            }
        };
        if !*self.ready() {
            self.as_mut()
                .set_error_string(QString::from("configuration is not ready"));
            self.as_mut().write_finished(key, false);
            return;
        }
        let Some(sender) = self.rust().patches.as_ref() else {
            return;
        };
        if sender
            .try_send(Patch {
                key: key.to_string(),
                value,
            })
            .is_err()
        {
            self.as_mut()
                .set_error_string(QString::from("configuration write queue is full or closed"));
            self.as_mut().write_finished(key, false);
        }
    }
}
