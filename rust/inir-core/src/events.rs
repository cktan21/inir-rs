use crate::state::AppState;
use inir_types::desktop::*;
use inir_types::{config::Config, ResourceState, SystemInfo};

#[derive(Debug, Clone)]
pub enum Event {
    System(SystemInfo),
    Resource(ResourceState),
    Config(Config),
    Network(NetworkState),
    Bluetooth(BluetoothState),
    Battery(BatteryState),
    Power(PowerState),
    Brightness(BrightnessState),
    Audio(AudioState),
    Media(MediaState),
    Niri(NiriState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    System,
    Resource,
    Config,
    Desktop,
}

impl AppState {
    /// Return a changed domain only when its value actually changes.
    pub fn reduce(&mut self, event: Event) -> Option<Domain> {
        match event {
            Event::System(next) if self.system != next => {
                self.system = next;
                Some(Domain::System)
            }
            Event::Resource(next) if self.resources != next => {
                self.resources = next;
                Some(Domain::Resource)
            }
            Event::Config(next) if self.config != next => {
                self.config = next;
                Some(Domain::Config)
            }
            Event::Network(next) if self.desktop.network != next => {
                self.desktop.network = next;
                Some(Domain::Desktop)
            }
            Event::Bluetooth(next) if self.desktop.bluetooth != next => {
                self.desktop.bluetooth = next;
                Some(Domain::Desktop)
            }
            Event::Battery(next) if self.desktop.battery != next => {
                self.desktop.battery = next;
                Some(Domain::Desktop)
            }
            Event::Power(next) if self.desktop.power != next => {
                self.desktop.power = next;
                Some(Domain::Desktop)
            }
            Event::Brightness(next) if self.desktop.brightness != next => {
                self.desktop.brightness = next;
                Some(Domain::Desktop)
            }
            Event::Audio(next) if self.desktop.audio != next => {
                self.desktop.audio = next;
                Some(Domain::Desktop)
            }
            Event::Media(next) if self.desktop.media != next => {
                self.desktop.media = next;
                Some(Domain::Desktop)
            }
            Event::Niri(next) if self.desktop.niri != next => {
                self.desktop.niri = next;
                Some(Domain::Desktop)
            }
            _ => None,
        }
    }
}
