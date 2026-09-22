import { useEffect, useMemo, useState } from "react";

import { Badge } from "./Badge";
import { removeProgram, scanProgram, undoOperation } from "../lib/api";
import { CATEGORY_LABEL, CONFIDENCE_LABEL, formatSize } from "../lib/format";
import type { ApplyReport, Category, Finding, Program, Scan, Selection } from "../types";

type Stage =
  | { status: "scanning" }
  | { status: "review"; scan: Scan }
  | { status: "removing"; scan: Scan }
  | { status: "done"; report: ApplyReport }
  | { status: "failed"; message: string };

/**
 * Uninstall a program and clean up after it.
 *
 * Three steps, deliberately: scan, review, remove. The middle one is not a
 * formality — nothing is ticked that Wave is not sure about, every row says
 * why it is there, and the button says how much it is about to move rather
 * than "OK".
 */
export function RemovalPanel({
  program,
  onClose,
  onFinished,
}: {
  program: Program;
  onClose: () => void;
  onFinished: () => void;
}) {
  const [stage, setStage] = useState<Stage>({ status: "scanning" });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [runUninstaller, setRunUninstaller] = useState(true);
  const [showUnsure, setShowUnsure] = useState(false);
  const [undone, setUndone] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setStage({ status: "scanning" });

    scanProgram(program.id)
      .then((scan) => {
        if (cancelled) return;
        setStage({ status: "review", scan });
        // Only what Wave is sure about starts ticked. Everything else is a
        // decision the user makes, not one made for them.
        setSelected(
          new Set(
            scan.findings
              .filter((finding) => finding.score.confidence === "safe")
              .map((finding) => finding.id),
          ),
        );
        setRunUninstaller(scan.uninstaller !== undefined);
      })
      .catch((error: unknown) => {
        if (!cancelled) setStage({ status: "failed", message: String(error) });
      });

    return () => {
      cancelled = true;
    };
  }, [program.id]);

  const scan = stage.status === "review" || stage.status === "removing" ? stage.scan : null;

  const visible = useMemo(() => {
    if (!scan) return [];
    return scan.findings.filter((finding) => showUnsure || finding.score.confidence !== "unsure");
  }, [scan, showUnsure]);

  const hiddenCount = scan ? scan.findings.length - visible.length : 0;
  const selectedSize = scan
    ? scan.findings
        .filter((finding) => selected.has(finding.id))
        .reduce((total, finding) => total + (finding.size_bytes ?? 0), 0)
    : 0;

  function toggle(finding: Finding) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(finding.id)) next.delete(finding.id);
      else next.add(finding.id);
      return next;
    });
  }

  function start() {
    if (!scan) return;
    setStage({ status: "removing", scan });

    const selection: Selection = {
      run_uninstaller: runUninstaller && scan.uninstaller !== undefined,
      findings: [...selected],
    };

    removeProgram(program.id, selection)
      .then((report) => {
        setStage({ status: "done", report });
        onFinished();
      })
      .catch((error: unknown) => setStage({ status: "failed", message: String(error) }));
  }

  return (
    <div className="bg-ground/80 absolute inset-0 z-10 flex items-center justify-center p-6">
      <div className="bg-surface border-line flex max-h-full w-[720px] flex-col rounded-lg border shadow-2xl">
        <header className="border-line flex items-start gap-3 border-b px-5 py-4">
          <div className="flex-1">
            <h2 className="text-[15px] font-semibold">Remove {program.name}</h2>
            <p className="text-muted mt-0.5 text-[12px]">
              Nothing is deleted. Everything moves to a backup you can undo.
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

        {stage.status === "scanning" && (
          <p className="text-muted px-5 py-8 text-[13px]">
            Looking for what {program.name} put on this machine…
          </p>
        )}

        {stage.status === "removing" && (
          <p className="text-muted px-5 py-8 text-[13px]">
            Removing. The uninstaller may take a while, and Wave waits for all of it.
          </p>
        )}

        {stage.status === "failed" && (
          <div className="text-caution m-5 rounded border border-red-500/30 bg-red-500/5 px-3 py-2 text-[13px] select-text">
            {stage.message}
          </div>
        )}

        {stage.status === "review" && scan && (
          <>
            <div className="border-line space-y-3 border-b px-5 py-3">
              {scan.uninstaller ? (
                <label className="flex items-start gap-2 text-[13px]">
                  <input
                    type="checkbox"
                    checked={runUninstaller}
                    onChange={(event) => setRunUninstaller(event.target.checked)}
                    className="mt-0.5"
                  />
                  <span>
                    Run the program's own uninstaller first
                    <span className="text-muted block font-mono text-[11px] break-all select-text">
                      {scan.uninstaller.kind === "command"
                        ? scan.uninstaller.display
                        : `Store package ${scan.uninstaller.package_family_name}`}
                    </span>
                    {scan.uninstaller.kind === "command" && !scan.uninstaller.silent && (
                      <span className="text-caution block text-[11px]">
                        This uninstaller has no silent switch, so it will open its own window.
                      </span>
                    )}
                  </span>
                </label>
              ) : (
                <p className="text-caution text-[12px]">
                  This program records no way to uninstall it. Wave can still remove what it left
                  behind.
                </p>
              )}
            </div>

            {scan.warnings.length > 0 && (
              <ul className="border-caution/30 bg-caution/5 text-caution mx-5 mt-3 space-y-1 rounded border px-3 py-2 text-[12px] select-text">
                {scan.warnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            )}

            <div className="min-h-0 flex-1 overflow-y-auto px-5 py-3">
              <FindingList findings={visible} selected={selected} onToggle={toggle} />

              {hiddenCount > 0 && (
                <button
                  type="button"
                  onClick={() => setShowUnsure(true)}
                  className="text-muted hover:text-ink mt-3 text-[12px] underline"
                >
                  Show {hiddenCount} finding{hiddenCount === 1 ? "" : "s"} Wave is unsure about
                </button>
              )}

              {scan.protected.length > 0 && <Spared items={scan.protected} />}
            </div>

            <footer className="border-line flex items-center gap-3 border-t px-5 py-3">
              <p className="text-muted flex-1 text-[12px]">
                {selected.size} of {scan.findings.length} selected
                {selectedSize > 0 && ` · ${formatSize(selectedSize)}`}
              </p>
              <button
                type="button"
                onClick={onClose}
                className="text-muted hover:text-ink px-3 py-1.5 text-[13px]"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={start}
                disabled={selected.size === 0 && !runUninstaller}
                className="bg-wave/15 border-wave/40 text-wave hover:bg-wave/25 rounded border px-3 py-1.5 text-[13px] disabled:opacity-40"
              >
                {runUninstaller ? "Uninstall and move " : "Move "}
                {selected.size} item{selected.size === 1 ? "" : "s"} to the backup
              </button>
            </footer>
          </>
        )}

        {stage.status === "done" && (
          <Report
            report={stage.report}
            undone={undone}
            onUndo={() => {
              undoOperation(stage.report.operation_id)
                .then((result) => {
                  setUndone(
                    result.failed === 0
                      ? `${result.restored} item${result.restored === 1 ? "" : "s"} put back.`
                      : `${result.restored} put back, ${result.failed} could not be: ${result.problems.join("; ")}`,
                  );
                  onFinished();
                })
                .catch((error: unknown) => setUndone(String(error)));
            }}
            onClose={onClose}
          />
        )}
      </div>
    </div>
  );
}

function FindingList({
  findings,
  selected,
  onToggle,
}: {
  findings: Finding[];
  selected: Set<string>;
  onToggle: (finding: Finding) => void;
}) {
  const grouped = new Map<Category, Finding[]>();
  for (const finding of findings) {
    const bucket = grouped.get(finding.category) ?? [];
    bucket.push(finding);
    grouped.set(finding.category, bucket);
  }

  if (findings.length === 0) {
    return (
      <p className="text-muted text-[13px]">
        Nothing left behind that Wave can see — the uninstaller alone will do.
      </p>
    );
  }

  return (
    <div className="space-y-4">
      {[...grouped.entries()].map(([category, entries]) => (
        <section key={category}>
          <h3 className="text-muted mb-1 text-[11px] tracking-wide uppercase">
            {CATEGORY_LABEL[category]}
          </h3>
          <ul className="space-y-1">
            {entries.map((finding) => (
              <li key={finding.id}>
                <label className="hover:bg-raised/50 flex items-start gap-2 rounded px-1.5 py-1">
                  <input
                    type="checkbox"
                    checked={selected.has(finding.id)}
                    onChange={() => onToggle(finding)}
                    className="mt-1"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center gap-2">
                      <span className="truncate font-mono text-[12px] select-text">
                        {finding.display}
                      </span>
                      {finding.size_bytes !== undefined && finding.size_bytes > 0 && (
                        <span className="text-muted shrink-0 text-[11px]">
                          {formatSize(finding.size_bytes)}
                        </span>
                      )}
                      <Badge
                        tone={finding.score.confidence === "safe" ? "wave" : "caution"}
                        title={`${finding.score.value} of 100`}
                      >
                        {CONFIDENCE_LABEL[finding.score.confidence]}
                      </Badge>
                    </span>
                    {/* The reasoning, not just the score: a selection nobody
                        can argue with is one nobody can check. */}
                    <span className="text-muted block text-[11px]">
                      {finding.score.evidence.map((item) => item.reason).join(" · ")}
                    </span>
                  </span>
                </label>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

/** What the scan found and deliberately left alone. */
function Spared({ items }: { items: { display: string; reason: string }[] }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="border-line mt-4 border-t pt-3">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="text-muted hover:text-ink text-[12px]"
      >
        <span aria-hidden>{expanded ? "▾" : "▸"}</span> {items.length} thing
        {items.length === 1 ? "" : "s"} left alone on purpose
      </button>
      {expanded && (
        <ul className="text-muted mt-2 space-y-1 text-[11px] select-text">
          {items.map((item) => (
            <li key={item.display}>
              <span className="font-mono break-all">{item.display}</span> — {item.reason}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Report({
  report,
  undone,
  onUndo,
  onClose,
}: {
  report: ApplyReport;
  undone: string | null;
  onUndo: () => void;
  onClose: () => void;
}) {
  return (
    <>
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <p className="text-[13px]">
          {report.done} removed, {report.skipped} already gone
          {report.failed > 0 && `, ${report.failed} failed`}.
        </p>
        <p className="text-muted mt-1 font-mono text-[11px] break-all select-text">
          Backup: {report.backup_root}
        </p>

        {report.journal_warnings.length > 0 && (
          <ul className="border-caution/30 bg-caution/5 text-caution mt-3 space-y-1 rounded border px-3 py-2 text-[12px] select-text">
            {report.journal_warnings.map((warning) => (
              <li key={warning}>{warning}</li>
            ))}
          </ul>
        )}

        <ul className="mt-3 space-y-1">
          {report.results.map((result) => (
            <li key={result.display} className="flex items-start gap-2 text-[12px]">
              <span
                aria-hidden
                className={
                  result.status === "failed"
                    ? "text-caution"
                    : result.status === "skipped"
                      ? "text-muted"
                      : "text-wave"
                }
              >
                {result.status === "failed" ? "✕" : result.status === "skipped" ? "–" : "✓"}
              </span>
              <span className="min-w-0 flex-1 select-text">
                <span className="break-all">{result.display}</span>
                {result.detail && (
                  <span className="text-muted block text-[11px] break-all">{result.detail}</span>
                )}
              </span>
            </li>
          ))}
        </ul>

        {undone && <p className="text-wave mt-3 text-[13px] select-text">{undone}</p>}
      </div>

      <footer className="border-line flex items-center gap-3 border-t px-5 py-3">
        <p className="text-muted flex-1 text-[12px]">
          Nothing was deleted. Undo puts the files and keys back; a program whose own uninstaller
          ran has to be installed again.
        </p>
        <button
          type="button"
          onClick={onUndo}
          disabled={undone !== null}
          className="border-line hover:bg-raised rounded border px-3 py-1.5 text-[13px] disabled:opacity-40"
        >
          Undo
        </button>
        <button
          type="button"
          onClick={onClose}
          className="bg-wave/15 border-wave/40 text-wave hover:bg-wave/25 rounded border px-3 py-1.5 text-[13px]"
        >
          Done
        </button>
      </footer>
    </>
  );
}
