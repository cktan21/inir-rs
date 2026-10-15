use inir_types::desktop::*;
use inir_types::{config::Config, ResourceState, SystemInfo};

#[derive(Debug, Clone)]
pub enum Event {
    System(SystemInfo),
    Resource(ResourceState),
    Config(Config),
    Network(NetworkState),
    Power(PowerState),
    Brightness(BrightnessState),
    Niri(NiriState),
    NiriFrame(std::sync::Arc<NiriState>),
    Keyboard(KeyboardLockState),
}
