//! Programs read from the MSI product database.
//!
//! MSI is the only source that knows, without guessing, which files belong to
//! a product. P0 only reads the product list; the component-to-file mapping
//! that makes this source authoritative arrives with the leftover scan.

use crate::model::{
    Architecture, InstallScope, Program, ProgramId, ProgramKind, ProgramSource, UninstallCommand,
};
use crate::provider::SystemProvider;
use crate::text;

pub fn collect(provider: &dyn SystemProvider, warnings: &mut Vec<String>) -> Vec<Program> {
    let products = match provider.msi_products() {
        Ok(products) => products,
        Err(error) => {
            warnings.push(format!("the MSI product database could not be read: {error}"));
            return Vec::new();
        }
    };

    products
        .into_iter()
        .filter_map(|product| {
            let raw_code = product.product_code.trim();
            if !text::is_guid(raw_code) {
                // Report what the database actually holds, not a normalized
                // version of it — the raw value is what someone would grep for.
                warnings.push(format!("MSI returned an unusable product code: {raw_code:?}"));
                return None;
            }
            // Product codes are compared case-insensitively but written in
            // upper case, and the registry key names follow suit.
            let product_code = raw_code.to_ascii_uppercase();

            // A product with no name cannot be shown or acted on; the registry
            // entry for the same code usually carries one, and the merge will
            // supply it.
            let name = product.name.as_deref().map(str::trim).filter(|n| !n.is_empty())?;

            Some(Program {
                id: ProgramId::msi(&product_code),
                name: name.to_string(),
                version: product.version.filter(|v| !v.trim().is_empty()),
                publisher: product.publisher.filter(|p| !p.trim().is_empty()),
                install_location: product
                    .install_location
                    .map(|path| provider.expand_env(&path))
                    .filter(|p| !p.trim().is_empty()),
                install_date: product
                    .install_date
                    .as_deref()
                    .and_then(text::normalize_install_date),
                estimated_size_bytes: None,
                uninstall: Some(UninstallCommand {
                    command: format!("msiexec.exe /x {product_code}"),
                    // MSI always supports a non-interactive removal, so unlike
                    // an arbitrary installer there is no need to hope the
                    // vendor recorded a quiet switch.
                    quiet_command: Some(format!(
                        "msiexec.exe /x {product_code} /passive /norestart"
                    )),
                    msi_product_code: Some(product_code),
                }),
                icon_path: None,
                parent_name: None,
                sources: vec![ProgramSource::Msi],
                scope: if product.machine_scoped {
                    InstallScope::Machine
                } else {
                    InstallScope::CurrentUser
                },
                // The product database does not record bitness; the matching
                // registry view does, and the merge takes it from there.
                architecture: Architecture::Unknown,
                kind: ProgramKind::Application,
            })
        })
        .collect()
}
