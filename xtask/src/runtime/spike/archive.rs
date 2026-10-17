//! Checking the cached download again, extracting it, and reporting what
//! the archive holds (spec 0003, AC-3, AC-16, and the archive report).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::plan::PlanArtifact;
use super::result::ArchiveReport;
use crate::runtime::cache::sha256_file;
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

/// Extracts the artifact into `into` (a new folder) and returns the
/// artifact root: `into/<archive_root>` when the entry names one, which
/// must then be the only thing at the top of the archive, else `into`
/// itself. A `binary` download is copied in under its executable name
/// with mode 0755. The magic number of the cached file must match
/// `format` before anything is extracted (AC-16).
pub fn extract(artifact: &PlanArtifact, cache_file: &Path, into: &Path) -> Result<PathBuf> {
    check_format(&artifact.format, file_kind(cache_file)?)?;
    fs::create_dir_all(into).with_context(|| format!("cannot create {}", into.display()))?;
    match artifact.format.as_str() {
        "tar_gz" => untar(
            cache_file,
            "-xzf",
            &[],
            into,
            artifact.archive_root.as_deref(),
        ),
        // Without `--no-mac-metadata`, bsdtar turns `__MACOSX/._<name>`
        // entries into extended attributes (or fails on them).
        "zip" => untar(
            cache_file,
            "-xf",
            &["--no-mac-metadata"],
            into,
            artifact.archive_root.as_deref(),
        ),
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
            Ok(into.to_path_buf())
        }
        other => bail!("unknown format `{other}`"),
    }
}

/// Extracts a `.tar.gz` or a `.zip` with the system tar, never with `-P`,
/// so a `..` member or one written through a link makes tar fail, and
/// returns the artifact root. File modes come from the archive.
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
        let root = extract(&artifact("tar_gz", Some("demo-1")), &archive, &into).unwrap();
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
            extract(&artifact("tar_gz", None), &archive, &into).unwrap(),
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
        let root = extract(&artifact("binary", None), &file, &into).unwrap();
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

    #[cfg(target_os = "macos")]
    #[test]
    fn a_zip_without_a_root_keeps_the_mode_of_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("clickhouse-macos-aarch64.zip");
        fs::write(&file, zip::stored(&[zip::file("clickhouse", 0o755, b"x")])).unwrap();
        let into = dir
            .path()
            .join("Stackly Spike Ü/clickhouse-26.8.1.1-macos-aarch64");
        let mut clickhouse = artifact("zip", None);
        clickhouse.executables = vec!["clickhouse".to_owned()];

        assert_eq!(extract(&clickhouse, &file, &into).unwrap(), into);
        let mode = fs::metadata(into.join("clickhouse"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(top_names(&into).unwrap(), ["clickhouse"]);
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
        let root = extract(&artifact("zip", Some("demo-1")), &file, &into).unwrap();
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

        extract(&artifact("zip", None), &file, &into).unwrap();
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
