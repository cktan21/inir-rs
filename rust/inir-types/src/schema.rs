use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SettingKind {
    Boolean,
    Slider { min: f64, max: f64, step: f64 },
    Choice { options: &'static [&'static str] },
    Color,
    String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingField {
    pub key: &'static str,
    pub category: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub kind: SettingKind,
}

/// Stable IDs, independent of labels, translations and layout family.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("network", "Network & Connectivity"),
    ("sound", "Sound"),
    ("displays-input", "Displays & Input"),
    ("power", "Power"),
    ("personalization", "Personalization"),
    ("desktop", "Desktop & Layouts"),
    ("apps-services", "Apps & Services"),
    ("system", "System"),
];

/// Initial metadata covers fields whose validation is implemented. Expand this
/// as each service reaches parity; do not advertise nonfunctional controls.
pub fn fields() -> Vec<SettingField> {
    vec![
        SettingField {
            key: "performance.lowPower",
            category: "system",
            title: "Low power mode",
            description: "Reduce visual effects and background work.",
            kind: SettingKind::Boolean,
        },
        SettingField {
            key: "performance.reduceAnimations",
            category: "system",
            title: "Reduce animations",
            description: "Minimize motion throughout the desktop.",
            kind: SettingKind::Boolean,
        },
        SettingField {
            key: "performance.compositorBlur",
            category: "system",
            title: "Compositor blur",
            description: "Allow Niri to blur active surfaces.",
            kind: SettingKind::Boolean,
        },
        SettingField {
            key: "performance.blurBackend",
            category: "system",
            title: "Blur backend",
            description: "Choose how panel blur is rendered.",
            kind: SettingKind::Choice {
                options: &["auto", "wallpaper", "compositor", "off"],
            },
        },
    ]
}
