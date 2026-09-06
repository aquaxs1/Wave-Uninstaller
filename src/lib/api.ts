import { invoke } from "@tauri-apps/api/core";

import type { Inventory } from "../types";

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

/** Whether the page is running inside the Tauri shell at all. */
export function isDesktop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
