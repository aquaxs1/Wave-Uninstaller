//! The inventory read from an invented machine.
//!
//! Each test names the real-world situation it stands for. When Wave gets a
//! program wrong on someone's actual PC, the fix belongs here first as a
//! fixture, then in the code.

use wave_core::fixture::FixtureProvider;
use wave_core::model::{Architecture, InstallScope, ProgramKind, ProgramSource};
use wave_core::{inventory, Inventory, Program};

const TYPICAL_MACHINE: &str = include_str!("fixtures/typical_machine.json");

fn typical_machine() -> Inventory {
    let provider = FixtureProvider::from_json(TYPICAL_MACHINE).expect("fixture should parse");
    inventory::collect(&provider)
}

fn find<'a>(inventory: &'a Inventory, name: &str) -> &'a Program {
    inventory.programs.iter().find(|program| program.name == name).unwrap_or_else(|| {
        let names: Vec<&str> = inventory.programs.iter().map(|p| p.name.as_str()).collect();
        panic!("no program named {name:?}; found {names:?}")
    })
}

#[test]
fn lists_every_program_once() {
    let inventory = typical_machine();
    let names: Vec<&str> = inventory.programs.iter().map(|p| p.name.as_str()).collect();

    assert_eq!(
        names,
        vec![
            "7-Zip 24.09 (x64)",
            "Acme Designer",
            "Microsoft Visual C++ 2015 UWP Runtime Package",
            "Microsoft Visual C++ 2015-2022 Redistributable (x64)",
            "Microsoft Visual Studio Code (User)",
            "Notepad++ (32-bit x86)",
            "Security Update for Contoso Suite (KB5034123)",
            "Windows Terminal",
        ],
        "programs should be merged across sources and sorted by name"
    );
}

#[test]
fn merges_an_msi_product_with_its_own_uninstall_key() {
    let inventory = typical_machine();
    let seven_zip = find(&inventory, "7-Zip 24.09 (x64)");

    // The uninstall key is named after the product code, so the two entries
    // are provably the same product rather than two similarly named ones.
    assert_eq!(seven_zip.sources, vec![ProgramSource::UninstallKey, ProgramSource::Msi]);
    assert_eq!(seven_zip.id.as_str(), "msi:{23170F69-40C1-2702-2409-000001000000}");

    // MSI reports the product name as "7-Zip 24.09 (x64 edition)", but the
    // list has to match what Apps & Features shows.
    assert_eq!(seven_zip.name, "7-Zip 24.09 (x64)");

    // Only the registry view knows the bitness.
    assert_eq!(seven_zip.architecture, Architecture::X64);
    assert_eq!(seven_zip.install_date.as_deref(), Some("2025-01-14"));
    assert_eq!(seven_zip.estimated_size_bytes, Some(5120 * 1024));
}

#[test]
fn merges_by_name_when_the_key_carries_no_product_code() {
    let inventory = typical_machine();
    let acme = find(&inventory, "Acme Designer");

    // The uninstall key is named "AcmeDesigner", so there is no product code
    // to match on. "Acme Corporation" and "Acme Corp." have to be recognized
    // as the same publisher for these two rows to become one.
    assert_eq!(acme.sources, vec![ProgramSource::UninstallKey, ProgramSource::Msi]);

    let uninstall = acme.uninstall.as_ref().expect("should have an uninstall command");
    // The vendor's own uninstaller may do cleanup that msiexec skips, so it
    // stays the command of record — but the product code comes along.
    assert_eq!(uninstall.command, "C:\\Program Files\\Acme\\Designer\\setup.exe --uninstall");
    assert_eq!(
        uninstall.msi_product_code.as_deref(),
        Some("{7B1D3E44-0000-4000-8000-0000000000AA}")
    );
    // MSI's `/passive` variant belongs to msiexec, not to setup.exe. Offering
    // it here would give the UI a "silent uninstall" button that runs a
    // different program than the normal one — and skips the vendor cleanup
    // the normal one was kept for.
    assert_eq!(uninstall.quiet_command, None);
}

#[test]
fn finds_32_bit_programs_in_the_redirected_view() {
    let inventory = typical_machine();
    let notepad = find(&inventory, "Notepad++ (32-bit x86)");

    // Reading only the native registry view would lose this entry entirely.
    assert_eq!(notepad.architecture, Architecture::X86);
    assert_eq!(notepad.scope, InstallScope::Machine);
    assert_eq!(notepad.install_location.as_deref(), Some("C:\\Program Files (x86)\\Notepad++"));
}

#[test]
fn does_not_mistake_an_inno_setup_key_for_an_msi_product() {
    let inventory = typical_machine();
    let vscode = find(&inventory, "Microsoft Visual Studio Code (User)");

    let uninstall = vscode.uninstall.as_ref().expect("should have an uninstall command");
    // The key is named after a GUID, but nothing corroborates MSI. Offering
    // `msiexec /x` here would simply fail.
    assert_eq!(uninstall.msi_product_code, None);
    assert!(
        uninstall.quiet_command.as_deref().is_some_and(|c| c.contains("/SILENT")),
        "the installer's own silent switch should be kept: {:?}",
        uninstall.quiet_command
    );
    assert_eq!(vscode.scope, InstallScope::CurrentUser);
    assert_eq!(vscode.sources, vec![ProgramSource::UninstallKey]);
}

#[test]
fn expands_environment_variables_in_install_locations() {
    let inventory = typical_machine();

    assert_eq!(
        find(&inventory, "Microsoft Visual Studio Code (User)").install_location.as_deref(),
        Some("C:\\Users\\sebi\\AppData\\Local\\Programs\\Microsoft VS Code"),
        "an unexpanded %LOCALAPPDATA% would make every later path comparison miss"
    );
}

#[test]
fn hides_shared_runtimes_and_updates_from_the_default_view() {
    let inventory = typical_machine();

    assert_eq!(
        find(&inventory, "Microsoft Visual C++ 2015-2022 Redistributable (x64)").kind,
        ProgramKind::SystemComponent
    );
    assert_eq!(
        find(&inventory, "Microsoft Visual C++ 2015 UWP Runtime Package").kind,
        ProgramKind::SystemComponent,
        "an Appx framework package is a shared runtime like any other"
    );

    let update = find(&inventory, "Security Update for Contoso Suite (KB5034123)");
    assert_eq!(update.kind, ProgramKind::Update);
    assert_eq!(update.parent_name.as_deref(), Some("Contoso Suite"));

    let visible: Vec<&str> = inventory.user_facing().map(|p| p.name.as_str()).collect();
    assert_eq!(
        visible,
        vec![
            "7-Zip 24.09 (x64)",
            "Acme Designer",
            "Microsoft Visual Studio Code (User)",
            "Notepad++ (32-bit x86)",
            "Windows Terminal",
        ]
    );
}

#[test]
fn reads_store_packages() {
    let inventory = typical_machine();
    let terminal = find(&inventory, "Windows Terminal");

    assert_eq!(terminal.id.as_str(), "appx:Microsoft.WindowsTerminal_8wekyb3d8bbwe");
    assert_eq!(terminal.architecture, Architecture::Neutral);
    assert_eq!(terminal.scope, InstallScope::CurrentUser);
    // Store packages are removed through the package manager, not a command
    // line. P1 wires that up.
    assert_eq!(terminal.uninstall, None);
    // The manifest reports the publisher as a certificate subject.
    assert_eq!(terminal.publisher.as_deref(), Some("Microsoft Corporation"));
}

#[test]
fn skips_entries_that_have_nothing_to_show() {
    let inventory = typical_machine();

    assert!(
        !inventory.programs.iter().any(|p| p.name.is_empty()),
        "an uninstall key without a DisplayName is not something a user could act on"
    );
}

#[test]
fn reports_what_it_could_not_read() {
    let inventory = typical_machine();

    // The protection list depends on knowing about every installed program.
    // A source that failed silently could let a leftover scan mistake another
    // program's folder for garbage, so failures have to be visible.
    assert_eq!(inventory.warnings.len(), 2, "warnings: {:#?}", inventory.warnings);
    assert!(
        inventory.warnings.iter().any(|w| w.contains("LockedVendorEntry")),
        "warnings: {:#?}",
        inventory.warnings
    );
    assert!(
        inventory.warnings.iter().any(|w| w.contains("not-a-guid")),
        "warnings: {:#?}",
        inventory.warnings
    );
}

#[test]
fn survives_a_source_being_unavailable_entirely() {
    let provider = FixtureProvider::from_json(
        r#"{
            "registry": {
                "HKLM:64:SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Thing": {
                    "DisplayName": { "type": "sz", "data": "Thing" }
                }
            },
            "failures": { "msi": "the installer service is not running", "appx": "no user context" }
        }"#,
    )
    .expect("fixture should parse");

    let inventory = inventory::collect(&provider);

    assert_eq!(inventory.programs.len(), 1, "the registry entry should still be listed");
    assert_eq!(inventory.warnings.len(), 2);
}

#[test]
fn an_empty_machine_is_not_an_error() {
    let provider = FixtureProvider::from_json("{}").expect("fixture should parse");
    let inventory = inventory::collect(&provider);

    assert!(inventory.programs.is_empty());
    assert!(inventory.warnings.is_empty(), "a missing uninstall key is normal, not a failure");
}
