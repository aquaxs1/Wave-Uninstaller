//! Normalization used to recognize that two entries describe the same thing.
//!
//! The inventory needs this to merge duplicates; the leftover scan will need
//! the same functions later to decide whether a folder named `Mozilla` belongs
//! to a publisher called `Mozilla Corporation`.

/// Company suffixes that carry no identifying information.
const LEGAL_SUFFIXES: &[&str] = &[
    "inc",
    "incorporated",
    "llc",
    "llp",
    "ltd",
    "limited",
    "gmbh",
    "mbh",
    "ug",
    "ag",
    "kg",
    "kgaa",
    "corp",
    "corporation",
    "co",
    "company",
    "sa",
    "sas",
    "sarl",
    "srl",
    "spa",
    "bv",
    "nv",
    "oy",
    "oyj",
    "ab",
    "as",
    "plc",
    "pty",
    "pte",
    "sl",
    "gbr",
    "ohg",
    "se",
];

/// Tokens inside a parenthetical that mark a build variant rather than a
/// different product: `Mozilla Firefox (x64 de)` is still Mozilla Firefox.
const VARIANT_TOKENS: &[&str] =
    &["x64", "x86", "amd64", "arm64", "arm", "win32", "win64", "32", "64", "bit", "32bit", "64bit"];

/// Lowercase, drop punctuation, collapse whitespace.
fn simplify(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;

    for ch in value.chars() {
        if ch.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.extend(ch.to_lowercase());
        } else {
            pending_space = true;
        }
    }

    out
}

/// A publisher name reduced to its identifying part.
///
/// `Mozilla Corporation` and `Mozilla Corp.` both become `mozilla`.
pub fn normalize_vendor(value: &str) -> String {
    let simplified = simplify(value);
    let mut tokens: Vec<&str> = simplified.split(' ').filter(|t| !t.is_empty()).collect();

    // Only trailing suffixes are noise. A leading "Co" could be part of the
    // actual name, and "Adobe Inc Systems" is not a thing.
    while let Some(last) = tokens.last() {
        if LEGAL_SUFFIXES.contains(last) && tokens.len() > 1 {
            tokens.pop();
        } else {
            break;
        }
    }

    tokens.join(" ")
}

/// Whether a parenthetical group is a build variant rather than part of the name.
fn is_variant_group(inner: &str) -> bool {
    let simplified = simplify(inner);
    let tokens: Vec<&str> = simplified.split(' ').filter(|t| !t.is_empty()).collect();

    !tokens.is_empty()
        && tokens.iter().all(|token| VARIANT_TOKENS.contains(token) || is_locale_code(token))
}

/// Whether a token is a language or region code.
///
/// Strictly two letters. Simplification has already split `en-US` into `en us`
/// by the time this runs, so a real locale never reaches here as four
/// characters — and accepting four would swallow ordinary words like `User`,
/// turning `Visual Studio Code (User)` into the machine-wide install.
fn is_locale_code(token: &str) -> bool {
    token.len() == 2 && token.chars().all(|c| c.is_ascii_alphabetic())
}

/// A product name reduced to its identifying part.
///
/// Drops parentheticals that only hold architecture or locale markers, then
/// applies the same simplification as [`normalize_vendor`]. Parentheticals with
/// real content are kept — `Visual Studio Code (User)` must not collapse into
/// the machine-wide install.
pub fn normalize_product_name(value: &str) -> String {
    let mut cleaned = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(open) = rest.find('(') {
        let Some(close_offset) = rest[open..].find(')') else { break };
        let close = open + close_offset;

        cleaned.push_str(&rest[..open]);
        let inner = &rest[open + 1..close];
        if !is_variant_group(inner) {
            cleaned.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    cleaned.push_str(rest);

    simplify(&cleaned)
}

/// Whether a string is a `{GUID}` as MSI writes product codes.
///
/// Uninstall keys created by MSI are *named* after the product code, which is
/// the one fully reliable way to tie a registry entry to an MSI product —
/// far better than comparing display names.
pub fn is_guid(value: &str) -> bool {
    let trimmed = value.trim();
    let Some(inner) = trimmed.strip_prefix('{').and_then(|v| v.strip_suffix('}')) else {
        return false;
    };
    if inner.len() != 36 {
        return false;
    }

    inner.chars().enumerate().all(|(index, ch)| match index {
        8 | 13 | 18 | 23 => ch == '-',
        _ => ch.is_ascii_hexdigit(),
    })
}

/// `YYYYMMDD` as installers write it, turned into ISO `YYYY-MM-DD`.
///
/// Anything that is not eight plausible digits is dropped rather than guessed
/// at — a wrong date in the UI is worse than no date.
pub fn normalize_install_date(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() != 8 || !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let month: u32 = trimmed[4..6].parse().ok()?;
    let day: u32 = trimmed[6..8].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    Some(format!("{}-{}-{}", &trimmed[0..4], &trimmed[4..6], &trimmed[6..8]))
}

/// The company name inside an X.509 subject.
///
/// Store packages report their publisher as a certificate subject —
/// `CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, C=US`.
/// Showing that verbatim is noise, and comparing it against a registry
/// publisher would never match.
pub fn publisher_from_subject(value: &str) -> String {
    let trimmed = value.trim();

    let Some(start) = find_common_name(trimmed) else {
        return trimmed.to_string();
    };
    let rest = &trimmed[start..];

    // Relative distinguished names are comma-separated, and a comma inside a
    // value is backslash-escaped.
    let mut name = String::with_capacity(rest.len());
    let mut escaped = false;
    for ch in rest.chars() {
        match ch {
            _ if escaped => {
                name.push(ch);
                escaped = false;
            }
            '\\' => escaped = true,
            ',' => break,
            _ => name.push(ch),
        }
    }

    let name = name.trim().trim_matches('"').trim();
    if name.is_empty() {
        trimmed.to_string()
    } else {
        name.to_string()
    }
}

/// Byte offset just past a `CN=` that starts a relative distinguished name.
fn find_common_name(subject: &str) -> Option<usize> {
    let lowered = subject.to_ascii_lowercase();
    let mut search_from = 0;

    while let Some(offset) = lowered[search_from..].find("cn=") {
        let start = search_from + offset;
        // Only at the beginning or right after a separator, so that an
        // organization called "Acme, Inc=CN=..." cannot be misread.
        let preceded_by_separator = subject[..start]
            .chars()
            .rev()
            .find(|ch| !ch.is_whitespace())
            .is_none_or(|ch| ch == ',' || ch == ';');

        if preceded_by_separator {
            return Some(start + 3);
        }
        search_from = start + 3;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_names_lose_their_legal_suffix() {
        assert_eq!(normalize_vendor("Mozilla Corporation"), "mozilla");
        assert_eq!(normalize_vendor("Mozilla Corp."), "mozilla");
        assert_eq!(normalize_vendor("Acme Software GmbH"), "acme software");
        assert_eq!(normalize_vendor("Valve"), "valve");
    }

    #[test]
    fn a_vendor_name_is_never_reduced_to_nothing() {
        // "Limited" as the entire company name is unhelpful but real; emptying
        // it would make every such publisher match every other one.
        assert_eq!(normalize_vendor("Limited"), "limited");
        assert_eq!(normalize_vendor("Co"), "co");
    }

    #[test]
    fn build_variants_do_not_split_a_product_in_two() {
        assert_eq!(normalize_product_name("Mozilla Firefox (x64 de)"), "mozilla firefox");
        assert_eq!(normalize_product_name("7-Zip (64-bit)"), "7 zip");
        assert_eq!(normalize_product_name("Notepad++ (32-bit x86)"), "notepad");
    }

    #[test]
    fn meaningful_parentheticals_are_kept() {
        // A per-user and a machine-wide VS Code are genuinely two installs.
        assert_ne!(
            normalize_product_name("Microsoft Visual Studio Code (User)"),
            normalize_product_name("Microsoft Visual Studio Code"),
        );
    }

    #[test]
    fn recognizes_product_code_guids() {
        assert!(is_guid("{23170F69-40C1-2702-2409-000001000000}"));
        // Inno Setup's key names look close enough to fool a sloppy check.
        assert!(!is_guid("{771FD6B0-FA20-440A-A002-3B3BAC16DC50}_is1"));
        assert!(!is_guid("23170F69-40C1-2702-2409-000001000000"));
        assert!(!is_guid("{not-a-guid}"));
        assert!(!is_guid(""));
    }

    #[test]
    fn certificate_subjects_yield_a_readable_publisher() {
        assert_eq!(
            publisher_from_subject(
                "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, C=US"
            ),
            "Microsoft Corporation"
        );
        assert_eq!(publisher_from_subject("CN=Valve"), "Valve");
        // Escaped commas belong to the value, not to the next component.
        assert_eq!(publisher_from_subject(r"CN=Acme\, Inc., C=US"), "Acme, Inc.");
        // Not a subject at all — leave it alone.
        assert_eq!(publisher_from_subject("Igor Pavlov"), "Igor Pavlov");
        assert_eq!(publisher_from_subject(""), "");
    }

    #[test]
    fn a_publisher_reduced_to_a_readable_name_still_normalizes() {
        // The Store and the registry have to agree on a publisher for the
        // leftover scan to match a folder against it.
        assert_eq!(
            normalize_vendor(&publisher_from_subject("CN=Microsoft Corporation, C=US")),
            normalize_vendor("Microsoft Corporation")
        );
    }

    #[test]
    fn install_dates_are_normalized_or_dropped() {
        assert_eq!(normalize_install_date("20250114").as_deref(), Some("2025-01-14"));
        // A wrong date shown with confidence is worse than none at all.
        assert_eq!(normalize_install_date("20251345"), None);
        assert_eq!(normalize_install_date("14.01.2025"), None);
        assert_eq!(normalize_install_date(""), None);
    }
}
