use super::types::{Prompt, State, PROMPT_TIMEOUT};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::{oneshot, watch};
use zbus::{zvariant::OwnedObjectPath, Connection};

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
pub enum AgentError {
    Rejected(String),
    Canceled(String),
}

struct Pending {
    device: String,
    reply: Option<oneshot::Sender<(bool, String)>>,
    stop: tokio_util::sync::CancellationToken,
}
#[derive(Clone)]
pub struct Broker {
    next: Arc<AtomicU64>,
    pending: Arc<Mutex<BTreeMap<String, Pending>>>,
    state: watch::Sender<State>,
    locked: Arc<AtomicBool>,
}
impl Broker {
    pub fn new(state: watch::Sender<State>) -> Self {
        Self {
            next: Arc::new(AtomicU64::new(1)),
            pending: Default::default(),
            state,
            locked: Default::default(),
        }
    }
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }
    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::Relaxed);
        if locked {
            self.cancel_device("");
        }
    }
    fn insert(
        &self,
        mut prompt: Prompt,
        reply: Option<oneshot::Sender<(bool, String)>>,
    ) -> Result<String, AgentError> {
        let mut pending = self.pending.lock().unwrap();
        if self.locked.load(Ordering::Relaxed) {
            return Err(AgentError::Canceled("Session is locked".into()));
        }
        if pending.len() >= 8 {
            return Err(AgentError::Rejected(
                "Another pairing request is in progress".into(),
            ));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed).to_string();
        prompt.id = id.clone();
        let stop = tokio_util::sync::CancellationToken::new();
        let displayed = reply.is_none();
        pending.insert(
            id.clone(),
            Pending {
                device: prompt.device.clone(),
                reply,
                stop: stop.clone(),
            },
        );
        self.state.send_modify(|s| s.prompts.push(prompt));
        if displayed {
            let broker = self.clone();
            let id = id.clone();
            tokio::spawn(async move {
                tokio::select! { _ = stop.cancelled() => {}, _ = tokio::time::sleep(PROMPT_TIMEOUT) => broker.remove(&id) }
            });
        }
        Ok(id)
    }
    pub async fn ask(&self, prompt: Prompt) -> Result<String, AgentError> {
        let (tx, rx) = oneshot::channel();
        let id = self.insert(prompt, Some(tx))?;
        let result = tokio::time::timeout(PROMPT_TIMEOUT, rx).await;
        self.remove(&id);
        match result {
            Ok(Ok((true, value))) => Ok(value),
            Ok(Ok((false, _))) => Err(AgentError::Rejected("Rejected by user".into())),
            _ => Err(AgentError::Canceled(
                "Pairing request canceled or timed out".into(),
            )),
        }
    }
    pub fn display(&self, prompt: Prompt) -> Result<(), AgentError> {
        let existing = self
            .state
            .borrow()
            .prompts
            .iter()
            .find(|p| p.device == prompt.device && p.kind == prompt.kind)
            .map(|p| p.id.clone());
        if let Some(id) = existing {
            self.state.send_modify(|s| {
                if let Some(p) = s.prompts.iter_mut().find(|p| p.id == id) {
                    p.value = prompt.value;
                    p.entered = prompt.entered;
                }
            });
        } else {
            self.insert(prompt, None)?;
        }
        Ok(())
    }
    fn remove(&self, id: &str) {
        if let Some(pending) = self.pending.lock().unwrap().remove(id) {
            pending.stop.cancel();
        }
        self.state.send_if_modified(|s| {
            let n = s.prompts.len();
            s.prompts.retain(|p| p.id != id);
            n != s.prompts.len()
        });
    }
    pub fn respond(&self, id: &str, accepted: bool, value: String) -> Result<(), String> {
        let pending = self
            .pending
            .lock()
            .unwrap()
            .remove(id)
            .ok_or("This request has expired")?;
        pending.stop.cancel();
        if let Some(reply) = pending.reply {
            let _ = reply.send((accepted, value));
        }
        self.remove(id);
        Ok(())
    }
    pub fn cancel_device(&self, device: &str) {
        let ids: Vec<_> = self
            .pending
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, p)| device.is_empty() || p.device == device)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.remove(&id);
        }
    }
}

#[derive(Clone)]
pub struct Agent {
    pub broker: Broker,
}
impl Agent {
    async fn prompt(
        &self,
        bus: &Connection,
        device: &OwnedObjectPath,
        kind: &str,
        value: String,
        entered: u16,
    ) -> Prompt {
        let props =
            super::super::dbus::properties(bus, "org.bluez", device.as_str(), "org.bluez.Device1")
                .await
                .unwrap_or_default();
        let name: String = super::super::dbus::read(&props, "Alias");
        Prompt {
            kind: kind.into(),
            device: device.to_string(),
            name,
            value,
            entered,
            ..Default::default()
        }
    }
}
#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {
        self.broker.cancel_device("");
        self.broker.state.send_modify(|s| s.agent_ready = false);
    }
    async fn request_pin_code(
        &self,
        device: OwnedObjectPath,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<String, AgentError> {
        let value = self
            .broker
            .ask(self.prompt(bus, &device, "pin", String::new(), 0).await)
            .await?;
        if value.is_empty() || value.len() > 16 || value.chars().any(char::is_control) {
            return Err(AgentError::Rejected(
                "PIN must contain 1–16 characters".into(),
            ));
        }
        Ok(value)
    }
    async fn display_pin_code(
        &self,
        device: OwnedObjectPath,
        pincode: String,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<(), AgentError> {
        self.broker
            .display(self.prompt(bus, &device, "displayPin", pincode, 0).await)
    }
    async fn request_passkey(
        &self,
        device: OwnedObjectPath,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<u32, AgentError> {
        let value = self
            .broker
            .ask(self.prompt(bus, &device, "passkey", String::new(), 0).await)
            .await?;
        if value.is_empty() || value.len() > 6 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(AgentError::Rejected(
                "Passkey must contain up to six digits".into(),
            ));
        }
        value
            .parse()
            .map_err(|_| AgentError::Rejected("Invalid passkey".into()))
    }
    async fn display_passkey(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        entered: u16,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<(), AgentError> {
        self.broker.display(
            self.prompt(
                bus,
                &device,
                "displayPasskey",
                format!("{passkey:06}"),
                entered,
            )
            .await,
        )
    }
    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<(), AgentError> {
        self.broker
            .ask(
                self.prompt(bus, &device, "confirmation", format!("{passkey:06}"), 0)
                    .await,
            )
            .await
            .map(|_| ())
    }
    async fn request_authorization(
        &self,
        device: OwnedObjectPath,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<(), AgentError> {
        self.broker
            .ask(
                self.prompt(bus, &device, "authorization", String::new(), 0)
                    .await,
            )
            .await
            .map(|_| ())
    }
    async fn authorize_service(
        &self,
        device: OwnedObjectPath,
        uuid: String,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<(), AgentError> {
        let props =
            super::super::dbus::properties(bus, "org.bluez", device.as_str(), "org.bluez.Device1")
                .await
                .unwrap_or_default();
        if super::super::dbus::read::<bool>(&props, "Trusted") {
            return Ok(());
        }
        self.broker
            .ask(self.prompt(bus, &device, "service", uuid, 0).await)
            .await
            .map(|_| ())
    }
    fn cancel(&self) {
        self.broker.cancel_device("");
    }
}

pub async fn register(bus: &Connection, broker: Broker) -> Result<(), String> {
    let path = "/org/inir/BluetoothAgent";
    bus.object_server()
        .at(path, Agent { broker })
        .await
        .map_err(|e| e.to_string())?;
    let manager = zbus::Proxy::new(bus, "org.bluez", "/org/bluez", "org.bluez.AgentManager1")
        .await
        .map_err(|e| e.to_string())?;
    let object = OwnedObjectPath::try_from(path).unwrap();
    manager
        .call::<_, _, ()>("RegisterAgent", &(object.clone(), "KeyboardDisplay"))
        .await
        .map_err(|e| e.to_string())?;
    manager
        .call::<_, _, ()>("RequestDefaultAgent", &(object,))
        .await
        .map_err(|e| e.to_string())
}
