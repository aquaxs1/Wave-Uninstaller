//! Putting a removal back.
//!
//! Undo reads the journal, not the file system: it restores exactly the steps
//! that were recorded as done, in reverse order, and nothing else. A backup
//! folder with no journal is data, not an operation — it can be opened and
//! copied from by hand, but Wave will not pretend to know where it came from.
//!
//! One thing cannot be undone, and the report says so plainly: a program whose
//! own uninstaller has run is uninstalled. Its files come back, its
//! registration does not.

use serde::{Deserialize, Serialize};

use crate::apply::ValueBackup;
use crate::error::{Error, Result};
use crate::journal::{self, Entry, Status};
use crate::mutator::{RegistryKeyBackup, SystemMutator};
use crate::path;
use crate::plan::Step;
use crate::provider::SystemProvider;
use crate::time;

/// Where backups live, under `%ProgramData%`. Mirrors [`crate::plan`].
const BACKUP_SUBPATH: &str = r"Wave\Backups";

/// One past removal, as the history list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub program_name: String,
    /// ISO 8601, UTC.
    pub at: String,
    pub backup_root: String,
    pub journal_path: String,
    /// Steps that ran and can be reversed.
    pub restorable: usize,
    /// True once everything in it has been put back.
    pub undone: bool,
    /// True when the operation has an `End` line — a missing one means Wave
    /// stopped in the middle, and the backup is the only record of it.
    pub complete: bool,
}

/// What an undo did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoReport {
    pub operation_id: String,
    pub restored: usize,
    pub failed: usize,
    /// Steps nothing can undo, with the reason.
    pub irreversible: Vec<String>,
    pub problems: Vec<String>,
}

/// The backup folder Wave writes its operations into.
pub fn backup_root(provider: &dyn SystemProvider) -> String {
    path::join(&provider.expand_env("%ProgramData%"), BACKUP_SUBPATH)
}

/// Every operation with a readable journal, newest first.
pub fn list_operations(provider: &dyn SystemProvider) -> Vec<Operation> {
    let root = backup_root(provider);
    let Ok(entries) = provider.read_dir(&root) else { return Vec::new() };

    let mut operations: Vec<Operation> = entries
        .into_iter()
        .filter(|entry| entry.is_directory && !entry.is_reparse_point)
        .filter_map(|entry| read_operation(provider, &entry.path))
        .collect();

    // The folder names start with a timestamp, so this is chronological.
    operations.sort_by(|a, b| b.id.cmp(&a.id));
    operations
}

fn read_operation(provider: &dyn SystemProvider, folder: &str) -> Option<Operation> {
    let journal_path = path::join(folder, "journal.jsonl");
    let entries = journal::parse(&provider.read_text_file(&journal_path).ok()?);

    let begin = entries.iter().find_map(|entry| match entry {
        Entry::Begin { operation_id, program_name, at, backup_root, .. } => {
            Some((operation_id.clone(), program_name.clone(), at.clone(), backup_root.clone()))
        }
        _ => None,
    })?;

    Some(Operation {
        id: begin.0,
        program_name: begin.1,
        at: begin.2,
        backup_root: begin.3,
        journal_path,
        restorable: journal::completed_steps(&entries)
            .iter()
            .filter(|(_, step)| is_reversible(step))
            .count(),
        undone: journal::is_undone(&entries),
        complete: entries.iter().any(|entry| matches!(entry, Entry::End { .. })),
    })
}

/// Put one operation back.
pub fn undo(
    provider: &dyn SystemProvider,
    mutator: &dyn SystemMutator,
    operation_id: &str,
) -> Result<UndoReport> {
    let folder = path::join(&backup_root(provider), operation_id);
    let journal_path = path::join(&folder, "journal.jsonl");
    let contents = provider
        .read_text_file(&journal_path)
        .map_err(|_| Error::not_found(format!("a journal for operation {operation_id}")))?;
    let entries = journal::parse(&contents);

    let mut report = UndoReport {
        operation_id: operation_id.to_string(),
        restored: 0,
        failed: 0,
        irreversible: Vec::new(),
        problems: Vec::new(),
    };

    // Reverse order: a folder that was moved out of a parent has to go back
    // before the parent it came from is restored on top of it.
    for (_, step) in journal::completed_steps(&entries).into_iter().rev() {
        match restore(provider, mutator, step) {
            Restored::Yes => report.restored += 1,
            Restored::Irreversible(reason) => report.irreversible.push(reason),
            Restored::Failed(problem) => {
                report.failed += 1;
                report.problems.push(problem);
            }
        }
    }

    let line = journal::render(&Entry::Undone {
        at: time::iso_utc(provider.now_unix_seconds()),
        restored: report.restored,
        failed: report.failed,
    });
    if let Err(error) = mutator.append_line(&journal_path, &line) {
        report.problems.push(format!("the journal could not record the undo: {error}"));
    }

    Ok(report)
}

enum Restored {
    Yes,
    Irreversible(String),
    Failed(String),
}

fn is_reversible(step: &Step) -> bool {
    !matches!(step, Step::RunUninstaller { .. } | Step::RemoveStorePackage { .. })
}

fn restore(provider: &dyn SystemProvider, mutator: &dyn SystemMutator, step: &Step) -> Restored {
    match step {
        Step::RunUninstaller { display, .. } => Restored::Irreversible(format!(
            "the program's own uninstaller ran ({display}); reinstalling it is the only way back"
        )),
        Step::RemoveStorePackage { package_family_name } => Restored::Irreversible(format!(
            "{package_family_name} was removed by the package manager and has to be reinstalled \
             from the Store"
        )),

        Step::MovePath { path: original, backup_path } => {
            if !provider.path_exists(backup_path) {
                return Restored::Failed(format!("{backup_path} is no longer in the backup"));
            }
            if provider.path_exists(original) {
                return Restored::Failed(format!(
                    "{original} exists again — something was reinstalled there, so the backup was \
                     left untouched"
                ));
            }
            match mutator.move_path(backup_path, original) {
                Ok(()) => Restored::Yes,
                Err(error) => {
                    Restored::Failed(format!("{original} could not be restored: {error}"))
                }
            }
        }

        Step::RemoveRegistryKey { backup_path, .. } => {
            let contents = match provider.read_text_file(backup_path) {
                Ok(contents) => contents,
                Err(error) => {
                    return Restored::Failed(format!("{backup_path} could not be read: {error}"))
                }
            };
            let backup: RegistryKeyBackup = match serde_json::from_str(&contents) {
                Ok(backup) => backup,
                Err(error) => {
                    return Restored::Failed(format!("{backup_path} is not readable: {error}"))
                }
            };

            // Parents before children, or a subkey has nothing to hang off.
            for key in backup.flatten() {
                if let Err(error) = mutator.restore_registry_key(key) {
                    return Restored::Failed(format!(
                        "{}\\{} could not be restored: {error}",
                        key.root.short_name(),
                        key.path
                    ));
                }
            }
            Restored::Yes
        }

        Step::RemoveRegistryValue { backup_path, .. } => {
            let contents = match provider.read_text_file(backup_path) {
                Ok(contents) => contents,
                Err(error) => {
                    return Restored::Failed(format!("{backup_path} could not be read: {error}"))
                }
            };
            let backup: ValueBackup = match serde_json::from_str(&contents) {
                Ok(backup) => backup,
                Err(error) => {
                    return Restored::Failed(format!("{backup_path} is not readable: {error}"))
                }
            };

            match mutator.restore_registry_value(
                backup.root,
                &backup.path,
                backup.view,
                &backup.name,
                &backup.value,
            ) {
                Ok(()) => Restored::Yes,
                Err(error) => Restored::Failed(format!(
                    "{}\\{} → {} could not be restored: {error}",
                    backup.root.short_name(),
                    backup.path,
                    backup.name
                )),
            }
        }
    }
}

/// Journal entries in a form the UI can list, for one operation.
pub fn read_journal(provider: &dyn SystemProvider, operation_id: &str) -> Result<Vec<Entry>> {
    let journal_path =
        path::join(&path::join(&backup_root(provider), operation_id), "journal.jsonl");
    Ok(journal::parse(&provider.read_text_file(&journal_path)?))
}

/// Whether an operation left anything behind that failed.
pub fn had_failures(entries: &[Entry]) -> bool {
    entries.iter().any(|entry| matches!(entry, Entry::Outcome { status: Status::Failed, .. }))
}
