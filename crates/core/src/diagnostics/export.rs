//! Export of a staged preview (spec 0034, AC-13): the exact bytes of the
//! preview, copied to a path the client chose.
//!
//! The destination is an absolute UTF-8 path of at most 1024 bytes without
//! a `.` or `..` component (refused, never normalized), whose file name is
//! at most 200 bytes and whose parent folder exists. The copy goes to
//! `.stackly-diagnostics-<uuid>.partial` in that folder, created exclusive
//! with mode 0600, since a rename cannot cross volumes; it is synced, the
//! destination is looked at once more right before the rename, and the
//! partial moves into place without replacing anything, or, when the client
//! asked to replace, over a file or symlink (a symlink is replaced as
//! itself). A failure removes the partial. The bytes are hashed on the way
//! and must match the preview.

use std::path::{Path, PathBuf};

use stackly_platform::{FileKind, Filesystem, PlatformError};
use uuid::Uuid;

use super::staging::{StagedPreview, WriteReason};

/// The longest destination, in bytes.
pub const DESTINATION_MAX: usize = 1024;
/// The longest file name of a destination, in bytes.
pub const FILE_NAME_MAX: usize = 200;

/// Why a destination is refused, the `reason` of
/// `diagnostics.destination_invalid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DestinationProblem {
    /// The path is not absolute.
    NotAbsolute,
    /// The path is too long, has a `.`, `..`, or empty component, or its
    /// file name is missing or too long.
    InvalidName,
    /// The parent folder does not exist.
    ParentMissing,
    /// The parent is not a folder.
    ParentNotDirectory,
    /// The destination is a folder.
    IsDirectory,
    /// The destination is neither a regular file nor a symlink.
    NotRegular,
}

impl DestinationProblem {
    /// The wire code, such as `parent_missing`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NotAbsolute => "not_absolute",
            Self::InvalidName => "invalid_name",
            Self::ParentMissing => "parent_missing",
            Self::ParentNotDirectory => "parent_not_directory",
            Self::IsDirectory => "is_directory",
            Self::NotRegular => "not_regular",
        }
    }
}

/// An export that did not happen.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    /// The destination is refused.
    #[error("the destination {} is refused ({})", .path.display(), .reason.as_code())]
    Invalid {
        /// Why.
        reason: DestinationProblem,
        /// The destination as given.
        path: PathBuf,
    },
    /// Something is at the destination and the client did not ask to
    /// replace it.
    #[error("{} already exists", .path.display())]
    Exists {
        /// The destination.
        path: PathBuf,
    },
    /// The copy or the move failed; no partial file is left.
    #[error("the bundle cannot be written to {} ({}): {message}", .path.display(), .reason.as_code())]
    Write {
        /// Why.
        reason: WriteReason,
        /// The destination.
        path: PathBuf,
        /// What failed.
        message: String,
    },
}

/// A finished export.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Exported {
    /// The file written, in its canonical parent folder.
    pub path: PathBuf,
    /// Its size.
    pub bytes: u64,
    /// Its sha256, equal to the preview's.
    pub sha256: String,
}

/// Checks the text of a destination: absolute, at most
/// [`DESTINATION_MAX`] bytes, no `.`, `..`, or empty component, and a file
/// name of at most [`FILE_NAME_MAX`] bytes.
///
/// # Errors
///
/// Returns the problem with the path.
pub fn check_destination(destination: &str) -> Result<PathBuf, DestinationProblem> {
    if !destination.starts_with('/') {
        return Err(DestinationProblem::NotAbsolute);
    }
    if destination.len() > DESTINATION_MAX || destination.contains('\0') {
        return Err(DestinationProblem::InvalidName);
    }
    let parts: Vec<&str> = destination[1..].split('/').collect();
    let Some(name) = parts.last() else {
        return Err(DestinationProblem::InvalidName);
    };
    if name.len() > FILE_NAME_MAX
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(DestinationProblem::InvalidName);
    }
    Ok(PathBuf::from(destination))
}

/// Copies `preview` to `destination`, replacing a file or symlink there
/// only when `replace` is true.
///
/// # Errors
///
/// Returns [`ExportError::Invalid`] for a refused destination,
/// [`ExportError::Exists`] when something is there and `replace` is false,
/// and [`ExportError::Write`] when the copy or the move fails.
pub fn export(
    fs: &dyn Filesystem,
    preview: &StagedPreview,
    destination: &str,
    replace: bool,
) -> Result<Exported, ExportError> {
    let given = PathBuf::from(destination);
    let invalid = |reason| ExportError::Invalid {
        reason,
        path: given.clone(),
    };
    let path = check_destination(destination).map_err(invalid)?;
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(invalid(DestinationProblem::InvalidName));
    };
    let parent = match fs.canonicalize(parent) {
        Ok(parent) => parent,
        Err(PlatformError::NotFound { .. }) => {
            return Err(invalid(DestinationProblem::ParentMissing));
        }
        Err(PlatformError::NotADirectory { .. }) => {
            return Err(invalid(DestinationProblem::ParentNotDirectory));
        }
        Err(error) => return Err(write_error(&given, &error)),
    };
    match fs.symlink_metadata(&parent) {
        Ok(Some(info)) if info.kind == FileKind::Dir => {}
        Ok(_) => return Err(invalid(DestinationProblem::ParentNotDirectory)),
        Err(error) => return Err(write_error(&given, &error)),
    }
    let target = parent.join(name);
    let exists = present(fs, &target, &given)?;
    if exists && !replace {
        return Err(ExportError::Exists { path: given });
    }

    let partial = parent.join(format!(".stackly-diagnostics-{}.partial", Uuid::now_v7()));
    let result = copy_and_move(fs, preview, &partial, &target, &given, replace);
    if result.is_err() {
        // The partial is gone after a move, so this only removes a leftover.
        let _ = fs.remove_file(&partial);
    }
    result
}

/// Whether a file or symlink is at `target`; a folder or anything else
/// there is refused.
fn present(fs: &dyn Filesystem, target: &Path, given: &Path) -> Result<bool, ExportError> {
    let invalid = |reason| ExportError::Invalid {
        reason,
        path: given.to_path_buf(),
    };
    match fs.symlink_metadata(target) {
        Ok(None) => Ok(false),
        Ok(Some(info)) => match info.kind {
            FileKind::File | FileKind::Symlink => Ok(true),
            FileKind::Dir => Err(invalid(DestinationProblem::IsDirectory)),
            FileKind::Other => Err(invalid(DestinationProblem::NotRegular)),
        },
        Err(error) => Err(write_error(given, &error)),
    }
}

fn copy_and_move(
    fs: &dyn Filesystem,
    preview: &StagedPreview,
    partial: &Path,
    target: &Path,
    given: &Path,
    replace: bool,
) -> Result<Exported, ExportError> {
    let mut file = fs
        .create_file_private(partial)
        .map_err(|error| write_error(given, &error))?;
    let io_error = |error: &std::io::Error| ExportError::Write {
        reason: WriteReason::of_io(error),
        path: given.to_path_buf(),
        message: error.to_string(),
    };
    let (bytes, sha256) = preview
        .copy_to(&mut file)
        .map_err(|error| io_error(&error))?;
    file.sync_all().map_err(|error| io_error(&error))?;
    drop(file);
    if sha256 != preview.sha256 || bytes != preview.bytes {
        return Err(ExportError::Write {
            reason: WriteReason::Io,
            path: given.to_path_buf(),
            message: "the copy does not match the preview".to_owned(),
        });
    }
    // The destination is looked at once more right before the move.
    let exists = present(fs, target, given)?;
    let moved = if exists && replace {
        fs.rename_replace(partial, target)
    } else {
        fs.rename_no_replace(partial, target)
    };
    match moved {
        Ok(()) => Ok(Exported {
            path: target.to_path_buf(),
            bytes,
            sha256,
        }),
        Err(PlatformError::AlreadyExists { .. }) if replace => fs
            .rename_replace(partial, target)
            .map(|()| Exported {
                path: target.to_path_buf(),
                bytes,
                sha256: preview.sha256.clone(),
            })
            .map_err(|error| write_error(given, &error)),
        Err(PlatformError::AlreadyExists { .. }) => Err(ExportError::Exists {
            path: given.to_path_buf(),
        }),
        Err(error) => Err(write_error(given, &error)),
    }
}

fn write_error(given: &Path, error: &PlatformError) -> ExportError {
    ExportError::Write {
        reason: WriteReason::of(error),
        path: given.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destination_is_absolute_without_dot_or_empty_components() {
        assert!(check_destination("/Users/me/Downloads/b.zip").is_ok());
        assert_eq!(
            check_destination("Downloads/b.zip"),
            Err(DestinationProblem::NotAbsolute)
        );
        assert_eq!(check_destination(""), Err(DestinationProblem::NotAbsolute));
        for path in [
            "/a/./b.zip",
            "/a/../b.zip",
            "/a//b.zip",
            "/a/b.zip/",
            "/",
            "/a/..",
            "/a/.",
            "/a\0/b",
        ] {
            assert_eq!(
                check_destination(path),
                Err(DestinationProblem::InvalidName),
                "{path:?}"
            );
        }
        let long_name = format!("/a/{}", "n".repeat(201));
        assert_eq!(
            check_destination(&long_name),
            Err(DestinationProblem::InvalidName)
        );
        let at_limit = format!("/a/{}", "n".repeat(200));
        assert!(check_destination(&at_limit).is_ok());
        let long_path = format!("/{}/b.zip", "d".repeat(1020));
        assert_eq!(
            check_destination(&long_path),
            Err(DestinationProblem::InvalidName)
        );
    }

    #[test]
    fn problems_have_their_wire_codes() {
        let codes: Vec<_> = [
            DestinationProblem::NotAbsolute,
            DestinationProblem::InvalidName,
            DestinationProblem::ParentMissing,
            DestinationProblem::ParentNotDirectory,
            DestinationProblem::IsDirectory,
            DestinationProblem::NotRegular,
        ]
        .iter()
        .map(|problem| problem.as_code())
        .collect();
        assert_eq!(
            codes,
            [
                "not_absolute",
                "invalid_name",
                "parent_missing",
                "parent_not_directory",
                "is_directory",
                "not_regular"
            ]
        );
    }
}
