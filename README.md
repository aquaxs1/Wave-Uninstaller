<div align="center">

<img src="./site/assets/img/logo.svg" width="76" height="76" alt="">

# Wave Uninstaller

**Uninstalling is the easy part. The hard part is what stays behind.**

[![CI](https://github.com/aquaxs1/Wave-uninstaller/actions/workflows/ci.yml/badge.svg)](https://github.com/aquaxs1/Wave-uninstaller/actions/workflows/ci.yml)
[![Phase](https://img.shields.io/badge/phase-P1%20done%2C%20P2%E2%80%93P3%20in%20part-5a5a63)](#where-the-project-stands)
[![License](https://img.shields.io/badge/license-MIT-5a5a63)](./LICENSE)

[![Platform](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?logo=windows&logoColor=white)](#developing)
[![Rust](https://img.shields.io/badge/Rust-1.82%2B-dea584?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)](https://v2.tauri.app)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)](https://react.dev)

[![Last commit](https://img.shields.io/github/last-commit/aquaxs1/Wave-uninstaller?label=last%20commit&color=5a5a63)](https://github.com/aquaxs1/Wave-uninstaller/commits/main)
[![Code size](https://img.shields.io/github/languages/code-size/aquaxs1/Wave-uninstaller?label=code&color=5a5a63)](https://github.com/aquaxs1/Wave-uninstaller)
[![Issues](https://img.shields.io/github/issues/aquaxs1/Wave-uninstaller?label=issues&color=5a5a63)](https://github.com/aquaxs1/Wave-uninstaller/issues)
[![Stars](https://img.shields.io/github/stars/aquaxs1/Wave-uninstaller?label=stars&color=5a5a63)](https://github.com/aquaxs1/Wave-uninstaller/stargazers)

</div>

---

A Windows uninstaller that cleans up after the regular uninstall: orphaned
registry keys, folders left in `AppData` and `ProgramData`, services, scheduled
tasks, startup entries, shortcuts, prefetch files.

Revo Uninstaller is the model — with the difference that **every** deletion
lands in a backup first and can be undone completely.

> [!IMPORTANT]
> **Wave removes things.** It runs a program's own uninstaller, finds what was
> left behind, and moves it to a backup under `%ProgramData%\Wave\Backups`.
> Nothing is deleted, every step is journalled before it runs, and every
> operation can be undone. It needs administrator rights and asks for them at
> startup. It is early software: read what it proposes before approving it.

![Wave Uninstaller's program list](./site/assets/img/inventory.png)

<sub>Rendered from <code>crates/wave-core/tests/fixtures/typical_machine.json</code>
rather than a real machine, which is why the entries are invented.</sub>

## Numbers

| | |
| --- | --- |
| Crates | 3 — core, Windows layer, app |
| Inventory sources | 3 — registry, MSI, Store packages |
| Places scanned | Program Files, ProgramData, AppData, start menu, vendor keys, App Paths, startup |
| Tests | 63, running on any operating system in milliseconds |
| CI jobs | 3 — Linux, frontend, Windows |
| Windows version | 10 (1809+) and 11, x64 and arm64 |
| Phases done | 2 of 6, plus part of two more |

## Where the project stands

| Phase | | Content |
| --- | --- | --- |
| **P0** | ✅ | **Inventory** — read the registry, MSI and Store packages and show them |
| **P1** | ✅ | **Uninstalling** — run the native uninstaller, wait for the process tree, remove the leftovers |
| **P2** | ◐ | **The scan** — files and registry, a score for every finding, a hard protection list. Services and scheduled tasks still open |
| **P3** | ◐ | **Undo** — backup, journal and restore work. Restore points and retention still open |
| **P4** | ▫️ | **The extras** — find leftovers with no uninstall, startup manager, forced uninstall |
| **P5** | ▫️ | **Shipping** — translations, code signing, an installer, updates |

The order is not negotiable: forced uninstallation without a working undo would
be a data-loss feature, which is why P3's backup and journal were built together
with P1 rather than after it. Details in [`PLAN.md`](./PLAN.md).

## Layout

```
crates/wave-core    What is installed, what belongs to it, what may go — knows no Windows
crates/wave-win32   Registry, MSI, MSIX, files, processes — makes no decisions
src-tauri           The Tauri shell and the commands the interface calls
src                 React + TypeScript + Tailwind
site                The project website (static, no build step)
```

The core never touches Win32 directly, only two traits: `SystemProvider`, which
reads, and `SystemMutator`, which writes. Each has two implementations: the real
one in `wave-win32`, and one backed by an invented machine described in JSON.
The scan is handed the reader and nothing else, so the code that decides what to
remove has no way to remove it.

This is not architectural decoration. The logic that decides which files may be
deleted has to be testable — deterministically, in seconds, without a VM. An
uninstaller whose decisions can only be checked by trying them will never be
trustworthy.

The rules that follow from it are in [`CLAUDE.md`](./CLAUDE.md).

## Download

Windows builds are on the
[releases page](https://github.com/aquaxs1/Wave-uninstaller/releases/latest),
built by [`release.yml`](./.github/workflows/release.yml) and linked from the
[download page](https://github.com/aquaxs1/Wave-uninstaller/blob/main/site/download.html):
an installer, the portable executable, and that same executable zipped —
which is what the site's download button serves, because browsers and
SmartScreen are considerably harder on an unsigned `.exe` downloaded directly.
Nothing is code-signed yet, so SmartScreen warns on first launch either way.

Remember what this build is: it removes programs for real, and it has not been
through wide testing on real machines. Everything it removes is recoverable from
the Backups window — that is the design, not a promise about your data.

## Developing

Prerequisites: Rust (stable, ≥ 1.82), Node 22, and for the app the
[Tauri system dependencies](https://v2.tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev      # the app (only meaningful on Windows)
```

Test the core — works on any operating system:

```bash
cargo test -p wave-core
```

Check the Windows layer without having Windows:

```bash
rustup target add x86_64-pc-windows-msvc
cargo clippy -p wave-win32 --target x86_64-pc-windows-msvc -- -D warnings
```

That catches mistakes in the Win32 calls because it type-checks against the
real bindings — in seconds rather than twelve minutes on the Windows runner.
Only linking needs a Windows host.

See what a fixture produces:

```bash
cargo run -p wave-core --example dump-inventory -- \
  crates/wave-core/tests/fixtures/typical_machine.json

cargo run -p wave-core --example dump-scan -- \
  crates/wave-core/tests/fixtures/removal_machine.json "Acme Designer"
```

The second one prints what Wave would remove for a program, what it spared and
why, and the plan it would run — the whole decision, without a Windows machine
anywhere near it.

On Linux only the core builds. `wave-app` needs a WebView and `wave-win32` is
entirely `#[cfg(windows)]` — the Windows job in CI covers both.

## Website

The project site lives in [`site/`](./site) as static HTML with no build step.
It is deployed on Vercel with the root directory set to `site`; details are in
[`site/README.md`](./site/README.md).

## License

MIT — see [`LICENSE`](./LICENSE).
