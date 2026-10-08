use cxx_qt_build::{CxxQtBuilder, PluginType, QmlModule};

fn main() {
    let builder = CxxQtBuilder::new_qml_module(
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
    ]);
    // Rust cdylibs hide C++ symbols. Rename Qt's generated entry points so
    // exported Rust trampolines can keep them visible to QPluginLoader.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.define("qt_plugin_instance", "inir_qt_plugin_instance");
            cc.define(
                "qt_plugin_query_metadata_v2",
                "inir_qt_plugin_query_metadata_v2",
            );
        })
    };
    builder.build();
}
