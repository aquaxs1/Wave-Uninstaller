# Wave Uninstaller — technical plan

A Windows uninstaller that finds and removes what is left behind after the
regular uninstall: orphaned registry keys, abandoned folders, services,
scheduled tasks, startup entries, shortcuts, prefetch files.

Guiding principle: **nothing is deleted, only moved.** Every operation lands in
a backup first and is fully reversible through a journal. An uninstaller that
catches the wrong directory once is unusable — so safety comes before
thoroughness.

- **Stack:** Tauri 2, Rust core (`windows` crate), React + TypeScript + Tailwind
- **Target:** Windows 10 (1809+) and 11, x64 + arm64
- **Scope for v1:** leftover scanner and cleaner. **No** install monitoring via
  a filter driver (see [Deliberately not in v1](#deliberately-not-in-v1)).

---

## 1. Architecture

```
┌──────────────────────────────────────────────┐
│  UI  —  React + TS + Tailwind (Vite)         │
│  Program list · scan wizard · backups        │
└───────────────────┬──────────────────────────┘
                    │  Tauri commands + events (progress streams)
┌───────────────────┴──────────────────────────┐
│  wave-core  (Rust, platform-independent)     │
│  ├─ inventory   What is installed?           │
│  ├─ scan        What belongs to it?          │
│  ├─ score       How safe is this?            │
│  ├─ plan        What exactly will happen?    │
│  └─ apply       Backup → execute → journal   │
└───────────────────┬──────────────────────────┘
                    │  trait SystemProvider
┌───────────────────┴──────────────────────────┐
│  wave-win32  (Rust, the real Windows calls)  │
│  Registry · MSI · SCM · Task Scheduler · FS  │
└──────────────────────────────────────────────┘
```

The decisive cut is `trait SystemProvider`. `wave-core` never calls Win32
directly; it goes through that trait. Which gives two implementations:

- `Win32Provider` — the real one, in `wave-win32`
- `FixtureProvider` — reads an invented registry and file system state from a
  JSON file

Writing is a second trait, `SystemMutator`, for the same reason twice over: the
scan is handed the reader alone and so cannot change anything, and the removal
logic can be tested against a fixture that really moves its paths and really
drops its keys. `FixtureProvider::mutator()` returns exactly that, which is how
the undo tests assert that a machine ends up the way it started.

Without it the scan and scoring logic could not be tested at all: in CI, on
Linux, deterministically, with no VM and no real installation. This is not
architectural decoration — without that cut, every test of the core is a manual
click in a virtual machine.

### Crates

| Crate | Purpose |
|---|---|
| `windows` | Win32, COM, WinRT — registry, MSI, SCM, task scheduler, restart manager |
| `tauri` 2 | Shell, IPC, updater |
| `serde` / `serde_json` | Models, journal, fixtures |
| `jwalk` | Parallel directory walk |
| `rayon` | Parallel scoring |
| ~~`strsim`~~ | Fuzzy name comparison — written out in `text.rs` instead: forty lines, used for one comparison, and a signal that decides whether a folder is removed should be readable in the repository that removes it |
| `rusqlite` | Scan cache and backup index |
| `zip` | Backup archives |
| `tracing` + `tracing-appender` | Structured logs on disk |
| `thiserror` | Error types |

### Privileges

v1 runs fully elevated (`requireAdministrator` in the manifest). That is more
honest than a UAC prompt in the middle of a deletion.

From v2 on, a split: the UI runs unprivileged and a helper process,
`wave-elevated.exe`, performs every write, addressed over a named pipe with a
per-session token. That cuts the attack surface considerably, but it is
needless work for a first version that runs at all.

One thing matters from day one because it is hard to retrofit: **every**
registry read must query both views, `KEY_WOW64_64KEY` and `KEY_WOW64_32KEY`. A
32-bit program on a 64-bit system lives under `WOW6432Node`, and anyone who
forgets that misses half the leftovers.

---

## 2. Inventory — what is actually installed?

Four sources, merged:

**Registry uninstall keys** — the classic list, the same one Apps & Features
shows:

- `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
- `HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall`
- `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`

Relevant values: `DisplayName`, `DisplayVersion`, `Publisher`,
`InstallLocation`, `InstallDate`, `UninstallString`, `QuietUninstallString`,
`EstimatedSize`, `SystemComponent` (=1 → hide), `ParentKeyName` (→ group
updates under the parent product).

**The MSI database** — `MsiEnumProducts` / `MsiGetProductInfo`. The valuable
part is `MsiEnumComponents` + `MsiGetComponentPath`: for MSI installations
Windows knows **exactly** which file belongs to which product. That is not a
heuristic, it is the truth. This mapping carries the highest weight in scoring
and is at the same time the protection against deleting files that belong to
*another* product.

**Store / UWP apps** — `Windows.Management.Deployment.PackageManager` over
WinRT. These apps uninstall cleanly, but leftovers stay under
`%LOCALAPPDATA%\Packages\<PackageFamilyName>` when a user has reset the app at
some point.

**Portable programs** — in no registry at all. v1 does not show them in the
main list, but the junk hunter (phase 4) finds their leftovers.

Deduplication by `(ProductCode | DisplayName+Publisher+Version)`. An MSI
product typically appears in both the registry *and* the MSI enumeration.

---

## 3. The uninstall sequence

```
 1. Snapshot (optional, on by default)
      Registry subtrees + a file index of the candidate paths
 2. Start the native uninstaller
      MSI:  msiexec /x {GUID} /passive /norestart
      EXE:  QuietUninstallString, else UninstallString
 3. Wait for the process tree
      A job object, not just the PID — NSIS and Inno start child
      processes and terminate themselves immediately
 4. Scan: snapshot diff + heuristics
 5. Present the result in three buckets
 6. Write the backup, then apply
```

On step 3 in detail, because this is where nearly every home-grown uninstaller
fails: NSIS copies itself to `%TEMP%` and restarts there, Inno Setup does the
same with `_iu*.tmp`, MSI delegates to the `msiserver` service. Wait only on
the original PID and the scan starts while the uninstaller is still running,
finding files that disappear on their own two seconds later. The fix: put the
process into a job object and wait for
`JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO`, with a timeout and an "uninstaller still
running" hint in the UI.

The snapshot in step 1 is optional on purpose: a broad file index costs time,
and the heuristics alone already find most of it. But the diff catches the
cases no heuristic can — registry values with cryptic GUID names, for one.

---

## 4. Where leftovers live

### File system

| Location | What |
|---|---|
| `%ProgramFiles%`, `%ProgramFiles(x86)%` | Leftover folders, empty vendor folders |
| `%ProgramData%` | Configuration, licences, logs |
| `%LOCALAPPDATA%`, `%APPDATA%` | Profiles, caches, settings |
| `%LOCALAPPDATA%\Temp`, `%SystemRoot%\Temp` | Installer remains |
| `%LOCALAPPDATA%\Packages` | UWP containers |
| `%ProgramData%\Package Cache` | Installers cached by MSI |
| `C:\Windows\Installer` | Orphaned `.msi`/`.msp` — **display only**, see below |
| `C:\Windows\Prefetch` | `PROGRAM.EXE-<HASH>.pf` |
| Start menu (user + all users) | Shortcuts |
| Desktop, Quick Launch | Shortcuts |
| `%APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations` | Jump lists |
| `%USERPROFILE%\Documents` | **Only with explicit consent** — this is user data |

`C:\Windows\Installer` is a trap. Files there look orphaned but are needed to
repair and uninstall *other* products. Wave shows findings there, never
preselects them, and attaches a warning.

### Registry

- `HKCU\Software\<Vendor>`, `HKLM\SOFTWARE\<Vendor>` (+ `WOW6432Node`)
- The product's own uninstall key
- `...\CurrentVersion\App Paths\<exe>`
- `...\Installer\Folders`, `Installer\Products`, `Features`, `Components`, `UpgradeCodes`
- `...\CurrentVersion\SharedDLLs` — refcounts, see the protection list
- `HKCU\...\Explorer\FileExts\<.ext>` — file associations
- `HKCR` / `HKLM\SOFTWARE\Classes`: ProgIDs, `CLSID`, `TypeLib`, `Interface`
- Shell extensions: `ContextMenuHandlers`, `ShellIconOverlayIdentifiers`
- Startup: `Run`, `RunOnce` (HKLM + HKCU, both bitnesses), `StartupApproved`
- `HKLM\SYSTEM\CurrentControlSet\Services` — services and drivers
- Firewall rules under `...\SharedAccess\Parameters\FirewallPolicy\FirewallRules`
- Event log sources, `Image File Execution Options`, `AppInit_DLLs`
- `MuiCache`, MRU lists

### System objects

- **Services** enumerated through the SCM, checking `ImagePath` against the
  install directory
- **Scheduled tasks** through `ITaskService` (not by parsing the XML files
  directly)
- **Fonts** the installer registered
- **Browser extensions** — reported only, never deleted automatically

`WinSxS` is never touched. Full stop.

---

## 5. Scoring — the actual heart of it

Every finding gets a score from weighted signals:

| Signal | Weight | Note |
|---|---|---|
| MSI component belongs to this product | very high | Authoritative, not a heuristic |
| Path sits under `InstallLocation` | high | |
| Registry value contains the install path | high | |
| PE version info (`CompanyName`/`ProductName`) matches | medium-high | From the resources of any EXE/DLL found |
| Authenticode signer matches the publisher | medium-high | |
| Vendor name in the path | medium | Normalized: `Inc`, `GmbH`, `LLC`, punctuation removed, case-folded |
| Product name as a token in the path | medium | Jaro-Winkler at token level |
| Timestamp inside the install window | low | Corroboration, never load-bearing alone |

Which produces three buckets:

- **≥ 85 — safe.** Preselected.
- **50–84 — review.** Shown, not preselected.
- **< 50 — unsure.** Visible only after clicking "show everything".

Every finding carries its reasoning ("path under InstallLocation; signer =
publisher"). Without that explanation a user cannot judge the selection, and
then they either click delete blindly or never at all.

### The protection list — hard, and before scoring

Anything caught here never becomes a finding, no matter how high it would have
scored:

- Windows directories, `System32`, `SysWOW64`, `WinSxS`, `drivers`
- Anything owned by `TrustedInstaller`
- Paths that are, or sit under, the `InstallLocation` of **another** installed
  product — which is why the complete inventory is held in memory
- Shared runtimes: VC++ redistributables, .NET, DirectX, Java
- `SharedDLLs` with a refcount > 1
- User data directories without an explicit opt-in

This list is why the inventory has to be complete before the first scan runs. A
finding is only a leftover if nobody else claims it.

---

## 6. Backup and undo

In Wave, deleting means moving.

- **Files** → `%ProgramData%\Wave\Backups\<opId>\`, keeping their relative path
  structure, including ACLs and timestamps
- **Registry** → structured JSON (type, data, timestamp) plus a `.reg` file for
  manual import, in case Wave itself is broken
- **Services / tasks** → the full configuration as JSON, so they can be
  recreated
- **Journal** → append-only JSONL, written *before* every individual action. If
  Wave crashes mid-operation, the next start detects the open transaction and
  offers a rollback.
- **System restore point** before risky operations, switchable
- **Retention** configurable (default: 30 days / 5 GB), then a real delete

The backup folder deliberately is not the recycle bin: the bin cannot hold
registry keys, has a per-drive size limit, and gets emptied by users without a
second thought.

---

## 7. Deletion mechanics

What goes wrong in practice, and what Wave does about it:

- **File locked** → the restart manager API (`RmStartSession`,
  `RmRegisterResources`, `RmGetList`) says *which* process holds it. The UI
  shows "Spotify is holding this file" instead of "access denied", and offers
  to close it.
- **Still locked** → `MoveFileEx` with `MOVEFILE_DELAY_UNTIL_REBOOT`, plus a
  note that a restart is needed
- **Paths over 260 characters** → the `\\?\` prefix and `longPathAware` in the
  manifest. In practice this is mostly `node_modules` inside developer tools.
- **Junctions and symlinks** → never walk into them, remove only the link.
  Otherwise a reparse point in `AppData` deletes half the profile.
- **No permission** → optionally take ownership
  (`SeTakeOwnershipPrivilege`) behind a separate confirmation
- **Secure delete** (overwriting) as an option, off by default

---

## 8. The interface

React + TypeScript + Tailwind, Vite as the bundler.

- **Programs** — a list with search, sorting by size / date / publisher, icon
  extracted from the EXE
- **Detail** — everything the inventory knows about the program
- **Uninstall** — a wizard: snapshot → uninstaller → scan → selection
- **Findings** — a tree with tri-state checkboxes, a risk badge and the
  reasoning per row, grouped by category (files / registry / services / …)
- **Backups** — history, restore, clean up
- **Settings** — scan depth, restore points, retention, language
- **Logs** — what Wave did, in plain words

Progress arrives as a stream of Tauri events, not as one command that blocks
for two minutes.

There is no global dry-run switch, and deliberately so: a mode that has to be
turned off once is a mode people forget the state of. Instead every removal goes
through the same three steps — scan, review, apply — and the review step shows
the concrete plan, each finding with its reasoning, before anything is touched.
Nothing is ticked in advance that Wave is not sure about, and the button says
what it is about to move.

---

## 9. Testing

None of this is possible without the `SystemProvider` abstraction from
section 1:

- **Unit tests** against `FixtureProvider` — an invented registry and file
  system state as JSON, the expected findings as a golden file. Runs on Linux,
  in CI, in seconds.
- **Integration tests** on `windows-latest` in GitHub Actions: real registry
  reads against a test key under `HKCU`, MSI enumeration, a file walk in a temp
  directory.
- **A manual matrix** in Windows Sandbox / Hyper-V with checkpoints, because a
  real uninstall cannot be meaningfully mocked. At minimum: an MSI program
  (7-Zip), an NSIS program (Notepad++), an Inno Setup program, a Squirrel app
  (Discord), a UWP app, and a 32-bit program on 64-bit.
- **Regression cover:** every false positive that turns up in the wild is
  frozen as a fixture.

CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
`tsc --noEmit`, `eslint`, and a build on `windows-latest`.

---

## 10. Phases

| Phase | Content | Result |
|---|---|---|
| **P0** ✅ | Tauri scaffold, manifest, CI, logging, the `SystemProvider` trait, inventory from all four sources | A read-only browser of every installed program. Deletes nothing, but is usable at once. |
| **P1** ✅ | Start the native uninstaller, wait on a job object, scan the `InstallLocation`, remove the uninstall key, back up files | A program can be uninstalled and the obvious leftovers disappear. |
| **P2** ◐ | The full scan (registry breadth, services, tasks, shortcuts), scoring, protection list | Files, vendor keys, `App Paths`, startup values, start menu shortcuts, scoring and the protection list are in. Services, scheduled tasks, shell extensions, prefetch and jump lists are not. |
| **P3** ◐ | Journal, undo, restore points, backup management | Backup, journal and undo are in, and were built together with P1 rather than after it — removing without them was never an option. Restore points, retention and ACL-preserving copies are not. |
| **P4** | Junk hunter (find leftovers with no uninstall), startup manager, service and task views, forced uninstall when the uninstaller is broken | The extras people buy Revo for. |
| **P5** | i18n (de/en), code signing, an installer (WiX), the Tauri updater | Shippable. |

The order is not negotiable: P3 must come before P4. Forced uninstallation
without a working undo is a data-loss feature — which is why the backup, the
journal and the undo shipped with P1 instead of waiting for a phase of their
own.

---

## 11. Risks

**False positives destroy user data.** The largest risk, and the reason behind
nearly every design decision here: back up instead of delete, conservative
defaults, protection list before scoring, dry run on first start, reasoning
attached to every finding.

**Antivirus software objects.** A program that deletes registry keys and moves
files inside `Program Files` looks like malware to a heuristic. The remedy: a
code signing certificate (EV, or building SmartScreen reputation takes months)
and early submission to Microsoft and the major vendors.

**Uninstallers behave unpredictably.** Some return immediately, some ask
questions, some restart. A timeout plus a manual "the uninstaller has finished"
button as the escape hatch.

**Windows changes.** Registry layouts and APIs shift between builds. Which is
why knowledge about paths lives in data tables, not scattered through the code.

**No telemetry channel.** Off by default. A tool with system privileges that
phones home does not deserve trust.

---

## Deliberately not in v1

- **Install monitoring via a filter driver.** Revo's core feature, but a kernel
  driver needs signing, WHQL effort, and risks bluescreens. If it is wanted
  later, it can also be done in user space through before/after snapshot
  comparison — slower, but with no driver.
- **All user profiles.** v1 scans only the signed-in user. Other profiles need
  `RegLoadKey` on someone else's `NTUSER.DAT` — doable, but complexity of its
  own.
- **Browser cleaner, startup optimization, "speed up your PC".** A different
  product.

---

## Next step

The rest of P2. Concretely: services through the SCM and scheduled tasks through
`ITaskService`, both of which need new calls in `wave-win32` and new fixture
shapes in `wave-core` — and `MsiEnumComponents` + `MsiGetComponentPath`, which is
the one source that knows without guessing which file belongs to which product
and therefore outranks every heuristic in the scoring table.

After that, the rest of P3: ACLs and timestamps in the backup, retention, and an
optional system restore point before a removal.
