#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(QString, hostname)]
        #[qproperty(QString, username)]
        #[qproperty(QString, display_name)]
        #[qproperty(QString, distro_name)]
        #[qproperty(QString, distro_id)]
        #[qproperty(QString, distro_icon)]
        #[qproperty(QString, home_url)]
        #[qproperty(QString, documentation_url)]
        #[qproperty(QString, support_url)]
        #[qproperty(QString, bug_report_url)]
        #[qproperty(QString, privacy_policy_url)]
        #[qproperty(QString, logo)]
        #[qproperty(QString, desktop_environment)]
        #[qproperty(QString, windowing_system)]
        #[qproperty(f64, uptime)]
        #[qproperty(f64, memory_total)]
        #[qproperty(f64, memory_available)]
        #[qproperty(f64, cpu_usage)]
        #[qproperty(bool, cpu_usage_available)]
        #[qproperty(bool, ready)]
        #[qproperty(QString, error_string)]
        type SystemInfo = super::SystemInfoRust;

        #[qinvokable]
        fn refresh_identity(self: Pin<&mut SystemInfo>);
        #[qinvokable]
        fn set_resource_monitoring(self: Pin<&mut SystemInfo>, active: bool);
        #[qinvokable]
        fn diagnostics(self: &SystemInfo) -> QVariant;
    }
    impl cxx_qt::Threading for SystemInfo {}
    impl cxx_qt::Initialize for SystemInfo {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use inir_core::{
    events::Event,
    runtime::{Backend, ResourceConsumer, TaskGuard},
};
use std::sync::Arc;

#[derive(Default)]
pub struct SystemInfoRust {
    hostname: QString,
    username: QString,
    display_name: QString,
    distro_name: QString,
    distro_id: QString,
    distro_icon: QString,
    home_url: QString,
    documentation_url: QString,
    support_url: QString,
    bug_report_url: QString,
    privacy_policy_url: QString,
    logo: QString,
    desktop_environment: QString,
    windowing_system: QString,
    uptime: f64,
    memory_total: f64,
    memory_available: f64,
    cpu_usage: f64,
    cpu_usage_available: bool,
    ready: bool,
    error_string: QString,
    backend: Option<Arc<Backend>>,
    consumer: Option<ResourceConsumer>,
    tasks: Vec<TaskGuard>,
}

impl cxx_qt::Initialize for qobject::SystemInfo {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        let backend = match Backend::shared() {
            Ok(backend) => backend,
            Err(err) => {
                self.as_mut()
                    .set_error_string(QString::from(err.to_string()));
                return;
            }
        };
        let qt = self.qt_thread();
        let mut identity = backend.system.clone();
        let identity_task = backend.spawn(async move {
            loop {
                let value = identity.borrow_and_update().clone();
                if qt
                    .queue(move |mut object| {
                        if !value.username.is_empty() {
                            object.as_mut().apply_identity(value);
                            object.as_mut().set_ready(true);
                        }
                    })
                    .is_err()
                {
                    break;
                }
                if identity.changed().await.is_err() {
                    break;
                }
            }
        });
        let qt = self.qt_thread();
        let mut resources = backend.resources.clone();
        let resource_task = backend.spawn(async move {
            loop {
                let sample = resources.borrow_and_update().clone();
                if qt
                    .queue(move |mut object| {
                        object.as_mut().set_uptime(sample.uptime);
                        object.as_mut().set_memory_total(sample.memory_total as f64);
                        object
                            .as_mut()
                            .set_memory_available(sample.memory_available as f64);
                        object
                            .as_mut()
                            .set_cpu_usage_available(sample.cpu_usage.is_some());
                        object
                            .as_mut()
                            .set_cpu_usage(sample.cpu_usage.unwrap_or_default());
                    })
                    .is_err()
                {
                    break;
                }
                if resources.changed().await.is_err() {
                    break;
                }
            }
        });
        let mut rust = self.as_mut().rust_mut();
        rust.backend = Some(backend);
        rust.tasks = vec![identity_task, resource_task];
    }
}

impl qobject::SystemInfo {
    fn apply_identity(mut self: Pin<&mut Self>, value: inir_types::SystemInfo) {
        self.as_mut()
            .set_hostname(QString::from(value.hostname.as_str()));
        self.as_mut()
            .set_username(QString::from(value.username.as_str()));
        self.as_mut()
            .set_display_name(QString::from(value.display_name.as_str()));
        self.as_mut()
            .set_distro_name(QString::from(value.distro_name.as_str()));
        self.as_mut()
            .set_distro_id(QString::from(value.distro_id.as_str()));
        self.as_mut()
            .set_distro_icon(QString::from(value.distro_icon.as_str()));
        self.as_mut()
            .set_home_url(QString::from(value.home_url.as_str()));
        self.as_mut()
            .set_documentation_url(QString::from(value.documentation_url.as_str()));
        self.as_mut()
            .set_support_url(QString::from(value.support_url.as_str()));
        self.as_mut()
            .set_bug_report_url(QString::from(value.bug_report_url.as_str()));
        self.as_mut()
            .set_privacy_policy_url(QString::from(value.privacy_policy_url.as_str()));
        self.as_mut().set_logo(QString::from(value.logo.as_str()));
        self.as_mut()
            .set_desktop_environment(QString::from(value.desktop_environment.as_str()));
        self.as_mut()
            .set_windowing_system(QString::from(value.windowing_system.as_str()));
    }

    pub fn refresh_identity(mut self: Pin<&mut Self>) {
        if let Some(ref backend) = self.rust().backend {
            let events = backend.events.clone();
            let task = backend.spawn(async move {
                if let Ok(identity) =
                    tokio::task::spawn_blocking(inir_core::services::system::identity).await
                {
                    let _ = events.send(Event::System(identity)).await;
                }
            });
            let mut rust = self.as_mut().rust_mut();
            rust.tasks.retain(|task| !task.is_finished());
            rust.tasks.push(task);
        }
    }

    pub fn set_resource_monitoring(mut self: Pin<&mut Self>, active: bool) {
        let consumer = if active && self.rust().consumer.is_none() {
            self.rust()
                .backend
                .as_ref()
                .map(|backend| backend.resource_consumer())
        } else {
            None
        };
        let mut rust = self.as_mut().rust_mut();
        if !active {
            rust.consumer = None;
        } else if let Some(consumer) = consumer {
            rust.consumer = Some(consumer);
        }
    }

    pub fn diagnostics(&self) -> cxx_qt_lib::QVariant {
        use crate::ipc::map;
        use inir_types::desktop::IpcValue as V;
        let backend = self.rust().backend.as_ref();
        let consumers = backend.map(|b| b.resource_consumers()).unwrap_or_default();
        let service_consumers = backend.map(|b| b.service_consumers()).unwrap_or_default();
        let status = |status: &inir_types::desktop::ServiceStatus| {
            map([
                ("ready", V::Bool(status.ready)),
                ("error", V::String(status.error.clone())),
            ])
        };
        // Diagnostic metadata only. Never clone a global desktop snapshot or
        // serialize JSON on the GUI thread, even for this explicit IPC request.
        let services = backend
            .map(|b| {
                map([
                    ("network", status(&b.network.borrow().status)),
                    ("power", status(&b.power.borrow().status)),
                    ("brightness", status(&b.brightness.borrow().status)),
                    ("niri", status(&b.niri.borrow().status)),
                    ("keyboard", status(&b.keyboard.borrow().status)),
                ])
            })
            .unwrap_or(V::Null);
        let task = |name: &str, active| {
            map([
                ("name", V::String(name.into())),
                ("active", V::Bool(active)),
            ])
        };
        crate::ipc::variant(&map([
            ("backend", V::String("rust".into())),
            ("tokioWorkerThreads", V::Number(1.0)),
            ("resourceConsumers", V::Number(consumers as f64)),
            ("desktopConsumers", V::Number(service_consumers as f64)),
            ("services", services),
            ("resourceSampling", V::Bool(consumers > 0)),
            (
                "tasks",
                V::List(vec![
                    task("event-reducer", backend.is_some()),
                    task("resources", consumers > 0),
                    task("desktop-services", service_consumers > 0),
                ]),
            ),
        ]))
    }
}
