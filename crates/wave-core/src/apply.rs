//! Carrying out a plan.
//!
//! Three rules hold for every step, and they are the reason this file is
//! allowed to exist at all:
//!
//! 1. Nothing is deleted. Files move into the operation's backup folder,
//!    registry keys are exported first and written back out as `.reg` next to
//!    the JSON.
//! 2. Every step is journalled *before* it runs. A crash in the middle leaves
//!    a record of what was in flight, which is what undo needs.
//! 3. A failed step does not stop the rest. A half-removed program with one
//!    locked file is a worse outcome than a fully removed one plus a warning.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::journal::{self, Entry, Status};
use crate::mutator::{ProcessOutcome, SystemMutator};
use crate::plan::{RemovalPlan, Step, UNINSTALLER_TIMEOUT_SECONDS};
use crate::provider::SystemProvider;
use crate::regfile;
use crate::time;

/// What one step did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    pub display: String,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// What a removal did, in full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyReport {
    pub operation_id: String,
    pub program_name: String,
    pub backup_root: String,
    pub journal_path: String,
    pub results: Vec<StepResult>,
    /// Present when the program's own uninstaller ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uninstaller: Option<ProcessOutcome>,
    pub done: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Journal lines that could not be written.
    ///
    /// Not cosmetic: a step whose line is missing is a step undo cannot put
    /// back, so this belongs in front of the user rather than in a log file.
    pub journal_warnings: Vec<String>,
}

impl ApplyReport {
    pub fn is_clean(&self) -> bool {
        self.failed == 0
    }
}

/// Run a plan.
///
/// Returns an error only when the backup folder itself cannot be created —
/// the one failure that must stop everything, because without it there is
/// nowhere to put what is being removed, and removing without a backup is the
/// thing Wave does not do.
pub fn apply(
    provider: &dyn SystemProvider,
    mutator: &dyn SystemMutator,
    plan: &RemovalPlan,
) -> Result<ApplyReport> {
    prepare_backup(mutator, plan)?;

    let mut journal_warnings = Vec::new();
    write_line(
        mutator,
        plan,
        &mut journal_warnings,
        &Entry::Begin {
            operation_id: plan.operation_id.clone(),
            program_id: plan.program_id.as_str().to_string(),
            program_name: plan.program_name.clone(),
            at: plan.created_at.clone(),
            backup_root: plan.backup_root.clone(),
            step_count: plan.steps.len(),
        },
    );

    let mut report = ApplyReport {
        operation_id: plan.operation_id.clone(),
        program_name: plan.program_name.clone(),
        backup_root: plan.backup_root.clone(),
        journal_path: plan.journal_path.clone(),
        results: Vec::new(),
        uninstaller: None,
        done: 0,
        skipped: 0,
        failed: 0,
        journal_warnings: Vec::new(),
    };

    for (index, step) in plan.steps.iter().enumerate() {
        // Before, not after. This line is the only thing that knows a folder
        // is on its way into the backup if the machine stops here.
        write_line(
            mutator,
            plan,
            &mut journal_warnings,
            &Entry::Intent { index, step: step.clone() },
        );

        let outcome = run_step(provider, mutator, step, &mut report);
        let (status, detail) = match outcome {
            Ok(detail) => (Status::Done, detail),
            Err(StepFailure::Skipped(reason)) => (Status::Skipped, Some(reason)),
            Err(StepFailure::Failed(error)) => (Status::Failed, Some(error)),
        };

        match status {
            Status::Done => report.done += 1,
            Status::Skipped => report.skipped += 1,
            Status::Failed => report.failed += 1,
        }

        write_line(
            mutator,
            plan,
            &mut journal_warnings,
            &Entry::Outcome { index, status, detail: detail.clone() },
        );
        report.results.push(StepResult { display: step.display(), status, detail });
    }

    write_line(
        mutator,
        plan,
        &mut journal_warnings,
        &Entry::End {
            at: time::iso_utc(provider.now_unix_seconds()),
            done: report.done,
            failed: report.failed,
            skipped: report.skipped,
        },
    );

    report.journal_warnings = journal_warnings;
    Ok(report)
}

/// Why a step did not complete.
enum StepFailure {
    /// There was nothing to do.
    Skipped(String),
    Failed(String),
}

fn run_step(
    provider: &dyn SystemProvider,
    mutator: &dyn SystemMutator,
    step: &Step,
    report: &mut ApplyReport,
) -> std::result::Result<Option<String>, StepFailure> {
    match step {
        Step::RunUninstaller { program, arguments, .. } => {
            let outcome = mutator
                .run_process(program, arguments, UNINSTALLER_TIMEOUT_SECONDS)
                .map_err(|error| StepFailure::Failed(error.to_string()))?;

            let detail = if outcome.timed_out {
                Some(format!(
                    "the uninstaller was still running after {UNINSTALLER_TIMEOUT_SECONDS} seconds; \
                     the cleanup below may have found files it was still using"
                ))
            } else {
                // Uninstallers are inconsistent about exit codes — 3010 means
                // "done, reboot needed", and plenty return 0 regardless — so a
                // code is reported, never acted on.
                outcome.exit_code.filter(|code| *code != 0).map(|code| format!("exit code {code}"))
            };
            report.uninstaller = Some(outcome);
            Ok(detail)
        }

        Step::RemoveStorePackage { package_family_name } => mutator
            .remove_appx_package(package_family_name)
            .map(|()| None)
            .map_err(|error| StepFailure::Failed(error.to_string())),

        Step::MovePath { path, backup_path } => {
            if !provider.path_exists(path) {
                return Err(StepFailure::Skipped(
                    "already gone — the program's own uninstaller removed it".to_string(),
                ));
            }
            mutator
                .move_path(path, backup_path)
                .map(|()| Some(format!("moved to {backup_path}")))
                .map_err(|error| StepFailure::Failed(error.to_string()))
        }

        Step::RemoveRegistryKey { root, path, view, backup_path, reg_file_path } => {
            let backup = match mutator.export_registry_key(*root, path, *view) {
                Ok(backup) => backup,
                Err(error) if error.is_not_found() => {
                    return Err(StepFailure::Skipped(
                        "already gone — the program's own uninstaller removed it".to_string(),
                    ))
                }
                Err(error) => return Err(StepFailure::Failed(error.to_string())),
            };

            let json = serde_json::to_string_pretty(&backup)
                .map_err(|error| StepFailure::Failed(error.to_string()))?;
            mutator
                .write_text_file(backup_path, &json)
                .map_err(|error| StepFailure::Failed(error.to_string()))?;
            // The second copy, in the format regedit imports, for the day Wave
            // itself is what is broken.
            let _ = mutator.write_text_file(reg_file_path, &regfile::render(&backup));

            mutator
                .delete_registry_key(*root, path, *view)
                .map(|()| Some(format!("exported to {backup_path}")))
                .map_err(|error| StepFailure::Failed(error.to_string()))
        }

        Step::RemoveRegistryValue { root, path, view, name, backup_path } => {
            let values = match provider.registry_values(*root, path, *view) {
                Ok(values) => values,
                Err(error) if error.is_not_found() => {
                    return Err(StepFailure::Skipped("the key is already gone".to_string()))
                }
                Err(error) => return Err(StepFailure::Failed(error.to_string())),
            };
            let Some(value) = values.get(name) else {
                return Err(StepFailure::Skipped("the value is already gone".to_string()));
            };

            let json = serde_json::to_string_pretty(&ValueBackup {
                root: *root,
                path: path.clone(),
                view: *view,
                name: name.clone(),
                value: value.clone(),
            })
            .map_err(|error| StepFailure::Failed(error.to_string()))?;
            mutator
                .write_text_file(backup_path, &json)
                .map_err(|error| StepFailure::Failed(error.to_string()))?;

            mutator
                .delete_registry_value(*root, path, *view, name)
                .map(|()| Some(format!("exported to {backup_path}")))
                .map_err(|error| StepFailure::Failed(error.to_string()))
        }
    }
}

/// A single registry value, captured before removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueBackup {
    pub root: crate::provider::RegistryRoot,
    pub path: String,
    pub view: crate::provider::RegistryView,
    pub name: String,
    pub value: crate::provider::RegistryValue,
}

/// Create the backup folder and drop the plan into it.
fn prepare_backup(mutator: &dyn SystemMutator, plan: &RemovalPlan) -> Result<()> {
    mutator.create_directory(&plan.backup_root).map_err(|error| Error::System {
        subject: format!("the backup folder {}", plan.backup_root),
        source: Box::new(error),
    })?;
    mutator.create_directory(&crate::path::join(&plan.backup_root, "files"))?;
    mutator.create_directory(&crate::path::join(&plan.backup_root, "registry"))?;

    let plan_json =
        serde_json::to_string_pretty(plan).map_err(|error| Error::system("the plan", error))?;
    mutator.write_text_file(&crate::path::join(&plan.backup_root, "plan.json"), &plan_json)
}

/// Append one journal line.
///
/// A journal that cannot be written does not stop the removal — but it does
/// change what the removal *is*, from reversible to not, so the failure is
/// collected and shown rather than swallowed.
fn write_line(
    mutator: &dyn SystemMutator,
    plan: &RemovalPlan,
    warnings: &mut Vec<String>,
    entry: &Entry,
) {
    if let Err(error) = mutator.append_line(&plan.journal_path, &journal::render(entry)) {
        tracing::error!(
            journal = %plan.journal_path,
            %error,
            "a journal line could not be written; this operation may not be undoable"
        );
        warnings.push(format!(
            "{} could not be written ({error}) — what follows it may not be undoable",
            plan.journal_path
        ));
    }
}
