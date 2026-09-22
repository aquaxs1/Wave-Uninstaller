//! Reads of the installed Store / MSIX packages.

use windows::core::HSTRING;
use windows::Management::Deployment::PackageManager;

use wave_core::error::{Error, Result};
use wave_core::provider::AppxPackage;

/// Every package installed for the signed-in user.
pub fn packages() -> Result<Vec<AppxPackage>> {
    let manager =
        PackageManager::new().map_err(|error| Error::system("the package manager", error))?;

    // An empty security id means "the current user". Enumerating every user's
    // packages needs administrator rights and is not what P0 shows.
    let found = manager
        .FindPackagesByUserSecurityId(&HSTRING::new())
        .map_err(|error| Error::system("the installed package list", error))?;

    let mut packages = Vec::new();
    for package in found {
        let Ok(identity) = package.Id() else { continue };
        let Ok(family_name) = identity.FamilyName() else { continue };

        let name = identity.Name().map(|value| value.to_string()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }

        packages.push(AppxPackage {
            package_family_name: family_name.to_string(),
            name,
            // A package that declares no display name is normal; the identity
            // name stands in, and wave-core picks whichever is present.
            display_name: package
                .DisplayName()
                .ok()
                .map(|value| value.to_string())
                .filter(|value| !value.trim().is_empty()),
            version: identity.Version().ok().map(|version| {
                format!(
                    "{}.{}.{}.{}",
                    version.Major, version.Minor, version.Build, version.Revision
                )
            }),
            publisher: identity
                .Publisher()
                .ok()
                .map(|value| value.to_string())
                .filter(|value| !value.trim().is_empty()),
            install_location: package
                .InstalledPath()
                .ok()
                .map(|value| value.to_string())
                .filter(|value| !value.trim().is_empty()),
            is_framework: package.IsFramework().unwrap_or(false),
        });
    }

    Ok(packages)
}

/// Remove a package for the signed-in user.
///
/// Which package the family name refers to is a lookup, not a judgement: a
/// family has exactly one installed version per user, and the deployment API
/// needs its full name.
pub fn remove(package_family_name: &str) -> Result<()> {
    let manager =
        PackageManager::new().map_err(|error| Error::system("the package manager", error))?;

    let found = manager
        .FindPackagesByUserSecurityIdPackageFamilyName(
            &HSTRING::new(),
            &HSTRING::from(package_family_name),
        )
        .map_err(|error| Error::system(format!("the package {package_family_name}"), error))?;

    let full_name = found
        .into_iter()
        .find_map(|package| package.Id().ok()?.FullName().ok())
        .ok_or_else(|| Error::not_found(format!("an installed package {package_family_name}")))?;

    // `RemovePackageAsync` reports progress Wave has no use for; the removal
    // itself is what matters, and the caller is already on a worker thread.
    let operation = manager
        .RemovePackageAsync(&full_name)
        .map_err(|error| Error::system(format!("removing {package_family_name}"), error))?;
    let result = operation
        .get()
        .map_err(|error| Error::system(format!("removing {package_family_name}"), error))?;

    let code = result.ExtendedErrorCode().unwrap_or_default();
    if code.is_err() {
        return Err(Error::system(
            format!("removing {package_family_name}"),
            windows::core::Error::from(code),
        ));
    }

    Ok(())
}
