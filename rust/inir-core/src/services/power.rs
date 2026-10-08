use super::dbus::{self, read};
use inir_types::desktop::{BatteryState, PowerState, ServiceStatus};
use zbus::{
    zvariant::{OwnedObjectPath, Value},
    Connection,
};

pub const UPOWER: &str = "org.freedesktop.UPower";
pub const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILE_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const OLD_PROFILES: &str = "net.hadess.PowerProfiles";
const OLD_PATH: &str = "/net/hadess/PowerProfiles";

pub async fn battery(bus: &Connection) -> zbus::Result<BatteryState> {
    let root = dbus::proxy(bus, UPOWER, "/org/freedesktop/UPower", UPOWER).await?;
    let path: OwnedObjectPath = root.call("GetDisplayDevice", &()).await?;
    let p = dbus::properties(bus, UPOWER, path.as_str(), "org.freedesktop.UPower.Device").await?;
    let root = dbus::properties(bus, UPOWER, "/org/freedesktop/UPower", UPOWER).await?;
    Ok(BatteryState {
        status: ServiceStatus {
            ready: true,
            error: String::new(),
        },
        available: read::<u32>(&p, "Type") == 2 && read(&p, "IsPresent"),
        on_battery: read(&root, "OnBattery"),
        state: read(&p, "State"),
        percentage: (read::<f64>(&p, "Percentage") / 100.0).clamp(0.0, 1.0),
        energy_rate: read(&p, "EnergyRate"),
        time_to_empty: read(&p, "TimeToEmpty"),
        time_to_full: read(&p, "TimeToFull"),
    })
}

async fn endpoint(bus: &Connection) -> zbus::Result<(&'static str, &'static str)> {
    if dbus::properties(bus, PROFILES, PROFILE_PATH, PROFILES)
        .await
        .is_ok()
    {
        Ok((PROFILES, PROFILE_PATH))
    } else {
        dbus::properties(bus, OLD_PROFILES, OLD_PATH, OLD_PROFILES).await?;
        Ok((OLD_PROFILES, OLD_PATH))
    }
}

pub async fn profiles(bus: &Connection) -> zbus::Result<PowerState> {
    let (service, path) = endpoint(bus).await?;
    let p = dbus::properties(bus, service, path, service).await?;
    Ok(PowerState {
        status: ServiceStatus {
            ready: true,
            error: String::new(),
        },
        active_profile: read(&p, "ActiveProfile"),
        profiles: read::<Vec<dbus::Properties>>(&p, "Profiles")
            .iter()
            .map(|p| read(p, "Profile"))
            .collect(),
        degraded: read(&p, "PerformanceDegraded"),
    })
}

pub async fn set_profile(bus: &Connection, profile: &str) -> Result<(), String> {
    if !profiles(bus)
        .await
        .map_err(|e| e.to_string())?
        .profiles
        .iter()
        .any(|p| p == profile)
    {
        return Err("unsupported power profile".into());
    }
    let (service, path) = endpoint(bus).await.map_err(|e| e.to_string())?;
    dbus::set(
        bus,
        service,
        path,
        service,
        "ActiveProfile",
        Value::from(profile),
    )
    .await
    .map_err(|e| e.to_string())
}
