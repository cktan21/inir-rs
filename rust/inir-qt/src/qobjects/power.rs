#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h"); type QString=cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h"); type QVariant=cxx_qt_lib::QVariant;
    }
    #[auto_cxx_name] extern "RustQt" {
        #[qobject] #[qml_element] #[qml_singleton]
        #[qproperty(bool, active)] #[qproperty(bool, ready)] #[qproperty(QString, error)]
        #[qproperty(QString, active_profile)] #[qproperty(QVariant, profiles)] #[qproperty(QString, degraded)]
        type NativePower=super::NativePowerRust;
        #[qinvokable] fn set_enabled(self:Pin<&mut NativePower>,enabled:bool);
        #[qinvokable] fn set_profile(self:Pin<&mut NativePower>,id:QString,profile:QString)->bool;
        #[qsignal] fn command_finished(self:Pin<&mut NativePower>,id:QString,success:bool,error:QString);
    }
    impl cxx_qt::Threading for NativePower {}
    impl cxx_qt::Initialize for NativePower {}
}
use cxx_qt::{CxxQtType,Threading};
use cxx_qt_lib::{QString,QVariant};
use inir_core::runtime::{Backend,DomainConsumer,TaskGuard};
use inir_types::desktop::Command;
use std::{pin::Pin,sync::{Arc,Mutex}};
#[derive(Default)]
pub struct NativePowerRust {
    active:bool,ready:bool,error:QString,active_profile:QString,profiles:QVariant,degraded:QString,
    backend:Option<Arc<Backend>>,consumer:Option<DomainConsumer>,task:Option<TaskGuard>,commands:Vec<TaskGuard>,generation:u64,
}
impl cxx_qt::Initialize for qobject::NativePower {
    fn initialize(mut self:Pin<&mut Self>) {
        match Backend::shared() { Ok(backend)=>self.as_mut().rust_mut().backend=Some(backend),
            Err(error)=>self.as_mut().set_error(QString::from(error.to_string())) }
    }
}
impl qobject::NativePower {
    pub fn set_enabled(mut self:Pin<&mut Self>,enabled:bool) {
        if *self.active()==enabled { return; }
        let generation=self.rust().generation.wrapping_add(1);
        self.as_mut().rust_mut().generation=generation;
        self.as_mut().set_active(enabled);
        if !enabled { self.as_mut().rust_mut().consumer=None; self.as_mut().rust_mut().task=None; self.as_mut().set_ready(false); return; }
        let Some(backend)=self.rust().backend.clone() else { return; };
        self.as_mut().rust_mut().consumer=Some(backend.acquire(1));
        let qt=self.qt_thread();
        let mut states=backend.power.clone();
        let pending=Arc::new(Mutex::new((false,None)));
        let task=backend.spawn(async move {
            loop {
                let state=states.borrow_and_update().clone();
                let queue={ let mut p=pending.lock().unwrap(); p.1=Some(state); let queue=!p.0; p.0=true; queue };
                if queue {
                    let pending=pending.clone();
                    if qt.queue(move |mut object| {
                        if object.rust().generation!=generation { return; }
                        let state={ let mut p=pending.lock().unwrap(); p.0=false; p.1.take().unwrap() };
                        object.as_mut().set_active_profile(QString::from(state.active_profile.as_str()));
                        object.as_mut().set_profiles(crate::ipc::strings(&state.profiles));
                        object.as_mut().set_degraded(QString::from(state.degraded.as_str()));
                        object.as_mut().set_error(QString::from(state.status.error.as_str()));
                        object.as_mut().set_ready(state.status.ready);
                    }).is_err() { break; }
                }
                if states.changed().await.is_err() { break; }
            }
        });
        self.as_mut().rust_mut().task=Some(task);
    }
    pub fn set_profile(mut self:Pin<&mut Self>,id:QString,profile:QString)->bool {
        if !*self.active() { return false; }
        let Some(backend)=self.rust().backend.clone() else { return false; };
        let qt=self.qt_thread(); let id=id.to_string(); let profile=profile.to_string();
        let task=backend.clone().spawn(async move {
            let result=backend.execute(Command::PowerProfile{profile}).await;
            let _=qt.queue(move |mut object| { object.as_mut().command_finished(QString::from(id),result.is_ok(),QString::from(result.err().unwrap_or_default())); });
        });
        let mut rust=self.as_mut().rust_mut(); rust.commands.retain(|task|!task.is_finished()); rust.commands.push(task); true
    }
}
