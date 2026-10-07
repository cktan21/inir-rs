use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    // Registered beside `qs.services` rather than over it: Gate 1 of
    // docs/plans/RUST_BACKEND_MIGRATION.md requires the native path to load
    // alongside the QML one so the two can be compared, which a shadowing
    // module name would prevent.
    //
    // Static registration, not Dynamic, even though the eventual plugin is
    // dynamic: CXX-Qt cannot lay out a dynamic QML plugin from a pure Cargo
    // build (it needs the CMake integration), so asking for one here would
    // produce a module that cannot be imported. Packaging the dynamic plugin is
    // itself Gate 1 work; this crate's job is to pin the interface and keep
    // compiling in CI.
    CxxQtBuilder::new_qml_module(QmlModule::new("qs.services.native"))
        .files(["src/audio.rs", "src/battery.rs", "src/brightness.rs"])
        .build();
}
