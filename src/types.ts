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

export type RegistryRoot = "HKLM" | "HKCU" | "HKCR" | "HKU";
export type RegistryView = "64" | "32";

export type Target =
  | { kind: "path"; path: string }
  | { kind: "registry_key"; root: RegistryRoot; path: string; view: RegistryView }
  | { kind: "registry_value"; root: RegistryRoot; path: string; view: RegistryView; name: string };

export type Category =
  | "install_folder"
  | "program_folder"
  | "app_data"
  | "program_data"
  | "start_menu"
  | "package_data"
  | "registry_key"
  | "registry_value";

/** `safe` is preselected, `review` is shown unticked, `unsure` is hidden. */
export type Confidence = "safe" | "review" | "unsure";

export interface Evidence {
  weight: number;
  reason: string;
}

export interface Score {
  value: number;
  confidence: Confidence;
  /** Why this scored what it did, strongest reason first. */
  evidence: Evidence[];
}

export interface Finding {
  id: string;
  target: Target;
  display: string;
  category: Category;
  score: Score;
  size_bytes?: number;
}

/** Something found and deliberately left alone, with the rule that spared it. */
export interface Skipped {
  display: string;
  reason: string;
}

export type Uninstaller =
  | { kind: "command"; program: string; arguments: string[]; display: string; silent: boolean }
  | { kind: "store_package"; package_family_name: string };

export interface Scan {
  program: Program;
  uninstaller?: Uninstaller;
  findings: Finding[];
  protected: Skipped[];
  warnings: string[];
}

/** What the user chose. The backend plans from this and nothing else. */
export interface Selection {
  run_uninstaller: boolean;
  findings: string[];
}

export interface PlanStep {
  action:
    | "run_uninstaller"
    | "remove_store_package"
    | "move_path"
    | "remove_registry_key"
    | "remove_registry_value";
  path?: string;
  backup_path?: string;
  display?: string;
  name?: string;
}

export interface RemovalPlan {
  operation_id: string;
  program_name: string;
  created_at: string;
  backup_root: string;
  journal_path: string;
  steps: PlanStep[];
}

export type StepStatus = "done" | "skipped" | "failed";

export interface StepResult {
  display: string;
  status: StepStatus;
  detail?: string;
}

export interface ApplyReport {
  operation_id: string;
  program_name: string;
  backup_root: string;
  journal_path: string;
  results: StepResult[];
  uninstaller?: { exit_code?: number; timed_out: boolean };
  done: number;
  skipped: number;
  failed: number;
  /** Journal lines that could not be written — what follows them is not undoable. */
  journal_warnings: string[];
}

export interface Operation {
  id: string;
  program_name: string;
  at: string;
  backup_root: string;
  journal_path: string;
  restorable: number;
  undone: boolean;
  complete: boolean;
}

export interface UndoReport {
  operation_id: string;
  restored: number;
  failed: number;
  /** Steps nothing can undo — an uninstaller that ran, above all. */
  irreversible: string[];
  problems: string[];
}
