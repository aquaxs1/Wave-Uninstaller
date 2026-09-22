//! The seam between the removal logic and the writes it asks Windows for.
//!
//! [`SystemProvider`] reads, this trait writes — deliberately two traits, so
//! that the scan cannot change anything even by accident and so that every
//! decision about *what* to remove stays in `wave-core`, testable against
//! [`crate::fixture::RecordingMutator`] instead of against a real machine.
//!
//! Nothing here decides anything. A mutator moves the path it is given, runs
//! the command it is handed, deletes the key it is named. Whether that path
//! may be touched at all was settled in [`crate::protect`] long before.
//!
//! [`SystemProvider`]: crate::provider::SystemProvider

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::provider::{RegistryRoot, RegistryValues, RegistryView};

/// A registry key with everything below it, captured before deletion.
///
/// Kept as data rather than as a `.reg` export so that restoring it needs no
/// parser and no `reg.exe`. Wave writes the `.reg` file too, next to this, for
/// the case where Wave itself is what broke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryKeyBackup {
    pub root: RegistryRoot,
    pub path: String,
    pub view: RegistryView,
    pub values: RegistryValues,
    pub subkeys: Vec<RegistryKeyBackup>,
}

impl RegistryKeyBackup {
    /// Every key in the backup, parents before children.
    ///
    /// Restoring in this order matters: a subkey cannot be created before the
    /// key it hangs off exists.
    pub fn flatten(&self) -> Vec<&RegistryKeyBackup> {
        let mut out = vec![self];
        for subkey in &self.subkeys {
            out.extend(subkey.flatten());
        }
        out
    }
}

/// What became of a process Wave started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutcome {
    /// `None` when the process tree outlived the timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// True when Wave stopped waiting rather than the uninstaller finishing.
    pub timed_out: bool,
}

impl ProcessOutcome {
    /// Whether the uninstaller reported success.
    ///
    /// Uninstallers are not consistent about this: MSI returns 3010 for
    /// "done, needs a reboot", and plenty of NSIS scripts return 0 whatever
    /// happened. So a non-zero code is a warning shown to the user, never a
    /// reason to skip the cleanup — the leftovers are there either way.
    pub fn looks_successful(&self) -> bool {
        matches!(self.exit_code, Some(0) | Some(1641) | Some(3010)) && !self.timed_out
    }
}

/// The writes Wave performs, and nothing else.
pub trait SystemMutator: Send + Sync {
    /// Create a directory and every missing parent.
    fn create_directory(&self, path: &str) -> Result<()>;

    /// Move a file or directory, creating the destination's parents.
    ///
    /// This is how Wave "deletes". A reparse point is moved as the link it is;
    /// the implementation never walks through one.
    fn move_path(&self, from: &str, to: &str) -> Result<()>;

    /// Write a text file, replacing it if it exists.
    fn write_text_file(&self, path: &str, contents: &str) -> Result<()>;

    /// Append one line to a file and flush it before returning.
    ///
    /// The flush is the point. The journal is only worth having if a line that
    /// describes an action is on disk *before* the action runs — a buffered
    /// write that dies with the process protects nobody.
    fn append_line(&self, path: &str, line: &str) -> Result<()>;

    /// Capture a key and everything below it.
    fn export_registry_key(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
    ) -> Result<RegistryKeyBackup>;

    /// Delete a key and everything below it.
    fn delete_registry_key(&self, root: RegistryRoot, path: &str, view: RegistryView)
        -> Result<()>;

    /// Delete a single value.
    fn delete_registry_value(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
        name: &str,
    ) -> Result<()>;

    /// Recreate a key with the values it had. Used by undo only.
    fn restore_registry_key(&self, backup: &RegistryKeyBackup) -> Result<()>;

    /// Write a single value back. Used by undo only.
    fn restore_registry_value(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
        name: &str,
        value: &crate::provider::RegistryValue,
    ) -> Result<()>;

    /// Run a program and wait for its whole process tree.
    ///
    /// The tree, not the process: NSIS copies itself to `%TEMP%` and restarts
    /// there, Inno Setup does the same with `_iu*.tmp`, MSI hands the work to
    /// a service. Waiting on the first PID means scanning for leftovers while
    /// the uninstaller is still creating and deleting them.
    fn run_process(
        &self,
        program: &str,
        arguments: &[String],
        timeout_seconds: u64,
    ) -> Result<ProcessOutcome>;

    /// Remove a Store package for the current user.
    ///
    /// Store apps have no uninstall command; the package manager is the only
    /// way to remove one, and it takes the package's own install folder with
    /// it. The family name is what the inventory carries, so resolving it to
    /// the installed package is the implementation's job — a lookup, not a
    /// decision.
    fn remove_appx_package(&self, package_family_name: &str) -> Result<()>;
}
