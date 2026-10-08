use inir_types::desktop::{Backlight, BrightnessState, ServiceStatus};
use std::{fs, io, path::Path};

pub fn snapshot(root: &Path) -> io::Result<BrightnessState> {
    let mut devices = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let maximum = number(&path.join("max_brightness"))?;
        if maximum == 0 {
            continue;
        }
        let raw = number(&path.join("brightness"))?.min(maximum);
        devices.push(Backlight {
            id: entry.file_name().to_string_lossy().into_owned(),
            kind: fs::read_to_string(path.join("type"))
                .unwrap_or_default()
                .trim()
                .into(),
            raw,
            maximum,
            value: f64::from(raw) / f64::from(maximum),
        });
    }
    devices.sort_by(|a, b| b.maximum.cmp(&a.maximum).then(a.id.cmp(&b.id)));
    Ok(BrightnessState {
        status: ServiceStatus {
            ready: true,
            error: String::new(),
        },
        devices,
    })
}

fn number(path: &Path) -> io::Result<u32> {
    fs::read_to_string(path)?
        .trim()
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn target(root: &Path, device: &str, value: f64) -> io::Result<(std::path::PathBuf, u32)> {
    if device.is_empty()
        || device.contains('/')
        || device == "."
        || device == ".."
        || !value.is_finite()
        || !(0.0..=1.0).contains(&value)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid backlight device or value",
        ));
    }
    let maximum = number(&root.join(device).join("max_brightness"))?;
    if maximum == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "zero maximum brightness",
        ));
    }
    Ok((
        root.join(device).join("brightness"),
        (value * f64::from(maximum)).round() as u32,
    ))
}

pub async fn set(bus: &zbus::Connection, device: &str, value: f64) -> Result<(), String> {
    let device = device.to_string();
    let (path, raw) =
        target(Path::new("/sys/class/backlight"), &device, value).map_err(|e| e.to_string())?;
    // Direct sysfs when udev grants access; logind handles privilege for the
    // current user's session without constructing or invoking a shell command.
    let result = tokio::task::spawn_blocking(move || fs::write(path, raw.to_string()))
        .await
        .map_err(|e| e.to_string())?;
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            let manager = super::dbus::proxy(
                bus,
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
            )
            .await
            .map_err(|e| e.to_string())?;
            let session: zbus::zvariant::OwnedObjectPath = manager
                .call("GetSessionByPID", &(std::process::id(),))
                .await
                .map_err(|e| e.to_string())?;
            super::dbus::proxy(
                bus,
                "org.freedesktop.login1",
                session.as_str(),
                "org.freedesktop.login1.Session",
            )
            .await
            .map_err(|e| e.to_string())?
            .call::<_, _, ()>("SetBrightness", &("backlight", device.as_str(), raw))
            .await
            .map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}
