import type { Architecture, Category, Confidence, InstallScope, ProgramSource } from "../types";

export function formatSize(bytes: number | undefined): string {
  if (bytes === undefined || bytes <= 0) return "—";

  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 && unit > 0 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

export function formatDate(iso: string | undefined): string {
  if (!iso) return "—";
  const parsed = new Date(`${iso}T00:00:00`);
  if (Number.isNaN(parsed.getTime())) return iso;
  return parsed.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export const ARCHITECTURE_LABEL: Record<Architecture, string> = {
  x86: "32-bit",
  x64: "64-bit",
  arm64: "ARM64",
  neutral: "any CPU",
  unknown: "",
};

export const SCOPE_LABEL: Record<InstallScope, string> = {
  machine: "All users",
  current_user: "This user",
};

export const SOURCE_LABEL: Record<ProgramSource, string> = {
  uninstall_key: "Registry",
  appx: "Store",
  msi: "MSI",
};

export const CATEGORY_LABEL: Record<Category, string> = {
  install_folder: "Install folder",
  program_folder: "Program folder",
  app_data: "Settings and caches",
  program_data: "Machine-wide data",
  start_menu: "Shortcuts",
  package_data: "Store package data",
  registry_key: "Registry keys",
  registry_value: "Registry values",
};

export const CONFIDENCE_LABEL: Record<Confidence, string> = {
  safe: "Certain",
  review: "Check this",
  unsure: "Unsure",
};
