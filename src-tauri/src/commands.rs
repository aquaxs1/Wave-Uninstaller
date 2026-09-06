//! The commands the UI can call.
//!
//! P0 is read-only on purpose: the app can show what is installed and nothing
//! else. Uninstalling and deleting arrive in P1 and P2, once there is a backup
//! and journal to undo them with.

use wave_core::Inventory;

use crate::system;

/// Everything installed on this machine.
#[tauri::command]
pub fn list_programs() -> Result<Inventory, String> {
    let provider = system::provider()?;

    let started = std::time::Instant::now();
    let inventory = wave_core::collect(&provider);

    tracing::info!(
        programs = inventory.programs.len(),
        warnings = inventory.warnings.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "inventory collected"
    );

    Ok(inventory)
}
