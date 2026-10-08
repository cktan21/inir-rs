use inir_types::{ResourceState, SystemInfo};
use std::{collections::HashMap, fs, io};

pub fn parse_os_release(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            if key.starts_with('#') || key.is_empty() {
                return None;
            }
            let value = value.trim();
            let value = if value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')))
            {
                &value[1..value.len() - 1]
            } else {
                value
            };
            Some((
                key.to_owned(),
                value.replace("\\\"", "\"").replace("\\\\", "\\"),
            ))
        })
        .collect()
}

pub fn identity() -> SystemInfo {
    let text = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let fields = parse_os_release(&text);
    let get = |key: &str| fields.get(key).cloned().unwrap_or_default();
    let distro_id = get("ID");
    let distro_icon = if text.to_lowercase().contains("nyarch") {
        "nyarch-symbolic"
    } else {
        match distro_id.as_str() {
            "arch" => "arch-symbolic",
            "endeavouros" => "endeavouros-symbolic",
            "cachyos" => "cachyos-symbolic",
            "nixos" => "nixos-symbolic",
            "fedora" => "fedora-symbolic",
            "linuxmint" | "ubuntu" | "zorin" | "popos" => "ubuntu-symbolic",
            "debian" | "raspbian" | "kali" => "debian-symbolic",
            "funtoo" | "gentoo" => "gentoo-symbolic",
            _ => "linux-symbolic",
        }
    }
    .to_owned();
    let (username, display_name) = user_identity();
    let distro_name = fields
        .get("PRETTY_NAME")
        .or_else(|| fields.get("NAME"))
        .cloned()
        .unwrap_or_else(|| "Unknown".into());
    let logo = get("LOGO");
    SystemInfo {
        hostname: fs::read_to_string("/proc/sys/kernel/hostname")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        username,
        display_name,
        distro_name,
        distro_id,
        distro_icon: distro_icon.clone(),
        home_url: get("HOME_URL"),
        documentation_url: get("DOCUMENTATION_URL"),
        support_url: get("SUPPORT_URL"),
        bug_report_url: get("BUG_REPORT_URL"),
        privacy_policy_url: get("PRIVACY_POLICY_URL"),
        logo: if logo.is_empty() { distro_icon } else { logo },
        desktop_environment: std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        windowing_system: if std::env::var("WAYLAND_DISPLAY")
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            "X11"
        } else {
            "Wayland"
        }
        .into(),
    }
}

fn user_identity() -> (String, String) {
    // Reentrant NSS lookup preserves getent's behavior, including non-local users.
    // The backing buffer stays alive until both C strings have been copied.
    unsafe {
        let mut size = 16384;
        loop {
            let mut buffer = vec![0u8; size];
            let mut entry: libc::passwd = std::mem::zeroed();
            let mut result = std::ptr::null_mut();
            let status = libc::getpwuid_r(
                libc::getuid(),
                &mut entry,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            );
            if status == libc::ERANGE && size < 1_048_576 {
                size *= 2;
                continue;
            }
            if status == 0 && !result.is_null() && !entry.pw_name.is_null() {
                let username = std::ffi::CStr::from_ptr(entry.pw_name)
                    .to_string_lossy()
                    .into_owned();
                let display = if entry.pw_gecos.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(entry.pw_gecos)
                        .to_string_lossy()
                        .split(',')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_owned()
                };
                return (
                    username.clone(),
                    if display.is_empty() {
                        username
                    } else {
                        display
                    },
                );
            }
            break;
        }
    }
    let username = std::env::var("USER").unwrap_or_else(|_| "user".into());
    (username.clone(), username)
}

#[derive(Debug, Default)]
pub struct Sampler {
    previous_cpu: Option<(u64, u64)>,
}

impl Sampler {
    pub fn sample(&mut self) -> io::Result<ResourceState> {
        self.parse(
            &fs::read_to_string("/proc/uptime")?,
            &fs::read_to_string("/proc/meminfo")?,
            &fs::read_to_string("/proc/stat")?,
        )
    }

    pub fn parse(&mut self, uptime: &str, meminfo: &str, stat: &str) -> io::Result<ResourceState> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid procfs sample");
        let uptime = uptime
            .split_whitespace()
            .next()
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())?;
        let fields: HashMap<_, _> = meminfo
            .lines()
            .filter_map(|line| {
                let (name, rest) = line.split_once(':')?;
                Some((
                    name,
                    rest.split_whitespace()
                        .next()?
                        .parse::<u64>()
                        .ok()?
                        .saturating_mul(1024),
                ))
            })
            .collect();
        let cpu: Vec<u64> = stat
            .lines()
            .find(|line| line.starts_with("cpu "))
            .ok_or_else(invalid)?
            .split_whitespace()
            .skip(1)
            .take(8)
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| invalid())?;
        if cpu.len() < 4 {
            return Err(invalid());
        }
        // guest and guest_nice are already included in user/nice; do not double-count.
        let total: u64 = cpu.iter().sum();
        let idle = cpu[3] + cpu.get(4).copied().unwrap_or(0);
        let cpu_usage = self.previous_cpu.and_then(|(old_total, old_idle)| {
            let elapsed = total.checked_sub(old_total)?;
            let idle = idle.checked_sub(old_idle)?;
            (elapsed > 0).then(|| (1.0 - idle as f64 / elapsed as f64).clamp(0.0, 1.0))
        });
        self.previous_cpu = Some((total, idle));
        let memory_total = *fields.get("MemTotal").ok_or_else(invalid)?;
        let memory_available = fields
            .get("MemAvailable")
            .copied()
            .unwrap_or_else(|| {
                fields.get("MemFree").copied().unwrap_or(0)
                    + fields.get("Buffers").copied().unwrap_or(0)
                    + fields.get("Cached").copied().unwrap_or(0)
            })
            .min(memory_total);
        Ok(ResourceState {
            uptime,
            memory_total,
            memory_available,
            swap_total: fields.get("SwapTotal").copied().unwrap_or(0),
            swap_free: fields.get("SwapFree").copied().unwrap_or(0),
            cpu_usage,
        })
    }
}
