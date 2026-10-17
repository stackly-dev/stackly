//! `cargo xtask runtime verify [--id <id>]`: downloads every `available`
//! artifact again from each of its URLs and checks that the bytes still
//! have the pinned `sha256` and `size`, then checks the upstream checksum
//! file by its kind and, when `gpg` is on PATH, the upstream signature
//! (spec 0003). Prints `OK`, `FAIL <reason>`, or `SKIP gpg` per artifact.

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use super::cache::{Cache, SIDE_FILE_MAX_SIZE, TooLarge, sha512_file};
use super::checksum::{base_name, expected_digest, gpg_available, parse_sums, verify_gpg};
use super::inventory::{Artifact, Inventory};

/// The result for one artifact.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// Digest, size, checksum, and signature all match.
    Ok,
    /// Everything that could be checked matches, but the GPG signature was
    /// not checked because `gpg` is not on PATH.
    SkipGpg,
    /// Something does not match or cannot be reached.
    Fail(String),
}

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    let only = match args {
        [] => None,
        ["--id", id] => Some(*id),
        _ => bail!(
            "unexpected arguments `{}`; use `cargo xtask runtime verify [--id <id>]`",
            args.join(" ")
        ),
    };
    let inventory = Inventory::load(root)?;
    let violations = inventory.violations(root);
    if !violations.is_empty() {
        for violation in &violations {
            eprintln!("{violation}");
        }
        bail!(
            "the inventory breaks {} rule(s); fix them first (`cargo xtask runtime check`)",
            violations.len()
        );
    }
    let artifacts: Vec<&Artifact> = match only {
        None => inventory.available().collect(),
        Some(id) => {
            let artifact = inventory
                .artifacts
                .iter()
                .find(|artifact| artifact.id.as_deref() == Some(id))
                .with_context(|| format!("the inventory has no artifact `{id}`"))?;
            if artifact.status.as_deref() != Some("available") {
                bail!("{id} is blocked, so there is nothing to download");
            }
            vec![artifact]
        }
    };
    if artifacts.is_empty() {
        println!("verify: the inventory has no available artifact");
        return Ok(ExitCode::SUCCESS);
    }

    let cache = Cache::open()?;
    let mut gpg = None;
    let mut failed = 0;
    for artifact in &artifacts {
        let id = artifact.id.as_deref().unwrap_or_default();
        let outcome = verify(root, &cache, artifact, &mut gpg)
            .unwrap_or_else(|err| Outcome::Fail(format!("{err:#}")));
        match &outcome {
            Outcome::Ok => println!("{id}: OK"),
            Outcome::SkipGpg => {
                println!("{id}: SKIP gpg (digest, size, and checksum match; gpg is not on PATH)")
            }
            Outcome::Fail(reason) => {
                failed += 1;
                println!("{id}: FAIL {reason}");
            }
        }
    }
    if failed > 0 {
        eprintln!("verify: {failed} of {} artifact(s) failed", artifacts.len());
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

/// Checks one artifact. `gpg` remembers whether `gpg` is on PATH once it
/// has been asked.
fn verify(
    root: &Path,
    cache: &Cache,
    artifact: &Artifact,
    gpg: &mut Option<bool>,
) -> Result<Outcome> {
    let sha256 = artifact
        .sha256
        .as_deref()
        .context("rule 3 guarantees sha256")?;
    let size = artifact
        .size
        .and_then(|size| u64::try_from(size).ok())
        .context("rule 3 guarantees a positive size")?;
    let urls = artifact.urls.as_deref().unwrap_or_default();
    let mut stored = None;
    for url in urls {
        eprintln!("verify: downloading {url}");
        let download = match cache.download(url, size) {
            Ok(download) => download,
            Err(err) if err.downcast_ref::<TooLarge>().is_some() => {
                return Ok(Outcome::Fail(format!(
                    "size mismatch: {url} is larger than the {size} bytes in the inventory"
                )));
            }
            Err(err) => return Err(err),
        };
        if download.size != size {
            return Ok(Outcome::Fail(format!(
                "size mismatch: {url} gave {} bytes, the inventory says {size}",
                download.size
            )));
        }
        if download.sha256 != sha256 {
            return Ok(Outcome::Fail(format!(
                "digest mismatch: {url} gave {}, the inventory says {sha256}",
                download.sha256
            )));
        }
        if stored.is_none() {
            stored = Some(cache.store(download)?);
        }
    }
    let stored = stored.context("rule 3 guarantees at least one URL")?;
    let names: Vec<&str> = urls.iter().map(|url| base_name(url)).collect();

    if let Some(checksum) = &artifact.upstream_checksum {
        let kind = checksum.kind.as_deref().unwrap_or("none");
        if kind != "none" {
            let url = checksum
                .url
                .as_deref()
                .context("rule 5 guarantees the checksum url")?;
            eprintln!("verify: downloading {url}");
            let file = cache.download(url, SIDE_FILE_MAX_SIZE)?;
            let text = std::fs::read_to_string(file.path())
                .with_context(|| format!("{url} is not a text file"))?;
            let expected = expected_digest(&parse_sums(&text), &names)
                .with_context(|| format!("cannot read the digest from {url}"))?;
            let actual = if kind == "sha512" {
                sha512_file(&stored)?
            } else {
                sha256.to_owned()
            };
            if expected != actual {
                return Ok(Outcome::Fail(format!(
                    "upstream checksum mismatch: {url} says {expected}, the download is {actual}"
                )));
            }
        }
    }

    if let Some(signature) = &artifact.upstream_signature
        && signature.kind.as_deref() == Some("gpg")
    {
        if !*gpg.get_or_insert_with(gpg_available) {
            return Ok(Outcome::SkipGpg);
        }
        let url = signature
            .url
            .as_deref()
            .context("rule 5 guarantees the signature url")?;
        let key = signature
            .key
            .as_deref()
            .context("rule 5 guarantees the key")?;
        let fingerprint = signature
            .fingerprint
            .as_deref()
            .context("rule 5 guarantees the fingerprint")?;
        eprintln!("verify: downloading {url}");
        let signature_file = cache.download(url, SIDE_FILE_MAX_SIZE)?;
        if let Err(err) = verify_gpg(&stored, signature_file.path(), &root.join(key), fingerprint) {
            return Ok(Outcome::Fail(format!("{err:#}")));
        }
    }
    Ok(Outcome::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root_with(inventory: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("runtimes")).unwrap();
        std::fs::write(root.path().join("runtimes/inventory.toml"), inventory).unwrap();
        root
    }

    #[test]
    fn an_empty_inventory_has_nothing_to_verify() {
        let root = root_with("schema_version = 1\n");
        assert_eq!(run(root.path(), &[]).unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn a_broken_inventory_is_refused_before_any_download() {
        let root = root_with("schema_version = 2\n");
        let err = run(root.path(), &[]).err().unwrap().to_string();
        assert!(err.starts_with("the inventory breaks 1 rule(s)"), "{err}");
    }

    #[test]
    fn an_unknown_or_blocked_id_is_refused() {
        let root = root_with(
            r#"schema_version = 1

[[artifact]]
id = "clickhouse-26.9.1.1-macos-aarch64"
provider = "clickhouse"
line = "26.9"
version = "26.9.1.1"
os = "macos"
arch = "aarch64"
status = "blocked"
blocked_reason = "no macOS asset"
blocked_owner = "@ojiepermana"
blocked_since = 2026-10-17
origin = "upstream"
distribution = "upstream_download"
signing_policy = "keep_or_adhoc"
upstream_checksum = { kind = "none" }
upstream_signature = { kind = "none" }
"#,
        );
        let err = run(root.path(), &["--id", "nats-2.15.1-macos-aarch64"])
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            "the inventory has no artifact `nats-2.15.1-macos-aarch64`"
        );
        let err = run(root.path(), &["--id", "clickhouse-26.9.1.1-macos-aarch64"])
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            "clickhouse-26.9.1.1-macos-aarch64 is blocked, so there is nothing to download"
        );
        assert!(run(root.path(), &["--id"]).is_err());
    }
}
