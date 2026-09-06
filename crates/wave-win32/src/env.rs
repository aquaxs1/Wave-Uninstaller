//! `%VARIABLE%` expansion.

use windows::core::HSTRING;
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;

/// Expand environment variables the way Windows does.
///
/// On failure the input is returned unchanged. An unexpanded `%APPDATA%` shows
/// up as an obviously wrong path in the UI, which is recoverable; a silently
/// emptied one would look like a plausible path pointing at the wrong place.
pub fn expand(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }

    let source = HSTRING::from(value);

    // The first call reports the required length in wide characters,
    // including the terminator.
    let required = unsafe { ExpandEnvironmentStringsW(&source, None) };
    if required == 0 {
        return value.to_string();
    }

    let mut buffer = vec![0u16; required as usize];
    let written = unsafe { ExpandEnvironmentStringsW(&source, Some(&mut buffer)) };
    if written == 0 || written as usize > buffer.len() {
        return value.to_string();
    }

    // `written` counts the terminating NUL.
    let end = (written as usize).saturating_sub(1);
    String::from_utf16_lossy(&buffer[..end])
}
