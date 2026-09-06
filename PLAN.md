# Wave Uninstaller — Technischer Plan

Ein Windows-Uninstaller, der nach dem regulären Deinstallieren die Reste findet
und entfernt: verwaiste Registry-Schlüssel, zurückgelassene Ordner, Dienste,
geplante Tasks, Autostart-Einträge, Verknüpfungen, Prefetch-Dateien.

Leitprinzip: **Nichts wird gelöscht, nur verschoben.** Jede Operation landet
zuerst in einem Backup und ist über ein Journal vollständig umkehrbar. Ein
Uninstaller, der einmal das falsche Verzeichnis erwischt, ist unbenutzbar —
also wird Sicherheit vor Gründlichkeit priorisiert.

- **Stack:** Tauri 2, Rust-Kern (`windows`-Crate), React + TypeScript + Tailwind
- **Ziel:** Windows 10 (1809+) und 11, x64 + arm64
- **Scope v1:** Leftover-Scanner und Cleaner. **Kein** Installations-Monitoring
  per Filtertreiber (siehe [Bewusst nicht in v1](#bewusst-nicht-in-v1)).

---

## 1. Architektur

```
┌──────────────────────────────────────────────┐
│  UI  —  React + TS + Tailwind (Vite)         │
│  Programmliste · Scan-Wizard · Backups       │
└───────────────────┬──────────────────────────┘
                    │  Tauri Commands + Events (Progress-Streams)
┌───────────────────┴──────────────────────────┐
│  wave-core  (Rust, plattformunabhängige Logik)│
│  ├─ inventory   Was ist installiert?          │
│  ├─ scan        Was gehört dazu?              │
│  ├─ score       Wie sicher ist das?           │
│  ├─ plan        Was passiert konkret?         │
│  └─ apply       Backup → Ausführung → Journal │
└───────────────────┬──────────────────────────┘
                    │  trait SystemProvider
┌───────────────────┴──────────────────────────┐
│  wave-win32  (Rust, echte Windows-Aufrufe)    │
│  Registry · MSI · SCM · Task Scheduler · FS   │
└──────────────────────────────────────────────┘
```

Der entscheidende Schnitt ist `trait SystemProvider`. `wave-core` ruft nie
direkt Win32 auf, sondern geht über dieses Trait. Damit gibt es zwei
Implementierungen:

- `Win32Provider` — die echte, in `wave-win32`
- `FixtureProvider` — liest einen erfundenen Registry-/Dateisystem-Zustand aus
  einer JSON-Datei

Nur so lässt sich die Scan- und Scoring-Logik überhaupt testen: in CI auf
Linux, deterministisch, ohne VM, ohne echte Installation. Das ist keine
Architektur-Kosmetik — ohne diesen Schnitt ist jeder Test des Kerns ein
manueller Klick in einer VM.

### Crates

| Crate | Zweck |
|---|---|
| `windows` | Win32, COM, WinRT — Registry, MSI, SCM, Task Scheduler, Restart Manager |
| `tauri` 2 | Shell, IPC, Updater |
| `serde` / `serde_json` | Modelle, Journal, Fixtures |
| `jwalk` | paralleler Verzeichnis-Walk |
| `rayon` | Parallelisierung des Scorings |
| `strsim` | Fuzzy-Namensvergleich (Jaro-Winkler) |
| `rusqlite` | Scan-Cache und Backup-Index |
| `zip` | Backup-Archive |
| `tracing` + `tracing-appender` | strukturierte Logs auf Platte |
| `thiserror` | Fehlertypen |

### Rechte

v1 läuft komplett elevated (`requireAdministrator` im Manifest). Das ist
ehrlicher als ein UAC-Prompt mitten im Löschvorgang.

Ab v2 Split: die GUI läuft unprivilegiert, ein Helper-Prozess
`wave-elevated.exe` macht alle Schreibzugriffe und wird über eine Named Pipe
mit per-Session-Token angesprochen. Das reduziert die Angriffsfläche
erheblich, ist aber für einen ersten funktionierenden Stand unnötiger Aufwand.

Wichtig ab Tag 1, weil später schwer nachzurüsten: **jeder** Registry-Zugriff
muss beide Sichten abfragen, `KEY_WOW64_64KEY` und `KEY_WOW64_32KEY`. Ein
32-Bit-Programm auf einem 64-Bit-System liegt unter `WOW6432Node`, und wer das
vergisst, findet die Hälfte der Reste nicht.

---

## 2. Inventar — was ist überhaupt installiert?

Vier Quellen, die zusammengeführt werden:

**Registry-Uninstall-Keys** — die klassische Liste, die auch
"Apps & Features" zeigt:
- `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
- `HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall`
- `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`

Relevante Werte: `DisplayName`, `DisplayVersion`, `Publisher`,
`InstallLocation`, `InstallDate`, `UninstallString`, `QuietUninstallString`,
`EstimatedSize`, `SystemComponent` (=1 → ausblenden), `ParentKeyName`
(→ Updates unter das Hauptprodukt gruppieren).

**MSI-Datenbank** — `MsiEnumProducts` / `MsiGetProductInfo`. Der wertvolle
Teil ist `MsiEnumComponents` + `MsiGetComponentPath`: Windows weiß bei
MSI-Installationen **exakt**, welche Datei zu welchem Produkt gehört. Das ist
keine Heuristik, das ist die Wahrheit. Diese Zuordnung bekommt im Scoring das
höchste Gewicht und ist gleichzeitig der Schutz davor, Dateien zu löschen, die
einem *anderen* Produkt gehören.

**Store-/UWP-Apps** — `Windows.Management.Deployment.PackageManager` über
WinRT. Diese Apps deinstallieren sich sauber; Reste bleiben aber unter
`%LOCALAPPDATA%\Packages\<PackageFamilyName>` liegen, wenn der Nutzer die App
mal zurückgesetzt hat.

**Portable Programme** — stehen in keiner Registry. v1 zeigt sie nicht in der
Hauptliste, aber der "Junk-Hunter" (Phase 4) findet ihre Reste.

Deduplizierung über `(ProductCode | DisplayName+Publisher+Version)`. Ein
MSI-Produkt taucht typischerweise in Registry *und* MSI-Enumeration auf.

---

## 3. Deinstallations-Ablauf

```
 1. Snapshot (optional, Standard: an)
      Registry-Teilbäume + Datei-Index der Kandidatenpfade
 2. Nativen Uninstaller starten
      MSI:  msiexec /x {GUID} /passive /norestart
      EXE:  QuietUninstallString, sonst UninstallString
 3. Auf Prozessbaum warten
      Job Object, nicht nur auf die PID — NSIS/Inno starten Kindprozesse
      und beenden sich sofort selbst
 4. Scan: Snapshot-Diff + Heuristik
 5. Ergebnis in drei Töpfen zeigen
 6. Backup schreiben, dann anwenden
```

Zu Schritt 3 im Detail, weil hier fast jeder selbstgebaute Uninstaller
scheitert: NSIS kopiert sich nach `%TEMP%` und startet sich dort neu, Inno
Setup macht dasselbe mit `_iu*.tmp`, MSI delegiert an den `msiserver`-Dienst.
Wer nur auf die ursprüngliche PID wartet, scannt los, während der Uninstaller
noch läuft, und findet Dateien, die zwei Sekunden später von selbst
verschwinden. Lösung: Prozess in ein Job Object stecken und auf
`JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO` warten, mit Timeout und einem
"Uninstaller läuft noch"-Hinweis in der UI.

Der Snapshot in Schritt 1 ist bewusst optional: er kostet bei einem breiten
Datei-Index Zeit, und die Heuristik allein findet schon das meiste. Der Diff
liefert aber die Fälle, die keine Heuristik findet — Registry-Werte mit
kryptischen GUID-Namen etwa.

---

## 4. Wo Reste liegen

### Dateisystem

| Ort | Was |
|---|---|
| `%ProgramFiles%`, `%ProgramFiles(x86)%` | Restordner, leere Vendor-Ordner |
| `%ProgramData%` | Konfiguration, Lizenzen, Logs |
| `%LOCALAPPDATA%`, `%APPDATA%` | Profile, Caches, Einstellungen |
| `%LOCALAPPDATA%\Temp`, `%SystemRoot%\Temp` | Installer-Reste |
| `%LOCALAPPDATA%\Packages` | UWP-Container |
| `%ProgramData%\Package Cache` | von MSI zwischengespeicherte Installer |
| `C:\Windows\Installer` | verwaiste `.msi`/`.msp` — **nur** anzeigen, siehe unten |
| `C:\Windows\Prefetch` | `PROGRAMM.EXE-<HASH>.pf` |
| Startmenü (User + All Users) | Verknüpfungen |
| Desktop, Quick Launch | Verknüpfungen |
| `%APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations` | Jump Lists |
| `%USERPROFILE%\Documents` | **nur mit expliziter Zustimmung** — hier liegen Nutzerdaten |

`C:\Windows\Installer` ist eine Falle. Dateien dort sehen verwaist aus, werden
aber für Reparatur und Deinstallation *anderer* Produkte gebraucht. Wave zeigt
Funde dort an, wählt sie nie vor und schreibt eine Warnung dazu.

### Registry

- `HKCU\Software\<Vendor>`, `HKLM\SOFTWARE\<Vendor>` (+ `WOW6432Node`)
- Uninstall-Key des Produkts selbst
- `...\CurrentVersion\App Paths\<exe>`
- `...\Installer\Folders`, `Installer\Products`, `Features`, `Components`, `UpgradeCodes`
- `...\CurrentVersion\SharedDLLs` — Refcounts, siehe Schutzliste
- `HKCU\...\Explorer\FileExts\<.ext>` — Dateizuordnungen
- `HKCR` / `HKLM\SOFTWARE\Classes`: ProgIDs, `CLSID`, `TypeLib`, `Interface`
- Shell-Erweiterungen: `ContextMenuHandlers`, `ShellIconOverlayIdentifiers`
- Autostart: `Run`, `RunOnce` (HKLM + HKCU, beide Bitness), `StartupApproved`
- `HKLM\SYSTEM\CurrentControlSet\Services` — Dienste und Treiber
- Firewall-Regeln unter `...\SharedAccess\Parameters\FirewallPolicy\FirewallRules`
- Event-Log-Quellen, `Image File Execution Options`, `AppInit_DLLs`
- `MuiCache`, MRU-Listen

### Systemobjekte

- **Dienste** über SCM enumerieren, `ImagePath` gegen das Installationsverzeichnis prüfen
- **Geplante Tasks** über `ITaskService` (nicht die XML-Dateien direkt parsen)
- **Fonts**, die der Installer registriert hat
- **Browser-Erweiterungen** — nur melden, nie automatisch löschen

`WinSxS` wird niemals angefasst. Punkt.

---

## 5. Scoring — das eigentliche Herzstück

Jeder Fund bekommt eine Punktzahl aus gewichteten Signalen:

| Signal | Gewicht | Anmerkung |
|---|---|---|
| MSI-Komponente gehört diesem Produkt | sehr hoch | autoritativ, keine Heuristik |
| Pfad liegt unter `InstallLocation` | hoch | |
| Registry-Wert enthält den Installationspfad | hoch | |
| PE-Versionsinfo (`CompanyName`/`ProductName`) passt | mittel-hoch | aus den Ressourcen der gefundenen EXE/DLL |
| Authenticode-Signierer passt zum Publisher | mittel-hoch | |
| Vendor-Name im Pfad | mittel | normalisiert: `Inc`, `GmbH`, `LLC`, Satzzeichen weg, case-fold |
| Produktname als Token im Pfad | mittel | Jaro-Winkler auf Token-Ebene |
| Zeitstempel im Installationsfenster | niedrig | Zusatzindiz, nie allein tragend |

Daraus drei Töpfe:

- **≥ 85 — Sicher.** Vorausgewählt.
- **50–84 — Prüfen.** Angezeigt, nicht vorausgewählt.
- **< 50 — Unsicher.** Erst nach Klick auf "Alle anzeigen" sichtbar.

Jeder Fund trägt seine Begründung mit ("Pfad unter InstallLocation; Signierer
= Publisher"). Ohne diese Erklärung kann ein Nutzer die Auswahl nicht
beurteilen, und dann klickt er entweder blind auf Löschen oder gar nicht.

### Schutzliste — hart, vor dem Scoring

Was hier greift, wird gar nicht erst zum Fund, egal wie hoch der Score wäre:

- Windows-Verzeichnisse, `System32`, `SysWOW64`, `WinSxS`, `drivers`
- Alles, dessen Besitzer `TrustedInstaller` ist
- Pfade, die die `InstallLocation` eines **anderen** installierten Produkts
  sind oder darunter liegen — dafür liegt das komplette Inventar im Speicher
- Geteilte Laufzeiten: VC++ Redistributables, .NET, DirectX, Java
- `SharedDLLs` mit Refcount > 1
- Nutzerdaten-Verzeichnisse ohne explizites Opt-in

Diese Liste ist der Grund, warum das Inventar vollständig sein muss, bevor der
erste Scan läuft. Ein Fund ist nur dann ein Rest, wenn ihn niemand sonst
beansprucht.

---

## 6. Backup und Undo

Löschen heißt in Wave: verschieben.

- **Dateien** → `%ProgramData%\Wave\Backups\<opId>\` unter Beibehaltung der
  relativen Pfadstruktur, inklusive ACLs und Zeitstempeln
- **Registry** → strukturiertes JSON (Typ, Daten, Zeitstempel) plus eine
  `.reg`-Datei zum manuellen Import, falls Wave selbst mal kaputt ist
- **Dienste / Tasks** → vollständige Konfiguration als JSON, damit sie
  neu angelegt werden können
- **Journal** → append-only JSONL, geschrieben *vor* jeder einzelnen Aktion.
  Stürzt Wave mitten in einer Operation ab, erkennt der nächste Start die
  offene Transaktion und bietet Rollback an.
- **Systemwiederherstellungspunkt** vor riskanten Operationen, abschaltbar
- **Aufbewahrung** konfigurierbar (Standard: 30 Tage / 5 GB), danach echtes
  Löschen

Der Backup-Ordner liegt bewusst nicht im Papierkorb: der Papierkorb kann keine
Registry-Schlüssel, hat pro Laufwerk ein Größenlimit und wird von Nutzern
geleert, ohne nachzudenken.

---

## 7. Löschmechanik

Was in der Praxis schiefgeht, und was Wave dagegen tut:

- **Datei gesperrt** → Restart-Manager-API (`RmStartSession`,
  `RmRegisterResources`, `RmGetList`) sagt, *welcher* Prozess sperrt. Die UI
  zeigt "Spotify hält diese Datei" statt "Zugriff verweigert" und bietet an,
  den Prozess zu beenden.
- **Immer noch gesperrt** → `MoveFileEx` mit `MOVEFILE_DELAY_UNTIL_REBOOT`,
  plus Hinweis, dass ein Neustart nötig ist
- **Pfade > 260 Zeichen** → `\\?\`-Präfix und `longPathAware` im Manifest.
  Betrifft in der Praxis vor allem `node_modules` in Entwickler-Tools.
- **Junctions und Symlinks** → nie hineinlaufen, nur den Link entfernen.
  Sonst löscht ein Reparse Point in `AppData` das halbe Profil.
- **Keine Berechtigung** → optionales Übernehmen der Besitzrechte
  (`SeTakeOwnershipPrivilege`) hinter einer separaten Bestätigung
- **Sicheres Löschen** (Überschreiben) als Option, standardmäßig aus

---

## 8. UI

React + TypeScript + Tailwind, Vite als Bundler.

- **Programme** — Liste mit Suche, Sortierung nach Größe / Datum / Publisher,
  Icon aus der EXE extrahiert
- **Detail** — alles, was das Inventar über das Programm weiß
- **Deinstallieren** — Wizard: Snapshot → Uninstaller → Scan → Auswahl
- **Funde** — Baum mit Tri-State-Checkboxen, Risiko-Badge und Begründung pro
  Zeile, Gruppierung nach Kategorie (Dateien / Registry / Dienste / …)
- **Backups** — Verlauf, Wiederherstellen, Aufräumen
- **Einstellungen** — Scan-Tiefe, Wiederherstellungspunkte, Aufbewahrung, Sprache
- **Logs** — was Wave getan hat, im Klartext

Fortschritt kommt über Tauri-Events als Stream, nicht als ein einzelner
Command, der zwei Minuten blockiert.

Beim ersten Start ist der **Trockenlauf** aktiv: Wave zeigt, was es tun würde,
löscht aber nichts. Muss einmal bewusst abgeschaltet werden.

---

## 9. Testen

Ohne die `SystemProvider`-Abstraktion aus Abschnitt 1 wäre das hier nicht
möglich:

- **Unit-Tests** gegen `FixtureProvider` — ein erfundener Registry- und
  Dateisystem-Zustand als JSON, erwartete Funde als Golden File. Läuft auf
  Linux in CI, in Sekunden.
- **Integrationstests** auf `windows-latest` in GitHub Actions: echte
  Registry-Lese-Operationen gegen einen Testschlüssel unter `HKCU`, MSI-Enum,
  Datei-Walk in einem Temp-Verzeichnis.
- **Manuelle Matrix** in Windows Sandbox / Hyper-V mit Prüfpunkten, weil eine
  echte Deinstallation sich nicht sinnvoll mocken lässt. Mindestens: ein
  MSI-Programm (7-Zip), ein NSIS-Programm (Notepad++), ein Inno-Setup-Programm,
  eine Squirrel-App (Discord), eine UWP-App, ein 32-Bit-Programm auf 64-Bit.
- **Regressionsschutz:** jeder Fehlfund, der in der Praxis auftaucht, wird als
  Fixture eingefroren.

CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
`tsc --noEmit`, `eslint`, Build auf `windows-latest`.

---

## 10. Phasen

| Phase | Inhalt | Ergebnis |
|---|---|---|
| **P0** | Tauri-Scaffold, Manifest, CI, Logging, `SystemProvider`-Trait, Inventar aus allen vier Quellen | Read-only-Browser aller installierten Programme. Löscht nichts, ist aber sofort benutzbar. |
| **P1** | Nativen Uninstaller starten, Job-Object-Warten, Scan der `InstallLocation`, Uninstall-Key entfernen, Backup für Dateien | Ein Programm lässt sich deinstallieren, offensichtliche Reste verschwinden. |
| **P2** | Vollständiger Scan (Registry-Breite, Dienste, Tasks, Verknüpfungen), Scoring, Schutzliste | Das eigentliche Feature. Ab hier ist es ein Revo-Konkurrent. |
| **P3** | Journal, Undo, Wiederherstellungspunkte, Backup-Verwaltung | Sicherheitsnetz komplett. |
| **P4** | Junk-Hunter (Reste ohne Deinstallation finden), Autostart-Manager, Dienste-/Task-Ansicht, erzwungene Deinstallation bei kaputtem Uninstaller | Die Extras, für die man Revo kauft. |
| **P5** | i18n (de/en), Code-Signing, Installer (WiX), Tauri-Updater | Auslieferbar. |

Reihenfolge ist nicht verhandelbar: P3 muss vor P4 kommen. Erzwungene
Deinstallation ohne funktionierendes Undo ist ein Datenverlust-Feature.

---

## 11. Risiken

**Falsch-positive Funde zerstören Nutzerdaten.** Das größte Risiko, und
gleichzeitig der Grund für fast jede Design-Entscheidung hier: Backup statt
Löschen, konservative Voreinstellungen, Schutzliste vor Scoring, Trockenlauf
beim ersten Start, Begründung an jedem Fund.

**Virenscanner schlagen an.** Ein Programm, das Registry-Schlüssel löscht und
Dateien in `Program Files` verschiebt, sieht für Heuristiken aus wie Malware.
Gegenmittel: Code-Signing-Zertifikat (EV, sonst dauert der SmartScreen-Aufbau
Monate), frühzeitige Einreichung bei Microsoft und den großen Anbietern.

**Uninstaller verhalten sich unvorhersehbar.** Manche geben sofort zurück,
manche fragen nach, manche starten neu. Timeout plus manueller
"Uninstaller ist fertig"-Button als Notausgang.

**Windows ändert sich.** Registry-Layouts und APIs verschieben sich zwischen
Builds. Deshalb liegt das Wissen über Pfade in Datentabellen, nicht verstreut
im Code.

**Kein Telemetrie-Rückkanal.** Standardmäßig aus. Ein Tool mit
Systemrechten, das nach Hause telefoniert, verdient kein Vertrauen.

---

## Bewusst nicht in v1

- **Installations-Monitoring per Filtertreiber.** Revos Kernfeature, aber ein
  Kernel-Treiber braucht Signierung, WHQL-Aufwand und riskiert Bluescreens.
  Falls später gewünscht, geht das auch in Userspace über Snapshot-Vergleiche
  vorher/nachher — langsamer, aber ohne Treiber.
- **Alle Benutzerprofile.** v1 scannt nur den angemeldeten Nutzer. Andere
  Profile brauchen `RegLoadKey` auf fremde `NTUSER.DAT` — machbar, aber
  eigenständige Komplexität.
- **Browser-Cleaner, Autostart-Optimierung, "PC beschleunigen".** Anderes
  Produkt.

---

## Nächster Schritt

P0. Konkret: Tauri-Scaffold aufsetzen, `SystemProvider` definieren, das
Inventar aus Registry + MSI + AppX lesen und als sortierbare Liste anzeigen.
Das ist in sich abgeschlossen, löscht nichts und legt trotzdem die
Abstraktion fest, an der alles Weitere hängt.
