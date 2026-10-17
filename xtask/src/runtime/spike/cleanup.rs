//! The cleanup routine (spec 0003, Rutin pembersihan), also
//! `cargo xtask runtime spike --cleanup`, and `--prune-cache`.
//!
//! It kills leftover provider processes and workers, removes the agent,
//! and deletes spike roots and socket folders this user owns. It never
//! touches the download cache; `--prune-cache` is the only thing that
//! deletes cache files.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::launchd;
use super::process::send;
use super::{OWNER_MARKER, ROOT_PREFIX, SOCKET_PARENT, SOCKET_PREFIX};

const PGREP: &str = "/usr/bin/pgrep";
/// Time between TERM and KILL for leftover processes.
const TERM_GRACE: Duration = Duration::from_secs(5);

/// What one sweep did, for the log line.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    /// Processes that got TERM.
    pub processes: usize,
    /// Whether an agent was removed.
    pub agent: bool,
    /// Folders deleted.
    pub folders: usize,
}

/// Cleans up everything any earlier spike of this user left behind. The
/// caller holds the spike lock, so nothing found here belongs to a
/// running spike.
pub fn sweep(uid: u32) -> Result<Swept> {
    let temp = std::env::temp_dir();
    let mut swept = Swept {
        processes: kill_matching(uid, &leftover_pattern(&temp))?,
        ..Swept::default()
    };
    if launchd::has_gui_session(uid)? {
        let domain = launchd::gui_domain(uid);
        swept.agent = launchd::job_state(&domain)? != launchd::JobState::Missing;
        launchd::bootout(&domain);
    }
    for root in owned_roots(&temp, uid)? {
        remove_dir(&root)?;
        swept.folders += 1;
    }
    for socket_dir in owned_socket_dirs(Path::new(SOCKET_PARENT), uid)? {
        remove_dir(&socket_dir)?;
        swept.folders += 1;
    }
    Ok(swept)
}

/// The extended regex for command lines of spike processes: a path under
/// this user's spike roots (as given or through `/private`), or a socket
/// folder `/tmp/sk-<8 hex>`. A process that only mentions the words, such
/// as the shell that started the driver, never matches.
fn leftover_pattern(temp: &Path) -> String {
    let temp = temp.to_string_lossy();
    let temp = temp.trim_end_matches('/');
    let temp = temp.strip_prefix("/private").unwrap_or(temp);
    format!(
        "(/private)?{}/{}|{}[0-9a-f]{{8}}",
        regex_escape(temp),
        regex_escape(ROOT_PREFIX),
        regex_escape(SOCKET_PREFIX)
    )
}

/// Escapes the characters an extended regex treats specially.
fn regex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.[](){}*+?|^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// True for a socket folder name `sk-<8 lowercase hex>`.
fn is_socket_name(name: &str) -> bool {
    name.strip_prefix("sk-").is_some_and(|hex| {
        hex.len() == 8 && hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
    })
}

/// TERM, a wait of up to five seconds, then KILL for every process of
/// `uid` whose command line matches the extended regex `pattern`. Returns
/// how many there were. The calling process is never one of them.
fn kill_matching(uid: u32, pattern: &str) -> Result<usize> {
    let pids = matching_pids(uid, pattern)?;
    for pid in &pids {
        // Ignored: the process may have ended on its own.
        let _ = send("TERM", &pid.to_string());
    }
    let deadline = Instant::now() + TERM_GRACE;
    let mut alive: Vec<u32> = pids.clone();
    while !alive.is_empty() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(200));
        alive.retain(|pid| send("0", &pid.to_string()).is_ok());
    }
    for pid in &alive {
        // Ignored: same as above.
        let _ = send("KILL", &pid.to_string());
    }
    Ok(pids.len())
}

fn matching_pids(uid: u32, pattern: &str) -> Result<Vec<u32>> {
    let output = Command::new(PGREP)
        .args(["-u", &uid.to_string(), "-f", pattern])
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("cannot run {PGREP}"))?;
    // pgrep exits 1 when nothing matches.
    let own = std::process::id();
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .filter(|pid| *pid != own)
        .collect())
}

/// Spike roots in `temp` that carry this user's owner marker.
pub fn owned_roots(temp: &Path, uid: u32) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    for path in entries_named(temp, ROOT_PREFIX)? {
        let marker = path.join(OWNER_MARKER);
        let marked = fs::read_to_string(&marker).is_ok_and(|text| text.trim() == uid.to_string());
        if marked && owned_dir(&path, uid) {
            roots.push(path);
        }
    }
    Ok(roots)
}

/// `/tmp/sk-<8 hex>` folders that belong to this user.
pub fn owned_socket_dirs(parent: &Path, uid: u32) -> Result<Vec<PathBuf>> {
    Ok(entries_named(parent, "sk-")?
        .into_iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| is_socket_name(&name.to_string_lossy()))
        })
        .filter(|path| owned_dir(path, uid))
        .collect())
}

fn owned_dir(path: &Path, uid: u32) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir() && meta.uid() == uid)
}

fn entries_named(dir: &Path, prefix: &str) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", dir.display())),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}

fn remove_dir(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("cannot delete {}", path.display())),
    }
}

/// Deletes cache files whose digest the inventory no longer pins. Only
/// names of 64 lowercase hex characters count, so `spike.lock` and
/// unfinished downloads stay. Returns the deleted names.
pub fn prune_cache(cache_dir: &Path, pinned: &HashSet<String>) -> Result<Vec<String>> {
    let mut deleted = Vec::new();
    let entries =
        fs::read_dir(cache_dir).with_context(|| format!("cannot read {}", cache_dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {}", cache_dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !stackly_evidence::is_sha256_hex(&name) || pinned.contains(&name) {
            continue;
        }
        let path = entry.path();
        let result = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.with_context(|| format!("cannot delete {}", path.display()))?;
        deleted.push(name);
    }
    deleted.sort();
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid() -> u32 {
        fs::metadata(std::env::temp_dir()).unwrap().uid()
    }

    #[test]
    fn only_marked_roots_of_this_user_are_found() {
        let temp = tempfile::tempdir().unwrap();
        let uid = uid();
        let marked = temp.path().join(format!("{ROOT_PREFIX}a"));
        fs::create_dir(&marked).unwrap();
        fs::write(marked.join(OWNER_MARKER), format!("{uid}\n")).unwrap();
        let unmarked = temp.path().join(format!("{ROOT_PREFIX}b"));
        fs::create_dir(&unmarked).unwrap();
        let other_user = temp.path().join(format!("{ROOT_PREFIX}c"));
        fs::create_dir(&other_user).unwrap();
        fs::write(other_user.join(OWNER_MARKER), format!("{}\n", uid + 1)).unwrap();
        fs::create_dir(temp.path().join("unrelated")).unwrap();
        assert_eq!(owned_roots(temp.path(), uid).unwrap(), [marked]);
    }

    #[test]
    fn socket_folders_match_by_prefix_and_owner() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("sk-0123abcd")).unwrap();
        fs::write(temp.path().join("sk-file"), "").unwrap();
        fs::create_dir(temp.path().join("other")).unwrap();
        assert_eq!(
            owned_socket_dirs(temp.path(), uid()).unwrap(),
            [temp.path().join("sk-0123abcd")]
        );
        assert!(
            owned_socket_dirs(temp.path(), uid() + 1)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_process_pattern_names_full_spike_paths_only() {
        assert_eq!(
            leftover_pattern(Path::new("/var/folders/x+y/T/")),
            "(/private)?/var/folders/x\\+y/T/stackly-runtime-spike-|/tmp/sk-[0-9a-f]{8}"
        );
        assert_eq!(
            leftover_pattern(Path::new("/private/tmp")),
            "(/private)?/tmp/stackly-runtime-spike-|/tmp/sk-[0-9a-f]{8}"
        );
        assert!(is_socket_name("sk-0123abcd"));
        assert!(
            !is_socket_name("sk-0123ABCD")
                && !is_socket_name("sk-123")
                && !is_socket_name("sk-file")
        );
    }

    #[test]
    fn prune_keeps_pinned_digests_the_lock_and_partial_downloads() {
        let cache = tempfile::tempdir().unwrap();
        let pinned = "a".repeat(64);
        let stale = "b".repeat(64);
        for name in [pinned.as_str(), stale.as_str(), "spike.lock", ".download-x"] {
            fs::write(cache.path().join(name), "x").unwrap();
        }
        let deleted = prune_cache(cache.path(), &HashSet::from([pinned.clone()])).unwrap();
        assert_eq!(deleted, [stale]);
        let mut left: Vec<String> = fs::read_dir(cache.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [".download-x".to_owned(), pinned, "spike.lock".to_owned()]
        );
    }
}
