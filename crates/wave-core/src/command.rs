//! Turning an `UninstallString` into something that can actually be started.
//!
//! This is parsing, and parsing is a decision — which is why it lives here and
//! not in `wave-win32`, where it could only be checked by running an
//! uninstaller and seeing what happened. The strings below are all real forms
//! found in the registry, and each one has a test.

use serde::{Deserialize, Serialize};

use crate::model::UninstallCommand;

/// Extensions that end the program part of an unquoted command line.
const EXECUTABLE_EXTENSIONS: &[&str] = &[".exe", ".com", ".bat", ".cmd"];

/// A command split into the program to start and its arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandLine {
    pub program: String,
    pub arguments: Vec<String>,
    /// Whether this form runs without asking the user anything.
    pub silent: bool,
}

impl CommandLine {
    /// The command as a user would read it in the UI.
    pub fn display(&self) -> String {
        let mut out = if self.program.contains(' ') {
            format!("\"{}\"", self.program)
        } else {
            self.program.clone()
        };
        for argument in &self.arguments {
            out.push(' ');
            if argument.contains(' ') {
                out.push('"');
                out.push_str(argument);
                out.push('"');
            } else {
                out.push_str(argument);
            }
        }
        out
    }
}

/// Split a command line the way Windows does.
///
/// Returns `None` for a string with no program in it at all.
pub fn parse(command: &str) -> Option<CommandLine> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (program, rest) = split_program(trimmed)?;
    Some(CommandLine { program, arguments: split_arguments(rest), silent: false })
}

/// The program part and whatever follows it.
///
/// The unquoted case is the interesting one: `C:\Program Files\Acme\setup.exe
/// --uninstall` has a space in the path and no quotes to say so. Cutting at
/// the first space would try to start `C:\Program`, so the split looks for an
/// executable extension instead and only falls back to the first space when
/// there is none.
fn split_program(command: &str) -> Option<(String, &str)> {
    if let Some(rest) = command.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((rest[..end].to_string(), &rest[end + 1..]));
    }

    let lowered = command.to_ascii_lowercase();
    for extension in EXECUTABLE_EXTENSIONS {
        let mut search_from = 0;
        while let Some(offset) = lowered[search_from..].find(extension) {
            let end = search_from + offset + extension.len();
            let follows = command[end..].chars().next();
            if follows.is_none_or(char::is_whitespace) {
                return Some((command[..end].to_string(), &command[end..]));
            }
            search_from = end;
        }
    }

    match command.find(char::is_whitespace) {
        Some(cut) => Some((command[..cut].to_string(), &command[cut..])),
        None => Some((command.to_string(), "")),
    }
}

/// Split arguments on whitespace, honouring quotes.
fn split_arguments(rest: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut has_content = false;

    for ch in rest.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                has_content = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has_content {
                    arguments.push(std::mem::take(&mut current));
                    has_content = false;
                }
            }
            c => {
                current.push(c);
                has_content = true;
            }
        }
    }
    if has_content {
        arguments.push(current);
    }

    arguments
}

/// How Wave would run a program's uninstaller.
///
/// Prefers the form that needs no clicking, in this order: the vendor's own
/// quiet string, then `msiexec` for an MSI product, then the plain uninstall
/// string — which may well open a window and wait for the user, so the UI says
/// so rather than pretending the removal is unattended.
pub fn uninstaller(uninstall: &UninstallCommand) -> Option<CommandLine> {
    if let Some(quiet) = uninstall.quiet_command.as_deref().filter(|q| !q.trim().is_empty()) {
        if let Some(mut parsed) = parse(quiet) {
            parsed.silent = true;
            return Some(parsed);
        }
    }

    if let Some(code) = uninstall.msi_product_code.as_deref() {
        return Some(CommandLine {
            program: "msiexec.exe".to_string(),
            // `/passive` shows a progress bar and asks nothing; `/norestart`
            // keeps an uninstaller from rebooting the machine out from under
            // a scan that has not run yet.
            arguments: vec![
                "/x".to_string(),
                code.to_string(),
                "/passive".to_string(),
                "/norestart".to_string(),
            ],
            silent: true,
        });
    }

    parse(&uninstall.command)
}

/// A command line in the form `CreateProcessW` expects.
///
/// The quoting rules are Windows', not a shell's: a run of backslashes is
/// doubled only when a quote follows it, and a quote inside an argument is
/// escaped with a backslash. Getting this wrong turns
/// `--path=C:\Program Files\` into two arguments, which for an uninstaller
/// means it removes the wrong thing or nothing at all.
pub fn render_for_windows(program: &str, arguments: &[String]) -> String {
    let mut out = quote(program);
    for argument in arguments {
        out.push(' ');
        out.push_str(&quote(argument));
    }
    out
}

fn quote(part: &str) -> String {
    if !part.is_empty() && !part.contains([' ', '\t', '"']) {
        return part.to_string();
    }

    let mut out = String::with_capacity(part.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in part.chars() {
        match ch {
            '\\' => {
                backslashes += 1;
                out.push(ch);
            }
            '"' => {
                // The backslashes before a quote are doubled, then the quote
                // itself is escaped.
                out.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                out.push_str("\\\"");
            }
            _ => {
                backslashes = 0;
                out.push(ch);
            }
        }
    }
    // A trailing backslash would otherwise escape the closing quote.
    out.push_str(&"\\".repeat(backslashes));
    out.push('"');

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_quoted_program_from_its_switches() {
        let parsed = parse(r#""C:\Users\s\AppData\Local\Programs\VS Code\unins000.exe" /SILENT"#)
            .expect("a command");
        assert_eq!(parsed.program, r"C:\Users\s\AppData\Local\Programs\VS Code\unins000.exe");
        assert_eq!(parsed.arguments, vec!["/SILENT"]);
    }

    #[test]
    fn finds_the_program_in_an_unquoted_path_with_spaces() {
        // The form that breaks every naive split-on-space.
        let parsed = parse(r"C:\Program Files\Acme\setup.exe --uninstall").expect("a command");
        assert_eq!(parsed.program, r"C:\Program Files\Acme\setup.exe");
        assert_eq!(parsed.arguments, vec!["--uninstall"]);
    }

    #[test]
    fn keeps_a_product_code_in_one_piece() {
        let parsed =
            parse("MsiExec.exe /X{23170F69-40C1-2702-2409-000001000000}").expect("a command");
        assert_eq!(parsed.program, "MsiExec.exe");
        assert_eq!(parsed.arguments, vec!["/X{23170F69-40C1-2702-2409-000001000000}"]);
    }

    #[test]
    fn a_path_without_an_extension_still_yields_a_program() {
        let parsed = parse(r"C:\Windows\System32\rundll32 shell32.dll").expect("a command");
        assert_eq!(parsed.program, r"C:\Windows\System32\rundll32");
        assert_eq!(parsed.arguments, vec!["shell32.dll"]);
    }

    #[test]
    fn an_msi_product_is_removed_without_a_dialog() {
        let command = UninstallCommand {
            command: "MsiExec.exe /X{ABC}".to_string(),
            quiet_command: None,
            msi_product_code: Some("{ABC}".to_string()),
        };
        let parsed = uninstaller(&command).expect("a command");
        assert!(parsed.silent);
        assert_eq!(parsed.arguments, vec!["/x", "{ABC}", "/passive", "/norestart"]);
    }

    #[test]
    fn a_vendor_quiet_string_wins_over_msiexec() {
        // The vendor's own uninstaller may clean up things msiexec does not,
        // and the quiet form is still unattended.
        let command = UninstallCommand {
            command: r"C:\Acme\setup.exe".to_string(),
            quiet_command: Some(r"C:\Acme\setup.exe /S".to_string()),
            msi_product_code: Some("{ABC}".to_string()),
        };
        let parsed = uninstaller(&command).expect("a command");
        assert_eq!(parsed.program, r"C:\Acme\setup.exe");
        assert_eq!(parsed.arguments, vec!["/S"]);
        assert!(parsed.silent);
    }

    #[test]
    fn renders_a_command_line_windows_will_split_back_correctly() {
        assert_eq!(
            render_for_windows(r"C:\Program Files\Acme\setup.exe", &["--uninstall".to_string()]),
            r#""C:\Program Files\Acme\setup.exe" --uninstall"#
        );
        // A trailing backslash inside quotes would escape the closing quote.
        assert_eq!(
            render_for_windows("msiexec.exe", &[r"/log=C:\temp dir\".to_string()]),
            r#"msiexec.exe "/log=C:\temp dir\\""#
        );
        assert_eq!(
            render_for_windows("setup.exe", &[r#"--name="Acme""#.to_string()]),
            r#"setup.exe "--name=\"Acme\"""#
        );
    }

    #[test]
    fn a_rendered_command_line_survives_a_round_trip() {
        let program = r"C:\Program Files\Acme\setup.exe";
        let rendered = render_for_windows(program, &["--uninstall".to_string()]);
        let parsed = parse(&rendered).expect("a command");
        assert_eq!(parsed.program, program);
        assert_eq!(parsed.arguments, vec!["--uninstall"]);
    }

    #[test]
    fn an_interactive_uninstaller_is_not_reported_as_silent() {
        let command = UninstallCommand {
            command: r"C:\Acme\uninstall.exe".to_string(),
            quiet_command: None,
            msi_product_code: None,
        };
        assert!(!uninstaller(&command).expect("a command").silent);
    }
}
