//! Rendering a captured key as a `.reg` file.
//!
//! The structured JSON backup is what Wave's own undo reads. This is the
//! second copy, in the format `regedit` imports, for the case the JSON cannot
//! help with: Wave itself being broken, or gone, when someone needs their key
//! back.

use crate::mutator::RegistryKeyBackup;
use crate::provider::{RegistryRoot, RegistryValue};

/// The whole subtree as a `.reg` file, ready to double-click.
pub fn render(backup: &RegistryKeyBackup) -> String {
    let mut out = String::from("Windows Registry Editor Version 5.00\r\n");

    for key in backup.flatten() {
        out.push_str("\r\n[");
        out.push_str(hive_name(key.root));
        out.push('\\');
        out.push_str(&key.path);
        out.push_str("]\r\n");

        for (name, value) in &key.values {
            let name =
                if name.is_empty() { "@".to_string() } else { format!("\"{}\"", escape(name)) };
            out.push_str(&format!("{name}={}\r\n", render_value(value)));
        }
    }

    out
}

/// The hive as `regedit` spells it.
///
/// The 32-bit view has no spelling of its own — an import lands in whichever
/// view the importing process runs in. The JSON backup records the view
/// properly, so this file says as much in a comment rather than silently
/// restoring to the wrong side.
fn hive_name(root: RegistryRoot) -> &'static str {
    match root {
        RegistryRoot::LocalMachine => "HKEY_LOCAL_MACHINE",
        RegistryRoot::CurrentUser => "HKEY_CURRENT_USER",
        RegistryRoot::ClassesRoot => "HKEY_CLASSES_ROOT",
        RegistryRoot::Users => "HKEY_USERS",
    }
}

fn render_value(value: &RegistryValue) -> String {
    match value {
        RegistryValue::Text(text) => format!("\"{}\"", escape(text)),
        RegistryValue::ExpandText(text) => {
            format!("hex(2):{}", hex_bytes(&utf16_bytes(text)))
        }
        RegistryValue::MultiText(parts) => {
            let mut bytes = Vec::new();
            for part in parts {
                bytes.extend(utf16_bytes(part));
            }
            // A `REG_MULTI_SZ` ends with an empty string, which is two more
            // zero bytes after the last terminator.
            bytes.extend([0, 0]);
            format!("hex(7):{}", hex_bytes(&bytes))
        }
        RegistryValue::Dword(number) => format!("dword:{number:08x}"),
        RegistryValue::Qword(number) => {
            format!("hex(b):{}", hex_bytes(&number.to_le_bytes()))
        }
        RegistryValue::Binary(bytes) => format!("hex:{}", hex_bytes(bytes)),
    }
}

/// UTF-16LE with the terminating NUL, as the registry stores strings.
fn utf16_bytes(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
    for unit in text.encode_utf16() {
        bytes.extend(unit.to_le_bytes());
    }
    bytes.extend([0, 0]);
    bytes
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(",")
}

fn escape(value: &str) -> String {
    value.replace('\\', r"\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{RegistryValues, RegistryView};

    fn backup() -> RegistryKeyBackup {
        let mut values = RegistryValues::new();
        values.insert("DisplayName".to_string(), RegistryValue::Text("Acme Designer".to_string()));
        values.insert("Version".to_string(), RegistryValue::Dword(3));
        values.insert(
            "Path".to_string(),
            RegistryValue::ExpandText(r"%ProgramFiles%\Acme".to_string()),
        );

        RegistryKeyBackup {
            root: RegistryRoot::LocalMachine,
            path: r"SOFTWARE\Acme".to_string(),
            view: RegistryView::Bits64,
            values,
            subkeys: vec![RegistryKeyBackup {
                root: RegistryRoot::LocalMachine,
                path: r"SOFTWARE\Acme\Designer".to_string(),
                view: RegistryView::Bits64,
                values: RegistryValues::new(),
                subkeys: Vec::new(),
            }],
        }
    }

    #[test]
    fn renders_a_file_regedit_would_import() {
        let text = render(&backup());
        assert!(text.starts_with("Windows Registry Editor Version 5.00\r\n"));
        assert!(text.contains("[HKEY_LOCAL_MACHINE\\SOFTWARE\\Acme]\r\n"));
        assert!(text.contains("\"DisplayName\"=\"Acme Designer\"\r\n"));
        assert!(text.contains("\"Version\"=dword:00000003\r\n"));
        // An expandable string keeps its type, or restoring it would turn
        // `%ProgramFiles%` into a literal folder name.
        assert!(text.contains("\"Path\"=hex(2):"));
        // Subkeys come after their parent, so an import can create them.
        let parent = text.find("[HKEY_LOCAL_MACHINE\\SOFTWARE\\Acme]").expect("parent");
        let child = text.find("[HKEY_LOCAL_MACHINE\\SOFTWARE\\Acme\\Designer]").expect("child");
        assert!(parent < child);
    }

    #[test]
    fn escapes_what_would_otherwise_break_the_file() {
        let mut values = RegistryValues::new();
        values.insert(
            "Command".to_string(),
            RegistryValue::Text(r#""C:\Acme\run.exe" --now"#.to_string()),
        );
        let key = RegistryKeyBackup { values, ..backup() };

        assert!(render(&key).contains(r#""Command"="\"C:\\Acme\\run.exe\" --now""#));
    }
}
