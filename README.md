<div align="center">

<img src="./site/assets/favicon.svg" width="72" height="72" alt="">

# Wave Uninstaller

**Deinstallieren ist der einfache Teil. Der schwierige sind die Reste.**

[![CI](https://github.com/aquaxs1/Wave-uninstaller/actions/workflows/ci.yml/badge.svg)](https://github.com/aquaxs1/Wave-uninstaller/actions/workflows/ci.yml)
[![Website](https://github.com/aquaxs1/Wave-uninstaller/actions/workflows/pages.yml/badge.svg)](https://github.com/aquaxs1/Wave-uninstaller/actions/workflows/pages.yml)
[![Phase](https://img.shields.io/badge/Phase-P0%20von%205-38bdf8)](#wo-das-projekt-steht)

[![Plattform](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?logo=windows&logoColor=white)](#entwickeln)
[![Rust](https://img.shields.io/badge/Rust-1.82%2B-dea584?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)](https://v2.tauri.app)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)](https://react.dev)

[![Letzter Commit](https://img.shields.io/github/last-commit/aquaxs1/Wave-uninstaller?label=letzter%20Commit&color=38bdf8)](https://github.com/aquaxs1/Wave-uninstaller/commits/main)
[![Codegröße](https://img.shields.io/github/languages/code-size/aquaxs1/Wave-uninstaller?label=Code&color=38bdf8)](https://github.com/aquaxs1/Wave-uninstaller)
[![Issues](https://img.shields.io/github/issues/aquaxs1/Wave-uninstaller?label=Issues&color=38bdf8)](https://github.com/aquaxs1/Wave-uninstaller/issues)
[![Sterne](https://img.shields.io/github/stars/aquaxs1/Wave-uninstaller?label=Sterne&color=38bdf8)](https://github.com/aquaxs1/Wave-uninstaller/stargazers)

</div>

---

Ein Uninstaller für Windows, der nach dem regulären Deinstallieren aufräumt:
verwaiste Registry-Schlüssel, zurückgelassene Ordner in `AppData` und
`ProgramData`, Dienste, geplante Tasks, Autostart-Einträge, Verknüpfungen,
Prefetch-Dateien.

Vorbild ist Revo Uninstaller — mit dem Unterschied, dass **jede** Löschung erst
in ein Backup wandert und vollständig rückgängig gemacht werden kann.

> [!IMPORTANT]
> **Phase P0 von 5.** Wave liest das Inventar und zeigt es an. Es deinstalliert
> nichts und löscht nichts. Das kommt in P1 und P2 — bewusst erst, nachdem P3
> ein funktionierendes Undo dafür gebaut hat.

![Die Programmliste von Wave Uninstaller](./site/assets/inventory-fixture.png)

<sub>Gerendert aus <code>crates/wave-core/tests/fixtures/typical_machine.json</code>,
nicht von einem echten System — daher die erfundenen Einträge.</sub>

## Zahlen

| | |
| --- | --- |
| Crates | 3 — Kern, Windows-Layer, App |
| Quellen fürs Inventar | 3 — Registry, MSI, Store-Pakete |
| Tests | 20, laufen auf jedem Betriebssystem in Millisekunden |
| CI-Jobs | 3 — Linux, Frontend, Windows |
| Windows-Version | 10 (1809+) und 11, x64 und arm64 |
| Phasen fertig | 1 von 6 |

## Wo das Projekt steht

| Phase | | Inhalt |
| --- | --- | --- |
| **P0** | ✅ | **Inventar** — Registry, MSI und Store-Pakete lesen und anzeigen |
| **P1** | ▫️ | **Deinstallieren** — nativen Uninstaller starten, auf den Prozessbaum warten, offensichtliche Reste entfernen |
| **P2** | ▫️ | **Der Scan** — vollständige Suche, Bewertung jedes Fundes, harte Schutzliste |
| **P3** | ▫️ | **Undo** — Journal, Wiederherstellung, Systemwiederherstellungspunkte |
| **P4** | ▫️ | **Die Extras** — Reste ohne Deinstallation finden, Autostart-Verwaltung, erzwungene Deinstallation |
| **P5** | ▫️ | **Ausliefern** — Übersetzungen, Code-Signing, Installer, Updates |

Die Reihenfolge ist nicht verhandelbar: erzwungene Deinstallation ohne
funktionierendes Undo wäre ein Datenverlust-Feature. Details in
[`PLAN.md`](./PLAN.md).

## Aufbau

```
crates/wave-core    Was ist installiert, was gehört zusammen — kennt kein Windows
crates/wave-win32   Registry, MSI, MSIX — kennt keine Entscheidungen
src-tauri           Tauri-Shell und die Commands, die die UI aufruft
src                 React + TypeScript + Tailwind
site                Die Projektseite (statisch, ohne Build)
```

Der Kern greift nie direkt auf Win32 zu, sondern nur über das Trait
`SystemProvider`. Davon gibt es zwei Implementierungen: die echte in
`wave-win32` und eine, die einen erfundenen Registry- und Dateisystem-Zustand
aus JSON liest.

Das ist keine Architektur-Kosmetik. Die Logik, die entscheidet, welche Dateien
gelöscht werden dürfen, muss testbar sein — deterministisch, in Sekunden, ohne
VM. Ein Uninstaller, dessen Entscheidungen man nur durch Ausprobieren prüfen
kann, wird nie vertrauenswürdig.

Die Regeln, die daraus folgen, stehen in [`CLAUDE.md`](./CLAUDE.md).

## Entwickeln

Voraussetzungen: Rust (stable, ≥ 1.82), Node 22, und für die App die
[Tauri-Systemabhängigkeiten](https://v2.tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev      # die App (nur unter Windows sinnvoll)
```

Kern testen — geht auf jedem Betriebssystem:

```bash
cargo test -p wave-core
```

Den Windows-Layer prüfen, ohne Windows zu haben:

```bash
rustup target add x86_64-pc-windows-msvc
cargo clippy -p wave-win32 --target x86_64-pc-windows-msvc -- -D warnings
```

Das findet Fehler in den Win32-Aufrufen, weil gegen die echten Bindings
typgeprüft wird — in Sekunden statt in zwölf Minuten auf dem Windows-Runner.
Nur das Linken braucht einen Windows-Host.

Sehen, was eine Fixture ergibt:

```bash
cargo run -p wave-core --example dump-inventory -- \
  crates/wave-core/tests/fixtures/typical_machine.json
```

Unter Linux baut nur der Kern. `wave-app` braucht eine WebView und
`wave-win32` ist vollständig `#[cfg(windows)]` — beides deckt der Windows-Job
in CI ab.

## Website

Die Projektseite liegt in [`site/`](./site) als statisches HTML ohne
Build-Schritt und wird von
[`.github/workflows/pages.yml`](./.github/workflows/pages.yml) auf GitHub Pages
veröffentlicht. Wie man sie lokal ansieht oder woanders ausliefert, steht in
[`site/README.md`](./site/README.md).

## Lizenz

Noch nicht festgelegt — damit gilt vorerst das gesetzliche Urheberrecht, also
keine Nutzungsrechte für Dritte.
