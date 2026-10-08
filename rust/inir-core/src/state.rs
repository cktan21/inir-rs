use inir_types::desktop::DesktopState;
use inir_types::{config::Config, ResourceState, SystemInfo};

/// Only implemented domains are represented here. Each subsequent migration
/// adds its typed state and event, instead of publishing placeholder services.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppState {
    pub system: SystemInfo,
    pub resources: ResourceState,
    pub config: Config,
    pub desktop: DesktopState,
}
