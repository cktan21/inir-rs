//! Dynamic QML plugin. All worker-to-QObject access is queued onto Qt's thread.
pub mod qobjects;

// Match Qt 6's QPluginMetaData ABI from QtCore/qplugin.h.
#[repr(C)]
pub struct PluginMetaData {
    data: *const std::ffi::c_void,
    size: usize,
}

extern "C" {
    fn inir_qt_plugin_instance() -> *mut std::ffi::c_void;
    fn inir_qt_plugin_query_metadata_v2() -> PluginMetaData;
}

#[no_mangle]
pub extern "C" fn qt_plugin_instance() -> *mut std::ffi::c_void {
    // Qt owns the returned QObject; the generated entry point manages it.
    unsafe { inir_qt_plugin_instance() }
}

#[no_mangle]
pub extern "C" fn qt_plugin_query_metadata_v2() -> PluginMetaData {
    // Metadata storage is static and owned by Qt's generated plugin code.
    unsafe { inir_qt_plugin_query_metadata_v2() }
}

pub(crate) fn init_logging() {
    static LOGGING: std::sync::Once = std::sync::Once::new();
    LOGGING.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_env("INIR_LOG")
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
        // A host application may already own the global subscriber.
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init();
    });
}
