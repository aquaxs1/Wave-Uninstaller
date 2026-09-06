//! Programs read from the `...\CurrentVersion\Uninstall` keys.

use crate::model::{
    Architecture, InstallScope, Program, ProgramId, ProgramKind, ProgramSource, UninstallCommand,
};
use crate::provider::{RegistryRoot, RegistryValues, RegistryView, SystemProvider};
use crate::text;

const UNINSTALL_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

/// `ReleaseType` values that mark an entry as a patch rather than a program.
const UPDATE_RELEASE_TYPES: &[&str] = &["security update", "update rollup", "hotfix", "update"];

/// One place to enumerate, and what finding something there implies.
struct Location {
    root: RegistryRoot,
    view: RegistryView,
    scope: InstallScope,
    architecture: Architecture,
}

/// Every uninstall key location worth reading.
///
/// `HKLM` is split by the WOW64 redirector, so the 32-bit view is where every
/// 32-bit program on a 64-bit machine lives — reading only the native view
/// loses roughly half the list. `HKCU\Software\...\Uninstall` is *not*
/// redirected, so it is read once and the bitness stays unknown.
fn locations() -> Vec<Location> {
    vec![
        Location {
            root: RegistryRoot::LocalMachine,
            view: RegistryView::Bits64,
            scope: InstallScope::Machine,
            architecture: Architecture::X64,
        },
        Location {
            root: RegistryRoot::LocalMachine,
            view: RegistryView::Bits32,
            scope: InstallScope::Machine,
            architecture: Architecture::X86,
        },
        Location {
            root: RegistryRoot::CurrentUser,
            view: RegistryView::Bits64,
            scope: InstallScope::CurrentUser,
            architecture: Architecture::Unknown,
        },
    ]
}

/// Read every uninstall entry. Individual unreadable keys are skipped and
/// reported; a missing root key is normal and not an error.
pub fn collect(provider: &dyn SystemProvider, warnings: &mut Vec<String>) -> Vec<Program> {
    let mut programs = Vec::new();

    for location in locations() {
        let key_names =
            match provider.registry_subkeys(location.root, UNINSTALL_PATH, location.view) {
                Ok(names) => names,
                Err(error) if error.is_not_found() => continue,
                Err(error) => {
                    warnings.push(format!(
                        "{}\\{} ({:?}) could not be enumerated: {error}",
                        location.root.short_name(),
                        UNINSTALL_PATH,
                        location.view
                    ));
                    continue;
                }
            };

        for key_name in key_names {
            let path = format!("{UNINSTALL_PATH}\\{key_name}");
            match provider.registry_values(location.root, &path, location.view) {
                Ok(values) => {
                    if let Some(program) = build(provider, &location, &key_name, &values) {
                        programs.push(program);
                    }
                }
                // A key that vanished between enumeration and read is normal —
                // an uninstaller may be running right now.
                Err(error) if error.is_not_found() => {}
                Err(error) => warnings.push(format!(
                    "{}\\{path} could not be read: {error}",
                    location.root.short_name()
                )),
            }
        }
    }

    programs
}

fn build(
    provider: &dyn SystemProvider,
    location: &Location,
    key_name: &str,
    values: &RegistryValues,
) -> Option<Program> {
    let text_value = |name: &str| -> Option<String> {
        let raw = values.get(name)?.as_text()?.trim();
        if raw.is_empty() {
            return None;
        }
        Some(provider.expand_env(raw))
    };

    // Windows itself hides nameless entries, and so do we: there is nothing to
    // show and nothing a user could recognize.
    let name = text_value("DisplayName")?;

    let uninstall_string = text_value("UninstallString");
    let quiet_uninstall_string = text_value("QuietUninstallString");
    let msi_product_code = msi_product_code(key_name, values, uninstall_string.as_deref());

    let uninstall = match (&uninstall_string, &quiet_uninstall_string, &msi_product_code) {
        (None, None, None) => None,
        _ => Some(UninstallCommand {
            command: uninstall_string.clone().or_else(|| quiet_uninstall_string.clone()).or_else(
                || msi_product_code.as_ref().map(|code| format!("msiexec.exe /x {code}")),
            )?,
            quiet_command: quiet_uninstall_string,
            msi_product_code: msi_product_code.clone(),
        }),
    };

    Some(Program {
        id: ProgramId::uninstall_key(location.scope, location.architecture, key_name),
        kind: classify(values),
        version: text_value("DisplayVersion"),
        publisher: text_value("Publisher"),
        install_location: text_value("InstallLocation").filter(|p| !p.trim().is_empty()),
        install_date: text_value("InstallDate").as_deref().and_then(text::normalize_install_date),
        // Recorded in KiB. Installers write this by hand and often get it
        // wrong, so it is a sorting hint rather than a measurement.
        estimated_size_bytes: values
            .get("EstimatedSize")
            .and_then(|value| value.as_u64())
            .and_then(|kib| kib.checked_mul(1024)),
        icon_path: text_value("DisplayIcon"),
        parent_name: text_value("ParentDisplayName").or_else(|| text_value("ParentKeyName")),
        uninstall,
        sources: vec![ProgramSource::UninstallKey],
        scope: location.scope,
        architecture: location.architecture,
        name,
    })
}

fn classify(values: &RegistryValues) -> ProgramKind {
    // Shared runtimes and plumbing set this. Removing them breaks whatever
    // depends on them, so they stay out of the default view.
    if values.get("SystemComponent").and_then(|v| v.as_u64()).unwrap_or(0) == 1 {
        return ProgramKind::SystemComponent;
    }

    let release_type =
        values.get("ReleaseType").and_then(|value| value.as_text()).map(str::to_ascii_lowercase);
    let is_update =
        release_type.as_deref().is_some_and(|value| UPDATE_RELEASE_TYPES.contains(&value))
            || values.contains_key("ParentKeyName")
            || values.contains_key("ParentDisplayName");

    if is_update {
        ProgramKind::Update
    } else {
        ProgramKind::Application
    }
}

/// The MSI product code an uninstall key belongs to, if it belongs to one.
///
/// An MSI-authored key is *named* after the product code, which ties it to the
/// MSI database far more reliably than comparing display names. But the name
/// alone is not enough: Inno Setup also names its keys after a GUID (usually
/// with an `_is1` suffix, though that is a convention rather than a rule), and
/// treating one of those as an MSI product would offer the user an
/// `msiexec /x` command that fails. So the GUID has to be corroborated —
/// either by the `WindowsInstaller` flag MSI writes, or by an uninstall string
/// that actually invokes msiexec.
fn msi_product_code(
    key_name: &str,
    values: &RegistryValues,
    uninstall_string: Option<&str>,
) -> Option<String> {
    if !text::is_guid(key_name) {
        return None;
    }

    let flagged = values.get("WindowsInstaller").and_then(|value| value.as_u64()) == Some(1);
    let invokes_msiexec =
        uninstall_string.is_some_and(|command| command.to_ascii_lowercase().contains("msiexec"));

    (flagged || invokes_msiexec).then(|| key_name.to_ascii_uppercase())
}
