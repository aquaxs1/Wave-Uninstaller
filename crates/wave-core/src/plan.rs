//! What exactly is going to happen, written down before anything happens.
//!
//! A plan is the thing a user approves. It names every action, where the
//! backup of it will land, and in which order — and it is also what the
//! journal records, so the plan a user saw and the plan that ran are the same
//! object rather than two descriptions that have to be kept in step.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::model::ProgramId;
use crate::path;
use crate::provider::{RegistryRoot, RegistryView, SystemProvider};
use crate::scan::{Finding, Scan, Target, Uninstaller};
use crate::time;

/// Where backups live, under `%ProgramData%`.
///
/// Deliberately not the recycle bin: the bin cannot hold registry keys, has a
/// per-drive size cap, and gets emptied without a second thought.
const BACKUP_SUBPATH: &str = r"Wave\Backups";

/// How long Wave waits for an uninstaller's process tree before giving up.
///
/// Generous on purpose. An uninstaller that is still working after ten
/// minutes is unusual, but a scan that starts while one is still running finds
/// files that vanish on their own a moment later.
pub const UNINSTALLER_TIMEOUT_SECONDS: u64 = 600;

/// What the user chose to do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    /// Run the program's own uninstaller first.
    pub run_uninstaller: bool,
    /// The ids of the findings to remove.
    pub findings: BTreeSet<String>,
}

impl Selection {
    /// Everything Wave preselected, plus the uninstaller.
    ///
    /// The default the UI opens with — not the default the code assumes: a
    /// plan is only built from what was actually passed in.
    pub fn recommended(scan: &Scan) -> Self {
        Self {
            run_uninstaller: scan.uninstaller.is_some(),
            findings: scan.preselected().map(|finding| finding.id.clone()).collect(),
        }
    }
}

/// One thing to do, and how to undo it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Step {
    /// Run the program's own uninstaller and wait for its whole process tree.
    RunUninstaller { program: String, arguments: Vec<String>, display: String, silent: bool },
    /// Hand a Store package to the package manager.
    RemoveStorePackage { package_family_name: String },
    /// Move a file or folder into the backup. This is what "delete" means here.
    MovePath { path: String, backup_path: String },
    /// Export a key and everything below it, then remove it.
    RemoveRegistryKey {
        root: RegistryRoot,
        path: String,
        view: RegistryView,
        backup_path: String,
        reg_file_path: String,
    },
    /// Export a single value, then remove it.
    RemoveRegistryValue {
        root: RegistryRoot,
        path: String,
        view: RegistryView,
        name: String,
        backup_path: String,
    },
}

impl Step {
    /// One line describing the step, as the UI and the journal show it.
    pub fn display(&self) -> String {
        match self {
            Self::RunUninstaller { display, .. } => format!("Run the uninstaller: {display}"),
            Self::RemoveStorePackage { package_family_name } => {
                format!("Remove the Store package {package_family_name}")
            }
            Self::MovePath { path, .. } => format!("Move {path} to the backup"),
            Self::RemoveRegistryKey { root, path, view, .. } => {
                format!("Remove {}\\{path} ({}-bit view)", root.short_name(), view_label(*view))
            }
            Self::RemoveRegistryValue { root, path, name, .. } => {
                format!("Remove the value {name} from {}\\{path}", root.short_name())
            }
        }
    }
}

/// Everything one removal will do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalPlan {
    pub operation_id: String,
    pub program_id: ProgramId,
    pub program_name: String,
    /// ISO 8601, UTC.
    pub created_at: String,
    /// The folder this operation's backups and journal live in.
    pub backup_root: String,
    pub journal_path: String,
    pub steps: Vec<Step>,
}

impl RemovalPlan {
    /// The bytes that will be moved, as far as the scan could measure them.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Turn a scan and a selection into an ordered plan.
pub fn build(provider: &dyn SystemProvider, scan: &Scan, selection: &Selection) -> RemovalPlan {
    let now = provider.now_unix_seconds();
    let operation_id = operation_id(now, scan.program.id.as_str());

    let program_data = provider.expand_env("%ProgramData%");
    let backup_root = path::join(&path::join(&program_data, BACKUP_SUBPATH), &operation_id);
    let journal_path = path::join(&backup_root, "journal.jsonl");

    let mut steps = Vec::new();

    if selection.run_uninstaller {
        match &scan.uninstaller {
            Some(Uninstaller::Command { program, arguments, display, silent }) => {
                steps.push(Step::RunUninstaller {
                    program: program.clone(),
                    arguments: arguments.clone(),
                    display: display.clone(),
                    silent: *silent,
                });
            }
            Some(Uninstaller::StorePackage { package_family_name }) => {
                steps.push(Step::RemoveStorePackage {
                    package_family_name: package_family_name.clone(),
                });
            }
            None => {}
        }
    }

    let chosen: Vec<&Finding> =
        scan.findings.iter().filter(|finding| selection.findings.contains(&finding.id)).collect();

    // Paths first, registry after: a key that still exists is what lets a
    // half-finished removal be recognized and finished, while a key removed
    // before the files would leave nothing pointing at them.
    let mut paths: Vec<&Finding> = chosen
        .iter()
        .copied()
        .filter(|finding| matches!(finding.target, Target::Path { .. }))
        .collect();
    // Deepest first, so that a folder is not moved out from under a child
    // that was also selected.
    paths.sort_by_key(|finding| std::cmp::Reverse(path::depth(&finding.display)));

    let mut moved: Vec<String> = Vec::new();
    for finding in paths {
        let Target::Path { path: candidate } = &finding.target else { continue };
        // A child of something already being moved travels with its parent;
        // listing it again would only produce a confusing failure.
        if moved.iter().any(|parent| path::is_within(candidate, parent)) {
            continue;
        }
        moved.push(candidate.clone());
        steps.push(Step::MovePath {
            backup_path: backup_path_for(&backup_root, candidate),
            path: candidate.clone(),
        });
    }

    let mut registry_index = 0usize;
    // Values before keys: a value inside a key that is about to go would
    // otherwise be reported as a failure by the time its turn came.
    for finding in chosen.iter().filter(|f| matches!(f.target, Target::RegistryValue { .. })) {
        let Target::RegistryValue { root, path: key_path, view, name } = &finding.target else {
            continue;
        };
        registry_index += 1;
        steps.push(Step::RemoveRegistryValue {
            root: *root,
            path: key_path.clone(),
            view: *view,
            name: name.clone(),
            backup_path: path::join(
                &backup_root,
                &format!("registry\\{registry_index:03}-value.json"),
            ),
        });
    }

    let mut keys: Vec<&Finding> = chosen
        .iter()
        .copied()
        .filter(|finding| matches!(finding.target, Target::RegistryKey { .. }))
        .collect();
    keys.sort_by_key(|finding| std::cmp::Reverse(finding.display.matches('\\').count()));

    let mut removed_keys: Vec<(RegistryRoot, String, RegistryView)> = Vec::new();
    for finding in keys {
        let Target::RegistryKey { root, path: key_path, view } = &finding.target else { continue };
        let lowered = key_path.to_ascii_lowercase();
        if removed_keys.iter().any(|(other_root, other, other_view)| {
            other_root == root
                && other_view == view
                && (lowered == *other || lowered.starts_with(&format!("{other}\\")))
        }) {
            continue;
        }
        removed_keys.push((*root, lowered, *view));

        registry_index += 1;
        steps.push(Step::RemoveRegistryKey {
            root: *root,
            path: key_path.clone(),
            view: *view,
            backup_path: path::join(
                &backup_root,
                &format!("registry\\{registry_index:03}-key.json"),
            ),
            reg_file_path: path::join(
                &backup_root,
                &format!("registry\\{registry_index:03}-key.reg"),
            ),
        });
    }

    RemovalPlan {
        operation_id,
        program_id: scan.program.id.clone(),
        program_name: scan.program.name.clone(),
        created_at: time::iso_utc(now),
        backup_root,
        journal_path,
        steps,
    }
}

/// Where a path's backup goes: the volume, then the path below it.
///
/// Keeping the structure means a restore is a move back rather than a guess,
/// and a user who opens the backup folder can see what came from where.
fn backup_path_for(backup_root: &str, candidate: &str) -> String {
    let volume = path::volume_label(candidate);
    let below = path::below_volume(candidate);
    path::join(&path::join(&path::join(backup_root, "files"), &volume), &below)
}

/// A folder name that sorts by time and cannot collide between two programs
/// removed in the same second.
fn operation_id(now: u64, program_id: &str) -> String {
    format!("{}-{:08x}", time::stamp(now), fingerprint(program_id))
}

/// FNV-1a, for a short stable suffix. Not a security hash and not used as one.
fn fingerprint(value: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn view_label(view: RegistryView) -> &'static str {
    match view {
        RegistryView::Bits64 => "64",
        RegistryView::Bits32 => "32",
    }
}
