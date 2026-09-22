import { invoke } from "@tauri-apps/api/core";

import type {
  ApplyReport,
  Inventory,
  Operation,
  RemovalPlan,
  Scan,
  Selection,
  UndoReport,
} from "../types";

/**
 * Read the installed-program inventory.
 *
 * Anything the backend could not read comes back in `warnings` rather than as
 * a thrown error: a partial list is still worth showing, as long as the gap is
 * visible.
 */
export async function listPrograms(): Promise<Inventory> {
  return invoke<Inventory>("list_programs");
}

/**
 * Find everything one program left behind.
 *
 * The backend keeps the result: a removal is planned from the scan Wave
 * actually performed, not from whatever the interface sends back.
 */
export async function scanProgram(programId: string): Promise<Scan> {
  return invoke<Scan>("scan_program", { programId });
}

/** Exactly what a selection would do, without doing any of it. */
export async function previewRemoval(
  programId: string,
  selection: Selection,
): Promise<RemovalPlan> {
  return invoke<RemovalPlan>("preview_removal", { programId, selection });
}

/**
 * Run the uninstaller and move the selected leftovers into the backup.
 *
 * The one call that changes anything on the machine.
 */
export async function removeProgram(programId: string, selection: Selection): Promise<ApplyReport> {
  return invoke<ApplyReport>("remove_program", { programId, selection });
}

/** Every removal there is a backup and a journal for. */
export async function listOperations(): Promise<Operation[]> {
  return invoke<Operation[]>("list_operations");
}

/** Put one removal back. */
export async function undoOperation(operationId: string): Promise<UndoReport> {
  return invoke<UndoReport>("undo_operation", { operationId });
}

/** Whether the page is running inside the Tauri shell at all. */
export function isDesktop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
