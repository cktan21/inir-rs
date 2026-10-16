//! Discover paths/labels only: probing must not read sensor values or wake a GPU.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap_or_default().trim().into()
}
fn children(path: impl AsRef<Path>, prefix: &str, suffix: &str) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with(prefix) && n.ends_with(suffix)
        })
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
}
fn inputs(hwmon: &Path) -> Vec<(PathBuf, String)> {
    children(hwmon, "temp", "_input")
        .into_iter()
        .map(|p| {
            let label = read(
                p.with_file_name(
                    p.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .replace("_input", "_label"),
                ),
            );
            (p, label)
        })
        .collect()
}
fn explicit_cpu(label: &str) -> bool {
    let lower = label.to_lowercase();
    let words: Vec<_> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    !words
        .iter()
        .any(|s| ["vrm", "vr", "soc", "pch"].contains(s))
        && words
            .iter()
            .any(|s| ["cpu", "package", "tdie", "tctl", "processor"].contains(s))
}
fn cpu(hwmon: &Path, name: &str) -> Option<(PathBuf, String)> {
    let values = inputs(hwmon);
    let needles: &[&str] = match name {
        "coretemp" => &["package"],
        "k10temp" | "zenpower" | "fam15h_power" => &["tdie", "tctl"],
        _ => &[],
    };
    for needle in needles {
        if let Some(v) = values
            .iter()
            .find(|(_, l)| l.to_lowercase().contains(needle))
        {
            return Some(v.clone());
        }
    }
    if needles.is_empty() {
        if let Some(v) = values.iter().find(|(_, l)| explicit_cpu(l)) {
            return Some(v.clone());
        }
    }
    values.into_iter().next()
}
fn resolved(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_owned())
        .to_string_lossy()
        .into_owned()
}
pub fn detect(root: &Path) -> BTreeMap<String, String> {
    let root = if root.is_absolute() {
        root.to_owned()
    } else {
        std::env::current_dir().unwrap_or_default().join(root)
    };
    let hwmons = children(root.join("class/hwmon"), "hwmon", "");
    let mut selected_cpu = None;
    for preferred in [
        "coretemp",
        "k10temp",
        "zenpower",
        "fam15h_power",
        "cpu_thermal",
        "via_cputemp",
    ] {
        for hwmon in &hwmons {
            if read(hwmon.join("name")).to_lowercase() == preferred {
                selected_cpu = cpu(hwmon, preferred);
                if selected_cpu.is_some() {
                    break;
                }
            }
        }
        if selected_cpu.is_some() {
            break;
        }
    }
    if selected_cpu.is_none() {
        for zone in children(root.join("class/thermal"), "thermal_zone", "") {
            let kind = read(zone.join("type")).to_lowercase();
            let numbered = kind
                .strip_prefix("cpu")
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
            if zone.join("temp").is_file()
                && (numbered
                    || ["x86_pkg_temp", "cpu", "cpu_thermal", "processor", "package"]
                        .contains(&kind.as_str()))
            {
                selected_cpu = Some((zone.join("temp"), kind));
                break;
            }
        }
    }
    if selected_cpu.is_none() {
        for hwmon in &hwmons {
            if ["nct6775", "it87", "w83627ehf", "lm75", "lm78", "lm85"]
                .contains(&read(hwmon.join("name")).to_lowercase().as_str())
            {
                if let Some(value) = cpu(hwmon, "board").filter(|(_, label)| explicit_cpu(label)) {
                    selected_cpu = Some(value);
                    break;
                }
            }
        }
    }
    let mut result = BTreeMap::new();
    if let Some((path, label)) = selected_cpu {
        result.insert("cpu".into(), resolved(&path));
        if !label.is_empty() {
            result.insert("cpuLabel".into(), label);
        }
    }
    let mut cards = Vec::new();
    for card in children(root.join("class/drm"), "card", "") {
        let name = card.file_name().unwrap().to_string_lossy();
        let Some(number) = name
            .strip_prefix("card")
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let device = card.join("device");
        if !card.is_dir() || !device.exists() {
            continue;
        }
        let vendor = read(device.join("vendor")).to_lowercase();
        let busy = device.join("gpu_busy_percent").is_file();
        cards.push((
            vendor != "0x10de",
            !busy,
            number,
            card,
            resolved(&device),
            vendor,
        ));
    }
    cards.sort();
    if let Some((_, _, _, card, device, vendor)) = cards.into_iter().next() {
        result.insert("gpuDevice".into(), device.clone());
        result.insert("gpuVendor".into(), vendor);
        result.insert("gpuCard".into(), card.to_string_lossy().into_owned());
        let runtime = Path::new(&device).join("power/runtime_status");
        if runtime.exists() {
            result.insert("gpuRuntime".into(), resolved(&runtime));
        }
        for hwmon in hwmons {
            let name = read(hwmon.join("name")).to_lowercase();
            if ![
                "amdgpu", "radeon", "nouveau", "nvidia", "i915", "xe", "panfrost", "lima", "v3d",
                "vc4",
            ]
            .contains(&name.as_str())
                || !hwmon.join("device").exists()
                || resolved(&hwmon.join("device")) != device
            {
                continue;
            }
            let values = inputs(&hwmon);
            let value = values
                .iter()
                .find(|(_, l)| l.to_lowercase().contains("edge"))
                .or_else(|| {
                    values.iter().find(|(_, l)| {
                        ["", "gpu", "gpu core", "core"].contains(&l.trim().to_lowercase().as_str())
                    })
                });
            if let Some((path, label)) = value {
                result.insert("gpu".into(), resolved(path));
                if !label.is_empty() {
                    result.insert("gpuLabel".into(), label.clone());
                }
                break;
            }
        }
    }
    result
}
