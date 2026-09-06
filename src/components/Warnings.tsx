import { useState } from "react";

/**
 * What the inventory could not read.
 *
 * This is not decoration. The leftover scan's protection list works by knowing
 * every installed program — if a source failed, another program's folder could
 * later be mistaken for garbage. So a gap in the inventory is shown to the
 * user rather than logged and forgotten.
 */
export function Warnings({ warnings }: { warnings: string[] }) {
  const [expanded, setExpanded] = useState(false);
  if (warnings.length === 0) return null;

  return (
    <div className="border-caution/30 bg-caution/5 mx-4 mb-3 rounded border">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="text-caution flex w-full items-center gap-2 px-3 py-2 text-left text-[13px]"
      >
        <span aria-hidden>{expanded ? "▾" : "▸"}</span>
        <span>
          {warnings.length} {warnings.length === 1 ? "source" : "sources"} could not be read — this
          list may be incomplete
        </span>
      </button>
      {expanded && (
        <ul className="text-muted border-caution/20 space-y-1 border-t px-3 py-2 font-mono text-[12px] select-text">
          {warnings.map((warning) => (
            <li key={warning} className="break-all">
              {warning}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
