//! Choosing which [`SystemProvider`] the application talks to.

use wave_core::SystemProvider;

/// The provider for the machine the app is running on.
///
/// Wave is a Windows program; the non-Windows branch exists only so the
/// workspace builds on a Linux CI runner, where `wave-core`'s tests run
/// against fixtures. It never pretends to have found anything.
#[cfg(windows)]
pub fn provider() -> Result<impl SystemProvider, String> {
    Ok(wave_win32::Win32Provider::new())
}

#[cfg(not(windows))]
pub fn provider() -> Result<impl SystemProvider, String> {
    // The type still has to be nameable, so hand back a provider over an empty
    // fixture — and refuse before it is ever used.
    let _unused = wave_core::fixture::FixtureProvider::from_json("{}");
    Err::<wave_core::fixture::FixtureProvider, String>(
        "Wave Uninstaller reads the Windows registry and only runs on Windows.".to_string(),
    )
}
