use super::dbus::{self, read, Objects};
use inir_types::desktop::{
    BluetoothAdapter, BluetoothDevice, BluetoothState, Command, ServiceStatus,
};
use zbus::{
    zvariant::{OwnedObjectPath, Value},
    Connection,
};

pub const SERVICE: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";

pub async fn snapshot(bus: &Connection) -> zbus::Result<BluetoothState> {
    let objects: Objects = dbus::proxy(bus, SERVICE, "/", "org.freedesktop.DBus.ObjectManager")
        .await?
        .call("GetManagedObjects", &())
        .await?;
    Ok(from_objects(objects))
}

pub fn from_objects(objects: Objects) -> BluetoothState {
    let mut state = BluetoothState {
        status: ServiceStatus {
            ready: true,
            error: String::new(),
        },
        ..Default::default()
    };
    for (path, interfaces) in objects {
        if let Some(p) = interfaces.get(ADAPTER) {
            state.adapters.push(BluetoothAdapter {
                id: path.to_string(),
                address: read(p, "Address"),
                name: read(p, "Alias"),
                powered: read(p, "Powered"),
                discovering: read(p, "Discovering"),
            });
        }
        if let Some(p) = interfaces.get(DEVICE) {
            let battery = interfaces
                .get("org.bluez.Battery1")
                .map(|p| read::<u8>(p, "Percentage") as f64 / 100.0);
            state.devices.push(BluetoothDevice {
                id: path.to_string(),
                adapter: read::<OwnedObjectPath>(p, "Adapter").to_string(),
                address: read(p, "Address"),
                name: read(p, "Alias"),
                icon: read(p, "Icon"),
                paired: read(p, "Paired"),
                trusted: read(p, "Trusted"),
                connected: read(p, "Connected"),
                battery,
            });
        }
    }
    state.adapters.sort_by(|a, b| a.id.cmp(&b.id));
    state
        .devices
        .sort_by(|a, b| b.connected.cmp(&a.connected).then(a.id.cmp(&b.id)));
    state
}

pub async fn execute(bus: &Connection, command: Command) -> Result<(), String> {
    let result: zbus::Result<()> = async {
        match command {
            Command::BluetoothEnabled { adapter, enabled } => {
                dbus::set(
                    bus,
                    SERVICE,
                    &adapter,
                    ADAPTER,
                    "Powered",
                    Value::from(enabled),
                )
                .await?
            }
            Command::BluetoothDiscovery { adapter, enabled } => {
                dbus::proxy(bus, SERVICE, &adapter, ADAPTER)
                    .await?
                    .call::<_, _, ()>(
                        if enabled {
                            "StartDiscovery"
                        } else {
                            "StopDiscovery"
                        },
                        &(),
                    )
                    .await?
            }
            Command::BluetoothConnect { device } => {
                dbus::proxy(bus, SERVICE, &device, DEVICE)
                    .await?
                    .call::<_, _, ()>("Connect", &())
                    .await?
            }
            Command::BluetoothDisconnect { device } => {
                dbus::proxy(bus, SERVICE, &device, DEVICE)
                    .await?
                    .call::<_, _, ()>("Disconnect", &())
                    .await?
            }
            // Interactive pairing is delegated to an existing BlueZ agent. A
            // native Agent1 implementation is required before full UI cutover.
            Command::BluetoothPair { device } => {
                dbus::proxy(bus, SERVICE, &device, DEVICE)
                    .await?
                    .call::<_, _, ()>("Pair", &())
                    .await?
            }
            Command::BluetoothForget { adapter, device } => {
                dbus::proxy(bus, SERVICE, &adapter, ADAPTER)
                    .await?
                    .call::<_, _, ()>(
                        "RemoveDevice",
                        &(OwnedObjectPath::try_from(device.as_str())?,),
                    )
                    .await?
            }
            _ => return Err(zbus::Error::Failure("invalid Bluetooth command".into())),
        }
        Ok(())
    }
    .await;
    result.map_err(|e| e.to_string())
}
