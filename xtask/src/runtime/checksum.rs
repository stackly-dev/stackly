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
        if !verify.status.success() {
            bail!("gpg signature does not verify with key {expected}");
        }
        check_detached_status(&status, &expected)
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

/// The signature class of a detached signature over a binary file.
const BINARY_SIGNATURE_CLASS: &str = "00";

/// Checks the `--status-fd` output of `gpg --verify <sig> <file>` for one
/// detached signature by `expected`: exactly one `VALIDSIG` line, of
/// signature class `00`, whose signing key or primary key fingerprint is
/// `expected`, and no `PLAINTEXT` line. A `PLAINTEXT` line means the
/// signature file carries its own literal data (an inline or clearsigned
/// message), and gpg then checks that data instead of the downloaded file.
fn check_detached_status(status: &str, expected: &str) -> Result<()> {
    let mut valid = Vec::new();
    for line in status.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("[GNUPG:]") {
            continue;
        }
        match fields.next() {
            Some("PLAINTEXT") => bail!(
                "the gpg signature is not detached: it carries its own signed data, so it says nothing about the download"
            ),
            Some("VALIDSIG") => valid.push(fields.collect::<Vec<&str>>()),
            _ => {}
        }
    }
    let [fields] = valid.as_slice() else {
        bail!(
            "gpg reports {} valid signatures, expected exactly one by key {expected}",
            valid.len()
        );
    };
    let class = fields.get(8).copied().unwrap_or_default();
    if class != BINARY_SIGNATURE_CLASS {
        bail!(
            "the gpg signature has class `{class}`, expected `{BINARY_SIGNATURE_CLASS}` (a detached signature over a binary file)"
        );
    }
    let signed = [fields.first(), fields.get(9)]
        .into_iter()
        .flatten()
        .any(|fpr| fpr.eq_ignore_ascii_case(expected));
    if !signed {
        bail!("gpg signature does not verify with key {expected}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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

    const PRIMARY: &str = "A4A9406876FCBD3C456770C88C718D3B5072E1F5";
    const SUBKEY: &str = "1111111111111111111111111111111111111111";

    /// The `VALIDSIG` line gpg prints for a good signature of `class`.
    fn validsig(class: &str) -> String {
        format!("[GNUPG:] VALIDSIG {SUBKEY} 2026-10-01 1790000000 0 4 0 1 8 {class} {PRIMARY}\n")
    }

    /// The status output of a good detached signature.
    fn detached() -> String {
        format!(
            "[GNUPG:] NEWSIG\n[GNUPG:] GOODSIG 8C718D3B5072E1F5 MySQL Release Engineering\n{}",
            validsig("00")
        )
    }

    #[test]
    fn validsig_matches_the_signing_or_the_primary_fingerprint() {
        check_detached_status(&detached(), PRIMARY).unwrap();
        check_detached_status(&detached(), SUBKEY).unwrap();
        let err = check_detached_status(&detached(), &"2".repeat(40))
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            format!("gpg signature does not verify with key {}", "2".repeat(40))
        );
    }

    #[test]
    fn a_status_without_validsig_is_refused() {
        let err = check_detached_status("[GNUPG:] BADSIG 8C718D3B5072E1F5 x\n", PRIMARY)
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            format!("gpg reports 0 valid signatures, expected exactly one by key {PRIMARY}")
        );
    }

    #[test]
    fn a_signature_that_carries_its_own_data_is_refused() {
        // `gpg --verify` of an inline signed message: the literal data
        // packet shows as PLAINTEXT, and VALIDSIG covers that data, not the
        // downloaded file.
        let inline = format!(
            "[GNUPG:] PLAINTEXT 62 1790000000 other.tar.gz\n[GNUPG:] PLAINTEXT_LENGTH 5\n{}",
            detached()
        );
        let err = check_detached_status(&inline, PRIMARY).err().unwrap();
        assert!(
            err.to_string()
                .starts_with("the gpg signature is not detached"),
            "{err}"
        );
    }

    #[test]
    fn a_clearsigned_text_signature_is_refused_by_its_class() {
        let clearsigned = format!("[GNUPG:] NEWSIG\n{}", validsig("01"));
        let err = check_detached_status(&clearsigned, PRIMARY).err().unwrap();
        assert_eq!(
            err.to_string(),
            "the gpg signature has class `01`, expected `00` (a detached signature over a binary file)"
        );
    }

    #[test]
    fn more_than_one_valid_signature_is_refused() {
        let twice = format!("{}{}", detached(), detached());
        let err = check_detached_status(&twice, PRIMARY).err().unwrap();
        assert_eq!(
            err.to_string(),
            format!("gpg reports 2 valid signatures, expected exactly one by key {PRIMARY}")
        );
    }

    /// A throwaway signing key: its GPG home, its exported public key, and
    /// its fingerprint. `None` when `gpg` is not on PATH.
    fn signing_key(dir: &Path) -> Option<(PathBuf, PathBuf, String)> {
        if !gpg_available() {
            eprintln!("gpg is not on PATH; the real gpg check is not run");
            return None;
        }
        let home = dir.join("signer");
        std::fs::create_dir(&home).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let gpg = |args: &[&str]| {
            let output = Command::new("gpg")
                .arg("--batch")
                .arg("--homedir")
                .arg(&home)
                .args(args)
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "gpg {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        gpg(&[
            "--passphrase",
            "",
            "--quick-gen-key",
            "Stackly Test <test@stackly.invalid>",
            "ed25519",
            "sign",
            "never",
        ]);
        let listing = String::from_utf8(gpg(&["--with-colons", "--list-keys"])).unwrap();
        let fingerprint = listing
            .lines()
            .find_map(|line| line.strip_prefix("fpr:"))
            .and_then(|rest| rest.split(':').find(|field| field.len() == 40))
            .unwrap()
            .to_owned();
        let key = dir.join("key.asc");
        std::fs::write(&key, gpg(&["--armor", "--export", &fingerprint])).unwrap();
        Some((home, key, fingerprint))
    }

    /// Signs `file` with the key in `home`, with `mode` (`--detach-sign`,
    /// `--sign`, or `--clearsign`), into `out`.
    fn sign(home: &Path, mode: &str, file: &Path, out: &Path) {
        let status = Command::new("gpg")
            .arg("--batch")
            .arg("--homedir")
            .arg(home)
            .args(["--armor", mode, "--output"])
            .arg(out)
            .arg(file)
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn only_a_detached_signature_over_the_file_verifies_with_real_gpg() {
        let dir = tempfile::tempdir().unwrap();
        let Some((home, key, fingerprint)) = signing_key(dir.path()) else {
            return;
        };
        let file = dir.path().join("artifact.tar.gz");
        std::fs::write(&file, b"the real artifact").unwrap();
        let other = dir.path().join("other.tar.gz");
        std::fs::write(&other, b"something else entirely").unwrap();

        let detached = dir.path().join("detached.asc");
        sign(&home, "--detach-sign", &file, &detached);
        verify_gpg(&file, &detached, &key, &fingerprint).unwrap();
        assert!(verify_gpg(&other, &detached, &key, &fingerprint).is_err());

        // An inline signed message over other bytes must not pass as a
        // signature of the artifact.
        let inline = dir.path().join("inline.asc");
        sign(&home, "--sign", &other, &inline);
        assert!(verify_gpg(&file, &inline, &key, &fingerprint).is_err());
        let clearsigned = dir.path().join("clearsigned.asc");
        sign(&home, "--clearsign", &other, &clearsigned);
        assert!(verify_gpg(&file, &clearsigned, &key, &fingerprint).is_err());

        let _ = Command::new("gpgconf")
            .arg("--homedir")
            .arg(&home)
            .args(["--kill", "all"])
            .status();
    }
}
