//! `cargo xtask runtime pin --url <https>... [--executable <path>]
//! [--sha256-url <https>]`: downloads every candidate URL, checks that they
//! give the same bytes, and prints the `format`, `urls`, `sha256`, `size`,
//! and `min_os` lines of an inventory entry (spec 0003). Digests are never
//! typed by hand.
//!
//! `cargo xtask runtime pin --file <tar.gz> --url <https>...
//! [--executable <path>]` does the same for a maintainer artifact built on
//! this Mac, without the network: the file is checked against the
//! `.sha256` beside it and copied into the cache, and `urls` are the
//! release URLs it will be uploaded to (AC-19).

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use super::cache::{Cache, Download, PIN_MAX_SIZE, SIDE_FILE_MAX_SIZE};
use super::checksum::{SumLine, base_name, expected_digest, parse_sums};
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
    /// A local maintainer artifact; with it nothing is downloaded.
    file: Option<PathBuf>,
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
    let pinned = match &options.file {
        Some(file) => pin_file(&cache, file, &options)?,
        None => pin_download(&cache, &options)?,
    };
    print!(
        "{}",
        toml::to_string(&pinned).context("cannot write the pinned values as TOML")?
    );
    Ok(())
}

/// Downloads every candidate URL into the cache and pins the bytes.
fn pin_download(cache: &Cache, options: &Options) -> Result<Pinned> {
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
        check_sha256_file(cache, sha256_url, &options.urls, &sha256)?;
    }
    let mut downloads = downloads.into_iter();
    let stored = cache.store(downloads.next().context("pin needs at least one --url")?)?;
    // The other candidates hold the same bytes; free their space now.
    drop(downloads);
    pinned(&stored, options, sha256, size)
}

/// Pins a local maintainer artifact without the network: every `--url`
/// must end in the file's own name (`gh` uploads it under that name), its
/// digest must equal the one `<file>.sha256` gives, and only then is it
/// copied, never moved, into the cache under that digest (AC-19).
fn pin_file(cache: &Cache, file: &Path, options: &Options) -> Result<Pinned> {
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("--file {} has no file name", file.display()))?;
    for url in &options.urls {
        let url_name = base_name(url);
        if url_name != name {
            bail!(
                "--url {url} ends in `{url_name}`, not in the file name `{name}`; gh uploads the file under its own name"
            );
        }
    }
    let expected = sha256_beside(file, name)?;
    eprintln!("pin: copying {} into the cache", file.display());
    let copy = cache.import(file, PIN_MAX_SIZE)?;
    if copy.sha256 != expected {
        bail!(
            "digest {} of {} does not match {name}.sha256, which says {expected}",
            copy.sha256,
            file.display()
        );
    }
    let (sha256, size) = (copy.sha256.clone(), copy.size);
    let stored = cache.store(copy)?;
    pinned(&stored, options, sha256, size)
}

/// The digest `<file>.sha256` gives: its one line must be
/// `<sha256>  <name>`, with the file's own name.
fn sha256_beside(file: &Path, name: &str) -> Result<String> {
    let mut side = OsString::from(file.as_os_str());
    side.push(".sha256");
    let side = PathBuf::from(side);
    let meta = match fs::metadata(&side) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            bail!(
                "{} is missing; build.sh writes it beside the artifact",
                side.display()
            )
        }
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", side.display())),
    };
    if meta.len() > SIDE_FILE_MAX_SIZE {
        bail!(
            "{} is larger than {SIDE_FILE_MAX_SIZE} bytes",
            side.display()
        );
    }
    let text = fs::read_to_string(&side)
        .with_context(|| format!("{} is not a text file", side.display()))?;
    match parse_sums(&text).as_slice() {
        [
            SumLine {
                hash,
                name: Some(listed),
            },
        ] if hash.len() == 64 && listed == name => Ok(hash.clone()),
        [
            SumLine {
                name: Some(listed), ..
            },
        ] if listed != name => bail!(
            "{} names `{listed}`, not the file name `{name}`",
            side.display()
        ),
        _ => bail!("{} must hold one line `<sha256>  {name}`", side.display()),
    }
}

/// The lines `pin` prints for the bytes now in the cache at `stored`.
fn pinned(stored: &Path, options: &Options, sha256: String, size: u64) -> Result<Pinned> {
    // The format comes from the bytes on every OS; only min_os needs macOS.
    let kind = macho::file_kind(stored)?;
    let format = kind.format();
    let min_os = if format.is_some() {
        read_min_os(stored, kind, options.executable.as_deref())?
    } else {
        eprintln!("{UNKNOWN_NOTE}");
        None
    };
    Ok(Pinned {
        format,
        urls: options.urls.clone(),
        sha256,
        size,
        min_os,
    })
}

const USAGE: &str = "use `cargo xtask runtime pin --url <https>... [--executable <path>] [--sha256-url <https>]` or `cargo xtask runtime pin --file <tar.gz> --url <https>... [--executable <path>]`";

fn parse(args: &[&str]) -> Result<Options> {
    let mut urls = Vec::new();
    let mut executable = None;
    let mut sha256_url = None;
    let mut file = None;
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
            "--file" => {
                let path = rest.next().context("--file needs the path of a .tar.gz")?;
                if file.replace(PathBuf::from(path)).is_some() {
                    bail!("--file is given more than once");
                }
            }
            other => bail!("unexpected argument `{other}`; {USAGE}"),
        }
    }
    if urls.is_empty() {
        bail!("pin needs at least one --url");
    }
    if file.is_some() && sha256_url.is_some() {
        bail!(
            "--file cannot go with --sha256-url: a maintainer artifact is checked against the .sha256 beside it"
        );
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
        file,
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
                file: None,
            }
        );
    }

    #[test]
    fn parse_reads_a_local_file_and_refuses_it_with_a_sha256_url() {
        let options = parse(&[
            "--file",
            "out/valkey.tar.gz",
            "--url",
            "https://a.example/valkey.tar.gz",
            "--executable",
            "bin/valkey-server",
        ])
        .unwrap();
        assert_eq!(options.file, Some(PathBuf::from("out/valkey.tar.gz")));
        assert_eq!(options.urls, ["https://a.example/valkey.tar.gz"]);
        let err = parse(&[
            "--file",
            "out/valkey.tar.gz",
            "--url",
            "https://a.example/valkey.tar.gz",
            "--sha256-url",
            "https://a.example/valkey.tar.gz.sha256",
        ])
        .err()
        .unwrap();
        assert_eq!(
            err.to_string(),
            "--file cannot go with --sha256-url: a maintainer artifact is checked against the .sha256 beside it"
        );
        assert!(parse(&["--file", "out/valkey.tar.gz"]).is_err());
        assert!(
            parse(&[
                "--file",
                "a.tar.gz",
                "--file",
                "b.tar.gz",
                "--url",
                "https://a.example/a.tar.gz"
            ])
            .is_err()
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

    /// The file name `build.sh` gives a maintainer artifact.
    const ARTIFACT: &str = "valkey-9.1.2-r1-macos-aarch64.tar.gz";
    /// Its release URL, on a host that never resolves: a network call would
    /// fail the test.
    const RELEASE_URL: &str = "https://stackly.invalid/stackly-dev/stackly/releases/download/runtime/valkey-9.1.2-r1-macos-aarch64/valkey-9.1.2-r1-macos-aarch64.tar.gz";

    /// `out/<ARTIFACT>` with gzip bytes and the `.sha256` beside it, as
    /// `build.sh` writes them; returns the artifact path, its digest, and a
    /// cache in the same temporary folder.
    fn maintainer_build(dir: &Path) -> (PathBuf, String, Cache) {
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let file = out.join(ARTIFACT);
        std::fs::write(&file, [0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, b'x']).unwrap();
        let (sha256, _) = crate::runtime::cache::sha256_file(&file).unwrap();
        std::fs::write(
            out.join(format!("{ARTIFACT}.sha256")),
            format!("{sha256}  {ARTIFACT}\n"),
        )
        .unwrap();
        (file, sha256, Cache::at(dir.join("cache")).unwrap())
    }

    fn file_options(file: &Path, url: &str) -> Options {
        Options {
            urls: vec![url.to_owned()],
            executable: None,
            sha256_url: None,
            file: Some(file.to_path_buf()),
        }
    }

    /// Every name in the cache folder, temporary files included.
    fn cache_entries(cache: &Cache) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(cache.dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn pin_file_copies_a_matching_artifact_into_the_cache_and_prints_the_release_urls() {
        let dir = tempfile::tempdir().unwrap();
        let (file, sha256, cache) = maintainer_build(dir.path());
        let pinned = pin_file(&cache, &file, &file_options(&file, RELEASE_URL)).unwrap();
        assert_eq!(
            toml::to_string(&pinned).unwrap(),
            format!(
                "format = \"tar_gz\"\nurls = [\"{RELEASE_URL}\"]\nsha256 = \"{sha256}\"\nsize = 9\n"
            )
        );
        assert_eq!(cache_entries(&cache), std::slice::from_ref(&sha256));
        assert_eq!(
            std::fs::read(cache.path(&sha256)).unwrap(),
            std::fs::read(&file).unwrap()
        );
        assert!(file.is_file(), "the artifact is copied, not moved");
    }

    #[test]
    fn pin_file_needs_the_sha256_file_beside_the_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let (file, _, cache) = maintainer_build(dir.path());
        std::fs::remove_file(dir.path().join("out").join(format!("{ARTIFACT}.sha256"))).unwrap();
        let err = pin_file(&cache, &file, &file_options(&file, RELEASE_URL))
            .err()
            .unwrap()
            .to_string();
        assert!(
            err.ends_with(&format!(
                "{ARTIFACT}.sha256 is missing; build.sh writes it beside the artifact"
            )),
            "{err}"
        );
        assert_eq!(cache_entries(&cache), Vec::<String>::new());
    }

    #[test]
    fn pin_file_refuses_a_digest_that_differs_from_the_sha256_file() {
        let dir = tempfile::tempdir().unwrap();
        let (file, sha256, cache) = maintainer_build(dir.path());
        let other = "0".repeat(64);
        std::fs::write(
            dir.path().join("out").join(format!("{ARTIFACT}.sha256")),
            format!("{other}  {ARTIFACT}\n"),
        )
        .unwrap();
        let err = pin_file(&cache, &file, &file_options(&file, RELEASE_URL))
            .err()
            .unwrap()
            .to_string();
        assert!(
            err.starts_with(&format!("digest {sha256} of "))
                && err.ends_with(&format!(
                    "does not match {ARTIFACT}.sha256, which says {other}"
                )),
            "{err}"
        );
        assert_eq!(cache_entries(&cache), Vec::<String>::new());
    }

    #[test]
    fn pin_file_refuses_a_sha256_file_that_names_another_file_or_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let (file, sha256, cache) = maintainer_build(dir.path());
        let side = dir.path().join("out").join(format!("{ARTIFACT}.sha256"));
        std::fs::write(&side, format!("{sha256}  valkey.tar.gz\n")).unwrap();
        let err = pin_file(&cache, &file, &file_options(&file, RELEASE_URL))
            .err()
            .unwrap()
            .to_string();
        assert!(
            err.ends_with(&format!(
                "names `valkey.tar.gz`, not the file name `{ARTIFACT}`"
            )),
            "{err}"
        );
        std::fs::write(&side, format!("{sha256}\n")).unwrap();
        let err = pin_file(&cache, &file, &file_options(&file, RELEASE_URL))
            .err()
            .unwrap()
            .to_string();
        assert!(
            err.ends_with(&format!("must hold one line `<sha256>  {ARTIFACT}`")),
            "{err}"
        );
        assert_eq!(cache_entries(&cache), Vec::<String>::new());
    }

    #[test]
    fn pin_file_refuses_a_url_that_ends_in_another_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let (file, _, cache) = maintainer_build(dir.path());
        let url = "https://stackly.invalid/runtime/valkey-9.1.2-r1/valkey.tar.gz";
        let err = pin_file(&cache, &file, &file_options(&file, url))
            .err()
            .unwrap()
            .to_string();
        assert_eq!(
            err,
            format!(
                "--url {url} ends in `valkey.tar.gz`, not in the file name `{ARTIFACT}`; gh uploads the file under its own name"
            )
        );
        assert_eq!(cache_entries(&cache), Vec::<String>::new());
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
