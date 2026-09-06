//! A [`SystemProvider`] backed by JSON instead of by Windows.
//!
//! This is what makes the inventory testable at all. A fixture describes an
//! invented machine — registry keys, MSI rows, Store packages — and the same
//! code that runs against the real system runs against it, on any platform, in
//! milliseconds, with the expected output checked in next to the input.
//!
//! Every false positive found in the wild should end up here as a fixture, so
//! the same mistake cannot come back.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::provider::{
    AppxPackage, MsiProduct, RegistryRoot, RegistryValues, RegistryView, SystemProvider,
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
    #[serde(default)]
    pub paths: BTreeSet<String>,

    /// Reads that should fail, so the error and warning paths get covered too.
    #[serde(default)]
    pub failures: Failures,
}

/// Simulated read failures.
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
}

/// A provider that answers from a [`Fixture`].
#[derive(Debug, Clone)]
pub struct FixtureProvider {
    fixture: Fixture,
    /// Lowercased address -> original address, because the registry is
    /// case-insensitive but preserves the casing it was written with.
    ///
    /// ASCII-only lowercasing on purpose: subkey enumeration slices the
    /// original address using offsets computed on the lowercased one, and full
    /// Unicode lowercasing can change a string's byte length.
    registry_index: BTreeMap<String, String>,
    denied: HashSet<String>,
    existing_paths: HashSet<String>,
}

impl FixtureProvider {
    pub fn new(fixture: Fixture) -> Self {
        let registry_index = fixture
            .registry
            .keys()
            .map(|address| (address.to_ascii_lowercase(), address.clone()))
            .collect();
        let denied = fixture
            .failures
            .registry_access_denied
            .iter()
            .map(|address| address.to_ascii_lowercase())
            .collect();
        let existing_paths = fixture.paths.iter().map(|path| path.to_ascii_lowercase()).collect();

        Self { fixture, registry_index, denied, existing_paths }
    }

    pub fn from_json(json: &str) -> std::result::Result<Self, serde_json::Error> {
        Ok(Self::new(serde_json::from_str(json)?))
    }

    fn address(root: RegistryRoot, path: &str, view: RegistryView) -> String {
        let view = match view {
            RegistryView::Bits64 => "64",
            RegistryView::Bits32 => "32",
        };
        format!("{}:{view}:{}", root.short_name(), path)
    }

    fn check_denied(&self, address: &str) -> Result<()> {
        if self.denied.contains(&address.to_ascii_lowercase()) {
            return Err(Error::access_denied(address));
        }
        Ok(())
    }
}

impl SystemProvider for FixtureProvider {
    fn registry_subkeys(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
    ) -> Result<Vec<String>> {
        let address = Self::address(root, path, view);
        self.check_denied(&address)?;

        let prefix = format!("{}\\", address.to_ascii_lowercase());
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        let mut key_itself_exists = self.registry_index.contains_key(&address.to_ascii_lowercase());

        for (lowered, original) in &self.registry_index {
            let Some(remainder) = lowered.strip_prefix(&prefix) else { continue };
            key_itself_exists = true;

            // The first segment of what is left is an immediate child.
            let child_len = remainder.split('\\').next().unwrap_or_default().len();
            let child = &original[address.len() + 1..address.len() + 1 + child_len];
            if seen.insert(child.to_ascii_lowercase()) {
                names.push(child.to_string());
            }
        }

        if !key_itself_exists {
            return Err(Error::not_found(address));
        }

        names.sort();
        Ok(names)
    }

    fn registry_values(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
    ) -> Result<RegistryValues> {
        let address = Self::address(root, path, view);
        self.check_denied(&address)?;

        self.registry_index
            .get(&address.to_ascii_lowercase())
            .and_then(|original| self.fixture.registry.get(original))
            .cloned()
            .ok_or_else(|| Error::not_found(address))
    }

    fn msi_products(&self) -> Result<Vec<MsiProduct>> {
        match &self.fixture.failures.msi {
            Some(message) => Err(Error::access_denied(message)),
            None => Ok(self.fixture.msi_products.clone()),
        }
    }

    fn appx_packages(&self) -> Result<Vec<AppxPackage>> {
        match &self.fixture.failures.appx {
            Some(message) => Err(Error::access_denied(message)),
            None => Ok(self.fixture.appx_packages.clone()),
        }
    }

    fn expand_env(&self, value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        let mut rest = value;

        while let Some(open) = rest.find('%') {
            let Some(close_offset) = rest[open + 1..].find('%') else { break };
            let close = open + 1 + close_offset;

            out.push_str(&rest[..open]);
            let name = &rest[open + 1..close];
            match self.fixture.environment.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)) {
                Some((_, replacement)) => out.push_str(replacement),
                // An unknown variable stays verbatim, exactly as Windows
                // leaves it — swallowing it would silently corrupt the path.
                None => out.push_str(&rest[open..=close]),
            }
            rest = &rest[close + 1..];
        }
        out.push_str(rest);

        out
    }

    fn path_exists(&self, path: &str) -> bool {
        self.existing_paths.contains(&path.to_ascii_lowercase())
    }
}
