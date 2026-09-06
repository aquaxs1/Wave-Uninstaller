//! Building the list of installed programs from every source Windows offers.

mod appx;
mod msi;
mod registry;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{
    Architecture, IdentityKey, InstallScope, Program, ProgramKind, ProgramSource, UninstallCommand,
};
use crate::provider::SystemProvider;

/// The result of one inventory pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    /// Every program found, merged across sources and sorted by name.
    pub programs: Vec<Program>,
    /// Sources or keys that could not be read.
    ///
    /// A partial inventory is still useful, but the leftover scan's protection
    /// list depends on knowing about *every* installed program — if a source
    /// silently failed, a folder belonging to a program Wave never saw could be
    /// mistaken for a leftover. So failures are collected and surfaced rather
    /// than logged and forgotten.
    pub warnings: Vec<String>,
}

impl Inventory {
    /// Only the entries the default list view shows.
    pub fn user_facing(&self) -> impl Iterator<Item = &Program> {
        self.programs.iter().filter(|program| program.is_user_facing())
    }
}

/// Read every source and merge the results.
pub fn collect(provider: &dyn SystemProvider) -> Inventory {
    let mut warnings = Vec::new();

    // Order matters only for determinism; the merge itself resolves each field
    // by source authority, not by arrival.
    let mut entries = registry::collect(provider, &mut warnings);
    entries.extend(msi::collect(provider, &mut warnings));
    entries.extend(appx::collect(provider, &mut warnings));

    let mut programs = merge(entries);
    programs.sort_by(|a, b| {
        a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id))
    });

    Inventory { programs, warnings }
}

/// How much a source can be trusted for a given field.
fn source_rank(source: ProgramSource) -> u8 {
    match source {
        ProgramSource::Msi => 3,
        ProgramSource::Appx => 2,
        ProgramSource::UninstallKey => 1,
    }
}

fn rank_of(program: &Program) -> u8 {
    program.sources.iter().copied().map(source_rank).max().unwrap_or(0)
}

/// Display names are ranked the other way round.
///
/// `DisplayName` from the uninstall key is the exact string Windows shows in
/// Apps & Features. Users compare Wave's list against that one, so matching it
/// matters more than using the technically more authoritative MSI product name.
fn name_rank(program: &Program) -> u8 {
    program
        .sources
        .iter()
        .map(|source| match source {
            ProgramSource::UninstallKey => 3,
            ProgramSource::Appx => 2,
            ProgramSource::Msi => 1,
        })
        .max()
        .unwrap_or(0)
}

/// Hide-by-default wins over show-by-default.
///
/// If any source says an entry is a system component, it stays out of the
/// default view. Being too cautious costs a user one click on "show all";
/// being too eager offers them a shared runtime to delete.
fn kind_severity(kind: ProgramKind) -> u8 {
    match kind {
        ProgramKind::SystemComponent => 2,
        ProgramKind::Update => 1,
        ProgramKind::Application => 0,
    }
}

fn merge(entries: Vec<Program>) -> Vec<Program> {
    let mut merged: Vec<Program> = Vec::new();
    let mut by_product_code: HashMap<String, usize> = HashMap::new();
    let mut by_identity: HashMap<IdentityKey, usize> = HashMap::new();

    for entry in entries {
        let product_code =
            entry.uninstall.as_ref().and_then(|uninstall| uninstall.msi_product_code.clone());

        // The product code is the strong link: an MSI-authored uninstall key is
        // named after the very code the MSI database reports, so the two
        // entries are the same product by construction rather than by
        // resemblance. Falling back to the name is the weak link, used only
        // when no code is available on either side.
        let existing = product_code
            .as_ref()
            .and_then(|code| by_product_code.get(code).copied())
            .or_else(|| by_identity.get(&entry.identity_key()).copied());

        let index = match existing {
            Some(index) => {
                merge_into(&mut merged[index], entry);
                index
            }
            None => {
                merged.push(entry);
                merged.len() - 1
            }
        };

        // Re-register both keys: merging can fill in a name or version that
        // was missing, which changes the identity key.
        if let Some(code) = merged[index]
            .uninstall
            .as_ref()
            .and_then(|uninstall| uninstall.msi_product_code.clone())
        {
            by_product_code.insert(code, index);
        }
        by_identity.insert(merged[index].identity_key(), index);
    }

    merged
}

fn merge_into(target: &mut Program, incoming: Program) {
    let prefer_incoming = rank_of(&incoming) > rank_of(target);
    let prefer_incoming_name = name_rank(&incoming) > name_rank(target);

    if prefer_incoming_name && !incoming.name.trim().is_empty() {
        target.name = incoming.name;
    }
    if prefer_incoming {
        target.id = incoming.id;
        target.scope = incoming.scope;
    }

    take_better(&mut target.version, incoming.version, prefer_incoming);
    take_better(&mut target.publisher, incoming.publisher, prefer_incoming);
    take_better(&mut target.install_location, incoming.install_location, prefer_incoming);
    take_better(&mut target.install_date, incoming.install_date, prefer_incoming);

    // Only the uninstall key records these, so there is never a conflict.
    take_better(&mut target.estimated_size_bytes, incoming.estimated_size_bytes, false);
    take_better(&mut target.icon_path, incoming.icon_path, false);
    take_better(&mut target.parent_name, incoming.parent_name, false);

    merge_uninstall(&mut target.uninstall, incoming.uninstall);

    // Neither MSI nor Appx reports bitness, so any concrete answer beats the
    // placeholder the registry view supplied.
    if target.architecture == Architecture::Unknown
        && incoming.architecture != Architecture::Unknown
    {
        target.architecture = incoming.architecture;
    }

    if kind_severity(incoming.kind) > kind_severity(target.kind) {
        target.kind = incoming.kind;
    }

    // A per-user install seen machine-wide by another source is machine-wide.
    if incoming.scope == InstallScope::Machine {
        target.scope = InstallScope::Machine;
    }

    for source in incoming.sources {
        if !target.sources.contains(&source) {
            target.sources.push(source);
        }
    }
    target.sources.sort();
}

fn take_better<T>(target: &mut Option<T>, incoming: Option<T>, prefer_incoming: bool) {
    match incoming {
        Some(value) if target.is_none() || prefer_incoming => *target = Some(value),
        _ => {}
    }
}

/// Keep the command an installer actually recorded, and carry the MSI product
/// code across.
///
/// A vendor's own `UninstallString` may do cleanup that a bare `msiexec` call
/// skips, so it stays the command of record. The product code travels with it
/// because it identifies the product rather than describing how to run
/// anything.
///
/// What must *not* travel is the quiet command. It is the silent variant of
/// whichever uninstaller supplied it, so pairing MSI's `msiexec /x ... /passive`
/// with a vendor's `setup.exe --uninstall` would give the UI two buttons that
/// run two different programs — and the quiet one would skip exactly the
/// cleanup the vendor command was kept for.
fn merge_uninstall(target: &mut Option<UninstallCommand>, incoming: Option<UninstallCommand>) {
    let Some(incoming) = incoming else { return };
    let Some(existing) = target.as_mut() else {
        *target = Some(incoming);
        return;
    };

    if existing.msi_product_code.is_none() {
        existing.msi_product_code = incoming.msi_product_code;
    }
}
