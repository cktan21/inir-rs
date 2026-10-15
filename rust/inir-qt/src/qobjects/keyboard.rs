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
        #[qproperty(bool, active)]
        #[qproperty(bool, ready)]
        #[qproperty(QString, error)]
        #[qproperty(bool, caps)]
        #[qproperty(bool, num)]
        #[qproperty(i32, devices)]
        type NativeKeyboard = super::NativeKeyboardRust;
        #[qinvokable]
        fn set_enabled(self: Pin<&mut NativeKeyboard>, enabled: bool);
        #[qsignal]
        fn state_updated(self: Pin<&mut NativeKeyboard>);
    }
    impl cxx_qt::Threading for NativeKeyboard {}
    impl cxx_qt::Initialize for NativeKeyboard {}
}
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use inir_core::runtime::{Backend, DomainConsumer, TaskGuard};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};
#[derive(Default)]
pub struct NativeKeyboardRust {
    active: bool,
    ready: bool,
    error: QString,
    caps: bool,
    num: bool,
    devices: i32,
    backend: Option<Arc<Backend>>,
    consumer: Option<DomainConsumer>,
    task: Option<TaskGuard>,
    generation: u64,
}
impl cxx_qt::Initialize for qobject::NativeKeyboard {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::init_logging();
        match Backend::shared() {
            Ok(backend) => self.as_mut().rust_mut().backend = Some(backend),
            Err(error) => self.as_mut().set_error(QString::from(error.to_string())),
        }
    }
}
impl qobject::NativeKeyboard {
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
            self.as_mut().set_devices(0);
            self.as_mut().set_error(QString::default());
            return;
        }
        let Some(backend) = self.rust().backend.clone() else {
            return;
        };
        // Domain index 4 is the keyboard lock worker; the lease starts it.
        self.as_mut().rust_mut().consumer = Some(backend.acquire(4));
        let qt = self.qt_thread();
        let mut states = backend.keyboard.clone();
        let pending = Arc::new(Mutex::new((false, None)));
        let task = backend.spawn(async move {
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
                            // Withdraw readiness before changing fields on failure,
                            // then announce one complete sample after every setter.
                            if !state.status.ready {
                                object.as_mut().set_ready(false);
                            }
                            object.as_mut().set_caps(state.caps);
                            object.as_mut().set_num(state.num);
                            object.as_mut().set_devices(state.devices as i32);
                            object
                                .as_mut()
                                .set_error(QString::from(state.status.error.as_str()));
                            object.as_mut().set_ready(state.status.ready);
                            object.as_mut().state_updated();
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
        self.as_mut().rust_mut().task = Some(task);
    }
}
