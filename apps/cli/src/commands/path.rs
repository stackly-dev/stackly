//! Paths the command line names: made absolute from the current folder
//! and normalized as text, so a path such as `../x/stackly.yaml` passes
//! the daemon's `import_path_code` (specs 0036, 0037, 0044). The daemon
//! reads the file or folder; the CLI never does.

use std::path::{Component, Path, PathBuf};

use crate::failure::CliFailure;

/// `path` joined to the current folder when relative, then normalized
/// without touching the filesystem: `.` is dropped and `..` removes the
/// component before it.
///
/// # Errors
///
/// `cli.usage` when the current folder cannot be read.
pub fn absolute(path: &Path) -> Result<PathBuf, CliFailure> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let cwd = std::env::current_dir().map_err(|error| {
            CliFailure::usage(
                "invalid_value",
                &format!("cannot read the current folder: {error}"),
            )
        })?;
        cwd.join(path)
    };
    Ok(normalize(&joined))
}

/// `path` with `.` dropped and each `..` folded into the component before
/// it, as text.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path` with the home folder written as `~`, for text people read. The
/// JSON result keeps the absolute path (spec 0044, *API surface*).
#[must_use]
pub fn home_masked(path: &str, home: Option<&Path>) -> String {
    let Some(home) = home.and_then(Path::to_str).filter(|home| !home.is_empty()) else {
        return path.to_owned();
    };
    let home = home.trim_end_matches('/');
    if path == home {
        return "~".to_owned();
    }
    match path.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizing_drops_dots_and_folds_parents_without_the_filesystem() {
        assert_eq!(
            normalize(Path::new("/a/./b/../c/providers/./mytool")),
            Path::new("/a/c/providers/mytool")
        );
        assert_eq!(normalize(Path::new("/../x")), Path::new("/x"));
        assert_eq!(
            absolute(Path::new("/srv/p/../q")).unwrap(),
            Path::new("/srv/q")
        );
        assert!(absolute(Path::new("rel")).unwrap().is_absolute());
        assert_eq!(
            absolute(Path::new("/w/./a/../stackly.yaml")).unwrap(),
            Path::new("/w/stackly.yaml")
        );
    }

    #[test]
    fn the_home_folder_is_written_as_a_tilde_only_at_a_component_boundary() {
        let home = Some(Path::new("/Users/dev"));
        assert_eq!(
            home_masked("/Users/dev/erp/stackly.yaml", home),
            "~/erp/stackly.yaml"
        );
        assert_eq!(home_masked("/Users/dev", home), "~");
        assert_eq!(home_masked("/Users/devops/x", home), "/Users/devops/x");
        assert_eq!(home_masked("/srv/x", home), "/srv/x");
        assert_eq!(home_masked("/srv/x", None), "/srv/x");
        assert_eq!(
            home_masked("/Users/dev/x", Some(Path::new("/Users/dev/"))),
            "~/x"
        );
    }
}
