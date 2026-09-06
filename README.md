# Wave Uninstaller

Ein Uninstaller für Windows, der nicht nur das Programm entfernt, sondern auch
die Reste: verwaiste Registry-Schlüssel, zurückgelassene Ordner in `AppData` und
`ProgramData`, Dienste, geplante Tasks, Autostart-Einträge und Verknüpfungen.

Vorbild ist Revo Uninstaller — mit dem Unterschied, dass **jede** Löschung erst
in ein Backup wandert und vollständig rückgängig gemacht werden kann.

> **Status:** Planungsphase. Es gibt noch keinen Code.
> Die technische Planung steht in [`PLAN.md`](./PLAN.md).

## Stack

- **Shell:** Tauri 2
- **Kern:** Rust (`windows`-Crate für Registry, MSI, Dienste, Task Scheduler)
- **UI:** React + TypeScript + Tailwind
- **Ziel:** Windows 10/11, x64 und arm64

## Lizenz

Noch nicht festgelegt.
