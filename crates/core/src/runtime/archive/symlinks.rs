//! The symlink rules (spec 0018, *Aturan symlink*): a target is resolved
//! lexically from the folder that holds the link, in the coordinates of
//! the tree, against the entries the archive created. Every component on
//! the way must be a folder the archive created, so the lexical answer is
//! the one the OS gives.

use super::RejectReason;
use super::limits::SYMLINK_CHAIN_MAX;

/// What a path names in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Found<'t> {
    /// A folder the archive created or implied.
    Dir,
    /// A regular file.
    File,
    /// A symlink with this target.
    Symlink(&'t str),
}

/// Checks the symlink at `link` with `target`: it stays inside the tree,
/// every component on the way is a folder, it ends at an entry, and a
/// chain of symlinks it starts holds at most [`SYMLINK_CHAIN_MAX`] links
/// and no cycle.
pub(super) fn check<'t>(
    find: &impl Fn(&str) -> Option<Found<'t>>,
    link: &str,
    target: &'t str,
) -> Result<(), RejectReason> {
    let mut visited = vec![link.to_owned()];
    let mut current = (link.to_owned(), target);
    for _ in 0..SYMLINK_CHAIN_MAX {
        let (link, target) = (&current.0, current.1);
        match resolve_one(find, link, target)? {
            Step::Done => return Ok(()),
            Step::Symlink { path, target } => {
                if visited.contains(&path) {
                    return Err(RejectReason::SymlinkChain);
                }
                visited.push(path.clone());
                current = (path, target);
            }
        }
    }
    Err(RejectReason::SymlinkChain)
}

/// Where one link of a chain ends.
enum Step<'t> {
    /// At a folder or file.
    Done,
    /// At another symlink, at `path`.
    Symlink { path: String, target: &'t str },
}

fn resolve_one<'t>(
    find: &impl Fn(&str) -> Option<Found<'t>>,
    link: &str,
    target: &str,
) -> Result<Step<'t>, RejectReason> {
    if target.starts_with('/') {
        return Err(RejectReason::SymlinkEscape);
    }
    let mut parts: Vec<&str> = link.split('/').collect();
    parts.pop();
    let components: Vec<&str> = target
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect();
    for (index, component) in components.iter().enumerate() {
        if *component == ".." {
            if parts.pop().is_none() {
                return Err(RejectReason::SymlinkEscape);
            }
            continue;
        }
        parts.push(component);
        let last = index + 1 == components.len();
        let path = parts.join("/");
        match find(&path) {
            Some(Found::Dir) => {}
            Some(Found::File) if last => return Ok(Step::Done),
            Some(Found::Symlink(next)) if last => {
                return Ok(Step::Symlink { path, target: next });
            }
            Some(Found::Symlink(_)) => return Err(RejectReason::SymlinkChain),
            Some(Found::File) | None => return Err(RejectReason::SymlinkDangling),
        }
    }
    // The target names the folder holding the link, or one above it.
    Ok(Step::Done)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree: `bin/`, `lib/`, `lib/libz.1.dylib` (file), `lib/libz.dylib`
    /// -> `libz.1.dylib`, `lib/a` -> `b`, `lib/b` -> `a`, `share` ->
    /// `lib`, and a chain `c0` -> `c1` -> ... -> `c9` -> `bin`.
    fn find(path: &str) -> Option<Found<'static>> {
        const CHAIN: [&str; 9] = ["c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9"];
        match path {
            "bin" | "lib" => Some(Found::Dir),
            "lib/libz.1.dylib" | "bin/tool" => Some(Found::File),
            "lib/libz.dylib" => Some(Found::Symlink("libz.1.dylib")),
            "lib/a" => Some(Found::Symlink("b")),
            "lib/b" => Some(Found::Symlink("a")),
            "share" => Some(Found::Symlink("lib")),
            "c9" => Some(Found::Symlink("bin")),
            other => other
                .strip_prefix('c')
                .and_then(|digit| digit.parse::<usize>().ok())
                .filter(|digit| *digit < 9)
                .map(|digit| Found::Symlink(CHAIN[digit])),
        }
    }

    #[test]
    fn internal_symlinks_resolve_like_the_mysql_layout() {
        assert_eq!(
            check(&find, "bin/libz.dylib", "../lib/libz.1.dylib"),
            Ok(())
        );
        assert_eq!(check(&find, "bin/libz", "../lib/libz.dylib"), Ok(()));
        assert_eq!(check(&find, "lib/up", ".."), Ok(()));
        assert_eq!(check(&find, "lib/here", "./"), Ok(()));
        assert_eq!(check(&find, "top", "lib/./libz.1.dylib"), Ok(()));
    }

    #[test]
    fn escaping_targets_are_refused() {
        assert_eq!(
            check(&find, "bin/x", "/usr/lib/libz.dylib"),
            Err(RejectReason::SymlinkEscape)
        );
        assert_eq!(
            check(&find, "bin/x", "../../etc/passwd"),
            Err(RejectReason::SymlinkEscape)
        );
        assert_eq!(check(&find, "x", ".."), Err(RejectReason::SymlinkEscape));
    }

    #[test]
    fn missing_ends_and_files_on_the_way_are_dangling() {
        assert_eq!(
            check(&find, "bin/x", "../lib/missing"),
            Err(RejectReason::SymlinkDangling)
        );
        assert_eq!(
            check(&find, "bin/x", "tool/inner"),
            Err(RejectReason::SymlinkDangling)
        );
        assert_eq!(
            check(&find, "bin/x", "tool/../tool"),
            Err(RejectReason::SymlinkDangling)
        );
    }

    #[test]
    fn symlinks_on_the_way_cycles_and_long_chains_are_refused() {
        assert_eq!(
            check(&find, "bin/x", "../share/libz.1.dylib"),
            Err(RejectReason::SymlinkChain)
        );
        assert_eq!(check(&find, "lib/a", "b"), Err(RejectReason::SymlinkChain));
        assert_eq!(check(&find, "x", "c0"), Err(RejectReason::SymlinkChain));
        assert_eq!(check(&find, "x", "c3"), Ok(()));
    }
}
