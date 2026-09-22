import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { History } from "./components/History";
import { Logo } from "./components/Logo";
import { ProgramDetail } from "./components/ProgramDetail";
import { ProgramTable, type SortKey } from "./components/ProgramTable";
import { RemovalPanel } from "./components/RemovalPanel";
import { Warnings } from "./components/Warnings";
import { isDesktop, listPrograms } from "./lib/api";
import type { Inventory, Program } from "./types";

type LoadState =
  | { status: "loading" }
  | { status: "ready"; inventory: Inventory }
  | { status: "failed"; message: string };

export function App() {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [query, setQuery] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [sort, setSort] = useState<{ key: SortKey; descending: boolean }>({
    key: "name",
    descending: false,
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // The program being removed is held as a whole, not as an id: once the
  // removal finishes it is gone from the inventory, and the report has to stay
  // on screen after the row behind it disappears.
  const [removing, setRemoving] = useState<Program | null>(null);
  const [showHistory, setShowHistory] = useState(false);
  const request = useRef(0);

  const load = useCallback(() => {
    if (!isDesktop()) {
      setState({
        status: "failed",
        message: "Open this through `npm run tauri dev` — the inventory comes from the Rust side.",
      });
      return;
    }

    // Only the newest read is allowed to land: a removal reloads the list
    // while an earlier read may still be in flight, and the older answer still
    // contains the program that was just removed.
    const token = (request.current += 1);
    setState({ status: "loading" });
    listPrograms()
      .then((inventory) => {
        if (token === request.current) setState({ status: "ready", inventory });
      })
      .catch((error: unknown) => {
        if (token === request.current) setState({ status: "failed", message: String(error) });
      });
  }, []);

  useEffect(load, [load]);

  const inventory = state.status === "ready" ? state.inventory : null;

  const visible = useMemo(() => {
    if (!inventory) return [];

    const needle = query.trim().toLowerCase();
    const matches = inventory.programs.filter((program) => {
      // Updates and shared runtimes stay out of the way until asked for:
      // they are the entries a user should be least likely to remove.
      if (!showHidden && program.kind !== "application") return false;
      if (needle.length === 0) return true;
      return (
        program.name.toLowerCase().includes(needle) ||
        (program.publisher ?? "").toLowerCase().includes(needle) ||
        (program.install_location ?? "").toLowerCase().includes(needle)
      );
    });

    const direction = sort.descending ? -1 : 1;
    return matches.sort((a, b) => {
      switch (sort.key) {
        case "size":
          return direction * ((a.estimated_size_bytes ?? 0) - (b.estimated_size_bytes ?? 0));
        case "date":
          return direction * (a.install_date ?? "").localeCompare(b.install_date ?? "");
        case "publisher":
          return direction * (a.publisher ?? "").localeCompare(b.publisher ?? "");
        default:
          return direction * a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
      }
    });
  }, [inventory, query, showHidden, sort]);

  const selected = visible.find((program) => program.id === selectedId) ?? null;
  const hiddenCount = inventory
    ? inventory.programs.filter((program) => program.kind !== "application").length
    : 0;

  function toggleSort(key: SortKey) {
    setSort((current) =>
      current.key === key
        ? { key, descending: !current.descending }
        : // Size and date are almost always wanted largest/newest first.
          { key, descending: key === "size" || key === "date" },
    );
  }

  return (
    <div className="relative flex h-full flex-col">
      <header className="border-line flex items-center gap-3 border-b px-4 py-3">
        <Logo className="text-wave size-7 shrink-0" />

        <div className="flex-1">
          <h1 className="text-[15px] font-semibold">Wave Uninstaller</h1>
          <p className="text-muted text-[12px]">
            {inventory
              ? `${visible.length} of ${inventory.programs.length} entries`
              : "Reading the registry…"}
          </p>
        </div>

        <input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search name, publisher or path"
          className="bg-surface border-line placeholder:text-muted/70 focus:border-wave w-72 rounded border px-2.5 py-1.5 text-[13px] outline-none select-text"
        />

        <button
          type="button"
          onClick={() => setShowHistory(true)}
          className="border-line hover:bg-raised rounded border px-2.5 py-1.5 text-[12px]"
        >
          Backups
        </button>

        <label className="text-muted flex items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={showHidden}
            onChange={(event) => setShowHidden(event.target.checked)}
          />
          Show updates and system components
          {hiddenCount > 0 && !showHidden && <span className="text-muted/70">({hiddenCount})</span>}
        </label>
      </header>

      {state.status === "failed" && (
        <div className="text-caution m-4 rounded border border-red-500/30 bg-red-500/5 px-3 py-2 text-[13px] select-text">
          {state.message}
        </div>
      )}

      {inventory && <Warnings warnings={inventory.warnings} />}

      <div className="flex min-h-0 flex-1">
        <main className="min-w-0 flex-1 overflow-y-auto">
          {state.status === "loading" && (
            <p className="text-muted px-4 py-6 text-[13px]">Reading the registry…</p>
          )}
          {inventory && visible.length === 0 && (
            <p className="text-muted px-4 py-6 text-[13px]">
              Nothing matches {query.trim().length > 0 ? `“${query.trim()}”` : "the current filter"}
              .
            </p>
          )}
          {visible.length > 0 && (
            <ProgramTable
              programs={visible}
              selectedId={selectedId}
              sort={sort}
              onSort={toggleSort}
              onSelect={(program) =>
                setSelectedId((current) => (current === program.id ? null : program.id))
              }
            />
          )}
        </main>

        {selected && (
          <ProgramDetail
            program={selected}
            onClose={() => setSelectedId(null)}
            onRemove={() => setRemoving(selected)}
          />
        )}
      </div>

      {removing && (
        <RemovalPanel
          program={removing}
          onClose={() => setRemoving(null)}
          // The machine changed, so the list has to be read again rather than
          // patched: an entry that is gone from the registry is gone from here.
          onFinished={load}
        />
      )}

      {showHistory && <History onClose={() => setShowHistory(false)} onRestored={load} />}
    </div>
  );
}
