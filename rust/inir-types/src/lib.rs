//! Qt-independent state and configuration shared by the backend and its clients.
pub mod config;
pub mod desktop;
pub mod schema;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub hostname: String,
    pub username: String,
    pub display_name: String,
    pub distro_name: String,
    pub distro_id: String,
    pub distro_icon: String,
    pub home_url: String,
    pub documentation_url: String,
    pub support_url: String,
    pub bug_report_url: String,
    pub privacy_policy_url: String,
    pub logo: String,
    pub desktop_environment: String,
    pub windowing_system: String,
}

/// Resource sizes use bytes in the core; compatibility adapters convert units.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceState {
    pub uptime: f64,
    pub memory_total: u64,
    pub memory_available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    /// Fraction in [0, 1]. None until two CPU samples are available.
    pub cpu_usage: Option<f64>,
}
