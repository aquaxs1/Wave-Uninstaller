import { useEffect, useState } from "react";

import { listOperations, undoOperation } from "../lib/api";
import type { Operation } from "../types";

/**
 * Every removal Wave has a backup and a journal for.
 *
 * The backups are the reason removing is safe at all, so they are not hidden
 * in a folder nobody looks in: this is the list, and every entry that still
 * has its journal can be put back from here.
 */
export function History({ onClose, onRestored }: { onClose: () => void; onRestored: () => void }) {
  const [operations, setOperations] = useState<Operation[] | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  function refresh() {
    listOperations()
      .then(setOperations)
      .catch((error: unknown) => setMessage(String(error)));
  }

  useEffect(refresh, []);

  function undo(operation: Operation) {
    setBusy(operation.id);
    undoOperation(operation.id)
      .then((report) => {
        setMessage(
          report.failed === 0
            ? `${operation.program_name}: ${report.restored} item${report.restored === 1 ? "" : "s"} put back.`
            : `${operation.program_name}: ${report.restored} put back, ${report.failed} could not be — ${report.problems.join("; ")}`,
        );
        refresh();
        onRestored();
      })
      .catch((error: unknown) => setMessage(String(error)))
      .finally(() => setBusy(null));
  }

  return (
    <div className="bg-ground/80 absolute inset-0 z-10 flex items-center justify-center p-6">
      <div className="bg-surface border-line flex max-h-full w-[640px] flex-col rounded-lg border shadow-2xl">
        <header className="border-line flex items-start gap-3 border-b px-5 py-4">
          <div className="flex-1">
            <h2 className="text-[15px] font-semibold">Backups</h2>
            <p className="text-muted mt-0.5 text-[12px]">
              What Wave removed, and what it can put back.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="text-muted hover:text-ink px-1 text-lg leading-none"
          >
            ×
          </button>
        </header>

        {message && <p className="text-wave px-5 pt-3 text-[12px] select-text">{message}</p>}

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-3">
          {operations === null && <p className="text-muted text-[13px]">Reading the journals…</p>}
          {operations?.length === 0 && (
            <p className="text-muted text-[13px]">Wave has not removed anything yet.</p>
          )}

          <ul className="space-y-2">
            {operations?.map((operation) => (
              <li
                key={operation.id}
                className="border-line flex items-start gap-3 rounded border px-3 py-2"
              >
                <div className="min-w-0 flex-1">
                  <p className="text-[13px] select-text">{operation.program_name}</p>
                  <p className="text-muted text-[11px] select-text">
                    {operation.at} · {operation.restorable} restorable
                    {!operation.complete && " · stopped partway through"}
                    {operation.undone && " · already put back"}
                  </p>
                  <p className="text-muted font-mono text-[11px] break-all select-text">
                    {operation.backup_root}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => undo(operation)}
                  disabled={operation.undone || operation.restorable === 0 || busy !== null}
                  className="border-line hover:bg-raised shrink-0 rounded border px-2.5 py-1 text-[12px] disabled:opacity-40"
                >
                  {busy === operation.id ? "Restoring…" : "Restore"}
                </button>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  );
}
