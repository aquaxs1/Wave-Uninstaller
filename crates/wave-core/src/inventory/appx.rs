//! Programs read from the Store / MSIX package registry.

use crate::model::{Architecture, InstallScope, Program, ProgramId, ProgramKind, ProgramSource};
use crate::provider::SystemProvider;
use crate::text;

pub fn collect(provider: &dyn SystemProvider, warnings: &mut Vec<String>) -> Vec<Program> {
    let packages = match provider.appx_packages() {
        Ok(packages) => packages,
        Err(error) => {
            warnings.push(format!("installed Store packages could not be listed: {error}"));
            return Vec::new();
        }
    };

    packages
        .into_iter()
        .filter_map(|package| {
            let family = package.package_family_name.trim();
            if family.is_empty() {
                return None;
            }

            // The manifest display name is what the Start menu shows; the
            // identity name (`Microsoft.WindowsTerminal`) is the fallback.
            let name = package
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| package.name.trim());
            if name.is_empty() {
                return None;
            }

            Some(Program {
                id: ProgramId::appx(family),
                name: name.to_string(),
                version: package.version.filter(|v| !v.trim().is_empty()),
                // Reported as a certificate subject; the readable company
                // name is what the UI shows and what the leftover scan will
                // later compare folder names against.
                publisher: package
                    .publisher
                    .as_deref()
                    .map(text::publisher_from_subject)
                    .filter(|p| !p.trim().is_empty()),
                install_location: package.install_location.filter(|p| !p.trim().is_empty()),
                install_date: None,
                estimated_size_bytes: None,
                // Store packages are removed through the package manager, not
                // through a command line, so there is no uninstall string to
                // record. P1 wires this to the real removal call.
                uninstall: None,
                icon_path: None,
                parent_name: None,
                sources: vec![ProgramSource::Appx],
                scope: InstallScope::CurrentUser,
                architecture: Architecture::Neutral,
                // Framework packages are shared runtimes that other apps
                // depend on — same reasoning as SystemComponent in the
                // registry, so they get the same treatment.
                kind: if package.is_framework {
                    ProgramKind::SystemComponent
                } else {
                    ProgramKind::Application
                },
            })
        })
        .collect()
}
