//! What may never be removed, whatever it scores.
//!
//! This runs *before* scoring, not after. A protected path is not a low-scoring
//! finding that stays unchecked — it never becomes a finding at all, so no
//! selection, preset or "select everything" button can reach it.
//!
//! The list is built per removal, because its most important rule is not a
//! constant: a folder is protected when *another installed program* claims it.
//! That is why the inventory has to be complete before a scan runs, and why a
//! source that failed to read turns into a visible warning — a program Wave
//! never saw claims nothing, and its folder looks like a leftover.

use std::collections::BTreeSet;

use crate::inventory::Inventory;
use crate::model::Program;
use crate::path;
use crate::provider::{RegistryRoot, RegistryView, SystemProvider};
use crate::text;

/// Directories that are removable only from the inside: the folder itself
/// stays, its children are fair game.
const CONTAINER_VARIABLES: &[&str] = &[
    "%ProgramFiles%",
    "%ProgramFiles(x86)%",
    "%ProgramData%",
    "%APPDATA%",
    "%LOCALAPPDATA%",
    "%USERPROFILE%",
    "%PUBLIC%",
    "%TEMP%",
];

/// Subtrees nothing may ever be removed from.
const FORBIDDEN_VARIABLES: &[&str] =
    &["%SystemRoot%", "%windir%", "%CommonProgramFiles%", "%CommonProgramFiles(x86)%"];

/// Profile folders that hold documents rather than program state.
///
/// The plan allows removing findings here behind an explicit opt-in. Until
/// that opt-in exists, they are simply protected: the cost of being wrong is a
/// user's files.
const USER_DATA_FOLDERS: &[&str] =
    &["Documents", "Desktop", "Downloads", "Pictures", "Music", "Videos", "OneDrive", "Favorites"];

/// Folder and key names that belong to shared runtimes.
///
/// Every one of these is something other programs link against. A program that
/// installed a runtime does not own it.
const SHARED_RUNTIME_NAMES: &[&str] = &[
    "microsoft shared",
    "common files",
    "windowsapps",
    "dotnet",
    "microsoft .net",
    "microsoft visual studio",
    "directx",
    "java",
    "oracle",
    "nvidia corporation",
    "windows kits",
    "windows defender",
    "package cache",
    "winsxs",
];

/// Registry subtrees that are never a leftover.
const FORBIDDEN_REGISTRY: &[&str] = &[
    r"SYSTEM",
    r"SECURITY",
    r"SAM",
    r"BCD00000000",
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\SharedDLLs",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\Folders",
    r"SOFTWARE\Microsoft\Cryptography",
    r"SOFTWARE\Policies",
];

/// The two places under `SOFTWARE\Microsoft` where a program's own key
/// legitimately lives, and which may therefore be removed with it: its entry
/// in the installed-programs list, and the `App Paths` registration that lets
/// `Win+R` start it by name. Everything else under Microsoft's tree is
/// Windows' own.
const PROGRAM_OWNED_PARENTS: &[&str] = &[
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
];

/// Whether something may be touched, and if not, why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    /// The wording ends up in the UI next to the path, so it says which rule
    /// caught it rather than only that something did.
    Protected(String),
}

impl Verdict {
    pub fn is_protected(&self) -> bool {
        matches!(self, Self::Protected(_))
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Protected(reason) => Some(reason),
            Self::Allowed => None,
        }
    }
}

/// The rules in force for one removal.
#[derive(Debug, Clone)]
pub struct ProtectionList {
    /// Paths that may not be removed, nor anything below them.
    forbidden: Vec<(String, String)>,
    /// Paths that may not be removed themselves, but whose children may.
    containers: BTreeSet<String>,
    /// Install locations of every *other* program, with the program's name.
    claimed: Vec<(String, String)>,
    /// The names of every other installed program.
    ///
    /// A folder or key named after one of them is theirs, whatever it scores
    /// here — `Acme Reader.lnk` is not a leftover of `Acme Designer` just
    /// because both start with Acme.
    other_names: Vec<String>,
    /// Normalized vendor names other installed programs publish under.
    other_vendors: BTreeSet<String>,
}

impl ProtectionList {
    /// Build the list for removing `subject` from a machine described by
    /// `inventory`.
    pub fn build(provider: &dyn SystemProvider, inventory: &Inventory, subject: &Program) -> Self {
        let mut forbidden: Vec<(String, String)> = Vec::new();
        let mut containers = BTreeSet::new();

        for variable in FORBIDDEN_VARIABLES {
            let expanded = provider.expand_env(variable);
            if !expanded.contains('%') {
                forbidden.push((path::normalize(&expanded), format!("{variable} and everything in it belongs to Windows or is shared between programs")));
            }
        }

        for variable in CONTAINER_VARIABLES {
            let expanded = provider.expand_env(variable);
            if !expanded.contains('%') {
                containers.insert(path::normalize(&expanded));
            }
        }

        let profile = provider.expand_env("%USERPROFILE%");
        if !profile.contains('%') {
            for folder in USER_DATA_FOLDERS {
                forbidden.push((
                    path::normalize(&path::join(&profile, folder)),
                    format!("{folder} holds your own files, not program state"),
                ));
            }
        }

        let mut claimed = Vec::new();
        let mut other_vendors = BTreeSet::new();
        let mut other_names = Vec::new();

        for program in &inventory.programs {
            if program.id == subject.id {
                continue;
            }
            if let Some(location) = &program.install_location {
                let normalized = path::normalize(location);
                // A shallow "install location" is an installer writing
                // `C:\` or `C:\Program Files` into the registry. Taking it at
                // face value would protect every program on the machine, and
                // ignoring it would hand another product's folder to the scan,
                // so it is dropped from the claims and caught by the depth
                // rule below instead.
                if path::depth(&normalized) >= 2 {
                    claimed.push((normalized, program.name.clone()));
                }
            }
            if let Some(vendor) = program.publisher.as_deref().map(text::normalize_vendor) {
                // Including the subject's own publisher, and especially it:
                // a second program by the same vendor is precisely what makes
                // the vendor's folder shared ground.
                if !vendor.is_empty() {
                    other_vendors.insert(vendor);
                }
            }
            if !program.name.trim().is_empty() && program.name != subject.name {
                other_names.push(program.name.clone());
            }
        }

        Self { forbidden, containers, claimed, other_vendors, other_names }
    }

    /// Whether a file or directory may be moved to the backup.
    pub fn path_verdict(&self, candidate: &str) -> Verdict {
        let normalized = path::normalize(candidate);
        if normalized.is_empty() {
            return Verdict::Protected("an empty path".to_string());
        }

        // Two segments is a program folder — `C:\Program Files\Acme`. One is a
        // root that thousands of programs share.
        if path::depth(&normalized) < 2 {
            return Verdict::Protected(format!(
                "{normalized} is a drive or a top-level folder, never one program's own"
            ));
        }

        for (forbidden, reason) in &self.forbidden {
            if path::is_within(&normalized, forbidden) {
                return Verdict::Protected(reason.clone());
            }
        }

        if self.containers.contains(&normalized) {
            return Verdict::Protected(format!(
                "{normalized} is shared by every installed program"
            ));
        }

        for segment in normalized.split('\\') {
            if SHARED_RUNTIME_NAMES.contains(&segment) {
                return Verdict::Protected(format!(
                    "{segment} is a shared runtime other programs depend on"
                ));
            }
        }

        if let Some(owner) = self.claimed_by_name(path::file_name(&normalized)) {
            return Verdict::Protected(format!("{owner} is installed and goes by that name"));
        }

        for (location, owner) in &self.claimed {
            // Both directions matter. Below another product's folder is
            // obviously theirs; *above* it means removing this path would take
            // their installation with it.
            if path::is_within(&normalized, location) || path::is_within(location, &normalized) {
                return Verdict::Protected(format!("{owner} is installed there"));
            }
        }

        Verdict::Allowed
    }

    /// Whether a registry key may be removed.
    pub fn registry_key_verdict(
        &self,
        root: RegistryRoot,
        key_path: &str,
        _view: RegistryView,
    ) -> Verdict {
        let normalized = key_path.trim_matches('\\');
        let lowered = normalized.to_ascii_lowercase();

        if normalized.is_empty() {
            return Verdict::Protected("the root of a hive".to_string());
        }

        // An entry directly below one of those parents, not the parent itself.
        let is_program_entry = PROGRAM_OWNED_PARENTS.iter().any(|parent| {
            let parent = parent.to_ascii_lowercase();
            lowered
                .strip_prefix(&format!("{parent}\\"))
                .is_some_and(|leaf| !leaf.is_empty() && !leaf.contains('\\'))
        });

        for forbidden in FORBIDDEN_REGISTRY {
            let forbidden = forbidden.to_ascii_lowercase();
            if lowered == forbidden || lowered.starts_with(&format!("{forbidden}\\")) {
                return Verdict::Protected(format!(
                    "{}\\{forbidden} is Windows' own",
                    root.short_name()
                ));
            }
        }

        // `SOFTWARE\Microsoft\...` is Windows' own — except for the uninstall
        // entry the program itself wrote, which is exactly what has to go.
        let under_microsoft = lowered.starts_with(r"software\microsoft\")
            || lowered.starts_with(r"software\wow6432node\microsoft\");
        if under_microsoft && !is_program_entry {
            return Verdict::Protected(
                "keys under Microsoft's own tree are not a leftover".to_string(),
            );
        }

        let segments: Vec<&str> = normalized.split('\\').filter(|s| !s.is_empty()).collect();
        // `SOFTWARE`, `SOFTWARE\WOW6432Node` and a bare vendor key are shared
        // ground; a product key one level further down is not.
        let depth_below_software = match segments.first().map(|s| s.to_ascii_lowercase()) {
            Some(first) if first == "software" => {
                let rest = &segments[1..];
                match rest.first().map(|s| s.to_ascii_lowercase()) {
                    Some(second) if second == "wow6432node" => rest.len() - 1,
                    _ => rest.len(),
                }
            }
            _ => return Verdict::Protected(format!("{normalized} is outside the software tree")),
        };

        if depth_below_software == 0 {
            return Verdict::Protected("the software tree itself".to_string());
        }

        if is_program_entry {
            return Verdict::Allowed;
        }

        let leaf = segments.last().copied().unwrap_or_default();
        if let Some(owner) = self.claimed_by_name(leaf) {
            return Verdict::Protected(format!("{owner} is installed and goes by that name"));
        }
        let leaf_vendor = text::normalize_vendor(leaf);
        if depth_below_software == 1 && self.other_vendors.contains(&leaf_vendor) {
            return Verdict::Protected(format!("another installed program is published by {leaf}"));
        }

        if SHARED_RUNTIME_NAMES.contains(&leaf.to_ascii_lowercase().as_str()) {
            return Verdict::Protected(format!("{leaf} is shared between programs"));
        }

        Verdict::Allowed
    }

    /// The installed program a name belongs to, if it is not the subject's.
    ///
    /// Compared with the same matcher the scan proposes findings with, so a
    /// name that would have been good enough to propose something is good
    /// enough to protect it.
    fn claimed_by_name(&self, name: &str) -> Option<&str> {
        let name = name.trim_end_matches(".lnk").trim_end_matches(".url");
        self.other_names
            .iter()
            .find(|other| {
                matches!(
                    text::match_name(name, other),
                    Some(text::NameMatch::Exact | text::NameMatch::Tokens)
                )
            })
            .map(String::as_str)
    }

    /// Whether another installed program is published by the same vendor.
    ///
    /// Decides whether a vendor's own folder or key can belong to this program
    /// alone. `Acme\Designer` is removable when Acme sells one product and a
    /// shared folder when they sell four.
    pub fn vendor_is_shared(&self, vendor: &str) -> bool {
        !vendor.is_empty() && self.other_vendors.contains(vendor)
    }

    /// Whether a single registry value may be removed.
    ///
    /// Values are judged more leniently than keys on purpose: a `Run` entry
    /// pointing into a folder that is about to disappear is dead weight, and
    /// removing it leaves the key it lives in — which belongs to Windows —
    /// untouched.
    pub fn registry_value_verdict(
        &self,
        _root: RegistryRoot,
        key_path: &str,
        _view: RegistryView,
        name: &str,
    ) -> Verdict {
        if name.trim().is_empty() {
            return Verdict::Protected("the default value of a key".to_string());
        }
        let lowered = key_path.to_ascii_lowercase();
        for forbidden in FORBIDDEN_REGISTRY {
            let forbidden = forbidden.to_ascii_lowercase();
            if lowered == forbidden || lowered.starts_with(&format!("{forbidden}\\")) {
                return Verdict::Protected(format!("{key_path} is Windows' own"));
            }
        }
        Verdict::Allowed
    }
}
