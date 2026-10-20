//! Member name normalization, the same for every format (spec 0018,
//! *Normalisasi nama anggota*).

use stackly_platform::{EntryPath, EntryPathError};

use super::RejectReason;
use super::limits::ENTRY_TEXT_MAX_CHARS;

/// The file name reserved for the runtime marker at the root of the tree.
pub(super) const MARKER_NAME: &str = ".stackly-runtime.json";

/// A member name after normalization, before `archiveRoot` is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Normalized {
    /// The root of the archive itself (`./` or `.` for a folder).
    Root,
    /// A relative path of plain components joined by `/`.
    Path(String),
}

/// Normalizes a raw member name: UTF-8 with no NUL or backslash, one
/// leading `./` dropped, and one trailing `/` dropped for a folder only;
/// then no leading `/` and no empty, `.`, or `..` component. Length limits
/// apply later, to the path below `archiveRoot`.
pub(super) fn normalize(raw: &[u8], is_dir: bool) -> Result<Normalized, RejectReason> {
    let text = std::str::from_utf8(raw).map_err(|_| RejectReason::NameInvalid)?;
    if text.contains(['\0', '\\']) {
        return Err(RejectReason::NameInvalid);
    }
    if text.is_empty() {
        return Err(RejectReason::NameInvalid);
    }
    let stripped = text.strip_prefix("./").unwrap_or(text);
    if stripped.starts_with('/') {
        return Err(RejectReason::AbsolutePath);
    }
    let name = if is_dir {
        let name = stripped.strip_suffix('/').unwrap_or(stripped);
        if name.is_empty() || name == "." {
            return Ok(Normalized::Root);
        }
        name
    } else if stripped.ends_with('/') {
        return Err(RejectReason::NameInvalid);
    } else {
        stripped
    };
    if name.split('/').any(|component| component == "..") {
        return Err(RejectReason::PathTraversal);
    }
    if name
        .split('/')
        .any(|component| component.is_empty() || component == ".")
    {
        return Err(RejectReason::NameInvalid);
    }
    Ok(Normalized::Path(name.to_owned()))
}

/// True for an AppleDouble member: its first component is `__MACOSX` or
/// its last component starts with `._`.
pub(super) fn is_apple_double(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or_default();
    let last = path.rsplit('/').next().unwrap_or_default();
    first == "__MACOSX" || last.starts_with("._")
}

/// Checks the length and depth limits of a path below `archiveRoot`.
pub(super) fn entry_path(path: &str) -> Result<EntryPath, RejectReason> {
    EntryPath::new(path).map_err(|err| match err {
        EntryPathError::Absolute => RejectReason::AbsolutePath,
        EntryPathError::Traversal => RejectReason::PathTraversal,
        EntryPathError::NameTooLong | EntryPathError::TooLong | EntryPathError::TooDeep => {
            RejectReason::NameTooLong
        }
        _ => RejectReason::NameInvalid,
    })
}

/// The member name an error carries: lossy UTF-8, cut to
/// [`ENTRY_TEXT_MAX_CHARS`] characters.
pub(super) fn entry_text(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .chars()
        .take(ENTRY_TEXT_MAX_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: &str) -> Normalized {
        Normalized::Path(text.to_owned())
    }

    #[test]
    fn one_leading_dot_slash_and_a_folder_slash_are_dropped() {
        assert_eq!(normalize(b"./bin/tool", false), Ok(path("bin/tool")));
        assert_eq!(normalize(b"bin/", true), Ok(path("bin")));
        assert_eq!(normalize(b"./bin/", true), Ok(path("bin")));
        assert_eq!(normalize(b"./", true), Ok(Normalized::Root));
        assert_eq!(normalize(b".", true), Ok(Normalized::Root));
    }

    #[test]
    fn broken_names_get_their_reason() {
        let cases: [(&[u8], bool, RejectReason); 12] = [
            (b"/etc/passwd", false, RejectReason::AbsolutePath),
            (b"/", true, RejectReason::AbsolutePath),
            (b".//etc", false, RejectReason::AbsolutePath),
            (b"a/../../b", false, RejectReason::PathTraversal),
            (b"..", true, RejectReason::PathTraversal),
            (b"a//b", false, RejectReason::NameInvalid),
            (b"././a", false, RejectReason::NameInvalid),
            (b"a/", false, RejectReason::NameInvalid),
            (b"", false, RejectReason::NameInvalid),
            (b"", true, RejectReason::NameInvalid),
            (b"a\\b", false, RejectReason::NameInvalid),
            (b"a\xffb", false, RejectReason::NameInvalid),
        ];
        for (raw, is_dir, reason) in cases {
            assert_eq!(normalize(raw, is_dir), Err(reason), "{raw:?}");
        }
        assert_eq!(normalize(b"a\0b", false), Err(RejectReason::NameInvalid));
    }

    #[test]
    fn apple_double_members_are_recognized_in_both_forms() {
        assert!(is_apple_double("__MACOSX/bin/._tool"));
        assert!(is_apple_double("bin/._tool"));
        assert!(is_apple_double("._bin"));
        assert!(!is_apple_double("bin/tool._x"));
        assert!(!is_apple_double("lib/__MACOSX"));
    }

    #[test]
    fn long_and_deep_paths_are_name_too_long() {
        assert_eq!(entry_path(&"n".repeat(256)), Err(RejectReason::NameTooLong));
        assert_eq!(
            entry_path(&vec!["d"; 33].join("/")),
            Err(RejectReason::NameTooLong)
        );
        assert!(entry_path("pgsql/bin/postgres").is_ok());
    }

    #[test]
    fn entry_text_is_lossy_and_bounded() {
        assert_eq!(entry_text(b"a\xffb"), "a\u{fffd}b");
        assert_eq!(
            entry_text("x".repeat(400).as_bytes()).chars().count(),
            ENTRY_TEXT_MAX_CHARS
        );
    }
}
