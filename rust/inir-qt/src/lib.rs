//! Dynamic QML plugin. All worker-to-QObject access is queued onto Qt's thread.
pub mod qobjects;

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
