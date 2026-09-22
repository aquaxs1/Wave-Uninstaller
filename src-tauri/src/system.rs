//! Choosing which system Wave talks to.
//!
//! Two traits, two objects: [`SystemProvider`] reads and [`SystemMutator`]
//! writes. The scan only ever receives the first one, which is what keeps the
//! code that decides *what* to remove unable to remove anything.

use wave_core::{SystemMutator, SystemProvider};

/// Only runs on Windows, and says so rather than pretending otherwise.
#[cfg(not(windows))]
const NOT_WINDOWS: &str = "Wave Uninstaller reads the Windows registry and only runs on Windows.";

/// The provider for the machine the app is running on.
#[cfg(windows)]
pub fn provider() -> Result<impl SystemProvider, String> {
    Ok(wave_win32::Win32Provider::new())
}

/// The mutator for the machine the app is running on.
#[cfg(windows)]
pub fn mutator() -> Result<impl SystemMutator, String> {
    Ok(wave_win32::Win32Mutator::new())
}

/// Wave is a Windows program; the non-Windows branches exist only so the
/// workspace builds on a Linux CI runner, where `wave-core`'s tests run
/// against fixtures. They never pretend to have found or changed anything.
#[cfg(not(windows))]
pub fn provider() -> Result<impl SystemProvider, String> {
    // The type still has to be nameable, so hand back a provider over an empty
    // fixture — and refuse before it is ever used.
    let _unused = wave_core::fixture::FixtureProvider::from_json("{}");
    Err::<wave_core::fixture::FixtureProvider, String>(NOT_WINDOWS.to_string())
}

#[cfg(not(windows))]
pub fn mutator() -> Result<impl SystemMutator, String> {
    let _unused = wave_core::fixture::FixtureProvider::from_json("{}");
    Err::<wave_core::fixture::FixtureMutator, String>(NOT_WINDOWS.to_string())
}
