//! `cargo xtask runtime pin --url <https>... [--executable <path>]
//! [--sha256-url <https>]`: downloads every candidate URL, checks that they
//! give the same bytes, and prints the `format`, `urls`, `sha256`, `size`,
//! and `min_os` lines of an inventory entry (spec 0003). Digests are never
//! typed by hand.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use super::cache::{Cache, Download, PIN_MAX_SIZE, SIDE_FILE_MAX_SIZE};
use super::checksum::{base_name, expected_digest, parse_sums};
use super::inventory::{is_https, is_inner_relative_path};
use super::macho::{self, FileKind};

/// The archive tool; never one from PATH.
const TAR: &str = "/usr/bin/tar";

/// Parsed arguments of `runtime pin`.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    urls: Vec<String>,
    executable: Option<String>,
    sha256_url: Option<String>,
}

/// What `pin` prints for a download that is not a Mach-O file, a `.tar.gz`,
/// or a `.zip`.
const UNKNOWN_NOTE: &str =
    "pin: the download is not a Mach-O file, a .tar.gz, or a .zip; format and min_os left out";

/// The TOML lines `pin` prints, in inventory order.
#[derive(Debug, Serialize)]
struct Pinned {
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'static str>,
    urls: Vec<String>,
    sha256: String,
    size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_os: Option<String>,
}

pub fn run(args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    let cache = Cache::open()?;
    let downloads = options
        .urls
        .iter()
        .map(|url| {
            eprintln!("pin: downloading {url}");
            cache.download(url, PIN_MAX_SIZE)
        })
        .collect::<Result<Vec<Download>>>()?;
    let first = same_bytes(&downloads)?;
    let (sha256, size) = (first.sha256.clone(), first.size);
    if let Some(sha256_url) = &options.sha256_url {
        check_sha256_file(&cache, sha256_url, &options.urls, &sha256)?;
    }
    let mut downloads = downloads.into_iter();
    let stored = cache.store(downloads.next().context("pin needs at least one --url")?)?;
    // The other candidates hold the same bytes; free their space now.
    drop(downloads);
    // The format comes from the bytes on every OS; only min_os needs macOS.
    let kind = macho::file_kind(&stored)?;
    let format = kind.format();
    let min_os = if format.is_some() {
        read_min_os(&stored, kind, options.executable.as_deref())?
    } else {
        eprintln!("{UNKNOWN_NOTE}");
        None
    };
    let pinned = Pinned {
        format,
        urls: options.urls,
        sha256,
        size,
        min_os,
    };
    print!(
        "{}",
        toml::to_string(&pinned).context("cannot write the pinned values as TOML")?
    );
    Ok(())
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut urls = Vec::new();
    let mut executable = None;
    let mut sha256_url = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match *arg {
            "--url" => urls.push((*rest.next().context("--url needs an https:// URL")?).to_owned()),
            "--executable" => {
                let path = rest.next().context("--executable needs a relative path")?;
                if !is_inner_relative_path(path) {
                    bail!(
                        "--executable must be a plain relative path, without `..`, `.`, or empty parts, found `{path}`"
                    );
                }
                executable = Some((*path).to_owned());
            }
            "--sha256-url" => {
                sha256_url =
                    Some((*rest.next().context("--sha256-url needs an https:// URL")?).to_owned());
            }
            other => bail!(
                "unexpected argument `{other}`; use `cargo xtask runtime pin --url <https>... [--executable <path>] [--sha256-url <https>]`"
            ),
        }
    }
    if urls.is_empty() {
        bail!("pin needs at least one --url");
    }
    for url in urls.iter().chain(&sha256_url) {
        if !is_https(url) {
            bail!("url must be an https:// URL, found `{url}`");
        }
    }
    Ok(Options {
        urls,
        executable,
        sha256_url,
    })
}

/// Every candidate URL must give the same digest and size; returns the
/// first download.
fn same_bytes(downloads: &[Download]) -> Result<&Download> {
    let first = downloads.first().context("pin needs at least one --url")?;
    if downloads
        .iter()
        .any(|d| d.sha256 != first.sha256 || d.size != first.size)
    {
        let found: Vec<String> = downloads
            .iter()
            .map(|d| format!("{} gave {} ({} bytes)", d.url, d.sha256, d.size))
            .collect();
        bail!(
            "the candidate URLs give different digests: {}",
            found.join("; ")
        );
    }
    Ok(first)
}

/// The digest must equal the one a `.sha256` (or `SHA256SUMS`) file gives
/// for the file name of one of the candidate URLs.
fn check_sha256_file(cache: &Cache, sha256_url: &str, urls: &[String], sha256: &str) -> Result<()> {
    eprintln!("pin: downloading {sha256_url}");
    let file = cache.download(sha256_url, SIDE_FILE_MAX_SIZE)?;
    let text = std::fs::read_to_string(file.path())
        .with_context(|| format!("{sha256_url} is not a text file"))?;
    let names: Vec<&str> = urls.iter().map(|url| base_name(url)).collect();
    let expected = expected_digest(&parse_sums(&text), &names)
        .with_context(|| format!("cannot read the digest from {sha256_url}"))?;
    if expected != sha256 {
        bail!("digest {sha256} does not match {sha256_url}, which says {expected}");
    }
    Ok(())
}

/// The macOS minimum of the main executable: the file itself for a
/// `binary` download, `--executable` inside a `.tar.gz` or a `.zip`. Only
/// on macOS, where `vtool` exists (and where `/usr/bin/tar` reads zip);
/// elsewhere the line is left out with a note.
fn read_min_os(stored: &Path, kind: FileKind, executable: Option<&str>) -> Result<Option<String>> {
    if !cfg!(target_os = "macos") {
        eprintln!("pin: min_os needs vtool; run pin on macOS to fill it");
        return Ok(None);
    }
    match kind {
        FileKind::MachO => macho::min_os(stored).map(Some),
        FileKind::Gzip | FileKind::Zip => {
            let Some(executable) = executable else {
                eprintln!("pin: pass --executable <path in the archive> to read min_os");
                return Ok(None);
            };
            let folder = tempfile::tempdir().context("cannot create a folder to extract into")?;
            let path = extract_one(stored, kind, executable, folder.path())?;
            macho::min_os(&path).map(Some)
        }
        FileKind::Other => {
            eprintln!("{UNKNOWN_NOTE}");
            Ok(None)
        }
    }
}

/// The tar flags that list an archive of `kind`, extract from it, and the
/// options that follow the archive when extracting.
fn tar_flags(kind: FileKind) -> Result<(&'static str, &'static str, &'static [&'static str])> {
    match kind {
        FileKind::Gzip => Ok(("-tzf", "-xzf", &[])),
        FileKind::Zip => Ok(("-tf", "-xf", &["--no-mac-metadata"])),
        FileKind::MachO | FileKind::Other => {
            bail!("a {} file is not an archive tar can list", kind.name())
        }
    }
}

/// Extracts only `executable` from the archive into `into` and returns its
/// path. `executable` is relative to the artifact root, which is the
/// archive itself or its single top folder.
fn extract_one(archive: &Path, kind: FileKind, executable: &str, into: &Path) -> Result<PathBuf> {
    let (list, extract, options) = tar_flags(kind)?;
    let listing = Command::new(TAR)
        .arg(list)
        .arg(archive)
        .output()
        .with_context(|| format!("cannot run {TAR}"))?;
    if !listing.status.success() {
        bail!(
            "tar cannot list the archive: {}",
            String::from_utf8_lossy(&listing.stderr).trim()
        );
    }
    let listing = String::from_utf8_lossy(&listing.stdout);
    let members: Vec<&str> = listing.lines().collect();
    let member = find_member(&members, executable).with_context(|| {
        format!("the archive has no {executable}, at its top or in its single top folder")
    })?;
    // `--` keeps a member name from ever being read as a tar option, and
    // the working folder keeps anything tar writes inside `into`.
    let status = Command::new(TAR)
        .current_dir(into)
        .arg(extract)
        .arg(archive)
        .args(options)
        .arg("-C")
        .arg(into)
        .arg("--")
        .arg(member)
        .status()
        .with_context(|| format!("cannot run {TAR}"))?;
    if !status.success() {
        bail!("tar cannot extract {member}");
    }
    Ok(into.join(normalize_member(member)))
}

/// The archive member that is `executable`, either at the top of the
/// archive or inside the one folder every member sits in. A member that
/// starts with `-` is never returned, so it cannot pose as a tar option.
fn find_member<'a>(members: &[&'a str], executable: &str) -> Option<&'a str> {
    let member = find_member_by_name(members, executable)?;
    (!member.starts_with('-')).then_some(member)
}

fn find_member_by_name<'a>(members: &[&'a str], executable: &str) -> Option<&'a str> {
    if let Some(member) = members.iter().find(|m| normalize_member(m) == executable) {
        return Some(member);
    }
    let top = single_top(members)?;
    let nested = format!("{top}/{executable}");
    members
        .iter()
        .find(|m| normalize_member(m) == nested)
        .copied()
}

/// The one top folder all members share, when there is exactly one.
fn single_top<'a>(members: &[&'a str]) -> Option<&'a str> {
    let mut tops = members
        .iter()
        .map(|m| normalize_member(m))
        .filter(|m| !m.is_empty())
        .map(|m| m.split('/').next().unwrap_or(m));
    let top = tops.next()?;
    tops.all(|other| other == top).then_some(top)
}

/// A member name without a leading `./` or a trailing `/`.
fn normalize_member(member: &str) -> &str {
    member
        .strip_prefix("./")
        .unwrap_or(member)
        .trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::test_zip as zip;

    #[test]
    fn parse_reads_every_url_and_the_options() {
        let options = parse(&[
            "--url",
            "https://a.example/x.tar.gz",
            "--url",
            "https://b.example/x.tar.gz",
            "--executable",
            "bin/mysqld",
            "--sha256-url",
            "https://a.example/x.tar.gz.sha256",
        ])
        .unwrap();
        assert_eq!(
            options,
            Options {
                urls: vec![
                    "https://a.example/x.tar.gz".to_owned(),
                    "https://b.example/x.tar.gz".to_owned()
                ],
                executable: Some("bin/mysqld".to_owned()),
                sha256_url: Some("https://a.example/x.tar.gz.sha256".to_owned()),
            }
        );
    }

    #[test]
    fn parse_refuses_plain_http_missing_urls_and_escaping_paths() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--url", "http://a.example/x.tar.gz"]).is_err());
        assert!(
            parse(&[
                "--url",
                "https://a.example/x",
                "--sha256-url",
                "http://a.example/s"
            ])
            .is_err()
        );
        assert!(parse(&["--url", "https://a.example/x", "--executable", "../bin/x"]).is_err());
        assert!(parse(&["--url", "https://a.example/x", "--verbose"]).is_err());
    }

    #[test]
    fn candidates_with_different_bytes_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let download = |url: &str, sha256: &str, size: u64| {
            let file = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
            Download {
                file,
                url: url.to_owned(),
                sha256: sha256.to_owned(),
                size,
            }
        };
        let same = [
            download("https://a/x", "aa", 1),
            download("https://b/x", "aa", 1),
        ];
        assert_eq!(same_bytes(&same).unwrap().url, "https://a/x");
        let different = [
            download("https://a/x", "aa", 1),
            download("https://b/x", "bb", 1),
        ];
        let err = same_bytes(&different).err().unwrap().to_string();
        assert!(err.contains("https://b/x gave bb"), "{err}");
    }

    #[test]
    fn the_executable_is_found_at_the_top_or_in_the_single_top_folder() {
        let nats = [
            "nats-server-v2.15.1-darwin-arm64/",
            "nats-server-v2.15.1-darwin-arm64/LICENSE",
            "nats-server-v2.15.1-darwin-arm64/nats-server",
        ];
        assert_eq!(
            find_member(&nats, "nats-server"),
            Some("nats-server-v2.15.1-darwin-arm64/nats-server")
        );
        let traefik = ["./LICENSE.md", "./traefik", "./CHANGELOG.md"];
        assert_eq!(find_member(&traefik, "traefik"), Some("./traefik"));
        let two_tops = ["a/bin/x", "b/bin/x"];
        assert_eq!(find_member(&two_tops, "bin/x"), None);
    }

    #[test]
    fn pinned_values_print_as_inventory_toml() {
        let pinned = Pinned {
            format: Some("tar_gz"),
            urls: vec!["https://a.example/x.tar.gz".to_owned()],
            sha256: "a".repeat(64),
            size: 42,
            min_os: Some("11.0".to_owned()),
        };
        assert_eq!(
            toml::to_string(&pinned).unwrap(),
            format!(
                "format = \"tar_gz\"\nurls = [\"https://a.example/x.tar.gz\"]\nsha256 = \"{}\"\nsize = 42\nmin_os = \"11.0\"\n",
                "a".repeat(64)
            )
        );
    }

    #[test]
    fn an_unknown_download_prints_neither_format_nor_min_os() {
        let pinned = Pinned {
            format: FileKind::Other.format(),
            urls: vec!["https://a.example/x.tar.zst".to_owned()],
            sha256: "a".repeat(64),
            size: 42,
            min_os: None,
        };
        assert_eq!(
            toml::to_string(&pinned).unwrap(),
            format!(
                "urls = [\"https://a.example/x.tar.zst\"]\nsha256 = \"{}\"\nsize = 42\n",
                "a".repeat(64)
            )
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.tar.zst");
        std::fs::write(&file, [0x28, 0xb5, 0x2f, 0xfd, 0, 0, 0, 0]).unwrap();
        assert_eq!(
            read_min_os(&file, FileKind::Other, Some("bin/x")).unwrap(),
            None
        );
    }

    #[test]
    fn a_zip_download_pins_format_zip_on_every_os() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("clickhouse-macos-aarch64.zip");
        std::fs::write(&file, zip::stored(&[zip::file("clickhouse", 0o755, b"x")])).unwrap();
        assert_eq!(macho::file_kind(&file).unwrap().format(), Some("zip"));
    }

    #[test]
    fn a_member_that_looks_like_a_tar_option_is_never_returned() {
        let top = "--use-compress-program=touch MARK x";
        let members = [format!("{top}/"), format!("{top}/nats-server")];
        let members: Vec<&str> = members.iter().map(String::as_str).collect();
        assert_eq!(find_member(&members, "nats-server"), None);
        assert_eq!(find_member(&["-rf"], "-rf"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_archive_whose_top_folder_is_a_tar_option_runs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let top = "--use-compress-program=touch MARK x";
        std::fs::create_dir_all(source.join(top)).unwrap();
        std::fs::copy("/usr/bin/true", source.join(top).join("nats-server")).unwrap();
        let archive = dir.path().join("evil.tar.gz");
        let status = Command::new(TAR)
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&source)
            .arg("--")
            .arg(top)
            .status()
            .unwrap();
        assert!(status.success());
        let into = dir.path().join("into");
        std::fs::create_dir(&into).unwrap();

        assert!(extract_one(&archive, FileKind::Gzip, "nats-server", &into).is_err());
        assert!(!into.join("MARK").exists());
        assert_eq!(std::fs::read_dir(&into).unwrap().count(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn min_os_is_read_from_an_executable_inside_a_tar_gz() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join("tool-1.0-darwin-arm64");
        std::fs::create_dir_all(top.join("bin")).unwrap();
        std::fs::copy("/usr/bin/true", top.join("bin/tool")).unwrap();
        let archive = dir.path().join("tool.tar.gz");
        let status = Command::new(TAR)
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(dir.path())
            .arg("tool-1.0-darwin-arm64")
            .status()
            .unwrap();
        assert!(status.success());

        let minimum = read_min_os(&archive, FileKind::Gzip, Some("bin/tool"))
            .unwrap()
            .unwrap();
        assert_eq!(minimum, macho::min_os(Path::new("/usr/bin/true")).unwrap());
        assert_eq!(read_min_os(&archive, FileKind::Gzip, None).unwrap(), None);
        assert!(read_min_os(&archive, FileKind::Gzip, Some("bin/missing")).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn min_os_is_read_from_an_executable_inside_a_zip() {
        let dir = tempfile::tempdir().unwrap();
        let tool = std::fs::read("/usr/bin/true").unwrap();
        let archive = dir.path().join("tool.zip");
        std::fs::write(
            &archive,
            zip::stored(&[zip::file("clickhouse", 0o755, &tool)]),
        )
        .unwrap();

        let minimum = read_min_os(&archive, FileKind::Zip, Some("clickhouse"))
            .unwrap()
            .unwrap();
        assert_eq!(minimum, macho::min_os(Path::new("/usr/bin/true")).unwrap());
        assert!(read_min_os(&archive, FileKind::Zip, Some("missing")).is_err());
    }
}
