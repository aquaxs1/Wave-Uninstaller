//! The seam between the scan logic and Windows.
//!
//! Everything in `wave-core` reads the system through [`SystemProvider`] and
//! never calls Win32 directly. `wave-win32` supplies the real implementation;
//! [`crate::fixture::FixtureProvider`] supplies an invented one built from
//! JSON, which is what makes the inventory and (later) the scoring logic
//! testable on any platform.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// The registry hives Wave reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RegistryRoot {
    #[serde(rename = "HKLM")]
    LocalMachine,
    #[serde(rename = "HKCU")]
    CurrentUser,
    #[serde(rename = "HKCR")]
    ClassesRoot,
    #[serde(rename = "HKU")]
    Users,
}

impl RegistryRoot {
    pub fn short_name(self) -> &'static str {
        match self {
            Self::LocalMachine => "HKLM",
            Self::CurrentUser => "HKCU",
            Self::ClassesRoot => "HKCR",
            Self::Users => "HKU",
        }
    }
}

/// Which side of the registry redirector to read.
///
/// On 64-bit Windows a 32-bit program's keys live under `WOW6432Node`, and the
/// OS hides that split from whichever view a process happens to run in. Every
/// read has to name a view explicitly, because asking only for the native one
/// silently misses every 32-bit program on the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RegistryView {
    /// `KEY_WOW64_64KEY`
    #[serde(rename = "64")]
    Bits64,
    /// `KEY_WOW64_32KEY`
    #[serde(rename = "32")]
    Bits32,
}

impl RegistryView {
    pub const ALL: [RegistryView; 2] = [RegistryView::Bits64, RegistryView::Bits32];
}

/// A registry value, narrowed to the types Wave actually encounters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RegistryValue {
    #[serde(rename = "sz")]
    Text(String),
    /// `REG_EXPAND_SZ` — still contains `%VARIABLES%`.
    #[serde(rename = "expand_sz")]
    ExpandText(String),
    #[serde(rename = "multi_sz")]
    MultiText(Vec<String>),
    #[serde(rename = "dword")]
    Dword(u32),
    #[serde(rename = "qword")]
    Qword(u64),
    #[serde(rename = "binary")]
    Binary(Vec<u8>),
}

impl RegistryValue {
    /// The value as text, for the string-ish types only.
    ///
    /// `REG_EXPAND_SZ` comes back unexpanded on purpose: expansion needs the
    /// environment, which belongs to the provider, not to the parsing code.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) | Self::ExpandText(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The value as an unsigned number, for the numeric types only.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Dword(n) => Some(u64::from(*n)),
            Self::Qword(n) => Some(*n),
            _ => None,
        }
    }
}

/// The values of a single registry key, keyed by value name.
///
/// The default value (`@`) is stored under the empty string, matching how the
/// registry itself names it.
pub type RegistryValues = BTreeMap<String, RegistryValue>;

/// One row of the MSI product database.
///
/// MSI is the one install technology where Windows knows the exact truth about
/// what belongs to a product, so these entries outrank anything derived from
/// the registry alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsiProduct {
    /// The `{GUID}` product code — a stable identity across versions.
    pub product_code: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub install_location: Option<String>,
    /// `YYYYMMDD` as MSI reports it; normalized later.
    #[serde(default)]
    pub install_date: Option<String>,
    /// False for a product installed only for the current user.
    #[serde(default)]
    pub machine_scoped: bool,
}

/// An installed Store / MSIX package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppxPackage {
    /// e.g. `Microsoft.WindowsTerminal_8wekyb3d8bbwe`
    pub package_family_name: String,
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    /// The read-only install folder under `WindowsApps`.
    #[serde(default)]
    pub install_location: Option<String>,
    /// Framework packages are shared runtimes — never a user-facing program.
    #[serde(default)]
    pub is_framework: bool,
}

/// Read-only access to the parts of Windows the inventory needs.
///
/// P0 only reads. Anything that writes — deleting keys, moving files — will
/// land on a separate trait so that a provider used for scanning cannot
/// accidentally be handed to code that mutates.
pub trait SystemProvider: Send + Sync {
    /// Immediate subkey names of `path`. Returns [`Error::NotFound`] when the
    /// key itself does not exist.
    ///
    /// [`Error::NotFound`]: crate::error::Error::NotFound
    fn registry_subkeys(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
    ) -> Result<Vec<String>>;

    /// All values of a single key.
    fn registry_values(
        &self,
        root: RegistryRoot,
        path: &str,
        view: RegistryView,
    ) -> Result<RegistryValues>;

    /// Every product in the MSI database.
    fn msi_products(&self) -> Result<Vec<MsiProduct>>;

    /// Every installed MSIX/Appx package for the current user.
    fn appx_packages(&self) -> Result<Vec<AppxPackage>>;

    /// Expand `%VARIABLES%` in a path against the process environment.
    fn expand_env(&self, value: &str) -> String;

    /// Whether a file or directory exists. Never follows into reparse points.
    fn path_exists(&self, path: &str) -> bool;
}
