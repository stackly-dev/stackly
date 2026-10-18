//! Checking the cached download again, extracting it, and reporting what
//! the archive holds (spec 0003, AC-3, AC-16, and the archive report).

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::plan::PlanArtifact;
use super::result::ArchiveReport;
use crate::runtime::cache::sha256_file;
use crate::runtime::inventory::is_inner_relative_path;
use crate::runtime::macho::{check_format, file_kind};

const TAR: &str = "/usr/bin/tar";

/// Hashes the cached file again and refuses it unless digest and size
/// match the plan. The reasons are the ones AC-3 names.
pub fn check_digest(path: &Path, sha256: &str, size: u64) -> Result<()> {
    let (actual, actual_size) = sha256_file(path)?;
    if actual_size != size {
        bail!("size mismatch: the cached file has {actual_size} bytes, the inventory says {size}");
    }
    if actual != sha256 {
        bail!("digest mismatch: the cached file hashes to {actual}, the inventory says {sha256}");
    }
    Ok(())
}

/// What [`extract`] made.
#[derive(Debug)]
pub struct Extracted {
    /// The artifact root.
    pub root: PathBuf,
    /// One `set mode 0755 on <path> (was <mode>)` per executable of a `zip`
    /// whose mode changed, in `executables` order.
    pub mode_notes: Vec<String>,
}

/// Extracts the artifact into `into` (a new folder) and returns the
/// artifact root: `into/<archive_root>` when the entry names one, which
/// must then be the only thing at the top of the archive, else `into`
/// itself. A `binary` download is copied in under its executable name
/// with mode 0755, and every executable of a `zip` gets mode 0755 too. The
/// magic number of the cached file must match `format` before anything is
/// extracted (AC-16).
pub fn extract(artifact: &PlanArtifact, cache_file: &Path, into: &Path) -> Result<Extracted> {
    check_format(&artifact.format, file_kind(cache_file)?)?;
    fs::create_dir_all(into).with_context(|| format!("cannot create {}", into.display()))?;
    match artifact.format.as_str() {
        "tar_gz" => Ok(Extracted {
            root: untar(
                cache_file,
                "-xzf",
                &[],
                into,
                artifact.archive_root.as_deref(),
            )?,
            mode_notes: Vec::new(),
        }),
        // Without `--no-mac-metadata`, bsdtar turns `__MACOSX/._<name>`
        // entries into extended attributes (or fails on them).
        "zip" => {
            let root = untar(
                cache_file,
                "-xf",
                &["--no-mac-metadata"],
                into,
                artifact.archive_root.as_deref(),
            )?;
            let mode_notes = make_executable(&root, &artifact.executables)?;
            Ok(Extracted { root, mode_notes })
        }
        "binary" => {
            let target = into.join(artifact.main_executable());
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("cannot create {}", parent.display()))?;
            }
            fs::copy(cache_file, &target)
                .with_context(|| format!("cannot copy the binary to {}", target.display()))?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                .with_context(|| format!("cannot make {} executable", target.display()))?;
            Ok(Extracted {
                root: into.to_path_buf(),
                mode_notes: Vec::new(),
            })
        }
        other => bail!("unknown format `{other}`"),
    }
}

/// Gives every listed executable of a `zip` mode 0755, since a zip may store
/// it without an execute bit (ClickHouse stores 0664). Returns one note per
/// file whose mode changed; a file already at 0755 is left alone.
fn make_executable(root: &Path, executables: &[String]) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    for listed in executables {
        let path = regular_file_below(root, listed)?;
        let mode = fs::symlink_metadata(&path)
            .with_context(|| format!("cannot read {}", path.display()))?
            .permissions()
            .mode()
            & 0o7777;
        if mode != 0o755 {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .with_context(|| format!("cannot make {} executable", path.display()))?;
            notes.push(format!("set mode 0755 on {listed} (was {mode:04o})"));
        }
    }
    Ok(notes)
}

/// `root/<listed>`, once every part of `listed` below the root exists, no
/// part is a link (`chmod` would follow it), and the last part is a regular
/// file. The root and the folders above it are not checked: the macOS temp
/// folder itself sits behind the `/var` link.
fn regular_file_below(root: &Path, listed: &str) -> Result<PathBuf> {
    if !is_inner_relative_path(listed) {
        bail!(
            "executable `{listed}` must be a plain relative path, without `..`, `.`, or empty parts"
        );
    }
    let parts: Vec<&str> = listed.split('/').collect();
    let mut path = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                bail!("the artifact has no executable {listed}");
            }
            Err(err) => {
                return Err(err).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        let last = index + 1 == parts.len();
        if meta.file_type().is_symlink() || (last && !meta.is_file()) {
            bail!("executable {listed} is not a regular file");
        }
        if !last && !meta.is_dir() {
            bail!("the artifact has no executable {listed}");
        }
    }
    Ok(path)
}

/// Extracts a `.tar.gz` or a `.zip` with the system tar, never with `-P`,
/// so a `..` member or one written through a link makes tar fail, and
/// returns the artifact root. File modes are the archive's under the
/// process umask, since tar runs without `-p`.
fn untar(
    cache_file: &Path,
    flag: &str,
    options: &[&str],
    into: &Path,
    archive_root: Option<&str>,
) -> Result<PathBuf> {
    let output = Command::new(TAR)
        .current_dir(into)
        .arg(flag)
        .arg(cache_file)
        .args(options)
        .arg("-C")
        .arg(into)
        .output()
        .with_context(|| format!("cannot run {TAR}"))?;
    if !output.status.success() {
        bail!(
            "tar cannot extract the archive: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    match archive_root {
        None => Ok(into.to_path_buf()),
        Some(top) => {
            let names = top_names(into)?;
            if names != [top] {
                bail!(
                    "the archive top must be exactly `{top}`, found {}",
                    names.join(", ")
                );
            }
            let root = into.join(top);
            if !fs::symlink_metadata(&root).is_ok_and(|meta| meta.is_dir()) {
                bail!("the archive top `{top}` is not a folder");
            }
            Ok(root)
        }
    }
}

fn top_names(dir: &Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()
        .with_context(|| format!("cannot read {}", dir.display()))?;
    names.sort();
    Ok(names)
}

/// One entry of a walk: the path relative to the root and its metadata,
/// never following links.
pub struct Entry {
    /// Relative to the walked root, `/` separated.
    pub relative: String,
    /// The absolute path.
    pub path: PathBuf,
    /// `symlink_metadata` of the entry.
    pub meta: fs::Metadata,
}

/// Every entry below `root`, folders included, sorted by path.
pub fn walk(root: &Path) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
            let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            if meta.is_dir() {
                pending.push(path.clone());
            }
            let relative = path
                .strip_prefix(root)
                .context("a walked path is inside its root")?
                .to_string_lossy()
                .into_owned();
            out.push(Entry {
                relative,
                path,
                meta,
            });
        }
    }
    out.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(out)
}

/// Counts files and links, and lists escaping links, setuid or setgid
/// files, and special files.
pub fn report(root: &Path) -> Result<ArchiveReport> {
    let mut report = ArchiveReport::default();
    for entry in walk(root)? {
        let kind = entry.meta.file_type();
        if kind.is_symlink() {
            let target = fs::read_link(&entry.path)
                .with_context(|| format!("cannot read the link {}", entry.path.display()))?;
            let parent = Path::new(&entry.relative).parent().unwrap_or(Path::new(""));
            if stays_inside(parent, &target) {
                report.symlinks_internal += 1;
            } else {
                report.symlinks_escaping.push(format!(
                    "{} -> {}",
                    entry.relative,
                    target.display()
                ));
            }
        } else if kind.is_file() {
            report.files += 1;
            if entry.meta.permissions().mode() & 0o6000 != 0 {
                report.setuid.push(entry.relative.clone());
            }
        } else if !kind.is_dir() {
            report.special_files.push(entry.relative.clone());
        }
    }
    Ok(report)
}

/// True when a link at `parent` (relative to the root) pointing at
/// `target` resolves, by its text alone, to a path inside the root.
fn stays_inside(parent: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    let mut depth: usize = parent
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::test_zip as zip;

    /// The first bytes of a thin arm64 Mach-O executable.
    const MACH_O: [u8; 8] = [0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 0x01];

    fn artifact(format: &str, archive_root: Option<&str>) -> PlanArtifact {
        PlanArtifact {
            id: "demo-1.0.0-macos-aarch64".to_owned(),
            provider: "nats".to_owned(),
            line: "1.0".to_owned(),
            version: "1.0.0".to_owned(),
            origin: "upstream".to_owned(),
            format: format.to_owned(),
            archive_root: archive_root.map(str::to_owned),
            executables: vec!["bin/demo".to_owned()],
            signing_policy: "keep_or_adhoc".to_owned(),
            sha256: "a".repeat(64),
            size: 1,
            urls: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    /// A `.tar.gz` of `files` (relative paths) built with the system tar.
    fn tarball(dir: &Path, files: &[&str]) -> PathBuf {
        let source = dir.join("source");
        for file in files {
            let path = source.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        let tops: Vec<String> = top_names(&source).unwrap();
        let archive = dir.join("a.tar.gz");
        let status = Command::new(TAR)
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&source)
            .args(&tops)
            .status()
            .unwrap();
        assert!(status.success());
        archive
    }

    #[test]
    fn a_changed_byte_or_size_is_refused_before_extraction() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        fs::write(&path, "abc").unwrap();
        let (sha256, _) = sha256_file(&path).unwrap();
        check_digest(&path, &sha256, 3).unwrap();
        fs::write(&path, "abd").unwrap();
        let err = check_digest(&path, &sha256, 3).unwrap_err().to_string();
        assert!(err.starts_with("digest mismatch"), "{err}");
        let err = check_digest(&path, &sha256, 4).unwrap_err().to_string();
        assert!(err.starts_with("size mismatch"), "{err}");
    }

    #[test]
    fn an_archive_root_must_be_the_only_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        let archive = tarball(dir.path(), &["demo-1/bin/demo", "demo-1/LICENSE"]);
        let into = dir.path().join("Stackly Spike Ü/one");
        let root = extract(&artifact("tar_gz", Some("demo-1")), &archive, &into)
            .unwrap()
            .root;
        assert_eq!(root, into.join("demo-1"));
        assert!(root.join("bin/demo").is_file());

        let err = extract(
            &artifact("tar_gz", Some("other")),
            &archive,
            &dir.path().join("two"),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "the archive top must be exactly `other`, found demo-1"
        );
    }

    #[test]
    fn an_archive_without_a_root_extracts_into_its_own_folder() {
        let dir = tempfile::tempdir().unwrap();
        let archive = tarball(dir.path(), &["traefik", "LICENSE.md"]);
        let into = dir.path().join("x");
        assert_eq!(
            extract(&artifact("tar_gz", None), &archive, &into)
                .unwrap()
                .root,
            into
        );
        assert!(into.join("traefik").is_file());
    }

    #[test]
    fn a_binary_is_copied_under_its_executable_name_and_made_executable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("download");
        fs::write(&file, MACH_O).unwrap();
        let into = dir.path().join("x");
        let root = extract(&artifact("binary", None), &file, &into)
            .unwrap()
            .root;
        let mode = fs::metadata(root.join("bin/demo"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn a_file_of_another_format_is_refused_before_tar_runs() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = tarball(dir.path(), &["demo-1/bin/demo"]);
        let zipped = dir.path().join("a.zip");
        fs::write(&zipped, zip::stored(&[zip::file("bin/demo", 0o755, b"x")])).unwrap();
        let text = dir.path().join("text");
        fs::write(&text, "bin").unwrap();
        let cases = [
            ("zip", &tarball, "format zip does not match the file (gzip)"),
            (
                "tar_gz",
                &zipped,
                "format tar_gz does not match the file (zip)",
            ),
            (
                "binary",
                &zipped,
                "format binary does not match the file (zip)",
            ),
            (
                "binary",
                &text,
                "format binary does not match the file (unknown)",
            ),
        ];
        for (format, file, expected) in cases {
            let into = dir.path().join(format!("into-{format}"));
            let err = extract(&artifact(format, None), file, &into).unwrap_err();
            assert_eq!(err.to_string(), expected);
            // Refused before anything, the extraction folder included.
            assert!(!into.exists(), "{format}: {}", into.display());
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
    }

    fn zip_artifact(executables: &[&str]) -> PlanArtifact {
        let mut zipped = artifact("zip", None);
        zipped.executables = executables.iter().map(|e| (*e).to_owned()).collect();
        zipped
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_zip_without_a_root_extracts_into_its_own_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("clickhouse-macos-aarch64.zip");
        fs::write(&file, zip::stored(&[zip::file("clickhouse", 0o755, b"x")])).unwrap();
        let into = dir
            .path()
            .join("Stackly Spike Ü/clickhouse-26.8.1.1-macos-aarch64");

        let extracted = extract(&zip_artifact(&["clickhouse"]), &file, &into).unwrap();
        assert_eq!(extracted.root, into);
        assert_eq!(mode(&into.join("clickhouse")), 0o755);
        // Already 0755, so nothing to note.
        assert!(extracted.mode_notes.is_empty());
        assert_eq!(top_names(&into).unwrap(), ["clickhouse"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_executables_of_a_zip_get_mode_0755_and_other_files_keep_theirs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.zip");
        // Stored 0644, so the result is the same under umask 022 or 002.
        let members = [
            zip::file("clickhouse", 0o644, b"x"),
            zip::dir("bin"),
            zip::file("bin/tool", 0o755, b"x"),
            zip::file("LICENSE", 0o644, b"x"),
        ];
        fs::write(&file, zip::stored(&members)).unwrap();
        let into = dir.path().join("into");

        let extracted = extract(&zip_artifact(&["clickhouse", "bin/tool"]), &file, &into).unwrap();
        assert_eq!(mode(&into.join("clickhouse")), 0o755);
        assert_eq!(mode(&into.join("bin/tool")), 0o755);
        assert_eq!(mode(&into.join("LICENSE")), 0o644);
        assert_eq!(
            extracted.mode_notes,
            ["set mode 0755 on clickhouse (was 0644)"]
        );
    }

    #[test]
    fn an_executable_mode_is_set_exactly_and_noted_in_listed_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("bin")).unwrap();
        for (name, bits) in [("bin/a", 0o600), ("bin/b", 0o4755), ("bin/c", 0o755)] {
            fs::write(root.join(name), "x").unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(bits)).unwrap();
        }
        let listed = ["bin/b", "bin/c", "bin/a", "bin/a"].map(str::to_owned);

        let notes = make_executable(root, &listed).unwrap();
        assert_eq!(
            notes,
            [
                "set mode 0755 on bin/b (was 4755)",
                "set mode 0755 on bin/a (was 0600)",
            ]
        );
        for name in ["bin/a", "bin/b", "bin/c"] {
            assert_eq!(mode(&root.join(name)), 0o755, "{name}");
        }
    }

    #[test]
    fn an_executable_behind_a_link_is_refused_and_its_target_keeps_its_mode() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("tool"), "x").unwrap();
        fs::set_permissions(outside.join("tool"), fs::Permissions::from_mode(0o600)).unwrap();
        let root = dir.path().join("root");
        fs::create_dir_all(root.join("real")).unwrap();
        fs::write(root.join("real/tool"), "x").unwrap();
        std::os::unix::fs::symlink(outside.join("tool"), root.join("link")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("dir")).unwrap();
        std::os::unix::fs::symlink(root.join("gone"), root.join("dangling")).unwrap();

        for listed in ["link", "dir/tool", "dangling", "real"] {
            let err = make_executable(&root, &[listed.to_owned()]).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("executable {listed} is not a regular file")
            );
        }
        assert_eq!(mode(&outside.join("tool")), 0o600);
        for listed in ["missing", "real/missing", "real/tool/x"] {
            let err = make_executable(&root, &[listed.to_owned()]).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("the artifact has no executable {listed}")
            );
        }
        assert!(make_executable(&root, &["../outside/tool".to_owned()]).is_err());
        assert_eq!(mode(&outside.join("tool")), 0o600);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_zip_archive_root_must_be_the_only_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.zip");
        let members = [
            zip::dir("demo-1"),
            zip::dir("demo-1/bin"),
            zip::file("demo-1/bin/demo", 0o755, b"x"),
            zip::file("demo-1/LICENSE", 0o644, b"x"),
        ];
        fs::write(&file, zip::stored(&members)).unwrap();
        let into = dir.path().join("Stackly Spike Ü/one");
        let root = extract(&artifact("zip", Some("demo-1")), &file, &into)
            .unwrap()
            .root;
        assert_eq!(root, into.join("demo-1"));
        assert!(root.join("bin/demo").is_file());

        let err = extract(
            &artifact("zip", Some("other")),
            &file,
            &dir.path().join("two"),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "the archive top must be exactly `other`, found demo-1"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_zip_member_that_climbs_out_fails_and_writes_nothing_outside() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.zip");
        let members = [
            zip::file("bin/demo", 0o755, b"x"),
            zip::file("../x", 0o644, b"x"),
        ];
        fs::write(&file, zip::stored(&members)).unwrap();
        let into = dir.path().join("spike/into");

        let err = extract(&artifact("zip", None), &file, &into).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("tar cannot extract the archive"),
            "{err}"
        );
        assert!(!dir.path().join("spike/x").exists());
        assert!(!dir.path().join("x").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_zip_member_written_through_a_link_fails_and_the_target_stays_empty() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let file = dir.path().join("a.zip");
        let members = [
            zip::symlink("link", outside.to_str().unwrap()),
            zip::file("link/x", 0o644, b"x"),
        ];
        fs::write(&file, zip::stored(&members)).unwrap();

        let err = extract(&artifact("zip", None), &file, &dir.path().join("into")).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("tar cannot extract the archive"),
            "{err}"
        );
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn apple_double_entries_of_a_zip_become_neither_files_nor_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.zip");
        let members = [
            zip::file("clickhouse", 0o755, b"x"),
            zip::dir("__MACOSX"),
            zip::file("__MACOSX/._clickhouse", 0o644, b"not apple double"),
        ];
        fs::write(&file, zip::stored(&members)).unwrap();
        let into = dir.path().join("into");

        extract(&zip_artifact(&["clickhouse"]), &file, &into).unwrap();
        assert_eq!(top_names(&into).unwrap(), ["clickhouse"]);
        let names = Command::new("/usr/bin/xattr")
            .arg(into.join("clickhouse"))
            .output()
            .unwrap();
        assert!(names.status.success());
        // macOS may add its own provenance attribute; nothing else.
        let names = String::from_utf8(names.stdout).unwrap();
        let extra: Vec<&str> = names
            .lines()
            .filter(|name| *name != "com.apple.provenance")
            .collect();
        assert!(extra.is_empty(), "{extra:?}");
    }

    #[test]
    fn the_report_counts_files_and_tells_inner_links_from_escaping_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::write(root.join("lib/libx.1.dylib"), "x").unwrap();
        std::os::unix::fs::symlink("libx.1.dylib", root.join("lib/libx.dylib")).unwrap();
        std::os::unix::fs::symlink("../../etc/passwd", root.join("lib/out")).unwrap();
        std::os::unix::fs::symlink("/usr/lib/libz.dylib", root.join("lib/abs")).unwrap();
        fs::write(root.join("suid"), "x").unwrap();
        fs::set_permissions(root.join("suid"), fs::Permissions::from_mode(0o4755)).unwrap();
        let report = report(root).unwrap();
        assert_eq!(report.files, 2);
        assert_eq!(report.symlinks_internal, 1);
        assert_eq!(
            report.symlinks_escaping,
            [
                "lib/abs -> /usr/lib/libz.dylib",
                "lib/out -> ../../etc/passwd"
            ]
        );
        assert_eq!(report.setuid, ["suid"]);
        assert!(report.special_files.is_empty());
    }
}
