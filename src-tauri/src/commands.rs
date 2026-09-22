//! The commands the UI can call.
//!
//! Removal is deliberately three calls rather than one: scan, then plan, then
//! apply. What the user approves and what runs are the same object, and
//! nothing is removed by a call that was only meant to look.

use std::sync::Mutex;

use wave_core::apply::ApplyReport;
use wave_core::plan::{self, RemovalPlan, Selection};
use wave_core::scan::Scan;
use wave_core::undo::{Operation, UndoReport};
use wave_core::{Inventory, Program};

use crate::system;

/// What the UI is currently looking at.
///
/// The scan is kept here rather than sent back and forth so that the plan is
/// built from what Wave actually found, not from whatever a caller says it
/// found.
#[derive(Default)]
pub struct Session {
    scan: Mutex<Option<Scan>>,
}

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

/// What one program would leave behind, and what Wave proposes to do about it.
#[tauri::command]
pub fn scan_program(
    program_id: String,
    session: tauri::State<'_, Session>,
) -> Result<Scan, String> {
    let provider = system::provider()?;

    // Fresh, not cached: the inventory is what the protection list is built
    // from, and a program installed since the list was last read would
    // otherwise have no claim on its own folder.
    let inventory = wave_core::collect(&provider);
    let program = find(&inventory, &program_id)?;

    let started = std::time::Instant::now();
    let scan = wave_core::scan(&provider, &inventory, &program);

    tracing::info!(
        program = %program.name,
        findings = scan.findings.len(),
        protected = scan.protected.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "scan finished"
    );

    *session.scan.lock().map_err(|_| "the session is unusable")? = Some(scan.clone());
    Ok(scan)
}

/// Exactly what a given selection would do, without doing any of it.
#[tauri::command]
pub fn preview_removal(
    program_id: String,
    selection: Selection,
    session: tauri::State<'_, Session>,
) -> Result<RemovalPlan, String> {
    let provider = system::provider()?;
    let scan = current_scan(&session, &program_id)?;

    Ok(plan::build(&provider, &scan, &selection))
}

/// Run the program's uninstaller and move the selected leftovers to the backup.
///
/// This is the one command that changes anything. It refuses a selection that
/// does not belong to the scan the UI is showing, because a plan built from a
/// stale scan could name a path that now belongs to something else.
#[tauri::command]
pub fn remove_program(
    program_id: String,
    selection: Selection,
    session: tauri::State<'_, Session>,
) -> Result<ApplyReport, String> {
    let provider = system::provider()?;
    let mutator = system::mutator()?;
    let scan = current_scan(&session, &program_id)?;

    let plan = plan::build(&provider, &scan, &selection);
    if plan.is_empty() {
        return Err("nothing was selected, so there is nothing to do".to_string());
    }

    tracing::warn!(
        program = %plan.program_name,
        operation = %plan.operation_id,
        steps = plan.steps.len(),
        backup = %plan.backup_root,
        "removing"
    );

    let report = wave_core::apply(&provider, &mutator, &plan).map_err(|error| error.to_string())?;

    tracing::info!(
        operation = %report.operation_id,
        done = report.done,
        skipped = report.skipped,
        failed = report.failed,
        "removal finished"
    );

    // The scan described a machine that no longer exists.
    *session.scan.lock().map_err(|_| "the session is unusable")? = None;
    Ok(report)
}

/// Every removal Wave has a backup and a journal for.
#[tauri::command]
pub fn list_operations() -> Result<Vec<Operation>, String> {
    let provider = system::provider()?;
    Ok(wave_core::undo::list_operations(&provider))
}

/// Put one removal back.
#[tauri::command]
pub fn undo_operation(operation_id: String) -> Result<UndoReport, String> {
    let provider = system::provider()?;
    let mutator = system::mutator()?;

    tracing::warn!(operation = %operation_id, "undoing");
    wave_core::undo(&provider, &mutator, &operation_id).map_err(|error| error.to_string())
}

fn find(inventory: &Inventory, program_id: &str) -> Result<Program, String> {
    inventory
        .programs
        .iter()
        .find(|program| program.id.as_str() == program_id)
        .cloned()
        .ok_or_else(|| format!("{program_id} is not installed any more"))
}

/// The scan the UI is showing, if it is for the program the caller names.
fn current_scan(session: &tauri::State<'_, Session>, program_id: &str) -> Result<Scan, String> {
    let scan = session.scan.lock().map_err(|_| "the session is unusable")?;
    match scan.as_ref() {
        Some(scan) if scan.program.id.as_str() == program_id => Ok(scan.clone()),
        _ => Err("scan the program again — what Wave found last is no longer current".to_string()),
    }
}
