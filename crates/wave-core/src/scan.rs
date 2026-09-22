//! Finding what a program leaves behind.
//!
//! The scan proposes; it never removes. It walks the places programs keep
//! their state, asks [`crate::protect::ProtectionList`] whether each candidate
//! may be touched at all, and weighs whatever survives with
//! [`crate::score`]. The result is a list of findings with a score and the
//! reasoning behind it — the input for a plan, and for a user deciding whether
//! that plan is sane.
//!
//! Every read goes through [`SystemProvider`], so the whole of this file is
//! exercised against invented machines in `tests/`, on any platform.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::command;
use crate::inventory::Inventory;
use crate::model::{Program, ProgramSource};
use crate::path;
use crate::protect::{ProtectionList, Verdict};
use crate::provider::{RegistryRoot, RegistryValue, RegistryView, SystemProvider};
use crate::score::{self, weight, Evidence, Score};
use crate::text::{self, NameMatch};

const UNINSTALL_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const APP_PATHS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";
const SOFTWARE: &str = "SOFTWARE";

/// Startup entries, as values rather than keys — the keys are Windows' own.
const RUN_KEYS: &[&str] = &[
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
];

/// How many entries a size measurement visits before giving up.
///
/// A leftover folder with more than this in it is unusual enough that a rough
/// number is better than making the user wait for an exact one.
const SIZE_BUDGET: usize = 20_000;

/// Where one finding lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Path { path: String },
    RegistryKey { root: RegistryRoot, path: String, view: RegistryView },
    RegistryValue { root: RegistryRoot, path: String, view: RegistryView, name: String },
}

/// What kind of leftover a finding is — the grouping the UI shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    InstallFolder,
    ProgramFolder,
    AppData,
    ProgramData,
    StartMenu,
    PackageData,
    RegistryKey,
    RegistryValue,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::InstallFolder => "Install folder",
            Self::ProgramFolder => "Program folder",
            Self::AppData => "Settings and caches",
            Self::ProgramData => "Machine-wide data",
            Self::StartMenu => "Shortcuts",
            Self::PackageData => "Store package data",
            Self::RegistryKey => "Registry keys",
            Self::RegistryValue => "Registry values",
        }
    }
}

/// One thing Wave proposes to remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Stable within a scan, and what a selection refers to.
    pub id: String,
    pub target: Target,
    /// The path or key as it is shown to the user.
    pub display: String,
    pub category: Category,
    pub score: Score,
    /// Absent when the size could not be measured within the budget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// Something that was found and deliberately left alone.
///
/// Shown in the UI rather than dropped silently: a user who can see what was
/// spared has a reason to trust what was not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    pub display: String,
    pub reason: String,
}

/// How the program's own uninstaller would be run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Uninstaller {
    Command {
        program: String,
        arguments: Vec<String>,
        /// The command as a user would read it.
        display: String,
        /// False when the uninstaller will open a window and wait.
        silent: bool,
    },
    /// Store apps are removed through the package manager.
    StorePackage { package_family_name: String },
}

/// The result of one scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scan {
    pub program: Program,
    /// `None` when the program recorded no way to uninstall it — the case
    /// where Wave is the only thing that can clean up after it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uninstaller: Option<Uninstaller>,
    pub findings: Vec<Finding>,
    pub protected: Vec<Skipped>,
    /// Reads that failed. A scan with a silent gap is the dangerous kind.
    pub warnings: Vec<String>,
}

impl Scan {
    /// The findings Wave ticks for the user.
    pub fn preselected(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|finding| finding.score.confidence == score::Confidence::Safe)
    }

    /// The total size of what is proposed, as far as it could be measured.
    pub fn total_size_bytes(&self) -> u64 {
        self.findings.iter().filter_map(|finding| finding.size_bytes).sum()
    }
}

/// Scan for everything belonging to `program`.
///
/// `inventory` has to be the full list of installed programs: the protection
/// list is built from it, and a program missing from it claims nothing — so
/// its folder can look like someone else's leftover.
pub fn scan(provider: &dyn SystemProvider, inventory: &Inventory, program: &Program) -> Scan {
    let mut scanner = Scanner {
        provider,
        program,
        protection: ProtectionList::build(provider, inventory, program),
        vendor: program.publisher.as_deref().map(text::normalize_vendor).unwrap_or_default(),
        package_family: package_family(program),
        findings: Vec::new(),
        protected: Vec::new(),
        warnings: Vec::new(),
        seen: BTreeSet::new(),
    };

    scanner.install_location();
    scanner.program_and_data_folders();
    scanner.store_package_data();
    scanner.start_menu();
    scanner.own_uninstall_key();
    scanner.vendor_keys();
    scanner.app_paths();
    scanner.startup_values();

    scanner.finish()
}

/// The Store package family an entry belongs to, if it is one.
fn package_family(program: &Program) -> Option<String> {
    if !program.sources.contains(&ProgramSource::Appx) {
        return None;
    }
    program.id.as_str().strip_prefix("appx:").map(str::to_string)
}

struct Scanner<'a> {
    provider: &'a dyn SystemProvider,
    program: &'a Program,
    protection: ProtectionList,
    /// The publisher, normalized once. Empty when the program has none.
    vendor: String,
    package_family: Option<String>,
    findings: Vec<Finding>,
    protected: Vec<Skipped>,
    warnings: Vec<String>,
    seen: BTreeSet<String>,
}

impl Scanner<'_> {
    fn finish(mut self) -> Scan {
        // Strongest first: the list is read top down, and the first rows are
        // the ones that will be ticked.
        self.findings.sort_by(|a, b| {
            b.score.value.cmp(&a.score.value).then_with(|| a.display.cmp(&b.display))
        });

        Scan {
            uninstaller: self.uninstaller(),
            program: self.program.clone(),
            findings: self.findings,
            protected: self.protected,
            warnings: self.warnings,
        }
    }

    fn uninstaller(&self) -> Option<Uninstaller> {
        if let Some(family) = &self.package_family {
            return Some(Uninstaller::StorePackage { package_family_name: family.clone() });
        }

        let parsed = command::uninstaller(self.program.uninstall.as_ref()?)?;
        Some(Uninstaller::Command {
            display: parsed.display(),
            program: parsed.program,
            arguments: parsed.arguments,
            silent: parsed.silent,
        })
    }

    /// The path the program recorded for itself.
    fn install_location(&mut self) {
        let Some(location) = self.program.install_location.clone() else { return };
        let mut evidence = vec![Evidence::new(
            weight::INSTALL_LOCATION,
            "recorded as the program's install location",
        )];
        evidence.extend(self.name_evidence(path::file_name(&location)));

        self.add_path(&location, Category::InstallFolder, evidence);
    }

    /// Folders under the roots where installed programs keep their files.
    fn program_and_data_folders(&mut self) {
        let roots: Vec<(String, Category)> = vec![
            (self.expand("%ProgramFiles%"), Category::ProgramFolder),
            (self.expand("%ProgramFiles(x86)%"), Category::ProgramFolder),
            (path::join(&self.expand("%LOCALAPPDATA%"), "Programs"), Category::ProgramFolder),
            (self.expand("%ProgramData%"), Category::ProgramData),
            (self.expand("%APPDATA%"), Category::AppData),
            (self.expand("%LOCALAPPDATA%"), Category::AppData),
        ];

        for (root, category) in roots {
            self.walk_root(&root, category, false);
        }
    }

    /// Start menu folders and shortcuts, for the user and for everyone.
    fn start_menu(&mut self) {
        let roots = [
            path::join(&self.expand("%APPDATA%"), r"Microsoft\Windows\Start Menu\Programs"),
            path::join(&self.expand("%ProgramData%"), r"Microsoft\Windows\Start Menu\Programs"),
        ];

        for root in roots {
            self.walk_root(&root, Category::StartMenu, true);
        }
    }

    /// One root, one level down — and a second level inside a vendor folder.
    ///
    /// `shortcuts` includes `.lnk` and `.url` files, which is what a start
    /// menu is made of; everywhere else only folders are considered.
    fn walk_root(&mut self, root: &str, category: Category, shortcuts: bool) {
        if root.contains('%') {
            return;
        }

        for entry in self.entries(root) {
            if entry.is_reparse_point {
                // Never followed, and worth saying so: a junction here is
                // exactly how a cleaner ends up deleting somewhere else. It
                // is checked before anything else, because what it points at
                // is none of Wave's business even when the name matches.
                if self.names_match(&entry.name).is_some() {
                    self.protected.push(Skipped {
                        display: entry.path.clone(),
                        reason: "a junction, and Wave never follows one".to_string(),
                    });
                }
                continue;
            }

            let Some(name) = comparable_name(&entry, shortcuts) else { continue };

            let product_match = text::match_name(&name, &self.program.name);
            let vendor_match = self.vendor_match(&name);

            // A folder that carries the publisher's name is treated as the
            // publisher's, not as this program's — unless it is named exactly
            // like the product. `C:\ProgramData\Acme` belongs to Acme, and
            // Acme may well have two programs installed.
            let is_vendor_folder = entry.is_directory
                && vendor_match.is_some()
                && product_match != Some(NameMatch::Exact);

            if is_vendor_folder {
                if self.protection.vendor_is_shared(&self.vendor) {
                    // Left alone, and said out loud: a folder two of the
                    // publisher's programs write into is not this program's
                    // to take, and a user who can see that spared folder has
                    // a reason to trust the rest of the list.
                    self.protected.push(Skipped {
                        display: entry.path.clone(),
                        reason: format!(
                            "{} has more than one program installed, so this folder is shared",
                            self.publisher_name()
                        ),
                    });
                } else {
                    let evidence = vec![
                        Evidence::new(
                            weight::VENDOR,
                            format!("named after the publisher, {}", self.publisher_name()),
                        ),
                        Evidence::new(
                            weight::PROGRAM_STATE_LOCATION,
                            format!("sits directly in {root}"),
                        ),
                    ];
                    self.add_path(&entry.path, category, evidence);
                }
                self.walk_vendor_folder(&entry.path, category, shortcuts);
                continue;
            }

            if product_match.is_some() {
                let mut evidence = self.name_evidence(&name);
                evidence.push(Evidence::new(
                    weight::PROGRAM_STATE_LOCATION,
                    format!("sits directly in {root}"),
                ));
                self.add_path(&entry.path, category, evidence);
            }
        }
    }

    /// The second level: what the publisher's own folder holds for this
    /// program in particular.
    fn walk_vendor_folder(&mut self, vendor_folder: &str, category: Category, shortcuts: bool) {
        for child in self.entries(vendor_folder) {
            if child.is_reparse_point {
                continue;
            }
            let Some(name) = comparable_name(&child, shortcuts) else { continue };
            if text::match_name(&name, &self.program.name).is_none() {
                continue;
            }

            let mut evidence = self.name_evidence(&name);
            evidence.push(Evidence::new(
                weight::UNDER_VENDOR,
                format!("inside a folder named after {}", self.publisher_name()),
            ));
            evidence.push(Evidence::new(
                weight::PROGRAM_STATE_LOCATION,
                format!("sits in {vendor_folder}"),
            ));
            self.add_path(&child.path, category, evidence);
        }
    }

    /// `%LOCALAPPDATA%\Packages\<family>` — what a Store app leaves when it
    /// has been reset at some point.
    fn store_package_data(&mut self) {
        let Some(family) = self.package_family.clone() else { return };
        let root = path::join(&self.expand("%LOCALAPPDATA%"), "Packages");
        if root.contains('%') {
            return;
        }

        for entry in self.entries(&root) {
            if !entry.is_directory || entry.is_reparse_point {
                continue;
            }
            if entry.name.eq_ignore_ascii_case(&family) {
                self.add_path(
                    &entry.path,
                    Category::PackageData,
                    vec![Evidence::new(
                        weight::PACKAGE_FAMILY,
                        format!("named after the package family {family}"),
                    )],
                );
            }
        }
    }

    /// The program's own entry in Apps & Features.
    ///
    /// This is the one finding that is not a heuristic: the key *is* the
    /// program's registration. A native uninstaller removes it itself, which
    /// is why the plan runs the uninstaller first and this finding usually
    /// turns out to be already gone — and why it matters when it is not.
    fn own_uninstall_key(&mut self) {
        for (root, view, key_name) in self.uninstall_key_locations() {
            let key_path = format!("{UNINSTALL_PATH}\\{key_name}");
            if !self.key_exists(root, &key_path, view) {
                continue;
            }

            let mut evidence = vec![Evidence::new(
                weight::OWN_UNINSTALL_KEY,
                "the program's own entry in the installed-programs list",
            )];
            if text::is_guid(&key_name) {
                evidence.push(Evidence::new(
                    weight::PRODUCT_CODE,
                    format!("named after the product code {key_name}"),
                ));
            }
            self.add_key(root, &key_path, view, Category::RegistryKey, evidence);
        }
    }

    /// Where this program's uninstall key could be, by how it was found.
    fn uninstall_key_locations(&self) -> Vec<(RegistryRoot, RegistryView, String)> {
        let id = self.program.id.as_str();

        if let Some(rest) = id.strip_prefix("reg:") {
            // `reg:hklm:64:<key name>` — the id records exactly where it came
            // from, so there is nothing to guess.
            let mut parts = rest.splitn(3, ':');
            let (Some(scope), Some(view), Some(key_name)) =
                (parts.next(), parts.next(), parts.next())
            else {
                return Vec::new();
            };
            let root = if scope == "hkcu" {
                RegistryRoot::CurrentUser
            } else {
                RegistryRoot::LocalMachine
            };
            let view = if view == "32" { RegistryView::Bits32 } else { RegistryView::Bits64 };
            return vec![(root, view, key_name.to_string())];
        }

        if let Some(code) = id.strip_prefix("msi:") {
            // An MSI product's key is named after the product code, but which
            // hive and view it lives in depends on how it was installed, so
            // all three are probed.
            return vec![
                (RegistryRoot::LocalMachine, RegistryView::Bits64, code.to_string()),
                (RegistryRoot::LocalMachine, RegistryView::Bits32, code.to_string()),
                (RegistryRoot::CurrentUser, RegistryView::Bits64, code.to_string()),
            ];
        }

        Vec::new()
    }

    /// `SOFTWARE\<Vendor>` and `SOFTWARE\<Vendor>\<Product>`, in both views.
    fn vendor_keys(&mut self) {
        for (root, view) in registry_locations() {
            let names = match self.provider.registry_subkeys(root, SOFTWARE, view) {
                Ok(names) => names,
                Err(error) if error.is_not_found() => continue,
                Err(error) => {
                    self.warn(format!(
                        "{}\\{SOFTWARE} could not be enumerated: {error}",
                        root.short_name()
                    ));
                    continue;
                }
            };

            for name in names {
                let key_path = format!("{SOFTWARE}\\{name}");
                let product_match = text::match_name(&name, &self.program.name);
                let vendor_match = self.vendor_match(&name);
                // Same rule as for folders: the publisher's key is the
                // publisher's unless it is named exactly like the product.
                let is_vendor_key =
                    vendor_match.is_some() && product_match != Some(NameMatch::Exact);

                if product_match.is_some() && !is_vendor_key {
                    let evidence = self.name_evidence(&name);
                    self.add_key(root, &key_path, view, Category::RegistryKey, evidence);
                    continue;
                }
                if !is_vendor_key {
                    continue;
                }

                let children =
                    self.provider.registry_subkeys(root, &key_path, view).unwrap_or_default();
                let matching: Vec<&String> = children
                    .iter()
                    .filter(|child| text::match_name(child, &self.program.name).is_some())
                    .collect();

                // The vendor key itself is only this program's when nothing
                // else lives under it and no other program shares the vendor.
                if children.len() == matching.len()
                    && !self.protection.vendor_is_shared(&self.vendor)
                {
                    let evidence = vec![Evidence::new(
                        weight::VENDOR,
                        format!(
                            "the publisher's own key, holding nothing but {}",
                            self.program.name
                        ),
                    )];
                    self.add_key(root, &key_path, view, Category::RegistryKey, evidence);
                    continue;
                }

                for child in matching {
                    let child_path = format!("{key_path}\\{child}");
                    let mut evidence = self.name_evidence(child);
                    evidence.push(Evidence::new(
                        weight::UNDER_VENDOR,
                        format!("under the publisher's key, {}", self.publisher_name()),
                    ));
                    self.add_key(root, &child_path, view, Category::RegistryKey, evidence);
                }
            }
        }
    }

    /// `App Paths` entries, which let `Win+R` start a program by name.
    fn app_paths(&mut self) {
        for (root, view) in registry_locations() {
            let names = self.provider.registry_subkeys(root, APP_PATHS, view).unwrap_or_default();
            for name in names {
                let key_path = format!("{APP_PATHS}\\{name}");
                let values = match self.provider.registry_values(root, &key_path, view) {
                    Ok(values) => values,
                    Err(_) => continue,
                };
                let target = values
                    .get("")
                    .or_else(|| values.get("Path"))
                    .and_then(|value| value.as_text())
                    .map(|value| self.provider.expand_env(value))
                    .unwrap_or_default();

                let mut evidence = Vec::new();
                if let Some(reason) = self.points_at_install_location(&target) {
                    evidence.push(reason);
                }
                if let Some(reason) = self.name_evidence(name.trim_end_matches(".exe")).pop() {
                    evidence.push(reason);
                }
                if evidence.is_empty() {
                    continue;
                }
                self.add_key(root, &key_path, view, Category::RegistryKey, evidence);
            }
        }
    }

    /// `Run` and `RunOnce` values that start something from the install folder.
    fn startup_values(&mut self) {
        for (root, view) in registry_locations() {
            for key_path in RUN_KEYS {
                let values = match self.provider.registry_values(root, key_path, view) {
                    Ok(values) => values,
                    Err(_) => continue,
                };

                for (name, value) in values {
                    let (RegistryValue::Text(data) | RegistryValue::ExpandText(data)) = &value
                    else {
                        continue;
                    };
                    let expanded = self.provider.expand_env(data);

                    let mut evidence = Vec::new();
                    if let Some(reason) = self.points_at_install_location(&expanded) {
                        evidence.push(reason);
                    }
                    if let Some(reason) = self.name_evidence(&name).pop() {
                        evidence.push(reason);
                    }
                    if evidence.is_empty() {
                        continue;
                    }
                    evidence.push(Evidence::new(
                        weight::PROGRAM_STATE_LOCATION,
                        "starts with Windows".to_string(),
                    ));
                    self.add_value(root, key_path, view, &name, evidence);
                }
            }
        }
    }

    // ---- shared helpers ----------------------------------------------------

    fn expand(&self, variable: &str) -> String {
        self.provider.expand_env(variable)
    }

    fn publisher_name(&self) -> String {
        self.program.publisher.clone().unwrap_or_else(|| self.vendor.clone())
    }

    fn entries(&mut self, path: &str) -> Vec<crate::provider::DirEntry> {
        match self.provider.read_dir(path) {
            Ok(entries) => entries,
            Err(error) if error.is_not_found() => Vec::new(),
            Err(error) => {
                self.warn(format!("{path} could not be listed: {error}"));
                Vec::new()
            }
        }
    }

    fn key_exists(&self, root: RegistryRoot, key_path: &str, view: RegistryView) -> bool {
        self.provider.registry_values(root, key_path, view).is_ok()
    }

    fn warn(&mut self, message: String) {
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }

    /// Whether a name looks like the product or the publisher.
    fn names_match(&self, name: &str) -> Option<NameMatch> {
        text::match_name(name, &self.program.name).or_else(|| self.vendor_match(name))
    }

    fn vendor_match(&self, name: &str) -> Option<NameMatch> {
        if self.vendor.is_empty() {
            return None;
        }
        text::match_name(name, &self.vendor)
    }

    /// The evidence a name match is worth.
    fn name_evidence(&self, name: &str) -> Vec<Evidence> {
        match text::match_name(name, &self.program.name) {
            Some(NameMatch::Exact) => {
                vec![Evidence::new(
                    weight::NAME_EXACT,
                    format!("named exactly like {}", self.program.name),
                )]
            }
            Some(NameMatch::Tokens) => vec![Evidence::new(
                weight::NAME_TOKENS,
                format!("its name is part of {}", self.program.name),
            )],
            Some(NameMatch::Fuzzy) => vec![Evidence::new(
                weight::NAME_FUZZY,
                format!("its name closely resembles {}", self.program.name),
            )],
            None => match self.vendor_match(name) {
                Some(_) => vec![Evidence::new(
                    weight::VENDOR,
                    format!("named after the publisher, {}", self.publisher_name()),
                )],
                None => Vec::new(),
            },
        }
    }

    /// Evidence that a command or path leads into the program's own folder.
    fn points_at_install_location(&self, value: &str) -> Option<Evidence> {
        let location = self.program.install_location.as_deref()?;
        let candidate = command::parse(value).map(|parsed| parsed.program).unwrap_or_default();
        let candidate = if candidate.is_empty() { value } else { &candidate };

        path::is_within(candidate, location).then(|| {
            Evidence::new(weight::POINTS_AT_INSTALL_LOCATION, format!("points into {location}"))
        })
    }

    fn add_path(&mut self, candidate: &str, category: Category, evidence: Vec<Evidence>) {
        let normalized = path::normalize(candidate);
        let id = format!("path:{normalized}");
        if evidence.is_empty() || !self.seen.insert(id.clone()) {
            return;
        }

        // The protection list runs before the score exists, so that no score
        // can ever outvote it.
        if let Verdict::Protected(reason) = self.protection.path_verdict(candidate) {
            self.protected.push(Skipped { display: candidate.to_string(), reason });
            return;
        }
        if !self.provider.path_exists(candidate) {
            return;
        }

        let mut evidence = evidence;
        if self.program.install_location.as_deref().is_some_and(|location| {
            !path::is_within(location, candidate) && path::is_within(candidate, location)
        }) {
            evidence.push(Evidence::new(
                weight::UNDER_INSTALL_LOCATION,
                "sits inside the program's install folder".to_string(),
            ));
        }

        let size = self.measure(candidate);
        self.findings.push(Finding {
            id,
            target: Target::Path { path: candidate.to_string() },
            display: candidate.to_string(),
            category,
            score: score::weigh(evidence),
            size_bytes: size,
        });
    }

    fn add_key(
        &mut self,
        root: RegistryRoot,
        key_path: &str,
        view: RegistryView,
        category: Category,
        evidence: Vec<Evidence>,
    ) {
        let display = format!("{}\\{key_path}", root.short_name());
        let id = format!(
            "key:{}:{}:{}",
            root.short_name(),
            view_label(view),
            key_path.to_ascii_lowercase()
        );
        if evidence.is_empty() || !self.seen.insert(id.clone()) {
            return;
        }

        if let Verdict::Protected(reason) =
            self.protection.registry_key_verdict(root, key_path, view)
        {
            self.protected.push(Skipped { display, reason });
            return;
        }

        self.findings.push(Finding {
            id,
            target: Target::RegistryKey { root, path: key_path.to_string(), view },
            display: format!("{display} ({}-bit view)", view_label(view)),
            category,
            score: score::weigh(evidence),
            size_bytes: None,
        });
    }

    fn add_value(
        &mut self,
        root: RegistryRoot,
        key_path: &str,
        view: RegistryView,
        name: &str,
        evidence: Vec<Evidence>,
    ) {
        let display = format!("{}\\{key_path} → {name}", root.short_name());
        let id = format!(
            "value:{}:{}:{}:{}",
            root.short_name(),
            view_label(view),
            key_path.to_ascii_lowercase(),
            name.to_ascii_lowercase()
        );
        if evidence.is_empty() || !self.seen.insert(id.clone()) {
            return;
        }

        if let Verdict::Protected(reason) =
            self.protection.registry_value_verdict(root, key_path, view, name)
        {
            self.protected.push(Skipped { display, reason });
            return;
        }

        self.findings.push(Finding {
            id,
            target: Target::RegistryValue {
                root,
                path: key_path.to_string(),
                view,
                name: name.to_string(),
            },
            display,
            category: Category::RegistryValue,
            score: score::weigh(evidence),
            size_bytes: None,
        });
    }

    /// The size of a file or folder, without following reparse points.
    fn measure(&self, candidate: &str) -> Option<u64> {
        let mut total = 0u64;
        let mut budget = SIZE_BUDGET;
        let mut pending = vec![candidate.to_string()];

        while let Some(current) = pending.pop() {
            let entries = self.provider.read_dir(&current).ok()?;
            for entry in entries {
                if budget == 0 {
                    return None;
                }
                budget -= 1;

                if entry.is_reparse_point {
                    continue;
                }
                if entry.is_directory {
                    pending.push(entry.path);
                } else {
                    total = total.saturating_add(entry.size_bytes);
                }
            }
        }

        Some(total)
    }
}

/// The name to compare a directory entry against, or `None` when the entry is
/// not something a leftover scan looks at.
///
/// A shortcut is compared without its extension — the start menu shows
/// `Acme Designer`, not `Acme Designer.lnk`.
fn comparable_name(entry: &crate::provider::DirEntry, shortcuts: bool) -> Option<String> {
    if entry.is_directory {
        return Some(entry.name.clone());
    }
    if !shortcuts {
        return None;
    }

    let lowered = entry.name.to_ascii_lowercase();
    for extension in [".lnk", ".url"] {
        if lowered.ends_with(extension) {
            return Some(entry.name[..entry.name.len() - extension.len()].to_string());
        }
    }
    None
}

/// Every hive and view a program's keys can live in.
///
/// `HKLM` is split by the WOW64 redirector; reading only the native view
/// misses every 32-bit program's keys, which is half the leftovers on a
/// 64-bit machine.
fn registry_locations() -> [(RegistryRoot, RegistryView); 3] {
    [
        (RegistryRoot::LocalMachine, RegistryView::Bits64),
        (RegistryRoot::LocalMachine, RegistryView::Bits32),
        (RegistryRoot::CurrentUser, RegistryView::Bits64),
    ]
}

fn view_label(view: RegistryView) -> &'static str {
    match view {
        RegistryView::Bits64 => "64",
        RegistryView::Bits32 => "32",
    }
}
