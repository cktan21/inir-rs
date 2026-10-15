use super::{
    text,
    types::{adapter_path, Network},
};
use crate::services::dbus::{properties, read};
use std::collections::HashMap;
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue, Value},
    Connection, Proxy,
};

type Settings = HashMap<String, HashMap<String, OwnedValue>>;
const NM: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";

fn owned(value: Value<'_>) -> OwnedValue {
    OwnedValue::try_from(value).expect("owned setting")
}

pub fn profile(address: Option<&str>, adapter: &str) -> Result<Settings, String> {
    adapter_path(adapter)?;
    let mut connection = HashMap::from([
        (
            "id".into(),
            owned(Value::from(if address.is_some() {
                "iNiR Bluetooth internet"
            } else {
                "iNiR Bluetooth sharing"
            })),
        ),
        ("type".into(), owned(Value::from("bluetooth"))),
        ("autoconnect".into(), owned(Value::from(false))),
    ]);
    let mut bluetooth = HashMap::from([(
        "type".into(),
        owned(Value::from(if address.is_some() { "panu" } else { "nap" })),
    )]);
    if let Some(address) = address {
        super::types::device_path(adapter, address)?;
        let bytes: Vec<u8> = address
            .split(':')
            .map(|p| u8::from_str_radix(p, 16).unwrap())
            .collect();
        bluetooth.insert("bdaddr".into(), owned(Value::from(bytes)));
    } else {
        // Keep bridges separate when more than one local adapter is present.
        let interface = format!("inirbtnap{}", &adapter[3..]);
        if interface.len() > 15 {
            return Err("Bluetooth adapter index is too long".into());
        }
        connection.insert("interface-name".into(), owned(Value::from(interface)));
    }
    let role = address
        .map(|a| format!("panu:{adapter}:{}", a.to_ascii_uppercase()))
        .unwrap_or_else(|| format!("nap:{adapter}"));
    let mut settings = HashMap::from([
        ("connection".into(), connection),
        ("bluetooth".into(), bluetooth),
        (
            "ipv4".into(),
            HashMap::from([(
                "method".into(),
                owned(Value::from(if address.is_some() {
                    "auto"
                } else {
                    "shared"
                })),
            )]),
        ),
        (
            "ipv6".into(),
            HashMap::from([(
                "method".into(),
                owned(Value::from(if address.is_some() {
                    "auto"
                } else {
                    "disabled"
                })),
            )]),
        ),
        (
            "user".into(),
            HashMap::from([(
                "data".into(),
                owned(Value::from(HashMap::from([(
                    "inir.bluetooth.role".to_string(),
                    role,
                )]))),
            )]),
        ),
    ]);
    if address.is_none() {
        settings.insert(
            "bridge".into(),
            HashMap::from([("stp".into(), owned(Value::from(false)))]),
        );
    }
    Ok(settings)
}

pub async fn active(bus: &Connection) -> Result<Vec<Network>, String> {
    let manager = Proxy::new(bus, NM, ROOT, NM)
        .await
        .map_err(|e| e.to_string())?;
    let paths: Vec<OwnedObjectPath> = manager
        .get_property("ActiveConnections")
        .await
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for path in paths {
        let props = properties(
            bus,
            NM,
            path.as_str(),
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        .await
        .map_err(|e| e.to_string())?;
        if read::<String>(&props, "Type") == "bluetooth" {
            result.push(Network {
                id: path.to_string(),
                uuid: read(&props, "Uuid"),
                name: read(&props, "Id"),
                state: read(&props, "State"),
            });
        }
    }
    Ok(result)
}

pub async fn activate(
    bus: &Connection,
    adapter: &str,
    address: Option<&str>,
) -> Result<(), String> {
    let mut wanted = profile(address, adapter)?;
    if address.is_none() {
        // For a NAP server bdaddr selects the *local* adapter, unlike PANU.
        let props = properties(
            bus,
            "org.bluez",
            &adapter_path(adapter)?,
            "org.bluez.Adapter1",
        )
        .await
        .map_err(|e| e.to_string())?;
        let local: String = read(&props, "Address");
        super::types::device_path(adapter, &local)?;
        let bytes: Vec<u8> = local
            .split(':')
            .map(|p| u8::from_str_radix(p, 16).unwrap())
            .collect();
        wanted
            .get_mut("bluetooth")
            .unwrap()
            .insert("bdaddr".into(), owned(Value::from(bytes)));
    }
    let settings = Proxy::new(
        bus,
        NM,
        "/org/freedesktop/NetworkManager/Settings",
        "org.freedesktop.NetworkManager.Settings",
    )
    .await
    .map_err(|e| e.to_string())?;
    let paths: Vec<OwnedObjectPath> = settings
        .call("ListConnections", &())
        .await
        .map_err(|e| e.to_string())?;
    let wanted_role: HashMap<String, String> = wanted["user"]["data"]
        .try_clone()
        .unwrap()
        .try_into()
        .unwrap();
    let mut selected = None;
    for path in paths {
        let proxy = Proxy::new(
            bus,
            NM,
            path.as_str(),
            "org.freedesktop.NetworkManager.Settings.Connection",
        )
        .await
        .map_err(|e| e.to_string())?;
        let existing: Settings = proxy
            .call("GetSettings", &())
            .await
            .map_err(|e| e.to_string())?;
        let role: Option<HashMap<String, String>> = existing
            .get("user")
            .and_then(|v| v.get("data"))
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| v.try_into().ok());
        let same_owner = role.as_ref().is_some_and(|v| {
            v.get("inir.bluetooth.role") == wanted_role.get("inir.bluetooth.role")
        });
        let compatible = address.is_some()
            && existing.get("bluetooth").is_some_and(|b| {
                read::<String>(b, "type") == "panu"
                    && b.get("bdaddr")
                        .and_then(|v| v.try_clone().ok())
                        .and_then(|v| Vec::<u8>::try_from(v).ok())
                        == wanted["bluetooth"]
                            .get("bdaddr")
                            .and_then(|v| v.try_clone().ok())
                            .and_then(|v| Vec::<u8>::try_from(v).ok())
            });
        if same_owner || compatible {
            selected = Some(path);
            break;
        }
    }
    let selected = match selected {
        Some(path) => path,
        None => {
            let uuid = tokio::fs::read_to_string("/proc/sys/kernel/random/uuid")
                .await
                .map_err(|e| e.to_string())?;
            wanted
                .get_mut("connection")
                .unwrap()
                .insert("uuid".into(), owned(Value::from(uuid.trim())));
            settings
                .call::<_, _, OwnedObjectPath>("AddConnection", &(wanted,))
                .await
                .map_err(|e| e.to_string())?
        }
    };
    let manager = Proxy::new(bus, NM, ROOT, NM)
        .await
        .map_err(|e| e.to_string())?;
    let _: OwnedObjectPath = manager
        .call(
            "ActivateConnection",
            &(
                selected,
                OwnedObjectPath::try_from("/").unwrap(),
                OwnedObjectPath::try_from("/").unwrap(),
            ),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn execute(
    bus: &Connection,
    operation: &str,
    args: &serde_json::Value,
) -> Result<(), String> {
    match operation {
        "connectTether" => {
            activate(bus, text(args, "adapter")?, Some(text(args, "address")?)).await
        }
        "startSharing" => activate(bus, text(args, "adapter")?, None).await,
        "disconnectTether" => {
            let path = text(args, "connection")?;
            if !path.starts_with("/org/freedesktop/NetworkManager/ActiveConnection/") {
                return Err("Invalid active connection".into());
            }
            let props = properties(
                bus,
                NM,
                path,
                "org.freedesktop.NetworkManager.Connection.Active",
            )
            .await
            .map_err(|e| e.to_string())?;
            if read::<String>(&props, "Type") != "bluetooth" {
                return Err("Only Bluetooth internet connections can be disconnected here".into());
            }
            let manager = Proxy::new(bus, NM, ROOT, NM)
                .await
                .map_err(|e| e.to_string())?;
            manager
                .call::<_, _, ()>(
                    "DeactivateConnection",
                    &(OwnedObjectPath::try_from(path).map_err(|e| e.to_string())?,),
                )
                .await
                .map_err(|e| e.to_string())
        }
        _ => Err("Unknown Bluetooth network operation".into()),
    }
}
