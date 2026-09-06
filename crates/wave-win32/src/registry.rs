//! Registry reads.

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS,
    WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, HKEY,
    HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ,
    KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ,
    REG_QWORD, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
};

use wave_core::error::{Error, Result};
use wave_core::provider::{RegistryRoot, RegistryValue, RegistryValues, RegistryView};

/// An open key that closes itself.
struct OpenKey(HKEY);

impl Drop for OpenKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn hive(root: RegistryRoot) -> HKEY {
    match root {
        RegistryRoot::LocalMachine => HKEY_LOCAL_MACHINE,
        RegistryRoot::CurrentUser => HKEY_CURRENT_USER,
        RegistryRoot::ClassesRoot => HKEY_CLASSES_ROOT,
        RegistryRoot::Users => HKEY_USERS,
    }
}

/// The access mask, including the redirector flag.
///
/// Naming the view on every open is what keeps 32-bit programs visible on a
/// 64-bit machine — without `KEY_WOW64_32KEY` their keys simply are not there.
fn access(view: RegistryView) -> REG_SAM_FLAGS {
    KEY_READ
        | match view {
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

fn to_error(status: WIN32_ERROR, subject: String) -> Error {
    match status {
        ERROR_FILE_NOT_FOUND => Error::not_found(subject),
        ERROR_ACCESS_DENIED => Error::access_denied(subject),
        other => Error::system(subject, windows::core::Error::from_hresult(other.to_hresult())),
    }
}

fn open(root: RegistryRoot, path: &str, view: RegistryView) -> Result<OpenKey> {
    let mut handle = HKEY::default();
    let status =
        unsafe { RegOpenKeyExW(hive(root), &HSTRING::from(path), None, access(view), &mut handle) };

    if status != ERROR_SUCCESS {
        return Err(to_error(status, describe(root, path, view)));
    }
    Ok(OpenKey(handle))
}

/// Immediate subkey names.
pub fn subkeys(root: RegistryRoot, path: &str, view: RegistryView) -> Result<Vec<String>> {
    let key = open(root, path, view)?;
    let subject = describe(root, path, view);

    let mut subkey_count = 0u32;
    let mut max_name_len = 0u32;
    let status = unsafe {
        RegQueryInfoKeyW(
            key.0,
            None,
            None,
            None,
            Some(&mut subkey_count),
            Some(&mut max_name_len),
            None,
            None,
            None,
            None,
            None,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(to_error(status, subject));
    }

    let mut names = Vec::with_capacity(subkey_count as usize);
    // `max_name_len` excludes the terminator, and a key can be created while
    // this loop runs, so leave headroom rather than sizing exactly.
    let capacity = max_name_len as usize + 2;

    for index in 0..subkey_count {
        let mut buffer = vec![0u16; capacity];
        let mut length = buffer.len() as u32;

        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut length,
                None,
                None,
                None,
                None,
            )
        };

        match status {
            ERROR_SUCCESS => names.push(String::from_utf16_lossy(&buffer[..length as usize])),
            // The key set shrank mid-enumeration — an uninstaller may well be
            // running right now. What was read so far is still valid.
            ERROR_NO_MORE_ITEMS => break,
            other => return Err(to_error(other, subject)),
        }
    }

    Ok(names)
}

/// Every value of a single key.
pub fn values(root: RegistryRoot, path: &str, view: RegistryView) -> Result<RegistryValues> {
    let key = open(root, path, view)?;
    let subject = describe(root, path, view);

    let mut value_count = 0u32;
    let mut max_name_len = 0u32;
    let mut max_data_len = 0u32;
    let status = unsafe {
        RegQueryInfoKeyW(
            key.0,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&mut value_count),
            Some(&mut max_name_len),
            Some(&mut max_data_len),
            None,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(to_error(status, subject));
    }

    let mut values = RegistryValues::new();
    let name_capacity = max_name_len as usize + 2;
    let mut data_capacity = max_data_len as usize + 2;

    for index in 0..value_count {
        let mut name = vec![0u16; name_capacity];
        let mut name_len = name.len() as u32;
        let mut data = vec![0u8; data_capacity];
        let mut data_len = data.len() as u32;
        let mut kind = 0u32;

        let mut status = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            )
        };

        // A value can grow between the size query and the read.
        if status == ERROR_MORE_DATA {
            data_capacity = data_len as usize + 2;
            data = vec![0u8; data_capacity];
            data_len = data.len() as u32;
            name_len = name.len() as u32;
            status = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut name_len,
                    None,
                    Some(&mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_len),
                )
            };
        }

        match status {
            ERROR_SUCCESS => {}
            ERROR_NO_MORE_ITEMS => break,
            other => return Err(to_error(other, subject)),
        }

        if let Some(value) = decode(kind, &data[..data_len as usize]) {
            values.insert(String::from_utf16_lossy(&name[..name_len as usize]), value);
        }
    }

    Ok(values)
}

/// Turn raw value bytes into a typed value, or `None` for types Wave ignores.
fn decode(kind: u32, data: &[u8]) -> Option<RegistryValue> {
    match REG_VALUE_TYPE(kind) {
        REG_SZ => Some(RegistryValue::Text(wide_string(data))),
        REG_EXPAND_SZ => Some(RegistryValue::ExpandText(wide_string(data))),
        REG_MULTI_SZ => Some(RegistryValue::MultiText(wide_string_list(data))),
        REG_DWORD => {
            let bytes: [u8; 4] = data.get(..4)?.try_into().ok()?;
            Some(RegistryValue::Dword(u32::from_le_bytes(bytes)))
        }
        REG_QWORD => {
            let bytes: [u8; 8] = data.get(..8)?.try_into().ok()?;
            Some(RegistryValue::Qword(u64::from_le_bytes(bytes)))
        }
        REG_BINARY => Some(RegistryValue::Binary(data.to_vec())),
        _ => None,
    }
}

/// Decode UTF-16 bytes, stopping at the first NUL.
///
/// Read through `chunks_exact` rather than by casting the pointer: registry
/// data carries no alignment guarantee, and a `&[u8]` to `&[u16]` transmute on
/// an odd address is undefined behaviour.
fn wide_string(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Decode a `REG_MULTI_SZ`: NUL-separated strings, terminated by an empty one.
fn wide_string_list(data: &[u8]) -> Vec<String> {
    let units: Vec<u16> =
        data.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();

    units
        .split(|unit| *unit == 0)
        .filter(|part| !part.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}
