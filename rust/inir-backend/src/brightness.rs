//! Brightness contract — the surface `services/display/Brightness.qml` exposes.
//!
//! Per-monitor objects stay on the QML side for now: `monitors` is a list of
//! `BrightnessMonitor` instances, and moving it needs the incremental-model work
//! that Gate 1 of docs/plans/RUST_BACKEND_MIGRATION.md calls for. What is
//! contracted here is the shell-wide state and the stepping/sleep entry points.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        /// Backlight and DDC state shared across outputs.
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(QString, backlight_device)]
        #[qproperty(bool, backlight_detection_ready)]
        #[qproperty(bool, asleep)]
        type BrightnessService = super::BrightnessServiceRust;

        /// Raised after any output's brightness settles, for surfaces that mirror it.
        #[qsignal]
        fn brightness_changed(self: Pin<&mut BrightnessService>);

        #[qinvokable]
        fn increase_brightness(self: Pin<&mut BrightnessService>);

        #[qinvokable]
        fn decrease_brightness(self: Pin<&mut BrightnessService>);

        /// Blanks outputs for sleep, remembering what to restore.
        #[qinvokable]
        fn sleep_begin(self: Pin<&mut BrightnessService>);

        /// Puts the pre-sleep brightness back once outputs are live again.
        #[qinvokable]
        fn restore_after_wake(self: Pin<&mut BrightnessService>);
    }
}

use core::pin::Pin;
use cxx_qt_lib::QString;

/// Contract-only state; a real implementation drives sysfs backlights and DDC.
#[derive(Default)]
pub struct BrightnessServiceRust {
    backlight_device: QString,
    backlight_detection_ready: bool,
    asleep: bool,
}

impl qobject::BrightnessService {
    pub fn increase_brightness(mut self: Pin<&mut Self>) {
        self.as_mut().brightness_changed();
    }

    pub fn decrease_brightness(mut self: Pin<&mut Self>) {
        self.as_mut().brightness_changed();
    }

    pub fn sleep_begin(mut self: Pin<&mut Self>) {
        self.as_mut().set_asleep(true);
    }

    pub fn restore_after_wake(mut self: Pin<&mut Self>) {
        self.as_mut().set_asleep(false);
        self.as_mut().brightness_changed();
    }
}
