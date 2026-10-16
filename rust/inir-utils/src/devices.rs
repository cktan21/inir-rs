use regex::Regex;
use serde_json::{json, Value};
use std::{fs, path::Path, sync::LazyLock};

fn read(path: &Path, name: &str) -> String {
    fs::read(path.join(name))
        .map(|s| String::from_utf8_lossy(&s).trim().to_owned())
        .unwrap_or_default()
}
fn expression(pattern: &str, value: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(value)
}
fn vendor_name(vendor: &str) -> String {
    static NOISE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)[,.]?\s*(co\.?|ltd\.?|inc\.?|corp(oration)?\.?|llc|gmbh|technology|technologies|electronics?|semiconductor|international|computer|limited)\b").unwrap()
    });
    let mut cleaned = vendor.to_owned();
    for _ in 0..3 {
        cleaned = NOISE
            .replace_all(&cleaned, "")
            .trim_matches([' ', ',', '.', '-'])
            .to_owned();
    }
    if cleaned.is_empty() {
        vendor.into()
    } else {
        cleaned
    }
}

pub fn kind(interfaces: &[String], device_class: &str, name: &str) -> Option<&'static str> {
    let classes: Vec<_> = if interfaces.is_empty() {
        vec![device_class]
    } else {
        interfaces.iter().filter_map(|i| i.get(..2)).collect()
    };
    if classes.contains(&"09") && classes.iter().all(|c| *c == "09") {
        return None;
    }
    for interface in interfaces {
        if interface.starts_with("03") && interface.ends_with("02") {
            return Some("mouse");
        }
        if interface.starts_with("03") && interface.ends_with("01") {
            return Some("keyboard");
        }
    }
    let pad = expression(
        r"(?i)pad|controller|joystick|dualsense|dualshock|xbox|8bitdo|joy-?con",
        name,
    );
    if interfaces
        .iter()
        .any(|i| ["ff5d", "ff47", "ff58"].iter().any(|p| i.starts_with(p)))
        || classes.contains(&"03") && pad
    {
        return Some("controller");
    }
    if classes.contains(&"08") {
        return None;
    }
    if classes.contains(&"0e") {
        return Some("webcam");
    }
    if classes.contains(&"01") {
        return Some(
            if expression(r"(?i)head(set|phone)|buds|earphone|airpods", name) {
                "headset"
            } else {
                "audio"
            },
        );
    }
    if classes.contains(&"06")
        || interfaces.iter().any(|i| i.starts_with("ff42"))
        || classes.contains(&"e0") && (classes.contains(&"0a") || classes.contains(&"02"))
    {
        return Some("phone");
    }
    if classes.contains(&"e0") {
        return Some("bluetooth");
    }
    if classes.contains(&"07") {
        return Some("printer");
    }
    if classes.contains(&"0b") {
        return Some("smartcard");
    }
    if classes.contains(&"02") || classes.contains(&"0a") {
        return Some("serial");
    }
    if classes.contains(&"03") {
        return Some(if pad { "controller" } else { "input" });
    }
    Some("device")
}

/// The monitor fd participates in the existing Tokio reactor. No subprocess,
/// polling timer, dedicated runtime or idle watcher thread is needed.
pub async fn watch(
    stop: &tokio_util::sync::CancellationToken,
    mut changed: impl FnMut(&'static str),
) -> Result<(), String> {
    let socket = udev::MonitorBuilder::new()
        .and_then(|builder| builder.match_subsystem_devtype("usb", "usb_device"))
        .and_then(|builder| builder.match_subsystem("block"))
        .and_then(|builder| builder.listen())
        .map_err(|error| format!("Cannot monitor device events: {error}"))?;
    let mut socket = tokio::io::unix::AsyncFd::new(socket).map_err(|error| error.to_string())?;
    changed("ready");
    loop {
        let mut readable = tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            ready = socket.readable_mut() => ready.map_err(|error| error.to_string())?,
        };
        // libudev sockets are nonblocking. Drain all queued events, then clear
        // readiness so an empty socket cannot spin the reactor.
        for event in readable.get_inner().iter() {
            if !matches!(
                event.event_type(),
                udev::EventType::Add | udev::EventType::Remove
            ) {
                continue;
            }
            match event.subsystem().and_then(|value| value.to_str()) {
                Some("usb") => changed("usb"),
                Some("block") => changed("block"),
                _ => {}
            }
        }
        readable.clear_ready();
    }
}

pub fn snapshot(root: &Path) -> Result<Value, String> {
    let mut entries: Vec<_> = match fs::read_dir(root) {
        Ok(entries) => entries.filter_map(Result::ok).collect(),
        Err(_) => return Ok(json!({"devices": []})),
    };
    entries.sort_by_key(|e| e.file_name());
    let mut devices = Vec::new();
    for entry in entries {
        let id = entry.file_name().to_string_lossy().into_owned();
        if id.contains(':') || id.starts_with("usb") {
            continue;
        }
        let path = entry.path();
        let vendor_id = read(&path, "idVendor");
        if vendor_id.is_empty() {
            continue;
        }
        let product_id = read(&path, "idProduct");
        let mut interfaces = Vec::new();
        if let Ok(children) = fs::read_dir(&path) {
            let mut children: Vec<_> = children.filter_map(Result::ok).collect();
            children.sort_by_key(|e| e.file_name());
            for child in children {
                if child
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{id}:"))
                {
                    let interface = [
                        "bInterfaceClass",
                        "bInterfaceSubClass",
                        "bInterfaceProtocol",
                    ]
                    .map(|p| read(&child.path(), p))
                    .concat()
                    .to_lowercase();
                    if interface.len() == 6 && interface.is_ascii() {
                        interfaces.push(interface);
                    }
                }
            }
        }
        let manufacturer = read(&path, "manufacturer");
        let product = read(&path, "product");
        let Some(kind) = kind(
            &interfaces,
            &read(&path, "bDeviceClass").to_lowercase(),
            &product,
        ) else {
            continue;
        };
        let vendor = vendor_name(&manufacturer);
        let junk = expression(
            r"(?i)demo|^uac\d*\b|^usb(\s*(device|audio|composite device))?$|^[0-9a-f]{4}$",
            &product,
        );
        let generic = expression(
            r"(?i)^(usb|gaming|wireless|2\.4g|generic|hid)\b|receiver|device|composite",
            &product,
        );
        let model = (4..=14).contains(&product.len())
            && product
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            && product.bytes().any(|c| c.is_ascii_digit())
            && product.bytes().any(|c| c.is_ascii_alphabetic());
        let name = if product.is_empty() || !vendor.is_empty() && junk {
            if vendor.is_empty() {
                product
            } else {
                vendor
            }
        } else if !vendor.is_empty()
            && (generic || model)
            && !product.to_lowercase().contains(&vendor.to_lowercase())
        {
            format!("{vendor} {product}")
        } else {
            product
        };
        devices.push(json!({"id": format!("{id}/{vendor_id}:{product_id}"), "name": name.split_whitespace().collect::<Vec<_>>().join(" "), "kind": kind}));
    }
    Ok(json!({"devices": devices}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classification_preserves_interface_precedence() {
        let list = |values: &[&str]| values.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            kind(&list(&["030101", "030102"]), "00", "receiver"),
            Some("keyboard")
        );
        assert_eq!(kind(&list(&["090000"]), "09", "hub"), None);
        assert_eq!(kind(&list(&["080650"]), "00", "drive"), None);
        assert_eq!(
            kind(&list(&["010100"]), "00", "Headphones"),
            Some("headset")
        );
        assert_eq!(
            kind(&list(&["e00101", "020600"]), "00", "phone"),
            Some("phone")
        );
    }
}
