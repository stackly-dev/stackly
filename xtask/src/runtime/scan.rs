//! `archive::scan` for the xtask (spec 0018, AC-21): the unpacked size and
//! entry count `runtime pin` prints and `runtime verify` compares, measured
//! by the same extractor the daemon installs with, in a tree in memory.
//! Runs on any OS.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, bail};
use stackly_core::runtime::archive::{ExtractLimits, ExtractReport, ExtractSpec, scan};
use stackly_domain::ArtifactFormat;
use stackly_platform::{EntryPath, ReadSeekFile};

/// What a scan measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    /// Bytes all regular files hold together.
    pub unpacked_size: u64,
    /// The entries, skipped AppleDouble members included.
    pub entry_count: u64,
}

/// A file on disk as the extractor reads it.
struct LocalFile {
    file: File,
    len: u64,
}

impl Read for LocalFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Seek for LocalFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

impl ReadSeekFile for LocalFile {
    fn len(&self) -> u64 {
        self.len
    }
}

/// The inventory `format` text as a format.
///
/// # Errors
///
/// Returns an error for text that names no format.
pub fn parse_format(format: &str) -> Result<ArtifactFormat> {
    format
        .parse()
        .map_err(|_| anyhow::anyhow!("`{format}` is not tar_gz, zip, or binary"))
}

/// Scans `path` the way the install extracts it: `archive_root` stripped,
/// `executables` (relative to the root) kept as the artifact lists them.
///
/// # Errors
///
/// Returns an error when the file cannot be read or the extractor rejects
/// the artifact.
pub fn measure(
    path: &Path,
    format: ArtifactFormat,
    archive_root: Option<&str>,
    executables: &[String],
) -> Result<Measured> {
    let report = run(path, format, archive_root, executables)?;
    Ok(Measured {
        unpacked_size: report.file_bytes,
        entry_count: report.entry_count(),
    })
}

/// Scans `path` the way `pin` does, where the inventory has no
/// `archive_root` yet: the archive's single top level folder, when every
/// entry lies in one, is the root. Returns the root it used with the
/// measurement.
///
/// # Errors
///
/// Returns an error when the file cannot be read or the extractor rejects
/// the artifact.
pub fn measure_detecting_root(
    path: &Path,
    format: ArtifactFormat,
    executables: &[String],
) -> Result<(Option<String>, Measured)> {
    let first = run(path, format, None, executables)?;
    let root = if format == ArtifactFormat::Binary {
        None
    } else {
        single_top_folder(&first)
    };
    let report = match &root {
        Some(root) => run(path, format, Some(root), executables)?,
        None => first,
    };
    Ok((
        root,
        Measured {
            unpacked_size: report.file_bytes,
            entry_count: report.entry_count(),
        },
    ))
}

/// The one folder every entry lies in, when the archive has one.
fn single_top_folder(report: &ExtractReport) -> Option<String> {
    use stackly_core::runtime::archive::EntryKind;
    let mut top: Option<&str> = None;
    let mut top_is_dir = false;
    for entry in &report.entries {
        let path = entry.path.as_str();
        let (name, nested) = match path.split_once('/') {
            Some((name, _)) => (name, true),
            None => (path, false),
        };
        match top {
            None => top = Some(name),
            Some(seen) if seen == name => {}
            Some(_) => return None,
        }
        if !nested {
            if entry.kind != EntryKind::Dir {
                return None;
            }
            top_is_dir = true;
        }
    }
    top.filter(|_| top_is_dir).map(str::to_owned)
}

fn run(
    path: &Path,
    format: ArtifactFormat,
    archive_root: Option<&str>,
    executables: &[String],
) -> Result<ExtractReport> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let len = file
        .metadata()
        .with_context(|| format!("cannot read {}", path.display()))?
        .len();
    let executables = executables
        .iter()
        .map(|executable| {
            EntryPath::new(executable)
                .map_err(|err| anyhow::anyhow!("executable `{executable}`: {err}"))
        })
        .collect::<Result<Vec<_>>>()?;
    if format == ArtifactFormat::Binary && executables.is_empty() {
        bail!("a binary artifact needs its executable name; pass --executable <name>");
    }
    let spec = ExtractSpec::new(
        archive_root.map(str::to_owned),
        executables,
        ExtractLimits::for_artifact(len, None, None),
    );
    let mut input = LocalFile { file, len };
    scan(format, &mut input, &spec)
        .map_err(|err| anyhow::anyhow!("{} cannot be extracted: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::super::test_zip::{dir, file, stored};
    use super::*;

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    // covers: AC-21
    #[test]
    fn a_zip_with_one_top_folder_is_measured_inside_it_and_the_folder_is_not_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let zip = write(
            &tmp,
            "a.zip",
            &stored(&[
                dir("pkg"),
                dir("pkg/bin"),
                file("pkg/bin/tool", 0o755, b"12345"),
                file("pkg/README", 0o644, b"abc"),
            ]),
        );

        let (root, measured) =
            measure_detecting_root(&zip, ArtifactFormat::Zip, &["bin/tool".to_owned()]).unwrap();

        assert_eq!(root.as_deref(), Some("pkg"));
        assert_eq!(
            measured,
            Measured {
                unpacked_size: 8,
                entry_count: 3
            }
        );
        assert_eq!(
            measure(
                &zip,
                ArtifactFormat::Zip,
                Some("pkg"),
                &["bin/tool".to_owned()]
            )
            .unwrap(),
            measured
        );
    }

    #[test]
    fn a_zip_with_files_at_its_top_has_no_root_and_counts_every_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let zip = write(
            &tmp,
            "b.zip",
            &stored(&[
                file("tool", 0o755, b"1234"),
                dir("share"),
                file("share/a", 0o644, b"x"),
            ]),
        );

        let (root, measured) =
            measure_detecting_root(&zip, ArtifactFormat::Zip, &["tool".to_owned()]).unwrap();

        assert_eq!(root, None);
        assert_eq!(
            measured,
            Measured {
                unpacked_size: 5,
                entry_count: 3
            }
        );
    }

    #[test]
    fn a_binary_needs_an_executable_and_measures_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        let binary = write(&tmp, "tool", &[0xcf, 0xfa, 0xed, 0xfe, 1, 2, 3, 4, 5, 6]);

        let err = measure(&binary, ArtifactFormat::Binary, None, &[]).unwrap_err();
        assert!(err.to_string().contains("--executable"), "{err}");

        let (root, measured) =
            measure_detecting_root(&binary, ArtifactFormat::Binary, &["tool".to_owned()]).unwrap();
        assert_eq!(root, None);
        assert_eq!(
            measured,
            Measured {
                unpacked_size: 10,
                entry_count: 1
            }
        );
    }

    #[test]
    fn an_artifact_the_extractor_rejects_names_the_file_and_the_reason() {
        let tmp = tempfile::tempdir().unwrap();
        let zip = write(&tmp, "c.zip", &stored(&[file("../escape", 0o644, b"x")]));

        let err = measure(&zip, ArtifactFormat::Zip, None, &[]).unwrap_err();

        assert!(
            err.to_string().contains("c.zip cannot be extracted"),
            "{err}"
        );
        assert!(parse_format("tar").is_err());
        assert_eq!(parse_format("tar_gz").unwrap(), ArtifactFormat::TarGz);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_tar_gz_made_by_tar_is_measured_like_the_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let tree = tmp.path().join("pkg");
        std::fs::create_dir_all(tree.join("bin")).unwrap();
        std::fs::write(tree.join("bin/tool"), b"12345").unwrap();
        std::fs::write(tree.join("README"), b"abc").unwrap();
        let archive = tmp.path().join("a.tar.gz");
        let status = std::process::Command::new("/usr/bin/tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(tmp.path())
            .arg("--no-mac-metadata")
            .arg("pkg")
            .status()
            .unwrap();
        assert!(status.success());

        let (root, measured) =
            measure_detecting_root(&archive, ArtifactFormat::TarGz, &["bin/tool".to_owned()])
                .unwrap();

        assert_eq!(root.as_deref(), Some("pkg"));
        assert_eq!(
            measured,
            Measured {
                unpacked_size: 8,
                entry_count: 3
            }
        );
    }
}
