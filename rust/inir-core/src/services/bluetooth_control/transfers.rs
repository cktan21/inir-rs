use super::{
    agent::Broker,
    types::{safe_filename, Prompt, Transfer},
    Event,
};
use crate::services::dbus::{properties, read};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::sync::mpsc;
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue},
    Connection, Proxy,
};

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.obex.Error")]
pub enum ObexError {
    Rejected(String),
    Canceled(String),
}

pub struct Agent {
    pub broker: Broker,
    pub root: PathBuf,
    pub events: mpsc::Sender<Event>,
}
#[zbus::interface(name = "org.bluez.obex.Agent1")]
impl Agent {
    fn release(&self) {
        self.broker.cancel_device("obex");
    }
    fn cancel(&self) {
        self.broker.cancel_device("obex");
    }
    async fn authorize_push(
        &self,
        transfer: OwnedObjectPath,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<String, ObexError> {
        let props = properties(
            bus,
            "org.bluez.obex",
            transfer.as_str(),
            "org.bluez.obex.Transfer1",
        )
        .await
        .map_err(|e| ObexError::Rejected(e.to_string()))?;
        let name = safe_filename(&read::<String>(&props, "Name"));
        let size = read::<u64>(&props, "Size");
        self.broker
            .ask(Prompt {
                kind: "incomingFile".into(),
                device: "obex".into(),
                name: name.clone(),
                size,
                ..Default::default()
            })
            .await
            .map_err(|e| ObexError::Rejected(e.to_string()))?;
        tokio::fs::create_dir_all(&self.root)
            .await
            .map_err(|e| ObexError::Rejected(e.to_string()))?;
        let directory = tempfile::Builder::new()
            .prefix(".incoming-")
            .tempdir_in(&self.root)
            .map_err(|e| ObexError::Rejected(e.to_string()))?
            .keep();
        let path = directory.join(&name);
        self.events
            .send(Event::Incoming(Transfer {
                id: transfer.to_string(),
                name,
                direction: "receive".into(),
                status: "queued".into(),
                size,
                filename: path.to_string_lossy().into(),
                ..Default::default()
            }))
            .await
            .map_err(|_| ObexError::Canceled("Shell stopped".into()))?;
        Ok(path.to_string_lossy().into())
    }
}

pub async fn register(
    bus: &Connection,
    broker: Broker,
    root: PathBuf,
    events: mpsc::Sender<Event>,
) -> Result<(), String> {
    let path = "/org/inir/ObexAgent";
    bus.object_server()
        .at(
            path,
            Agent {
                broker,
                root,
                events,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    Proxy::new(
        bus,
        "org.bluez.obex",
        "/org/bluez/obex",
        "org.bluez.obex.AgentManager1",
    )
    .await
    .map_err(|e| e.to_string())?
    .call::<_, _, ()>(
        "RegisterAgent",
        &(OwnedObjectPath::try_from(path).unwrap(),),
    )
    .await
    .map_err(|e| e.to_string())
}

pub fn from_properties(id: &str, props: &HashMap<String, OwnedValue>) -> Transfer {
    Transfer {
        id: id.into(),
        name: read(props, "Name"),
        direction: "send".into(),
        status: read(props, "Status"),
        size: read(props, "Size"),
        transferred: read(props, "Transferred"),
        filename: read(props, "Filename"),
        error: String::new(),
    }
}

pub async fn send(
    bus: &Connection,
    system: &Connection,
    args: &serde_json::Value,
) -> Result<(String, Vec<Transfer>), String> {
    let adapter = super::types::adapter_path(super::text(args, "adapter")?)?;
    let adapter_props = properties(system, "org.bluez", &adapter, "org.bluez.Adapter1")
        .await
        .map_err(|e| e.to_string())?;
    let address = super::text(args, "address")?;
    super::types::device_path(super::text(args, "adapter")?, address)?;
    let files = args["files"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 100)
        .ok_or("Select between 1 and 100 files")?;
    let mut paths = Vec::new();
    for file in files {
        let raw = file.as_str().ok_or("Invalid file path")?;
        let path = PathBuf::from(raw);
        if !path.is_absolute()
            || !tokio::fs::metadata(&path)
                .await
                .map_err(|e| e.to_string())?
                .is_file()
        {
            return Err("Select readable local files".into());
        }
        tokio::fs::File::open(&path)
            .await
            .map_err(|e| e.to_string())?;
        paths.push(raw.to_string());
    }
    let client = Proxy::new(
        bus,
        "org.bluez.obex",
        "/org/bluez/obex",
        "org.bluez.obex.Client1",
    )
    .await
    .map_err(|e| e.to_string())?;
    let source: String = read(&adapter_props, "Address");
    let session: OwnedObjectPath = client
        .call(
            "CreateSession",
            &(
                address,
                HashMap::from([
                    ("Target", zbus::zvariant::Value::from("opp")),
                    ("Source", zbus::zvariant::Value::from(source.as_str())),
                ]),
            ),
        )
        .await
        .map_err(|e| e.to_string())?;
    let result = async {
        let proxy = Proxy::new(
            bus,
            "org.bluez.obex",
            session.as_str(),
            "org.bluez.obex.ObjectPush1",
        )
        .await
        .map_err(|e| e.to_string())?;
        let mut transfers = Vec::new();
        for path in paths {
            let (id, props): (OwnedObjectPath, HashMap<String, OwnedValue>) = proxy
                .call("SendFile", &(path,))
                .await
                .map_err(|e| e.to_string())?;
            transfers.push(from_properties(id.as_str(), &props));
        }
        Ok::<_, String>(transfers)
    }
    .await;
    match result {
        Ok(rows) => Ok((session.to_string(), rows)),
        Err(error) => {
            let _ = client.call::<_, _, ()>("RemoveSession", &(session,)).await;
            Err(error)
        }
    }
}

pub async fn finalize(path: &str, root: &Path, name: &str) -> Result<String, String> {
    let source = Path::new(path);
    // Only finalize a path produced by our incoming-transfer reservation.
    if source.parent().and_then(|p| p.parent()) != Some(root)
        || !source
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|p| p.to_string_lossy().starts_with(".incoming-"))
    {
        return Err("Invalid incoming transfer location".into());
    }
    let name = safe_filename(name);
    for number in 0..1000 {
        let destination = root.join(if number == 0 {
            name.clone()
        } else {
            format!("{number}-{name}")
        });
        match tokio::fs::hard_link(source, &destination).await {
            Ok(()) => {
                tokio::fs::remove_file(source)
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = tokio::fs::remove_dir(source.parent().unwrap()).await;
                return Ok(destination.to_string_lossy().into());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Too many files with the same name".into())
}
