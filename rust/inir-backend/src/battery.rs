//! Battery contract — the surface `services/power/Battery.qml` exposes.
//!
//! Battery is pure observed state: it has no root-level invokables, and the
//! charge-limit thresholds it reports are read from config rather than set here.
//! That makes it the cheapest domain to move natively, since there is no command
//! surface to preserve — only UPower polling and the derived flags below.

#[cxx_qt::bridge]
pub mod qobject {
    extern "RustQt" {
        /// UPower display-device state plus the thresholds the shell derives from it.
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, available)]
        #[qproperty(bool, is_charging)]
        #[qproperty(bool, is_plugged_in)]
        #[qproperty(bool, on_battery)]
        #[qproperty(f64, percentage)]
        #[qproperty(bool, is_low)]
        #[qproperty(bool, is_critical)]
        #[qproperty(bool, is_full)]
        #[qproperty(f64, energy_rate)]
        #[qproperty(f64, time_to_empty)]
        #[qproperty(f64, time_to_full)]
        #[qproperty(bool, charge_limit_enabled)]
        #[qproperty(i32, charge_limit_threshold)]
        #[qproperty(bool, charge_limit_supported)]
        #[qproperty(bool, charge_limit_active)]
        #[qproperty(i32, current_charge_limit)]
        type BatteryService = super::BatteryServiceRust;
    }
}

/// Contract-only state; a real implementation subscribes to UPower over D-Bus.
pub struct BatteryServiceRust {
    available: bool,
    is_charging: bool,
    is_plugged_in: bool,
    on_battery: bool,
    /// 0.0–1.0, matching the QML service rather than a 0–100 percentage.
    percentage: f64,
    is_low: bool,
    is_critical: bool,
    is_full: bool,
    energy_rate: f64,
    time_to_empty: f64,
    time_to_full: f64,
    charge_limit_enabled: bool,
    charge_limit_threshold: i32,
    charge_limit_supported: bool,
    charge_limit_active: bool,
    current_charge_limit: i32,
}

impl Default for BatteryServiceRust {
    fn default() -> Self {
        Self {
            available: false,
            is_charging: false,
            is_plugged_in: false,
            on_battery: false,
            percentage: 1.0,
            is_low: false,
            is_critical: false,
            is_full: false,
            energy_rate: 0.0,
            time_to_empty: 0.0,
            time_to_full: 0.0,
            charge_limit_enabled: false,
            charge_limit_threshold: 80,
            charge_limit_supported: false,
            charge_limit_active: false,
            current_charge_limit: -1,
        }
    }
}
