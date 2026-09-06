//! The program inventory as the rest of Wave sees it.

use serde::{Deserialize, Serialize};

use crate::text;

/// Where an inventory entry came from.
///
/// Ordered by how much the entry can be trusted: MSI knows exactly which files
/// belong to a product, an Appx manifest is similarly authoritative, and a
/// bare uninstall key is whatever the installer chose to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramSource {
    /// A key under `...\CurrentVersion\Uninstall`.
    UninstallKey,
    /// A Store / MSIX package.
    Appx,
    /// A row of the MSI product database.
    Msi,
}

/// Machine-wide, or only for the signed-in user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallScope {
    Machine,
    CurrentUser,
}

/// Which registry view an entry was found in — i.e. the program's bitness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86,
    X64,
    Arm64,
    /// Store packages that run on any architecture.
    Neutral,
    Unknown,
}

/// What kind of entry this is — drives what the list shows by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramKind {
    /// A normal program a user would recognize.
    Application,
    /// A patch or update, usually parented to an application.
    Update,
    /// Flagged `SystemComponent`, or a shared runtime. Hidden by default —
    /// removing these breaks other programs.
    SystemComponent,
}

/// How to launch the program's own uninstaller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallCommand {
    /// The raw `UninstallString`, as written by the installer.
    pub command: String,
    /// `QuietUninstallString` if the installer offered one — this is the form
    /// that runs without user interaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiet_command: Option<String>,
    /// Set when the product is MSI-based, so the uninstall can go through
    /// `msiexec /x {code}` instead of the recorded string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msi_product_code: Option<String>,
}

/// A stable, human-readable identifier for one inventory entry.
///
/// Deliberately not a hash: `msi:{6F320B93-...}` in a log or a bug report says
/// where to look, and stays the same across runs and across Wave versions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProgramId(pub String);

impl ProgramId {
    pub fn msi(product_code: &str) -> Self {
        Self(format!("msi:{}", product_code.to_ascii_uppercase()))
    }

    pub fn appx(package_family_name: &str) -> Self {
        Self(format!("appx:{package_family_name}"))
    }

    pub fn uninstall_key(scope: InstallScope, arch: Architecture, key_name: &str) -> Self {
        let scope = match scope {
            InstallScope::Machine => "hklm",
            InstallScope::CurrentUser => "hkcu",
        };
        let view = match arch {
            Architecture::X86 => "32",
            _ => "64",
        };
        Self(format!("reg:{scope}:{view}:{key_name}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProgramId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One installed program, after entries from every source have been merged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    pub id: ProgramId,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Where the program says it lives. Absent more often than one would hope,
    /// which is exactly why the leftover scan cannot rely on it alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_location: Option<String>,
    /// ISO `YYYY-MM-DD`, normalized from the registry's `YYYYMMDD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_date: Option<String>,
    /// `EstimatedSize`, converted from the registry's KiB to bytes. Installers
    /// write this by hand and frequently get it wrong or omit it; treat it as a
    /// hint for sorting, not a measurement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uninstall: Option<UninstallCommand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_path: Option<String>,
    /// The display name of the parent product, for updates and patches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_name: Option<String>,
    /// Every source this program was found in, deduplicated and sorted.
    pub sources: Vec<ProgramSource>,
    pub scope: InstallScope,
    pub architecture: Architecture,
    pub kind: ProgramKind,
}

impl Program {
    /// Whether the program should appear in the default list view.
    pub fn is_user_facing(&self) -> bool {
        self.kind == ProgramKind::Application
    }

    /// The key used to recognize two entries as the same program.
    ///
    /// Name and publisher are normalized so that `Mozilla Firefox` from the
    /// uninstall key and `Mozilla Firefox (x64 de)` from MSI do not become two
    /// rows. Version is part of the key on purpose: two genuinely different
    /// versions installed side by side are two programs.
    pub fn identity_key(&self) -> IdentityKey {
        IdentityKey {
            name: text::normalize_product_name(&self.name),
            publisher: self.publisher.as_deref().map(text::normalize_vendor).unwrap_or_default(),
            version: self.version.clone().unwrap_or_default(),
            scope: self.scope,
        }
    }
}

/// Merge key for [`Program::identity_key`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdentityKey {
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub scope: InstallScope,
}
