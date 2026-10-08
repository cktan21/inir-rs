//! Direct D-Bus access with property caches disabled: snapshots are taken after
//! subscribing to signals, so daemon restarts and invalidated properties work.
use std::collections::HashMap;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{proxy::CacheProperties, Connection, Proxy};

pub type Properties = HashMap<String, OwnedValue>;
pub type Objects = HashMap<OwnedObjectPath, HashMap<String, Properties>>;

pub async fn proxy<'a>(
    bus: &'a Connection,
    service: &'a str,
    path: &'a str,
    interface: &'a str,
) -> zbus::Result<Proxy<'a>> {
    Proxy::new(bus, service, path, interface).await
}

pub async fn properties(
    bus: &Connection,
    service: &str,
    path: &str,
    interface: &str,
) -> zbus::Result<Properties> {
    let p = Proxy::new(bus, service, path, "org.freedesktop.DBus.Properties").await?;
    p.call("GetAll", &(interface,)).await
}

pub fn read<T>(properties: &Properties, name: &str) -> T
where
    T: TryFrom<OwnedValue> + Default,
{
    properties
        .get(name)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| T::try_from(v).ok())
        .unwrap_or_default()
}

pub async fn set(
    bus: &Connection,
    service: &str,
    path: &str,
    interface: &str,
    name: &str,
    value: Value<'_>,
) -> zbus::Result<()> {
    let p = zbus::proxy::Builder::<Proxy>::new(bus)
        .destination(service)?
        .path(path)?
        .interface(interface)?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    p.set_property(name, value).await.map_err(Into::into)
}
