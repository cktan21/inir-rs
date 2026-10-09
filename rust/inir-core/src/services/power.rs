use super::dbus::{self, read};
use inir_types::desktop::{PowerState, ServiceStatus};
use zbus::{zvariant::Value, Connection};

pub const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILE_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const OLD_PROFILES: &str = "net.hadess.PowerProfiles";
const OLD_PATH: &str = "/net/hadess/PowerProfiles";

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
