//! The real [`SystemProvider`] — the one that talks to Windows.
//!
//! Everything here is a thin, faithful translation of a Win32 or WinRT call
//! into `wave-core`'s vocabulary. Decisions belong in `wave-core`, where they
//! can be tested against fixtures; this crate only fetches.
//!
//! On non-Windows targets the crate still compiles, but exposes nothing. That
//! is deliberate: the workspace has to build on Linux so the core's tests can
//! run in CI without a Windows runner.

#![cfg(windows)]

mod appx;
mod env;
mod fs;
mod msi;
mod process;
mod registry;
mod regwrite;

use wave_core::error::Result;
use wave_core::mutator::{ProcessOutcome, RegistryKeyBackup, SystemMutator};
use wave_core::provider::{
    AppxPackage, DirEntry, MsiProduct, RegistryRoot, RegistryValue, RegistryValues, RegistryView,
    SystemProvider,
};

/// Reads the live system.
#[derive(Debug, Default, Clone, Copy)]
pub struct Win32Provider;

impl Win32Provider {
    pub fn new() -> Self {
        Self
    }
}

impl SystemProvider for Win32Provider {
    fn registry_subkeys(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
    ) -> Result<Vec<String>> {
        registry::subkeys(root, path, view)
    }

    fn registry_values(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
    ) -> Result<RegistryValues> {
        registry::values(root, path, view)
    }

    fn msi_products(&self) -> Result<Vec<MsiProduct>> {
        msi::products()
    }

    fn appx_packages(&self) -> Result<Vec<AppxPackage>> {
        appx::packages()
    }

    fn expand_env(&self, value: &str) -> String {
        env::expand(value)
    }

    fn path_exists(&self, path: &str) -> bool {
        // symlink_metadata, not metadata: a junction in AppData must report as
        // an existing link rather than as whatever it points at. Following it
        // is how a cleaner ends up walking into a directory it was never meant
        // to touch.
        std::fs::symlink_metadata(path).is_ok()
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        fs::read_dir(path)
    }

    fn read_text_file(&self, path: &str) -> Result<String> {
        fs::read_text_file(path)
    }

    fn now_unix_seconds(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            // A clock set before 1970 is not worth refusing to work over; the
            // operation simply gets the epoch as its timestamp.
            .unwrap_or_default()
    }
}

/// Changes the live system.
///
/// The counterpart to [`Win32Provider`], and deliberately a separate type: the
/// scan is handed the provider and nothing else, so no amount of refactoring
/// can give the code that decides what to remove the means to remove it.
#[derive(Debug, Default, Clone, Copy)]
pub struct Win32Mutator;

impl Win32Mutator {
    pub fn new() -> Self {
        Self
    }
}

impl SystemMutator for Win32Mutator {
    fn create_directory(&self, path: &str) -> Result<()> {
        fs::create_directory(path)
    }

    fn move_path(&self, from: &str, to: &str) -> Result<()> {
        fs::move_path(from, to)
    }

    fn write_text_file(&self, path: &str, contents: &str) -> Result<()> {
        fs::write_text_file(path, contents)
    }

    fn append_line(&self, path: &str, line: &str) -> Result<()> {
        fs::append_line(path, line)
    }

    fn export_registry_key(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
    ) -> Result<RegistryKeyBackup> {
        regwrite::export(root, path, view)
    }

    fn delete_registry_key(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
    ) -> Result<()> {
        regwrite::delete_key(root, path, view)
    }

    fn delete_registry_value(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
        name: &str,
    ) -> Result<()> {
        regwrite::delete_value(root, path, view, name)
    }

    fn restore_registry_key(&self, backup: &RegistryKeyBackup) -> Result<()> {
        regwrite::restore_key(backup)
    }

    fn restore_registry_value(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
        name: &str,
        value: &RegistryValue,
    ) -> Result<()> {
        regwrite::restore_value(root, path, view, name, value)
    }

    fn run_process(
        &self,
        program: &str,
        arguments: &[String],
        timeout_seconds: u64,
    ) -> Result<ProcessOutcome> {
        process::run(program, arguments, timeout_seconds)
    }

    fn remove_appx_package(&self, package_family_name: &str) -> Result<()> {
        appx::remove(package_family_name)
    }
}
