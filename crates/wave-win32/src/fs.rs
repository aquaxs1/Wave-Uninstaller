//! File system writes.
//!
//! Nothing here decides what may be touched — by the time a path arrives, the
//! protection list in `wave-core` has already had its say. What this file is
//! responsible for is doing exactly what was asked and nothing more: in
//! particular, never walking through a reparse point.

use std::fs;
use std::io::Write;
use std::os::windows::fs::MetadataExt;

use wave_core::error::{Error, Result};
use wave_core::provider::DirEntry;

/// `FILE_ATTRIBUTE_REPARSE_POINT`.
///
/// Read from the attributes rather than inferred from `is_symlink`: a junction
/// is a reparse point that is not a symlink, and it is the form that actually
/// turns up in `AppData`.
const REPARSE_POINT: u32 = 0x0000_0400;

/// Windows' own "the file is on another volume" error.
const ERROR_NOT_SAME_DEVICE: i32 = 17;

/// One level of a directory, without following anything.
pub fn read_dir(path: &str) -> Result<Vec<DirEntry>> {
    let reader = fs::read_dir(path).map_err(|error| to_error(path, error))?;

    let mut entries = Vec::new();
    for entry in reader {
        // An entry that vanished mid-enumeration is normal — an uninstaller
        // may be running right now — and never a reason to fail the listing.
        let Ok(entry) = entry else { continue };
        let Ok(metadata) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();

        entries.push(DirEntry {
            path: entry.path().to_string_lossy().to_string(),
            is_directory: metadata.is_dir(),
            is_reparse_point: metadata.file_attributes() & REPARSE_POINT != 0,
            size_bytes: if metadata.is_dir() { 0 } else { metadata.len() },
            name,
        });
    }

    Ok(entries)
}

pub fn read_text_file(path: &str) -> Result<String> {
    fs::read_to_string(path).map_err(|error| to_error(path, error))
}

pub fn create_directory(path: &str) -> Result<()> {
    fs::create_dir_all(path).map_err(|error| to_error(path, error))
}

pub fn write_text_file(path: &str, contents: &str) -> Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(path, contents).map_err(|error| to_error(path, error))
}

/// Append one line and flush it before returning.
///
/// The flush is not incidental. A journal line has to be on disk before the
/// action it describes runs, and a buffered write that dies with the process
/// describes nothing.
pub fn append_line(path: &str, line: &str) -> Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| to_error(path, error))?;
    file.write_all(line.as_bytes()).map_err(|error| to_error(path, error))?;
    file.write_all(b"\r\n").map_err(|error| to_error(path, error))?;
    file.sync_data().map_err(|error| to_error(path, error))
}

/// Move a file or directory, creating the destination's parents.
///
/// A rename is the whole operation when both sides are on one volume, and it
/// moves a junction as the link it is rather than following it. Across
/// volumes there is no rename, so the tree is copied first and only removed
/// once the copy is complete — a backup that exists before the original goes
/// away is the entire point of moving instead of deleting.
pub fn move_path(from: &str, to: &str) -> Result<()> {
    if let Some(parent) = std::path::Path::new(to).parent() {
        fs::create_dir_all(parent).map_err(|error| to_error(&parent.to_string_lossy(), error))?;
    }

    match fs::rename(from, to) {
        Ok(()) => return Ok(()),
        Err(error) if error.raw_os_error() != Some(ERROR_NOT_SAME_DEVICE) => {
            return Err(to_error(from, error))
        }
        Err(_) => {}
    }

    copy_tree(from, to)?;
    remove_tree(from)
}

/// Copy a file or a whole directory to a new location.
///
/// Refuses outright when it meets a reparse point inside the tree: recreating
/// a junction is not something this code does, and copying through one would
/// pull in a directory that was never part of the removal. Refusing leaves
/// everything where it is, which is the safe half of the trade.
fn copy_tree(from: &str, to: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(from).map_err(|error| to_error(from, error))?;

    if metadata.file_attributes() & REPARSE_POINT != 0 {
        return Err(Error::Unsupported(format!(
            "{from} is a junction and the backup is on another volume; Wave will not copy through \
             a reparse point"
        )));
    }

    if !metadata.is_dir() {
        fs::copy(from, to).map_err(|error| to_error(from, error))?;
        return Ok(());
    }

    fs::create_dir_all(to).map_err(|error| to_error(to, error))?;
    for entry in fs::read_dir(from).map_err(|error| to_error(from, error))? {
        let entry = entry.map_err(|error| to_error(from, error))?;
        let name = entry.file_name();
        let source = entry.path();
        let destination = std::path::Path::new(to).join(&name);
        copy_tree(&source.to_string_lossy(), &destination.to_string_lossy())?;
    }

    Ok(())
}

/// Remove what has already been copied elsewhere.
///
/// Only ever called after [`copy_tree`] succeeded, so the contents exist in
/// the backup before this runs. A reparse point is removed as the link it is.
fn remove_tree(path: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| to_error(path, error))?;

    if metadata.file_attributes() & REPARSE_POINT != 0 {
        return if metadata.is_dir() {
            fs::remove_dir(path).map_err(|error| to_error(path, error))
        } else {
            fs::remove_file(path).map_err(|error| to_error(path, error))
        };
    }

    if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(|error| to_error(path, error))
    } else {
        fs::remove_file(path).map_err(|error| to_error(path, error))
    }
}

fn to_error(subject: &str, error: std::io::Error) -> Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => Error::not_found(subject),
        std::io::ErrorKind::PermissionDenied => Error::access_denied(subject),
        _ => Error::system(subject, error),
    }
}
