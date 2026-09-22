//! The Wave Uninstaller desktop application.

mod commands;
mod system;

/// Start the app.
pub fn run() {
    init_logging();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(commands::Session::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_programs,
            commands::scan_program,
            commands::preview_removal,
            commands::remove_program,
            commands::list_operations,
            commands::undo_operation,
        ])
        .run(tauri::generate_context!())
        .expect("the Tauri application failed to start");
}

/// Log to stderr, at info level unless `WAVE_LOG` says otherwise.
///
/// A program that removes things has to be able to say what it did. The
/// operation journal under `%ProgramData%\Wave\Backups` is the record that
/// matters and the one undo reads; this is the running commentary next to it.
fn init_logging() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_env("WAVE_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,wave_core=debug,wave_win32=debug"));

    fmt().with_env_filter(filter).with_target(true).init();
}
