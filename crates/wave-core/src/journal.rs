//! The append-only record of what Wave did.
//!
//! One JSON object per line, written *before* the action it describes and
//! again after it. That order is the whole point: a journal written afterwards
//! documents only the operations that finished, and the one that matters is
//! the one that did not — the folder that was moved when the power went out.
//!
//! It is also what undo reads. Nothing else knows where a backup came from.

use serde::{Deserialize, Serialize};

use crate::plan::Step;

/// One line of a journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Entry {
    /// The plan, before anything ran.
    Begin {
        operation_id: String,
        program_id: String,
        program_name: String,
        at: String,
        backup_root: String,
        step_count: usize,
    },
    /// What is about to happen. Written before the step runs.
    Intent { index: usize, step: Step },
    /// What happened. Written after.
    Outcome {
        index: usize,
        status: Status,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// The operation finished, successfully or not.
    End { at: String, done: usize, failed: usize, skipped: usize },
    /// Everything in this operation was put back.
    Undone { at: String, restored: usize, failed: usize },
}

/// How one step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Done,
    /// Nothing to do — usually because the program's own uninstaller had
    /// already removed it.
    Skipped,
    Failed,
}

/// Parse a journal, ignoring lines a newer version of Wave wrote.
///
/// An unreadable line is never a reason to refuse the whole journal: a partial
/// undo of a known-good prefix beats telling someone their backup is
/// unreadable.
pub fn parse(contents: &str) -> Vec<Entry> {
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// One line of JSON, without a trailing newline.
pub fn render(entry: &Entry) -> String {
    // A journal line that cannot be serialized would be a silent gap, so the
    // fallback is a line that still says something happened.
    serde_json::to_string(entry).unwrap_or_else(|error| {
        format!("{{\"event\":\"unserializable\",\"error\":{:?}}}", error.to_string())
    })
}

/// The steps that actually ran, newest last.
pub fn completed_steps(entries: &[Entry]) -> Vec<(usize, &Step)> {
    let mut intents: Vec<(usize, &Step)> = Vec::new();
    for entry in entries {
        if let Entry::Intent { index, step } = entry {
            intents.push((*index, step));
        }
    }

    intents
        .into_iter()
        .filter(|(index, _)| {
            entries.iter().any(|entry| {
                matches!(entry, Entry::Outcome { index: other, status: Status::Done, .. } if other == index)
            })
        })
        .collect()
}

/// Whether this operation has already been put back.
pub fn is_undone(entries: &[Entry]) -> bool {
    entries.iter().any(|entry| matches!(entry, Entry::Undone { .. }))
}
