//! Audio contract — the surface `services/media/Audio.qml` exposes today.
//!
//! Member names here are the snake_case spelling of the QML ones; CXX-Qt emits
//! the camelCase Qt names that QML consumers already bind to, so a layout keeps
//! reading `Audio.value` and `Audio.toggleMute()` whichever side implements it.
//! `scripts/test-rust-contract.py` fails if the two drift apart.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        /// Volume, mute and microphone state for the default PipeWire nodes.
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, ready)]
        #[qproperty(f64, value)]
        #[qproperty(f64, ceiling)]
        #[qproperty(f64, hard_max_value)]
        #[qproperty(bool, mic_muted)]
        #[qproperty(f64, mic_volume)]
        #[qproperty(bool, mic_being_accessed)]
        #[qproperty(QString, audio_theme)]
        type AudioService = super::AudioServiceRust;

        /// Raised when a volume change is clamped or refused by the protection policy.
        #[qsignal]
        fn sink_protection_triggered(self: Pin<&mut AudioService>, reason: QString);

        #[qinvokable]
        fn toggle_mute(self: Pin<&mut AudioService>);

        #[qinvokable]
        fn toggle_mic_mute(self: Pin<&mut AudioService>);

        #[qinvokable]
        fn set_sink_volume(self: Pin<&mut AudioService>, target: f64);

        #[qinvokable]
        fn set_source_volume(self: Pin<&mut AudioService>, target: f64);

        #[qinvokable]
        fn increment_volume(self: Pin<&mut AudioService>);

        #[qinvokable]
        fn decrement_volume(self: Pin<&mut AudioService>);
    }
}

use core::pin::Pin;
use cxx_qt_lib::QString;

/// Contract-only state. A real implementation replaces these fields with a
/// PipeWire client; the QML-visible surface above is what must not change.
pub struct AudioServiceRust {
    ready: bool,
    value: f64,
    ceiling: f64,
    hard_max_value: f64,
    mic_muted: bool,
    mic_volume: f64,
    mic_being_accessed: bool,
    audio_theme: QString,
}

impl Default for AudioServiceRust {
    fn default() -> Self {
        Self {
            ready: false,
            value: 0.0,
            ceiling: 1.0,
            // Mirrors Audio.qml's own hard ceiling of 200%.
            hard_max_value: 2.00,
            mic_muted: false,
            mic_volume: 0.0,
            mic_being_accessed: false,
            audio_theme: QString::from("freedesktop"),
        }
    }
}

impl qobject::AudioService {
    /// Clamps to the protection ceiling the same way the QML service does, so a
    /// caller cannot push past it by going through the native path.
    fn clamp(&self, target: f64) -> f64 {
        target.clamp(0.0, self.ceiling().min(*self.hard_max_value()))
    }

    pub fn toggle_mute(self: Pin<&mut Self>) {
        let muted = *self.value() == 0.0;
        self.set_value(if muted { 1.0 } else { 0.0 });
    }

    pub fn toggle_mic_mute(mut self: Pin<&mut Self>) {
        let next = !*self.mic_muted();
        self.as_mut().set_mic_muted(next);
    }

    pub fn set_sink_volume(mut self: Pin<&mut Self>, target: f64) {
        let clamped = self.clamp(target);
        if clamped < target {
            self.as_mut()
                .sink_protection_triggered(QString::from("ceiling"));
        }
        self.as_mut().set_value(clamped);
    }

    pub fn set_source_volume(mut self: Pin<&mut Self>, target: f64) {
        self.as_mut().set_mic_volume(target.clamp(0.0, 1.0));
    }

    pub fn increment_volume(mut self: Pin<&mut Self>) {
        let next = *self.value() + 0.05;
        self.as_mut().set_sink_volume(next);
    }

    pub fn decrement_volume(mut self: Pin<&mut Self>) {
        let next = *self.value() - 0.05;
        self.as_mut().set_sink_volume(next);
    }
}
