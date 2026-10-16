use serde_json::{json, Map, Value};
use std::{ffi::CString, fs};

pub fn memory_maps(pid: u32) -> Result<Value, String> {
    let text = fs::read_to_string(format!("/proc/{pid}/maps")).map_err(|e| e.to_string())?;
    let total = text.lines().filter(|l| l.contains("JSGCHeap")).count();
    let deleted = text
        .lines()
        .filter(|l| l.contains("JSGCHeap") && l.contains("deleted"))
        .count();
    Ok(json!({"total": total, "deleted": deleted}))
}

pub fn network_counters(interfaces: &[String]) -> Result<Value, String> {
    if interfaces.len() > 256 {
        return Err("Too many network interfaces".into());
    }
    let mut values = Map::new();
    for interface in interfaces {
        if interface.is_empty()
            || interface == "."
            || interface == ".."
            || interface.contains('/')
            || interface.contains('\0')
        {
            return Err("Invalid network interface name".into());
        }
        let read = |kind| {
            fs::read_to_string(format!(
                "/sys/class/net/{interface}/statistics/{kind}_bytes"
            ))
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
        };
        if let (Some(rx), Some(tx)) = (read("rx"), read("tx")) {
            values.insert(interface.clone(), json!({"rx": rx, "tx": tx}));
        }
    }
    Ok(Value::Object(values))
}

pub fn disk(path: &str) -> Result<Value, String> {
    let path = CString::new(path).map_err(|_| "Invalid disk path")?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let stats = unsafe { stats.assume_init() };
    let size = stats.f_frsize;
    Ok(json!({"total": stats.f_blocks.saturating_mul(size),
        "used": stats.f_blocks.saturating_sub(stats.f_bfree).saturating_mul(size),
        "available": stats.f_bavail.saturating_mul(size)}))
}
