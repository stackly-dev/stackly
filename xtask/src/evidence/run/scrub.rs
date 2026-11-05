//! Host path scrubbing for `output.log` (spec 0002, "Folder dan file",
//! audit note 91): the workspace root becomes `$REPO`, the temporary folder
//! `$TMPDIR`, and the home folder `$HOME`, so a log that goes into git keeps
//! no user name, worktree folder, or per user temporary folder id.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// The placeholder for the workspace root.
pub const REPO: &str = "$REPO";
/// The placeholder for the temporary folder (`TMPDIR`).
pub const TMPDIR: &str = "$TMPDIR";
/// The placeholder for the home folder (`HOME`).
pub const HOME: &str = "$HOME";

/// Host path prefixes and the placeholder each one becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathScrub {
    /// `(prefix, placeholder)`, longest prefix first, so the workspace root
    /// under the home folder becomes `$REPO` rather than `$HOME/...`.
    pairs: Vec<(String, &'static str)>,
}

impl PathScrub {
    /// The scrub for this host: `workspace_root`, `TMPDIR`, and `HOME`, each
    /// as given and as resolved (macOS reaches `/var/folders` through
    /// `/private/var/folders`).
    #[must_use]
    pub fn from_env(workspace_root: &Path) -> Self {
        let tmpdir = std::env::var_os("TMPDIR").map(std::path::PathBuf::from);
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        Self::new(workspace_root, tmpdir.as_deref(), home.as_deref())
    }

    /// The scrub for the given folders; a missing or root folder is skipped.
    #[must_use]
    pub fn new(workspace_root: &Path, tmpdir: Option<&Path>, home: Option<&Path>) -> Self {
        let mut pairs = Vec::new();
        for (folder, placeholder) in [(Some(workspace_root), REPO), (tmpdir, TMPDIR), (home, HOME)]
        {
            let Some(folder) = folder else { continue };
            let mut forms = vec![folder.to_path_buf()];
            if let Ok(real) = folder.canonicalize() {
                forms.push(real);
            }
            for form in forms {
                let text = form.to_string_lossy();
                let text = text.trim_end_matches('/');
                if text.len() > 1 && !pairs.iter().any(|(known, _)| known == text) {
                    pairs.push((text.to_owned(), placeholder));
                }
            }
        }
        pairs.sort_by_key(|pair| std::cmp::Reverse(pair.0.len()));
        Self { pairs }
    }

    /// `bytes` with every prefix replaced by its placeholder. A prefix
    /// matches only as a whole path: `/Users/dev` never matches inside
    /// `/Users/developer`. Bytes that are not UTF-8 stay as they are.
    #[must_use]
    pub fn apply(&self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        let mut at = 0;
        'outer: while at < bytes.len() {
            for (prefix, placeholder) in &self.pairs {
                let prefix = prefix.as_bytes();
                if bytes[at..].starts_with(prefix)
                    && bytes
                        .get(at + prefix.len())
                        .is_none_or(|next| !is_path_byte(*next))
                {
                    out.extend_from_slice(placeholder.as_bytes());
                    at += prefix.len();
                    continue 'outer;
                }
            }
            out.push(bytes[at]);
            at += 1;
        }
        out
    }

    /// Rewrites `path` in place with [`Self::apply`], through a temporary
    /// file and a rename, so a cut write never leaves half a log.
    ///
    /// # Errors
    ///
    /// When the file cannot be read, written, or renamed.
    pub fn apply_to_file(&self, path: &Path) -> Result<()> {
        let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        let scrubbed = self.apply(&bytes);
        if scrubbed == bytes {
            return Ok(());
        }
        let staging = path.with_extension("log.tmp");
        fs::write(&staging, scrubbed)
            .with_context(|| format!("cannot write {}", staging.display()))?;
        fs::rename(&staging, path).with_context(|| format!("cannot replace {}", path.display()))?;
        Ok(())
    }
}

/// A byte that would continue a path name, so a prefix followed by it is
/// only part of a longer name.
fn is_path_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') || byte >= 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrub() -> PathScrub {
        PathScrub::new(
            Path::new("/Users/dev/Development/Stackly"),
            Some(Path::new("/var/folders/sx/abc123/T/")),
            Some(Path::new("/Users/dev")),
        )
    }

    #[test]
    fn output_log_replaces_repo_tmpdir_and_home_with_placeholders() {
        let log = "Compiling xtask (/Users/dev/Development/Stackly/xtask)\n\
                   root /var/folders/sx/abc123/T/.tmpX1/data\n\
                   rustc at /Users/dev/.rustup and /Users/dev\n\
                   worktree /Users/dev/Worktrees/Stackly/arc-1/apps/cli\n";
        assert_eq!(
            String::from_utf8(scrub().apply(log.as_bytes())).unwrap(),
            "Compiling xtask ($REPO/xtask)\n\
             root $TMPDIR/.tmpX1/data\n\
             rustc at $HOME/.rustup and $HOME\n\
             worktree $HOME/Worktrees/Stackly/arc-1/apps/cli\n"
        );
    }

    #[test]
    fn a_prefix_inside_a_longer_name_is_left_alone() {
        let log = "/Users/developer/x /Users/dev-2/y /Users/dev.bak /Users/dev, ok";
        assert_eq!(
            String::from_utf8(scrub().apply(log.as_bytes())).unwrap(),
            "/Users/developer/x /Users/dev-2/y /Users/dev.bak $HOME, ok"
        );
    }

    #[test]
    fn bytes_that_are_not_utf8_survive_the_scrub() {
        let mut log = b"\xff /Users/dev/a \xfe".to_vec();
        log = scrub().apply(&log);
        assert_eq!(log, b"\xff $HOME/a \xfe");
    }

    #[test]
    fn the_resolved_tmpdir_is_scrubbed_as_well_as_the_given_one() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().canonicalize().unwrap();
        let given = dir.path().to_path_buf();
        let scrub = PathScrub::new(Path::new("/nowhere/repo"), Some(&given), None);
        let log = format!("{}/a {}/b", real.display(), given.display());
        assert_eq!(
            String::from_utf8(scrub.apply(log.as_bytes())).unwrap(),
            "$TMPDIR/a $TMPDIR/b"
        );
    }

    #[test]
    fn a_file_is_rewritten_in_place_and_leaves_no_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("output.log");
        fs::write(&log, "at /Users/dev/Development/Stackly/crates\n").unwrap();
        scrub().apply_to_file(&log).unwrap();
        assert_eq!(fs::read_to_string(&log).unwrap(), "at $REPO/crates\n");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
