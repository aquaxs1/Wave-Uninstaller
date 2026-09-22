import { Badge } from "./Badge";
import {
  ARCHITECTURE_LABEL,
  formatDate,
  formatSize,
  SCOPE_LABEL,
  SOURCE_LABEL,
} from "../lib/format";
import type { Program } from "../types";

function Field({ label, value, mono }: { label: string; value?: string; mono?: boolean }) {
  return (
    <div className="border-line/60 border-b py-2 last:border-b-0">
      <div className="text-muted text-[11px] tracking-wide uppercase">{label}</div>
      <div className={`mt-0.5 break-words select-text ${mono ? "font-mono text-[12px]" : ""}`}>
        {value && value.length > 0 ? value : <span className="text-muted">—</span>}
      </div>
    </div>
  );
}

export function ProgramDetail({
  program,
  onClose,
  onRemove,
}: {
  program: Program;
  onClose: () => void;
  onRemove: () => void;
}) {
  const architecture = ARCHITECTURE_LABEL[program.architecture];

  return (
    <aside className="bg-surface border-line flex w-96 shrink-0 flex-col border-l">
      <header className="border-line flex items-start gap-2 border-b px-4 py-3">
        <h2 className="flex-1 text-[15px] leading-snug font-medium select-text">{program.name}</h2>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close details"
          className="text-muted hover:text-ink -mr-1 px-1 text-lg leading-none"
        >
          ×
        </button>
      </header>

      <div className="flex flex-wrap gap-1.5 px-4 py-3">
        {program.sources.map((source) => (
          <Badge key={source} tone="wave">
            {SOURCE_LABEL[source]}
          </Badge>
        ))}
        {architecture && <Badge>{architecture}</Badge>}
        <Badge>{SCOPE_LABEL[program.scope]}</Badge>
        {program.kind !== "application" && (
          <Badge tone="caution">{program.kind === "update" ? "Update" : "System component"}</Badge>
        )}
      </div>

      <div className="flex-1 overflow-y-auto px-4 pb-4">
        <Field label="Publisher" value={program.publisher} />
        <Field label="Version" value={program.version} />
        <Field label="Installed" value={formatDate(program.install_date)} />
        <Field
          label="Reported size"
          value={
            program.estimated_size_bytes === undefined
              ? undefined
              : `${formatSize(program.estimated_size_bytes)} (self-reported)`
          }
        />
        <Field label="Install location" value={program.install_location} mono />
        {program.parent_name && <Field label="Belongs to" value={program.parent_name} />}
        <Field label="Uninstall command" value={program.uninstall?.command} mono />
        <Field label="Silent command" value={program.uninstall?.quiet_command} mono />
        <Field label="MSI product code" value={program.uninstall?.msi_product_code} mono />
        <Field label="Wave id" value={program.id} mono />
      </div>

      <footer className="border-line border-t px-4 py-3">
        <button
          type="button"
          onClick={onRemove}
          className="bg-wave/15 border-wave/40 text-wave hover:bg-wave/25 w-full rounded border px-3 py-2 text-[13px]"
        >
          Uninstall and clean up
        </button>
        <p className="text-muted mt-2 text-[11px]">
          {program.kind === "application"
            ? "Wave shows what it found before it removes anything, and moves it to a backup rather than deleting it."
            : "Other programs depend on this entry. Wave will show what it found, but think twice."}
        </p>
      </footer>
    </aside>
  );
}
