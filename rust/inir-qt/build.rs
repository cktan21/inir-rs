use cxx_qt_build::{CxxQtBuilder, PluginType, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("qs.services.native").plugin_type(PluginType::Dynamic),
    )
    .qt_module("Qml")
    .crate_include_root(Some("include".into()))
    .cpp_file("include/service_models.h")
    .cpp_file("src/service_models.cpp")
    .files([
        "src/qobjects/system_info.rs",
        "src/qobjects/config.rs",
        "src/qobjects/desktop.rs",
    ])
    .build();
}
