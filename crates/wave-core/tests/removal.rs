//! Removing a program from an invented machine.
//!
//! The fixture describes a PC with two programs from the same publisher
//! installed — which is the situation that separates a careful uninstaller
//! from a destructive one — plus a junction, a locked file and the documents
//! the user would least like to lose.
//!
//! Every test names the real-world situation it stands for. When Wave removes
//! the wrong thing on someone's actual PC, the fix belongs here first as a
//! fixture, then in the code.

use wave_core::fixture::{FixtureMutator, FixtureProvider};
use wave_core::journal::{self, Entry, Status};
use wave_core::plan::{self, Selection, Step};
use wave_core::protect::ProtectionList;
use wave_core::provider::{RegistryRoot, RegistryValue, RegistryView, SystemProvider};
use wave_core::scan::{Scan, Target};
use wave_core::score::Confidence;
use wave_core::{apply, inventory, scan, undo, Inventory, Program};

const REMOVAL_MACHINE: &str = include_str!("fixtures/removal_machine.json");

const DESIGNER: &str = "Acme Designer";
const INSTALL_FOLDER: &str = r"C:\Program Files\Acme\Designer";
const PROGRAM_DATA: &str = r"C:\ProgramData\Acme\Designer";
const ROAMING: &str = r"C:\Users\sebi\AppData\Roaming\Acme\Designer";
const LOCAL: &str = r"C:\Users\sebi\AppData\Local\Acme\Designer";
const SHORTCUT: &str = r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Acme Designer.lnk";
const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\AcmeDesigner";

struct Machine {
    provider: FixtureProvider,
    mutator: FixtureMutator,
    inventory: Inventory,
}

impl Machine {
    fn new() -> Self {
        let provider = FixtureProvider::from_json(REMOVAL_MACHINE).expect("the fixture parses");
        let mutator = provider.mutator();
        let inventory = inventory::collect(&provider);
        assert!(inventory.warnings.is_empty(), "the fixture should read cleanly");

        Self { provider, mutator, inventory }
    }

    fn program(&self, name: &str) -> Program {
        self.inventory
            .programs
            .iter()
            .find(|program| program.name == name)
            .unwrap_or_else(|| panic!("no program called {name}"))
            .clone()
    }

    fn scan(&self, name: &str) -> Scan {
        scan::scan(&self.provider, &self.inventory, &self.program(name))
    }

    fn exists(&self, path: &str) -> bool {
        self.provider.path_exists(path)
    }

    fn key_exists(&self, path: &str) -> bool {
        self.provider
            .registry_values(RegistryRoot::LocalMachine, path, RegistryView::Bits64)
            .is_ok()
    }

    fn journal(&self, plan: &plan::RemovalPlan) -> Vec<Entry> {
        journal::parse(
            &self.provider.read_text_file(&plan.journal_path).expect("a journal was written"),
        )
    }
}

fn displays(scan: &Scan) -> Vec<String> {
    scan.findings.iter().map(|finding| finding.display.clone()).collect()
}

fn find_id(scan: &Scan, needle: &str) -> String {
    scan.findings
        .iter()
        .find(|finding| finding.display.contains(needle))
        .unwrap_or_else(|| panic!("no finding for {needle} in {:?}", displays(scan)))
        .id
        .clone()
}

// ---- what the scan proposes ------------------------------------------------

#[test]
fn finds_what_the_program_scattered_across_the_machine() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);

    let found = displays(&scan);
    for expected in [INSTALL_FOLDER, PROGRAM_DATA, ROAMING, LOCAL, SHORTCUT] {
        assert!(
            found.iter().any(|display| display == expected),
            "{expected} missing from {found:?}"
        );
    }
    assert!(
        found.iter().any(|display| display.contains(UNINSTALL_KEY)),
        "the program's own uninstall key is the one finding that is not a heuristic"
    );
    assert!(
        found.iter().any(|display| display.contains("Run → Acme Designer")),
        "a startup entry pointing into the install folder is a leftover"
    );
    assert!(scan.warnings.is_empty(), "nothing in this fixture should fail to read");
}

#[test]
fn leaves_the_other_program_by_the_same_publisher_alone() {
    // The situation that separates a careful uninstaller from a destructive
    // one: Acme sells two programs, so the folder called Acme is not one
    // program's to take.
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);

    for spared in
        [r"C:\Program Files\Acme\Reader", r"C:\ProgramData\Acme", r"C:\Program Files\Acme"]
    {
        assert!(
            !displays(&scan).iter().any(|display| display == spared),
            "{spared} belongs to Acme, not to Acme Designer alone"
        );
    }
    assert!(
        !displays(&scan).iter().any(|display| display.contains("Acme Reader")),
        "nothing named after another installed program may be proposed"
    );

    // And it is said out loud rather than quietly dropped.
    assert!(
        scan.protected.iter().any(|skipped| skipped.display == r"C:\ProgramData\Acme"
            && skipped.reason.contains("shared")),
        "the shared folder should be listed as spared: {:?}",
        scan.protected
    );
}

#[test]
fn never_follows_a_junction() {
    // A junction in AppData is how a cleaner ends up deleting a folder
    // somewhere else entirely.
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);

    let junction = r"C:\Users\sebi\AppData\Local\Acme Designer";
    assert!(!displays(&scan).iter().any(|display| display == junction));
    assert!(
        scan.protected
            .iter()
            .any(|skipped| skipped.display == junction && skipped.reason.contains("junction")),
        "the junction should be named as skipped: {:?}",
        scan.protected
    );
}

#[test]
fn the_protection_list_outranks_any_score() {
    let machine = Machine::new();
    let program = machine.program(DESIGNER);
    let protection = ProtectionList::build(&machine.provider, &machine.inventory, &program);

    for forbidden in [
        r"C:\Windows",
        r"C:\Windows\System32",
        r"C:\Program Files",
        r"C:\Program Files\Common Files",
        r"C:\",
        r"C:\Users\sebi\Documents\Acme Designer",
        r"C:\Program Files\Acme\Reader",
    ] {
        assert!(
            protection.path_verdict(forbidden).is_protected(),
            "{forbidden} must never become a finding"
        );
    }

    // The program's own folder, by contrast, is exactly what may go.
    assert!(!protection.path_verdict(INSTALL_FOLDER).is_protected());

    for forbidden in [r"SYSTEM\CurrentControlSet\Services", r"SOFTWARE", r"SOFTWARE\Acme"] {
        assert!(
            protection
                .registry_key_verdict(RegistryRoot::LocalMachine, forbidden, RegistryView::Bits64)
                .is_protected(),
            "{forbidden} must never become a finding"
        );
    }
}

#[test]
fn only_what_is_certain_is_ticked_in_advance() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);

    let preselected: Vec<&str> =
        scan.preselected().map(|finding| finding.display.as_str()).collect();
    assert!(preselected.contains(&INSTALL_FOLDER));

    // The publisher's shared key holds settings for both products, so it is
    // shown for review rather than ticked.
    let vendor_key = scan
        .findings
        .iter()
        .find(|finding| finding.display.contains(r"SOFTWARE\Acme\Designer"))
        .expect("the product key under the publisher's key");
    assert_eq!(vendor_key.score.confidence, Confidence::Review);

    // Every finding carries its reasoning; a selection nobody can argue with
    // is one nobody can check.
    assert!(scan.findings.iter().all(|finding| !finding.score.evidence.is_empty()));
}

// ---- what the plan says will happen ----------------------------------------

#[test]
fn the_plan_runs_the_uninstaller_first_and_backs_up_everything_else() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);
    let plan = plan::build(&machine.provider, &scan, &Selection::recommended(&scan));

    assert!(matches!(plan.steps.first(), Some(Step::RunUninstaller { silent: true, .. })));
    assert!(plan.backup_root.starts_with(r"C:\ProgramData\Wave\Backups\"));

    // Every removal has somewhere to put what it removes.
    for step in &plan.steps {
        match step {
            Step::MovePath { backup_path, .. } => {
                assert!(backup_path.starts_with(&plan.backup_root), "{backup_path}")
            }
            Step::RemoveRegistryKey { backup_path, reg_file_path, .. } => {
                assert!(backup_path.starts_with(&plan.backup_root));
                assert!(reg_file_path.ends_with(".reg"));
            }
            Step::RemoveRegistryValue { backup_path, .. } => {
                assert!(backup_path.starts_with(&plan.backup_root))
            }
            _ => {}
        }
    }

    // The uninstall key goes last: while it is there, a half-finished removal
    // is still recognizable as this program.
    assert!(
        matches!(plan.steps.last(), Some(Step::RemoveRegistryKey { path, .. }) if path == UNINSTALL_KEY)
    );
}

#[test]
fn a_child_of_something_already_being_moved_is_not_listed_twice() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);

    let mut selection = Selection::recommended(&scan);
    // Pretend the scan also proposed a folder inside the install folder.
    let plan = plan::build(&machine.provider, &scan, &selection);
    let before = plan.steps.len();

    selection.findings.insert(find_id(&scan, INSTALL_FOLDER));
    let plan = plan::build(&machine.provider, &scan, &selection);
    assert_eq!(plan.steps.len(), before, "selecting the same finding twice changes nothing");
}

#[test]
fn nothing_outside_the_selection_reaches_the_plan() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);

    let only_the_shortcut = Selection {
        run_uninstaller: false,
        findings: [find_id(&scan, SHORTCUT)].into_iter().collect(),
    };
    let plan = plan::build(&machine.provider, &scan, &only_the_shortcut);

    assert_eq!(plan.steps.len(), 1);
    assert!(matches!(&plan.steps[0], Step::MovePath { path, .. } if path == SHORTCUT));
}

// ---- what applying it does -------------------------------------------------

#[test]
fn removing_means_moving_into_the_backup() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);
    let plan = plan::build(&machine.provider, &scan, &Selection::recommended(&scan));

    let report = apply::apply(&machine.provider, &machine.mutator, &plan).expect("the plan runs");

    assert!(report.is_clean(), "nothing should fail here: {:?}", report.results);
    assert!(report.journal_warnings.is_empty());

    // Gone from where it was...
    assert!(!machine.exists(PROGRAM_DATA));
    assert!(!machine.exists(ROAMING));
    assert!(!machine.exists(SHORTCUT));
    assert!(!machine.key_exists(UNINSTALL_KEY));

    // ...and present in the backup, path structure intact.
    let backed_up = format!(r"{}\files\c\programdata\acme\designer", plan.backup_root);
    assert!(machine.exists(&backed_up), "the folder should be in the backup, not deleted");

    // The registry backup is written twice: as data for Wave's own undo, and
    // as a .reg file for the day Wave is what is broken.
    let reg_file = plan
        .steps
        .iter()
        .find_map(|step| match step {
            Step::RemoveRegistryKey { reg_file_path, .. } => Some(reg_file_path.clone()),
            _ => None,
        })
        .expect("a key removal");
    let contents = machine.provider.read_text_file(&reg_file).expect("a .reg file");
    assert!(contents.starts_with("Windows Registry Editor Version 5.00"));
}

#[test]
fn every_step_is_journalled_before_it_runs() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);
    let plan = plan::build(&machine.provider, &scan, &Selection::recommended(&scan));
    apply::apply(&machine.provider, &machine.mutator, &plan).expect("the plan runs");

    let entries = machine.journal(&plan);
    assert!(matches!(entries.first(), Some(Entry::Begin { .. })));
    assert!(matches!(entries.last(), Some(Entry::End { .. })));

    // The intent for every step is on disk before its outcome. This is the
    // whole reason the journal exists: a crash in between has to leave a
    // record of what was in flight.
    for (index, _) in plan.steps.iter().enumerate() {
        let intent = entries
            .iter()
            .position(
                |entry| matches!(entry, Entry::Intent { index: other, .. } if *other == index),
            )
            .unwrap_or_else(|| panic!("no intent for step {index}"));
        let outcome = entries
            .iter()
            .position(
                |entry| matches!(entry, Entry::Outcome { index: other, .. } if *other == index),
            )
            .unwrap_or_else(|| panic!("no outcome for step {index}"));
        assert!(intent < outcome, "step {index} was journalled after the fact");
    }
}

#[test]
fn what_the_uninstaller_already_took_is_not_reported_as_a_failure() {
    // The realistic case: the program's own uninstaller removes its install
    // folder and its registry entry, and Wave's cleanup finds them gone.
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);
    let plan = plan::build(&machine.provider, &scan, &Selection::recommended(&scan));
    let report = apply::apply(&machine.provider, &machine.mutator, &plan).expect("the plan runs");

    let install_folder = report
        .results
        .iter()
        .find(|result| result.display.starts_with(&format!("Move {INSTALL_FOLDER}")))
        .expect("the install folder step");
    assert_eq!(install_folder.status, Status::Skipped);
    assert!(install_folder.detail.as_deref().unwrap_or_default().contains("already gone"));
    assert_eq!(report.failed, 0);

    // The uninstaller ran first, before anything was moved.
    let actions = machine.mutator.actions();
    let ran = actions.iter().position(|action| action.starts_with("run ")).expect("it ran");
    let moved = actions.iter().position(|action| action.starts_with("move ")).expect("a move");
    assert!(ran < moved);
}

#[test]
fn a_locked_file_fails_visibly_without_stopping_the_rest() {
    let mut fixture: wave_core::fixture::Fixture =
        serde_json::from_str(REMOVAL_MACHINE).expect("the fixture parses");
    fixture.failures.locked_paths.insert(ROAMING.to_string());
    let provider = FixtureProvider::new(fixture);
    let mutator = provider.mutator();
    let inventory = inventory::collect(&provider);
    let program = inventory.programs.iter().find(|p| p.name == DESIGNER).expect("the program");

    let scan = scan::scan(&provider, &inventory, program);
    let plan = plan::build(&provider, &scan, &Selection::recommended(&scan));
    let report = apply::apply(&provider, &mutator, &plan).expect("the plan runs");

    assert_eq!(report.failed, 1);
    let failed = report.results.iter().find(|result| result.status == Status::Failed).unwrap();
    assert!(failed.display.contains(ROAMING));
    assert!(failed.detail.as_deref().unwrap_or_default().contains("in use"));

    // Everything else still happened.
    assert!(provider.path_exists(ROAMING), "a file that could not be moved stays where it is");
    assert!(!provider.path_exists(PROGRAM_DATA));
}

// ---- and what undo does ----------------------------------------------------

#[test]
fn undo_puts_the_files_and_keys_back() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);
    let plan = plan::build(&machine.provider, &scan, &Selection::recommended(&scan));
    apply::apply(&machine.provider, &machine.mutator, &plan).expect("the plan runs");

    let report = undo::undo(&machine.provider, &machine.mutator, &plan.operation_id)
        .expect("the operation can be found");

    assert_eq!(report.failed, 0, "{:?}", report.problems);
    assert!(machine.exists(PROGRAM_DATA), "the folder should be back where it came from");
    assert!(machine.exists(ROAMING));
    assert!(machine.exists(SHORTCUT));

    let values = machine
        .provider
        .registry_values(
            RegistryRoot::LocalMachine,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            RegistryView::Bits64,
        )
        .expect("the Run key");
    assert!(
        matches!(values.get(DESIGNER), Some(RegistryValue::Text(_))),
        "the startup value should be restored with its type"
    );

    // What cannot be undone is said plainly rather than quietly ignored.
    assert!(
        report.irreversible.iter().any(|reason| reason.contains("uninstaller")),
        "{:?}",
        report.irreversible
    );
}

#[test]
fn the_history_lists_what_was_done_and_whether_it_was_undone() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);
    let plan = plan::build(&machine.provider, &scan, &Selection::recommended(&scan));
    apply::apply(&machine.provider, &machine.mutator, &plan).expect("the plan runs");

    let operations = undo::list_operations(&machine.provider);
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].program_name, DESIGNER);
    assert!(operations[0].complete);
    assert!(!operations[0].undone);
    assert!(operations[0].restorable > 0);

    undo::undo(&machine.provider, &machine.mutator, &plan.operation_id).expect("the undo runs");
    assert!(undo::list_operations(&machine.provider)[0].undone);
}

#[test]
fn a_store_app_is_handed_to_the_package_manager() {
    let machine = Machine::new();
    let mut fixture = machine.provider.snapshot();
    fixture.appx_packages.push(wave_core::provider::AppxPackage {
        package_family_name: "Acme.Sketch_1a2b3c".to_string(),
        name: "Acme.Sketch".to_string(),
        display_name: Some("Acme Sketch".to_string()),
        version: Some("2.0.0.0".to_string()),
        publisher: Some("CN=Acme Corporation".to_string()),
        install_location: None,
        is_framework: false,
    });
    let provider = FixtureProvider::new(fixture);
    let mutator = provider.mutator();
    let inventory = inventory::collect(&provider);
    let program = inventory.programs.iter().find(|p| p.name == "Acme Sketch").expect("the app");

    let scan = scan::scan(&provider, &inventory, program);
    let plan = plan::build(&provider, &scan, &Selection::recommended(&scan));
    apply::apply(&provider, &mutator, &plan).expect("the plan runs");

    assert!(
        mutator.actions().iter().any(|action| action == "remove-package Acme.Sketch_1a2b3c"),
        "a Store app has no uninstall command; the package manager is the only way: {:?}",
        mutator.actions()
    );
}

#[test]
fn a_finding_is_always_backed_by_a_target_that_can_be_acted_on() {
    let machine = Machine::new();
    let scan = machine.scan(DESIGNER);

    for finding in &scan.findings {
        match &finding.target {
            Target::Path { path } => assert!(machine.exists(path), "{path} does not exist"),
            Target::RegistryKey { path, .. } => assert!(machine.key_exists(path), "{path}"),
            Target::RegistryValue { path, name, .. } => {
                let values = machine
                    .provider
                    .registry_values(RegistryRoot::LocalMachine, path, RegistryView::Bits64)
                    .expect("the key");
                assert!(values.contains_key(name), "{path} has no value {name}");
            }
        }
    }
}
