//! Windows path handling that works on any platform.
//!
//! `std::path` follows the rules of whatever it is compiled for, and these
//! comparisons have to give the same answer on the Linux runner that runs the
//! tests as on the machine being cleaned. "Is this folder inside that one" is
//! the single most load-bearing question the protection list asks, so it is
//! answered here, by hand, with tests next to it.

/// A path reduced to a comparable form: lower case, backslashes, no trailing
/// separator, no repeated separators.
///
/// Case folding is ASCII-only. Windows compares file names with a full Unicode
/// table, but a `to_lowercase` that changes a string's length would break the
/// prefix arithmetic below, and every path Wave compares against — program
/// files, appdata, vendor folders — is ASCII where it matters.
pub fn normalize(path: &str) -> String {
    let trimmed = path.trim().trim_matches('"').trim();
    // `\\?\C:\...` is the long-path form; it names the same file.
    let trimmed = trimmed.strip_prefix(r"\\?\").unwrap_or(trimmed);

    let mut out = String::with_capacity(trimmed.len());
    let mut last_was_separator = false;
    for ch in trimmed.chars() {
        let ch = if ch == '/' { '\\' } else { ch };
        if ch == '\\' {
            // A leading `\\` is a UNC prefix and has to survive; every other
            // doubled separator is noise.
            if last_was_separator && out != "\\" {
                continue;
            }
            last_was_separator = true;
        } else {
            last_was_separator = false;
        }
        out.push(ch.to_ascii_lowercase());
    }

    // `C:` alone means "the current directory on C:", never the drive root, so
    // the root keeps its separator and nothing else does.
    while out.len() > 3 && out.ends_with('\\') {
        out.pop();
    }
    out
}

/// Whether `path` is the same as `ancestor` or sits below it.
///
/// Compares whole segments: `C:\Program Files\Acme` is not inside
/// `C:\Program Files\Ac`, however well the strings line up.
pub fn is_within(path: &str, ancestor: &str) -> bool {
    let path = normalize(path);
    let ancestor = normalize(ancestor);
    if ancestor.is_empty() || path.is_empty() {
        return false;
    }
    if path == ancestor {
        return true;
    }

    // `c:\` keeps its separator after normalization; every other ancestor has
    // none, so trimming here makes both forms line up.
    let ancestor = ancestor.trim_end_matches('\\');
    path.len() > ancestor.len()
        && path.starts_with(ancestor)
        && path[ancestor.len()..].starts_with('\\')
}

/// The last segment of a path — the file or folder name.
pub fn file_name(path: &str) -> &str {
    let trimmed = path.trim().trim_end_matches(['\\', '/']);
    match trimmed.rsplit(['\\', '/']).next() {
        Some(name) if !name.is_empty() => name,
        _ => trimmed,
    }
}

/// Everything before the last segment, or `None` for a drive or UNC root.
pub fn parent(path: &str) -> Option<String> {
    let normalized = normalize(path);
    // A drive or a UNC share has no parent to walk up to, and saying otherwise
    // turns every loop over parents into an endless one.
    if depth(&normalized) == 0 {
        return None;
    }

    let cut = normalized.rfind('\\')?;
    let parent = &normalized[..cut];
    if parent.len() == 2 && parent.ends_with(':') {
        // `C:` is not a usable parent; `C:\` is.
        return Some(format!("{parent}\\"));
    }
    if parent.is_empty() {
        return None;
    }
    Some(parent.to_string())
}

/// Join a parent and a child segment with a single separator.
pub fn join(parent: &str, child: &str) -> String {
    let parent = parent.trim_end_matches(['\\', '/']);
    let child = child.trim_start_matches(['\\', '/']);
    if parent.is_empty() {
        return child.to_string();
    }
    format!("{parent}\\{child}")
}

/// How many segments a path has below its root.
///
/// `C:\` is 0, `C:\Program Files` is 1. The protection list uses this to
/// refuse anything shallow: a finding two segments deep is a program folder, a
/// finding zero segments deep is a drive.
pub fn depth(path: &str) -> usize {
    let normalized = normalize(path);
    let without_root = match normalized.strip_prefix(r"\\") {
        // `\\server\share` is the root of a UNC path, so both parts are root.
        Some(rest) => rest.splitn(3, '\\').nth(2).unwrap_or("").to_string(),
        None => match normalized.split_once(":\\") {
            Some((_, rest)) => rest.to_string(),
            None => normalized.clone(),
        },
    };

    without_root.split('\\').filter(|segment| !segment.is_empty()).count()
}

/// The drive letter or UNC share a path lives on, as a folder name safe to use
/// inside the backup directory.
///
/// `C:\Program Files\Acme` becomes `C`, `\\nas\share\x` becomes `nas_share`.
/// Keeping them apart matters: two programs on two drives can otherwise map to
/// the same backup path and the second would overwrite the first.
pub fn volume_label(path: &str) -> String {
    let normalized = normalize(path);
    if let Some(rest) = normalized.strip_prefix(r"\\") {
        let mut parts = rest.split('\\').filter(|p| !p.is_empty());
        let server = parts.next().unwrap_or("unc");
        let share = parts.next().unwrap_or("share");
        return format!("{server}_{share}");
    }
    match normalized.split_once(':') {
        Some((drive, _)) if !drive.is_empty() => drive.to_string(),
        _ => "path".to_string(),
    }
}

/// The part of a path below its volume, for mirroring it inside the backup.
pub fn below_volume(path: &str) -> String {
    let normalized = normalize(path);
    if let Some(rest) = normalized.strip_prefix(r"\\") {
        let mut parts = rest.splitn(3, '\\');
        parts.next();
        parts.next();
        return parts.next().unwrap_or("").to_string();
    }
    match normalized.split_once(":\\") {
        Some((_, rest)) => rest.to_string(),
        None => normalized.trim_start_matches('\\').to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_the_forms_the_registry_actually_contains() {
        assert_eq!(normalize(r"C:\Program Files\7-Zip\"), r"c:\program files\7-zip");
        assert_eq!(normalize(r#"  "C:/Program Files//Acme"  "#), r"c:\program files\acme");
        assert_eq!(normalize(r"\\?\C:\Windows"), r"c:\windows");
        assert_eq!(normalize(r"C:\"), r"c:\");
        assert_eq!(normalize(r"\\nas\share\Acme"), r"\\nas\share\acme");
    }

    #[test]
    fn containment_respects_segment_boundaries() {
        assert!(is_within(r"C:\Program Files\Acme\bin", r"C:\Program Files\Acme"));
        assert!(is_within(r"C:\Program Files\Acme", r"C:\Program Files\Acme\"));
        // The single most dangerous false positive there is: a prefix match
        // that ignores where the folder name ends.
        assert!(!is_within(r"C:\Program Files\Acme Reader", r"C:\Program Files\Acme"));
        assert!(!is_within(r"C:\Program Files", r"C:\Program Files\Acme"));
    }

    #[test]
    fn depth_tells_a_drive_from_a_program_folder() {
        assert_eq!(depth(r"C:\"), 0);
        assert_eq!(depth(r"C:\Program Files"), 1);
        assert_eq!(depth(r"C:\Program Files\Acme\bin"), 3);
        assert_eq!(depth(r"\\nas\share"), 0);
        assert_eq!(depth(r"\\nas\share\Acme"), 1);
    }

    #[test]
    fn backup_paths_keep_volumes_apart() {
        assert_eq!(volume_label(r"C:\Program Files\Acme"), "c");
        assert_eq!(below_volume(r"C:\Program Files\Acme"), r"program files\acme");
        assert_eq!(volume_label(r"\\nas\share\Acme"), "nas_share");
        assert_eq!(below_volume(r"\\nas\share\Acme\bin"), r"acme\bin");
    }

    #[test]
    fn parents_and_names() {
        assert_eq!(file_name(r"C:\Program Files\Acme\"), "Acme");
        assert_eq!(parent(r"C:\Program Files\Acme").as_deref(), Some(r"c:\program files"));
        assert_eq!(parent(r"C:\Program Files").as_deref(), Some(r"c:\"));
        assert_eq!(parent(r"C:\"), None);
        assert_eq!(join(r"C:\Program Files", r"\Acme"), r"C:\Program Files\Acme");
    }
}
