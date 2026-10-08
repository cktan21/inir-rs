use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SCHEMA_VERSION: u32 = 1;

/// Keep extension fields while migrating: existing layouts have many settings
/// beyond the first typed sections. Absent options stay absent so QML defaults
/// continue to apply, rather than being overwritten by invented Rust defaults.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Section {
    #[serde(flatten)]
    pub extensions: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low_power: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduce_animations: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compositor_blur: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blur_backend: Option<String>,
    #[serde(flatten)]
    pub extensions: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub niri: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iris: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waffle: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<PerformanceConfig>,
    #[serde(flatten)]
    pub extensions: Map<String, Value>,
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version > SCHEMA_VERSION {
            return Err(format!(
                "unsupported config schema {} (maximum {SCHEMA_VERSION})",
                self.schema_version
            ));
        }
        if let Some(ref section) = self.performance {
            if let Some(ref backend) = section.blur_backend {
                if !["auto", "wallpaper", "compositor", "off"].contains(&backend.as_str()) {
                    return Err(
                        "performance.blurBackend must be auto, wallpaper, compositor or off".into(),
                    );
                }
            }
        }
        Ok(())
    }
}
