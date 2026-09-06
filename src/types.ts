/**
 * Mirrors the serde shape of `wave-core`'s model.
 *
 * Kept by hand for now. Once the command surface grows past a single call it
 * is worth generating these from Rust instead of trusting two files to agree.
 */

export type ProgramSource = "uninstall_key" | "appx" | "msi";
export type InstallScope = "machine" | "current_user";
export type Architecture = "x86" | "x64" | "arm64" | "neutral" | "unknown";
export type ProgramKind = "application" | "update" | "system_component";

export interface UninstallCommand {
  command: string;
  quiet_command?: string;
  msi_product_code?: string;
}

export interface Program {
  id: string;
  name: string;
  version?: string;
  publisher?: string;
  install_location?: string;
  /** ISO `YYYY-MM-DD`. */
  install_date?: string;
  /** Self-reported by the installer, so a hint rather than a measurement. */
  estimated_size_bytes?: number;
  uninstall?: UninstallCommand;
  icon_path?: string;
  parent_name?: string;
  sources: ProgramSource[];
  scope: InstallScope;
  architecture: Architecture;
  kind: ProgramKind;
}

export interface Inventory {
  programs: Program[];
  /** Sources or keys that could not be read. Shown, never swallowed. */
  warnings: string[];
}
