import { Badge } from "./Badge";
import { ARCHITECTURE_LABEL, formatDate, formatSize, SOURCE_LABEL } from "../lib/format";
import type { Program } from "../types";

export type SortKey = "name" | "size" | "date" | "publisher";

const COLUMNS: { key: SortKey | null; label: string; className: string }[] = [
  { key: "name", label: "Name", className: "" },
  { key: "publisher", label: "Publisher", className: "w-52" },
  { key: null, label: "Version", className: "w-32" },
  { key: "size", label: "Size", className: "w-24 text-right" },
  { key: "date", label: "Installed", className: "w-32" },
];

export function ProgramTable({
  programs,
  selectedId,
  sort,
  onSort,
  onSelect,
}: {
  programs: Program[];
  selectedId: string | null;
  sort: { key: SortKey; descending: boolean };
  onSort: (key: SortKey) => void;
  onSelect: (program: Program) => void;
}) {
  return (
    <table className="w-full border-collapse text-left">
      <thead className="bg-ground/95 sticky top-0 z-10 backdrop-blur">
        <tr className="border-line text-muted border-b text-[12px] tracking-wide">
          {COLUMNS.map((column) => (
            <th key={column.label} className={`px-3 py-2 font-medium ${column.className}`}>
              {column.key ? (
                <button
                  type="button"
                  onClick={() => onSort(column.key as SortKey)}
                  className="hover:text-ink inline-flex items-center gap-1"
                >
                  {column.label}
                  {sort.key === column.key && (
                    <span aria-hidden>{sort.descending ? "▾" : "▴"}</span>
                  )}
                </button>
              ) : (
                column.label
              )}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {programs.map((program) => {
          const architecture = ARCHITECTURE_LABEL[program.architecture];
          const selected = program.id === selectedId;

          return (
            <tr
              key={program.id}
              onClick={() => onSelect(program)}
              className={`border-line/50 cursor-default border-b ${
                selected ? "bg-wave/10" : "hover:bg-raised/60"
              }`}
            >
              <td className="px-3 py-1.5">
                <div className="flex items-center gap-2">
                  <span className="truncate">{program.name}</span>
                  {program.kind === "system_component" && <Badge tone="caution">System</Badge>}
                  {program.kind === "update" && <Badge tone="caution">Update</Badge>}
                  {program.sources.includes("appx") && (
                    <Badge title="Store package">{SOURCE_LABEL.appx}</Badge>
                  )}
                  {architecture && <Badge>{architecture}</Badge>}
                </div>
              </td>
              <td className="text-muted truncate px-3 py-1.5">{program.publisher ?? "—"}</td>
              <td className="text-muted truncate px-3 py-1.5 font-mono text-[12px]">
                {program.version ?? "—"}
              </td>
              <td
                className="text-muted px-3 py-1.5 text-right tabular-nums"
                title={
                  program.estimated_size_bytes === undefined
                    ? undefined
                    : "Reported by the installer, not measured"
                }
              >
                {formatSize(program.estimated_size_bytes)}
              </td>
              <td className="text-muted px-3 py-1.5 tabular-nums">
                {formatDate(program.install_date)}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
