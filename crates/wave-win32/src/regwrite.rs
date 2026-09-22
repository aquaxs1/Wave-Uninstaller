//! Registry writes: exporting a key, removing one, putting one back.
//!
//! Every open names a view, exactly as the reads do. A delete that forgets
//! `KEY_WOW64_32KEY` silently does nothing to a 32-bit program's keys, and
//! "nothing happened" is the hardest kind of bug to notice in a cleaner.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW,
    RegSetValueExW, HKEY, KEY_READ, KEY_SET_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_BINARY,
    REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_OPTION_NON_VOLATILE, REG_QWORD, REG_SAM_FLAGS,
    REG_SZ, REG_VALUE_TYPE,
};

use wave_core::error::{Error, Result};
use wave_core::mutator::RegistryKeyBackup;
use wave_core::provider::{RegistryRoot, RegistryValue, RegistryView};

use crate::registry;

/// An open key that closes itself.
struct OpenKey(HKEY);

impl Drop for OpenKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn view_flag(view: RegistryView) -> REG_SAM_FLAGS {
    match view {
        RegistryView::Bits64 => KEY_WOW64_64KEY,
        RegistryView::Bits32 => KEY_WOW64_32KEY,
    }
}

fn describe(root: RegistryRoot, path: &str, view: RegistryView) -> String {
    let bits = match view {
        RegistryView::Bits64 => "64-bit view",
        RegistryView::Bits32 => "32-bit view",
    };
    format!("{}\\{path} ({bits})", root.short_name())
}

fn to_error(status: windows::Win32::Foundation::WIN32_ERROR, subject: String) -> Error {
    match status {
        ERROR_FILE_NOT_FOUND => Error::not_found(subject),
        ERROR_ACCESS_DENIED => Error::access_denied(subject),
        other => Error::system(subject, windows::core::Error::from_hresult(other.to_hresult())),
    }
}

/// A key and everything below it, as data.
pub fn export(root: RegistryRoot, path: &str, view: RegistryView) -> Result<RegistryKeyBackup> {
    let values = registry::values(root, path, view)?;

    let mut subkeys = Vec::new();
    for name in registry::subkeys(root, path, view)? {
        let child = format!("{path}\\{name}");
        match export(root, &child, view) {
            Ok(backup) => subkeys.push(backup),
            // A subkey that disappeared between listing and reading is not a
            // reason to abandon the export — but it does mean the backup is
            // missing something, so it is logged rather than swallowed.
            Err(error) => tracing::warn!(key = %child, %error, "a subkey could not be exported"),
        }
    }

    Ok(RegistryKeyBackup { root, path: path.to_string(), view, values, subkeys })
}

/// The `DELETE` standard access right.
///
/// Spelled out rather than imported: the constant the `windows` crate exposes
/// under that name is typed for file handles, and a registry key needs the
/// same bit with a different type.
const DELETE_ACCESS: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0001_0000);

/// Remove a key and everything below it.
///
/// Two calls, because neither does the whole job with the redirector in play.
/// `RegDeleteTreeW` takes no view flag, so it has to run against a handle that
/// was opened with one — otherwise a 32-bit program's key is read through the
/// 64-bit view and the delete silently clears the wrong side, or nothing at
/// all. `RegDeleteKeyExW` then removes the now-empty key itself, and that one
/// does name the view.
pub fn delete_key(root: RegistryRoot, path: &str, view: RegistryView) -> Result<()> {
    let subject = describe(root, path, view);

    {
        let key = open(root, path, KEY_READ | KEY_SET_VALUE | DELETE_ACCESS | view_flag(view))?;
        // A null subkey means "everything below this key", leaving the key.
        let status = unsafe { RegDeleteTreeW(key.0, PCWSTR::null()) };
        if status != ERROR_SUCCESS {
            return Err(to_error(status, subject));
        }
    }

    let status = unsafe {
        RegDeleteKeyExW(registry::hive(root), &HSTRING::from(path), view_flag(view).0, None)
    };
    if status != ERROR_SUCCESS {
        return Err(to_error(status, subject));
    }

    Ok(())
}

/// Remove a single value, leaving the key it lives in alone.
pub fn delete_value(root: RegistryRoot, path: &str, view: RegistryView, name: &str) -> Result<()> {
    let key = open(root, path, KEY_READ | KEY_SET_VALUE | view_flag(view))?;
    let status = unsafe { RegDeleteValueW(key.0, &HSTRING::from(name)) };
    if status != ERROR_SUCCESS {
        return Err(to_error(status, format!("{} → {name}", describe(root, path, view))));
    }
    Ok(())
}

/// Recreate one key with the values it had. Undo only.
pub fn restore_key(backup: &RegistryKeyBackup) -> Result<()> {
    let key = create(backup.root, &backup.path, backup.view)?;
    for (name, value) in &backup.values {
        write_value(&key, name, value).map_err(|error| {
            Error::system(describe(backup.root, &backup.path, backup.view), error)
        })?;
    }
    Ok(())
}

/// Write one value back into a key that already exists. Undo only.
pub fn restore_value(
    root: RegistryRoot,
    path: &str,
    view: RegistryView,
    name: &str,
    value: &RegistryValue,
) -> Result<()> {
    let key = create(root, path, view)?;
    write_value(&key, name, value).map_err(|error| Error::system(describe(root, path, view), error))
}

fn open(root: RegistryRoot, path: &str, access: REG_SAM_FLAGS) -> Result<OpenKey> {
    let mut handle = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(registry::hive(root), &HSTRING::from(path), None, access, &mut handle)
    };
    if status != ERROR_SUCCESS {
        return Err(to_error(status, format!("{}\\{path}", root.short_name())));
    }
    Ok(OpenKey(handle))
}

fn create(root: RegistryRoot, path: &str, view: RegistryView) -> Result<OpenKey> {
    let mut handle = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            registry::hive(root),
            &HSTRING::from(path),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_SET_VALUE | view_flag(view),
            None,
            &mut handle,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(to_error(status, describe(root, path, view)));
    }
    Ok(OpenKey(handle))
}

/// Write one value with the type it was captured as.
///
/// The type matters as much as the data: restoring a `REG_EXPAND_SZ` as a
/// plain string turns `%ProgramFiles%` into a folder nobody has.
fn write_value(key: &OpenKey, name: &str, value: &RegistryValue) -> windows::core::Result<()> {
    let name = HSTRING::from(name);
    let (kind, bytes): (REG_VALUE_TYPE, Vec<u8>) = match value {
        RegistryValue::Text(text) => (REG_SZ, wide_bytes(text)),
        RegistryValue::ExpandText(text) => (REG_EXPAND_SZ, wide_bytes(text)),
        RegistryValue::MultiText(parts) => {
            let mut bytes = Vec::new();
            for part in parts {
                bytes.extend(wide_bytes(part));
            }
            // The list ends with an empty string of its own.
            bytes.extend([0, 0]);
            (REG_MULTI_SZ, bytes)
        }
        RegistryValue::Dword(number) => (REG_DWORD, number.to_le_bytes().to_vec()),
        RegistryValue::Qword(number) => (REG_QWORD, number.to_le_bytes().to_vec()),
        RegistryValue::Binary(bytes) => (REG_BINARY, bytes.clone()),
    };

    unsafe { RegSetValueExW(key.0, &name, None, kind, Some(&bytes)).ok() }
}

/// UTF-16LE with the terminating NUL, as the registry stores strings.
fn wide_bytes(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
    for unit in text.encode_utf16() {
        bytes.extend(unit.to_le_bytes());
    }
    bytes.extend([0, 0]);
    bytes
}
