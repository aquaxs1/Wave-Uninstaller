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
mod msi;
mod registry;

use wave_core::error::Result;
use wave_core::provider::{
    AppxPackage, MsiProduct, RegistryRoot, RegistryValues, RegistryView, SystemProvider,
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
}
