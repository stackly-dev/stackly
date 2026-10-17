//! Upstream checksum files and GPG signatures, the second layer of trust
//! behind the pinned digest (spec 0003, *Security model*).

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use super::inventory::normalize_fingerprint;

/// One line of a checksum file: a hex digest and, when given, a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SumLine {
    /// The digest in lower case.
    pub hash: String,
    /// The file name, without the `*` binary marker.
    pub name: Option<String>,
}

/// Parses `SHA256SUMS`, `checksums.txt`, `.sha256`, and `.sha512` files:
/// `<hex>  <name>`, `<hex> *<name>`, or a bare `<hex>`. Blank lines,
/// comments, and lines that do not start with a hex digest are skipped.
pub fn parse_sums(text: &str) -> Vec<SumLine> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (hash, rest) = line
                .split_once(char::is_whitespace)
                .map_or((line, ""), |(hash, rest)| (hash, rest.trim()));
            let is_digest = (hash.len() == 64 || hash.len() == 128)
                && hash.bytes().all(|b| b.is_ascii_hexdigit());
            is_digest.then(|| SumLine {
                hash: hash.to_ascii_lowercase(),
                name: Some(rest.trim_start_matches('*'))
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// The digest a checksum file gives for one of `names` (the file names of
/// the candidate URLs). A file with a single bare digest applies to any
/// name.
pub fn expected_digest(lines: &[SumLine], names: &[&str]) -> Result<String> {
    if let Some(line) = lines.iter().find(|line| {
        line.name
            .as_deref()
            .is_some_and(|name| names.contains(&base_name(name)))
    }) {
        return Ok(line.hash.clone());
    }
    match lines {
        [SumLine { hash, name: None }] => Ok(hash.clone()),
        _ => bail!("the checksum file has no line for {}", names.join(" or ")),
    }
}

/// The last path segment of a URL or path, without query or fragment.
pub fn base_name(url: &str) -> &str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/').next().unwrap_or(path)
}

/// True when `gpg` runs from PATH.
pub fn gpg_available() -> bool {
    Command::new("gpg")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Checks a detached signature with only `key` in a throwaway keyring, and
/// requires that the signing key (or its primary key) has `fingerprint`.
pub fn verify_gpg(file: &Path, signature: &Path, key: &Path, fingerprint: &str) -> Result<()> {
    let expected = normalize_fingerprint(fingerprint)
        .with_context(|| format!("fingerprint `{fingerprint}` is not 40 or 64 hex characters"))?;
    let home = tempfile::tempdir().context("cannot create a throwaway GPG home")?;
    let gpg = |args: &[&std::ffi::OsStr]| {
        Command::new("gpg")
            .arg("--batch")
            .arg("--homedir")
            .arg(home.path())
            .args(args)
            .stdin(Stdio::null())
            .output()
            .context("cannot run gpg")
    };
    let result = (|| {
        let import = gpg(&["--import".as_ref(), key.as_os_str()])?;
        if !import.status.success() {
            bail!(
                "gpg cannot import {}: {}",
                key.display(),
                String::from_utf8_lossy(&import.stderr).trim()
            );
        }
        let verify = gpg(&[
            "--status-fd".as_ref(),
            "1".as_ref(),
            "--verify".as_ref(),
            signature.as_os_str(),
            file.as_os_str(),
        ])?;
        let status = String::from_utf8_lossy(&verify.stdout);
        if !verify.status.success() || !signed_by(&status, &expected) {
            bail!("gpg signature does not verify with key {expected}");
        }
        Ok(())
    })();
    // gpg may have started helpers (keyboxd, gpg-agent) in the throwaway
    // home; stop them before the folder goes. Best effort.
    let _ = Command::new("gpgconf")
        .arg("--homedir")
        .arg(home.path())
        .args(["--kill", "all"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    result
}

/// True when the `--status-fd` output has a `VALIDSIG` line whose signing
/// key or primary key fingerprint is `expected`.
fn signed_by(status: &str, expected: &str) -> bool {
    status.lines().any(|line| {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("[GNUPG:]") || fields.next() != Some("VALIDSIG") {
            return false;
        }
        let fields: Vec<&str> = fields.collect();
        let signing = fields.first().copied();
        let primary = fields.get(9).copied();
        [signing, primary]
            .into_iter()
            .flatten()
            .any(|fpr| fpr.eq_ignore_ascii_case(expected))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn sums_files_list_one_digest_per_file_name() {
        let text = format!(
            "# release checksums\n{A}  nats-server-v2.15.1-darwin-arm64.tar.gz\n\n{}  *nats-server-v2.15.1-linux-amd64.tar.gz\nnot a digest line\n",
            B.to_ascii_uppercase()
        );
        let lines = parse_sums(&text);
        assert_eq!(
            lines,
            [
                SumLine {
                    hash: A.to_owned(),
                    name: Some("nats-server-v2.15.1-darwin-arm64.tar.gz".to_owned()),
                },
                SumLine {
                    hash: B.to_owned(),
                    name: Some("nats-server-v2.15.1-linux-amd64.tar.gz".to_owned()),
                },
            ]
        );
        assert_eq!(
            expected_digest(&lines, &["nats-server-v2.15.1-linux-amd64.tar.gz"]).unwrap(),
            B
        );
    }

    #[test]
    fn a_missing_file_name_is_an_error_not_a_match() {
        let lines = parse_sums(&format!("{A}  a.tar.gz\n{B}  b.tar.gz\n"));
        let err = expected_digest(&lines, &["c.tar.gz"]).err().unwrap();
        assert_eq!(
            err.to_string(),
            "the checksum file has no line for c.tar.gz"
        );
    }

    #[test]
    fn a_single_bare_digest_applies_to_any_name() {
        let lines = parse_sums(&format!("{A}\n"));
        assert_eq!(expected_digest(&lines, &["x.tar.gz"]).unwrap(), A);
    }

    #[test]
    fn names_with_folders_match_on_the_file_name() {
        let lines = parse_sums(&format!("{A}  ./dist/x.tar.gz\n"));
        assert_eq!(expected_digest(&lines, &["x.tar.gz"]).unwrap(), A);
    }

    #[test]
    fn base_name_drops_the_path_query_and_fragment() {
        assert_eq!(
            base_name("https://example.org/r/v1/x.tar.gz?raw=1#top"),
            "x.tar.gz"
        );
        assert_eq!(base_name("x.tar.gz"), "x.tar.gz");
    }

    #[test]
    fn validsig_matches_the_signing_or_the_primary_fingerprint() {
        let primary = "A4A9406876FCBD3C456770C88C718D3B5072E1F5";
        let subkey = "1111111111111111111111111111111111111111";
        let status = format!(
            "[GNUPG:] NEWSIG\n[GNUPG:] GOODSIG 8C718D3B5072E1F5 MySQL Release Engineering\n\
             [GNUPG:] VALIDSIG {subkey} 2026-10-01 1790000000 0 4 0 1 8 00 {primary}\n"
        );
        assert!(signed_by(&status, primary));
        assert!(signed_by(&status, subkey));
        assert!(!signed_by(&status, &"2".repeat(40)));
        assert!(!signed_by("[GNUPG:] BADSIG 8C718D3B5072E1F5 x\n", primary));
    }
}
