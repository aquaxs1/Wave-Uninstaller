//! The Wave Uninstaller desktop application.

mod commands;
mod system;

/// Start the app.
pub fn run() {
    init_logging();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![commands::list_programs])
        .run(tauri::generate_context!())
        .expect("the Tauri application failed to start");
}

/// Log to stderr, at info level unless `WAVE_LOG` says otherwise.
///
/// A program that deletes things has to be able to say what it did. P3 adds a
/// log file on disk alongside the operation journal; for now stderr is enough
/// to debug the inventory.
fn init_logging() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_env("WAVE_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,wave_core=debug,wave_win32=debug"));

    fmt().with_env_filter(filter).with_target(true).init();
}
