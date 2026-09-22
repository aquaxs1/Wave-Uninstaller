//! A machine made of JSON instead of Windows.
//!
//! This is what makes the removal logic testable at all. A fixture describes
//! an invented machine — registry keys, MSI rows, Store packages, folders —
//! and the same code that runs against a real PC runs against it, on any
//! platform, in milliseconds, with the expected outcome checked in next to the
//! input.
//!
//! [`FixtureProvider`] reads that machine. [`FixtureMutator`] changes it: a
//! move really moves the paths, a deleted key really disappears from the
//! fixture. So a test can remove a program, assert what the journal says, run
//! the undo and assert the machine is back the way it started — which is the
//! only way to check an undo without a virtual machine and a backup tape.
//!
//! Every false positive found in the wild should end up here as a fixture, so
//! the same mistake cannot come back.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::mutator::{ProcessOutcome, RegistryKeyBackup, SystemMutator};
use crate::path;
use crate::provider::{
    AppxPackage, DirEntry, MsiProduct, RegistryRoot, RegistryValue, RegistryValues, RegistryView,
    SystemProvider,
};

/// The on-disk shape of a fixture file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Fixture {
    /// Registry keys addressed by their full path, e.g.
    /// `HKLM:64:SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\7-Zip`.
    ///
    /// Flat rather than nested: subkey enumeration is derived by prefix, which
    /// keeps fixtures readable and avoids a tree that has to be kept in sync
    /// with itself.
    #[serde(default)]
    pub registry: BTreeMap<String, RegistryValues>,

    #[serde(default)]
    pub msi_products: Vec<MsiProduct>,

    #[serde(default)]
    pub appx_packages: Vec<AppxPackage>,

    /// Variables for `%NAME%` expansion.
    #[serde(default)]
    pub environment: BTreeMap<String, String>,

    /// Paths that exist on the invented machine.
    ///
    /// A path is a directory when it is listed in [`Fixture::directories`] or
    /// when another path sits below it; everything else is a file.
    #[serde(default)]
    pub paths: BTreeSet<String>,

    /// Paths that are directories even though nothing is inside them.
    #[serde(default)]
    pub directories: BTreeSet<String>,

    /// Junctions and symlinks. Wave must never walk into one.
    #[serde(default)]
    pub reparse_points: BTreeSet<String>,

    /// File sizes, for the ones a test cares about.
    #[serde(default)]
    pub file_sizes: BTreeMap<String, u64>,

    /// Files with contents — journals and backup manifests.
    #[serde(default)]
    pub text_files: BTreeMap<String, String>,

    /// What the clock says, so journals are reproducible.
    #[serde(default = "default_clock")]
    pub now_unix_seconds: u64,

    /// What happens when Wave starts the uninstaller.
    #[serde(default)]
    pub uninstaller: UninstallerBehaviour,

    /// Reads and writes that should fail, so the error paths get covered too.
    #[serde(default)]
    pub failures: Failures,
}

/// 2026-01-01T00:00:00Z — a fixed instant, because a test that renders the
/// real clock into a journal cannot assert what it wrote.
fn default_clock() -> u64 {
    1_767_225_600
}

/// How the invented uninstaller behaves when it is run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UninstallerBehaviour {
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub timed_out: bool,
    /// Paths the uninstaller takes with it — the realistic case, where Wave's
    /// own cleanup then finds half of its findings already gone.
    #[serde(default)]
    pub removes_paths: BTreeSet<String>,
    /// Registry addresses the uninstaller removes, in `ROOT:VIEW:PATH` form.
    #[serde(default)]
    pub removes_registry: BTreeSet<String>,
}

impl Default for UninstallerBehaviour {
    fn default() -> Self {
        Self {
            exit_code: Some(0),
            timed_out: false,
            removes_paths: BTreeSet::new(),
            removes_registry: BTreeSet::new(),
        }
    }
}

/// Simulated failures.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Failures {
    /// Registry paths, in the same `ROOT:VIEW:PATH` form as [`Fixture::registry`],
    /// that return "access denied".
    #[serde(default)]
    pub registry_access_denied: BTreeSet<String>,
    /// When set, [`SystemProvider::msi_products`] fails with this message.
    #[serde(default)]
    pub msi: Option<String>,
    /// When set, [`SystemProvider::appx_packages`] fails with this message.
    #[serde(default)]
    pub appx: Option<String>,
    /// Paths that cannot be moved — the locked file every real cleanup meets.
    #[serde(default)]
    pub locked_paths: BTreeSet<String>,
}

/// A provider that answers from a [`Fixture`].
///
/// Cheap to clone: clones share one machine, which is what lets a provider and
/// the mutator built from it see each other's changes.
#[derive(Debug, Clone)]
pub struct FixtureProvider {
    state: Arc<Mutex<Fixture>>,
}

impl FixtureProvider {
    pub fn new(fixture: Fixture) -> Self {
        Self { state: Arc::new(Mutex::new(fixture)) }
    }

    pub fn from_json(json: &str) -> std::result::Result<Self, serde_json::Error> {
        Ok(Self::new(serde_json::from_str(json)?))
    }

    /// A mutator that changes the same invented machine this provider reads.
    pub fn mutator(&self) -> FixtureMutator {
        FixtureMutator { state: Arc::clone(&self.state), actions: Mutex::new(Vec::new()) }
    }

    /// The machine as it is now, for assertions.
    pub fn snapshot(&self) -> Fixture {
        self.state.lock().expect("the fixture lock").clone()
    }

    fn with<T>(&self, read: impl FnOnce(&Fixture) -> T) -> T {
        read(&self.state.lock().expect("the fixture lock"))
    }
}

/// `ROOT:VIEW:PATH`, the address form fixtures use for registry keys.
fn address(root: RegistryRoot, key_path: &str, view: RegistryView) -> String {
    let view = match view {
        RegistryView::Bits64 => "64",
        RegistryView::Bits32 => "32",
    };
    format!("{}:{view}:{}", root.short_name(), key_path)
}

/// The key name of an address, for rebuilding a path from a subkey.
fn address_path(address: &str) -> &str {
    address.splitn(3, ':').nth(2).unwrap_or_default()
}

impl SystemProvider for FixtureProvider {
    fn registry_subkeys(
        &self,
        root: RegistryRoot,
        key_path: &str,
        view: RegistryView,
    ) -> Result<Vec<String>> {
        let address = address(root, key_path, view);
        self.with(|fixture| {
            check_denied(fixture, &address)?;

            let prefix = format!("{}\\", address.to_ascii_lowercase());
            let mut names = Vec::new();
            let mut seen = BTreeSet::new();
            let mut key_exists =
                fixture.registry.keys().any(|other| other.eq_ignore_ascii_case(&address));

            for original in fixture.registry.keys() {
                let lowered = original.to_ascii_lowercase();
                let Some(remainder) = lowered.strip_prefix(&prefix) else { continue };
                key_exists = true;

                // The first segment of what is left is an immediate child.
                let child_len = remainder.split('\\').next().unwrap_or_default().len();
                let child = &original[address.len() + 1..address.len() + 1 + child_len];
                if seen.insert(child.to_ascii_lowercase()) {
                    names.push(child.to_string());
                }
            }

            if !key_exists {
                return Err(Error::not_found(address));
            }

            names.sort();
            Ok(names)
        })
    }

    fn registry_values(
        &self,
        root: RegistryRoot,
        key_path: &str,
        view: RegistryView,
    ) -> Result<RegistryValues> {
        let address = address(root, key_path, view);
        self.with(|fixture| {
            check_denied(fixture, &address)?;
            fixture
                .registry
                .iter()
                .find(|(other, _)| other.eq_ignore_ascii_case(&address))
                .map(|(_, values)| values.clone())
                .ok_or_else(|| Error::not_found(address))
        })
    }

    fn msi_products(&self) -> Result<Vec<MsiProduct>> {
        self.with(|fixture| match &fixture.failures.msi {
            Some(message) => Err(Error::access_denied(message.clone())),
            None => Ok(fixture.msi_products.clone()),
        })
    }

    fn appx_packages(&self) -> Result<Vec<AppxPackage>> {
        self.with(|fixture| match &fixture.failures.appx {
            Some(message) => Err(Error::access_denied(message.clone())),
            None => Ok(fixture.appx_packages.clone()),
        })
    }

    fn expand_env(&self, value: &str) -> String {
        self.with(|fixture| expand(fixture, value))
    }

    fn path_exists(&self, candidate: &str) -> bool {
        let normalized = path::normalize(candidate);
        self.with(|fixture| exists(fixture, &normalized))
    }

    fn read_dir(&self, candidate: &str) -> Result<Vec<DirEntry>> {
        let normalized = path::normalize(candidate);
        self.with(|fixture| {
            if !exists(fixture, &normalized) || !is_directory(fixture, &normalized) {
                return Err(Error::not_found(candidate));
            }

            let prefix = format!("{normalized}\\");
            let mut entries: BTreeMap<String, DirEntry> = BTreeMap::new();
            for stored in all_paths(fixture) {
                let lowered = path::normalize(&stored);
                let Some(remainder) = lowered.strip_prefix(&prefix) else { continue };
                let length = remainder.split('\\').next().unwrap_or_default().len();
                if length == 0 {
                    continue;
                }
                // Sliced out of the stored path rather than the lowercased
                // one: Windows preserves the casing a folder was created
                // with, and a fixture that does not would hide every place
                // Wave wrongly compares paths case-sensitively.
                let offset = stored.len() - lowered.len() + prefix.len();
                let name = stored[offset..offset + length].to_string();
                // Joined onto the caller's spelling, not the lowercased one:
                // Windows hands back the path it was given plus the entry's
                // own casing, and fixtures that quietly lowercase everything
                // hide case bugs instead of catching them.
                let full = path::join(candidate.trim_end_matches(['\\', '/']), &name);
                entries.entry(name.clone()).or_insert_with(|| DirEntry {
                    is_directory: is_directory(fixture, &full),
                    is_reparse_point: contains_path(&fixture.reparse_points, &full),
                    size_bytes: fixture
                        .file_sizes
                        .iter()
                        .find(|(stored, _)| path::normalize(stored) == full)
                        .map(|(_, size)| *size)
                        .unwrap_or(0),
                    name,
                    path: full,
                });
            }

            Ok(entries.into_values().collect())
        })
    }

    fn read_text_file(&self, candidate: &str) -> Result<String> {
        let normalized = path::normalize(candidate);
        self.with(|fixture| {
            fixture
                .text_files
                .iter()
                .find(|(stored, _)| path::normalize(stored) == normalized)
                .map(|(_, contents)| contents.clone())
                .ok_or_else(|| Error::not_found(candidate))
        })
    }

    fn now_unix_seconds(&self) -> u64 {
        self.with(|fixture| fixture.now_unix_seconds)
    }
}

/// A mutator that really changes the invented machine.
#[derive(Debug)]
pub struct FixtureMutator {
    state: Arc<Mutex<Fixture>>,
    actions: Mutex<Vec<String>>,
}

impl FixtureMutator {
    /// Everything this mutator was asked to do, in order.
    ///
    /// Assertions read better against this than against the resulting state:
    /// "the uninstaller ran before anything was moved" is a statement about
    /// the sequence, not about the machine afterwards.
    pub fn actions(&self) -> Vec<String> {
        self.actions.lock().expect("the action log").clone()
    }

    fn record(&self, action: String) {
        self.actions.lock().expect("the action log").push(action);
    }

    fn with<T>(&self, change: impl FnOnce(&mut Fixture) -> T) -> T {
        change(&mut self.state.lock().expect("the fixture lock"))
    }
}

impl SystemMutator for FixtureMutator {
    fn create_directory(&self, candidate: &str) -> Result<()> {
        self.record(format!("create-directory {candidate}"));
        let normalized = path::normalize(candidate);
        self.with(|fixture| {
            fixture.paths.insert(normalized.clone());
            fixture.directories.insert(normalized);
            Ok(())
        })
    }

    fn move_path(&self, from: &str, to: &str) -> Result<()> {
        self.record(format!("move {from} -> {to}"));
        let from = path::normalize(from);
        let to = path::normalize(to);

        self.with(|fixture| {
            if contains_path(&fixture.failures.locked_paths, &from) {
                return Err(Error::access_denied(format!("{from} is in use by another process")));
            }
            if !exists(fixture, &from) {
                return Err(Error::not_found(from));
            }

            let rename = |stored: &str| -> Option<String> {
                let normalized = path::normalize(stored);
                if normalized == from {
                    return Some(to.clone());
                }
                normalized
                    .strip_prefix(&format!("{from}\\"))
                    .map(|remainder| path::join(&to, remainder))
            };

            fixture.paths = fixture
                .paths
                .iter()
                .map(|stored| rename(stored).unwrap_or_else(|| path::normalize(stored)))
                .collect();
            fixture.directories = fixture
                .directories
                .iter()
                .map(|stored| rename(stored).unwrap_or_else(|| path::normalize(stored)))
                .collect();
            fixture.reparse_points = fixture
                .reparse_points
                .iter()
                .map(|stored| rename(stored).unwrap_or_else(|| path::normalize(stored)))
                .collect();
            fixture.file_sizes = fixture
                .file_sizes
                .iter()
                .map(|(stored, size)| {
                    (rename(stored).unwrap_or_else(|| path::normalize(stored)), *size)
                })
                .collect();
            fixture.text_files = fixture
                .text_files
                .iter()
                .map(|(stored, contents)| {
                    (rename(stored).unwrap_or_else(|| path::normalize(stored)), contents.clone())
                })
                .collect();

            // Everything above the destination now exists, the way a real move
            // creates the folders it needs.
            let mut parent = path::parent(&to);
            while let Some(current) = parent {
                fixture.paths.insert(current.clone());
                fixture.directories.insert(current.clone());
                parent = path::parent(&current);
            }

            Ok(())
        })
    }

    fn write_text_file(&self, candidate: &str, contents: &str) -> Result<()> {
        self.record(format!("write {candidate}"));
        let normalized = path::normalize(candidate);
        self.with(|fixture| {
            fixture.paths.insert(normalized.clone());
            fixture.text_files.insert(normalized, contents.to_string());
            Ok(())
        })
    }

    fn append_line(&self, candidate: &str, line: &str) -> Result<()> {
        let normalized = path::normalize(candidate);
        self.with(|fixture| {
            fixture.paths.insert(normalized.clone());
            let file = fixture.text_files.entry(normalized).or_default();
            file.push_str(line);
            file.push('\n');
            Ok(())
        })
    }

    fn export_registry_key(
        &self,
        root: RegistryRoot,
        key_path: &str,
        view: RegistryView,
    ) -> Result<RegistryKeyBackup> {
        self.record(format!("export {}", address(root, key_path, view)));
        self.with(|fixture| export(fixture, root, key_path, view))
    }

    fn delete_registry_key(
        &self,
        root: RegistryRoot,
        key_path: &str,
        view: RegistryView,
    ) -> Result<()> {
        let address = address(root, key_path, view);
        self.record(format!("delete-key {address}"));
        self.with(|fixture| {
            let prefix = format!("{}\\", address.to_ascii_lowercase());
            let before = fixture.registry.len();
            fixture.registry.retain(|stored, _| {
                let lowered = stored.to_ascii_lowercase();
                lowered != address.to_ascii_lowercase() && !lowered.starts_with(&prefix)
            });
            if fixture.registry.len() == before {
                return Err(Error::not_found(address));
            }
            Ok(())
        })
    }

    fn delete_registry_value(
        &self,
        root: RegistryRoot,
        key_path: &str,
        view: RegistryView,
        name: &str,
    ) -> Result<()> {
        let address = address(root, key_path, view);
        self.record(format!("delete-value {address} -> {name}"));
        self.with(|fixture| {
            let Some((_, values)) = fixture
                .registry
                .iter_mut()
                .find(|(stored, _)| stored.eq_ignore_ascii_case(&address))
            else {
                return Err(Error::not_found(address));
            };
            values.remove(name).map(|_| ()).ok_or_else(|| Error::not_found(name))
        })
    }

    fn restore_registry_key(&self, backup: &RegistryKeyBackup) -> Result<()> {
        let address = address(backup.root, &backup.path, backup.view);
        self.record(format!("restore-key {address}"));
        self.with(|fixture| {
            fixture.registry.insert(address, backup.values.clone());
            Ok(())
        })
    }

    fn restore_registry_value(
        &self,
        root: RegistryRoot,
        key_path: &str,
        view: RegistryView,
        name: &str,
        value: &RegistryValue,
    ) -> Result<()> {
        let address = address(root, key_path, view);
        self.record(format!("restore-value {address} -> {name}"));
        self.with(|fixture| {
            fixture.registry.entry(address).or_default().insert(name.to_string(), value.clone());
            Ok(())
        })
    }

    fn run_process(
        &self,
        program: &str,
        arguments: &[String],
        _timeout_seconds: u64,
    ) -> Result<ProcessOutcome> {
        self.record(format!("run {program} {}", arguments.join(" ")));
        self.with(|fixture| {
            let behaviour = fixture.uninstaller.clone();
            for removed in &behaviour.removes_paths {
                remove_path(fixture, &path::normalize(removed));
            }
            for removed in &behaviour.removes_registry {
                let lowered = removed.to_ascii_lowercase();
                fixture.registry.retain(|stored, _| {
                    let stored = stored.to_ascii_lowercase();
                    stored != lowered && !stored.starts_with(&format!("{lowered}\\"))
                });
            }

            Ok(ProcessOutcome { exit_code: behaviour.exit_code, timed_out: behaviour.timed_out })
        })
    }

    fn remove_appx_package(&self, package_family_name: &str) -> Result<()> {
        self.record(format!("remove-package {package_family_name}"));
        self.with(|fixture| {
            fixture
                .appx_packages
                .retain(|package| package.package_family_name != package_family_name);
            Ok(())
        })
    }
}

// ---- reading and changing the invented file system -------------------------

fn all_paths(fixture: &Fixture) -> Vec<String> {
    let mut paths: Vec<String> = fixture.paths.iter().cloned().collect();
    paths.extend(fixture.directories.iter().cloned());
    paths.extend(fixture.text_files.keys().cloned());
    paths.extend(fixture.file_sizes.keys().cloned());
    paths
}

fn contains_path(set: &BTreeSet<String>, candidate: &str) -> bool {
    let normalized = path::normalize(candidate);
    set.iter().any(|stored| {
        let stored = path::normalize(stored);
        stored == normalized || path::is_within(&normalized, &stored)
    })
}

fn exists(fixture: &Fixture, candidate: &str) -> bool {
    let normalized = path::normalize(candidate);
    all_paths(fixture).iter().any(|stored| {
        let stored = path::normalize(stored);
        stored == normalized || path::is_within(&stored, &normalized)
    })
}

fn is_directory(fixture: &Fixture, candidate: &str) -> bool {
    let normalized = path::normalize(candidate);
    if fixture.directories.iter().any(|stored| path::normalize(stored) == normalized) {
        return true;
    }
    // Anything with something below it is a directory, whatever the fixture
    // bothered to list.
    all_paths(fixture).iter().any(|stored| {
        let stored = path::normalize(stored);
        stored != normalized && path::is_within(&stored, &normalized)
    })
}

fn remove_path(fixture: &mut Fixture, candidate: &str) {
    let normalized = path::normalize(candidate);
    let keep = |stored: &String| {
        let stored = path::normalize(stored);
        stored != normalized && !path::is_within(&stored, &normalized)
    };
    fixture.paths.retain(keep);
    fixture.directories.retain(keep);
    fixture.reparse_points.retain(keep);
    fixture.file_sizes.retain(|stored, _| keep(stored));
    fixture.text_files.retain(|stored, _| keep(stored));
}

fn check_denied(fixture: &Fixture, address: &str) -> Result<()> {
    let denied = fixture
        .failures
        .registry_access_denied
        .iter()
        .any(|stored| stored.eq_ignore_ascii_case(address));
    if denied {
        return Err(Error::access_denied(address.to_string()));
    }
    Ok(())
}

fn export(
    fixture: &Fixture,
    root: RegistryRoot,
    key_path: &str,
    view: RegistryView,
) -> Result<RegistryKeyBackup> {
    let address = address(root, key_path, view);
    let values = fixture
        .registry
        .iter()
        .find(|(stored, _)| stored.eq_ignore_ascii_case(&address))
        .map(|(_, values)| values.clone())
        .ok_or_else(|| Error::not_found(address.clone()))?;

    let prefix = format!("{}\\", address.to_ascii_lowercase());
    let mut children = BTreeSet::new();
    for stored in fixture.registry.keys() {
        let lowered = stored.to_ascii_lowercase();
        let Some(remainder) = lowered.strip_prefix(&prefix) else { continue };
        let child_len = remainder.split('\\').next().unwrap_or_default().len();
        children.insert(stored[address.len() + 1..address.len() + 1 + child_len].to_string());
    }

    let mut subkeys = Vec::new();
    for child in children {
        let child_path = format!("{}\\{child}", address_path(&address));
        subkeys.push(export(fixture, root, &child_path, view)?);
    }

    Ok(RegistryKeyBackup { root, path: key_path.to_string(), view, values, subkeys })
}

/// `%NAME%` expansion against the fixture's environment.
fn expand(fixture: &Fixture, value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(open) = rest.find('%') {
        let Some(close_offset) = rest[open + 1..].find('%') else { break };
        let close = open + 1 + close_offset;

        out.push_str(&rest[..open]);
        let name = &rest[open + 1..close];
        match fixture.environment.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)) {
            Some((_, replacement)) => out.push_str(replacement),
            // An unknown variable stays verbatim, exactly as Windows leaves
            // it — swallowing it would silently corrupt the path.
            None => out.push_str(&rest[open..=close]),
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);

    out
}
