//! Observing signatures and applying the signing policy of an entry
//! (spec 0003, AC-8). Only the extracted copy is ever re-signed, always
//! every Mach-O file of it, and never with `sudo`.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

const CODESIGN: &str = "/usr/bin/codesign";

/// The signature `codesign -dv` reports for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    /// `developer_id`, `apple`, `certificate`, `adhoc`, `linker_signed`,
    /// or `unsigned`.
    pub kind: String,
    /// The leaf authority, for a certificate signature.
    pub authority: Option<String>,
    /// The team identifier, when set.
    pub team_id: Option<String>,
}

/// Reads the signature of `path`.
pub fn observe(path: &Path) -> Result<Observed> {
    let output = Command::new(CODESIGN)
        .args(["-dv", "--verbose=2"])
        .arg(path)
        .output()
        .with_context(|| format!("cannot run {CODESIGN}"))?;
    // codesign prints the details on stderr, and exits 1 when unsigned.
    Ok(parse_display(&String::from_utf8_lossy(&output.stderr)))
}

/// Reads `codesign -dv --verbose=2` output.
pub fn parse_display(text: &str) -> Observed {
    if text.contains("code object is not signed at all") {
        return Observed {
            kind: "unsigned".to_owned(),
            authority: None,
            team_id: None,
        };
    }
    let authority = text
        .lines()
        .find_map(|line| line.strip_prefix("Authority="))
        .map(str::to_owned);
    let team_id = text
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| *team != "not set")
        .map(str::to_owned);
    let flags = text
        .lines()
        .find(|line| line.starts_with("CodeDirectory "))
        .unwrap_or_default();
    let kind = match authority.as_deref() {
        Some(leaf) if leaf.starts_with("Developer ID Application") => "developer_id",
        Some(leaf) if leaf.contains("Software Signing") || leaf.starts_with("Apple") => "apple",
        Some(_) => "certificate",
        None if flags.contains("linker-signed") => "linker_signed",
        None => "adhoc",
    };
    Observed {
        kind: kind.to_owned(),
        authority,
        team_id,
    }
}

/// True when `codesign --verify --strict` accepts `path`.
pub fn verifies(path: &Path) -> Result<bool> {
    let status = Command::new(CODESIGN)
        .args(["--verify", "--strict"])
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("cannot run {CODESIGN}"))?;
    Ok(status.success())
}

/// Applies the entry's signing policy to every Mach-O file of the copy and
/// returns true when the spike signed them ad hoc.
///
/// `keep_or_adhoc`: keep the upstream signatures when every file
/// verifies, else sign every file ad hoc without `--preserve-metadata`, so
/// hardened runtime and library validation drop together.
/// `adhoc_pipeline`: the pipeline signed every file; one that does not
/// verify fails the step.
pub fn apply_policy(policy: &str, files: &[&Path]) -> Result<bool> {
    let mut failing = Vec::new();
    for file in files {
        if !verifies(file)? {
            failing.push(file.display().to_string());
        }
    }
    match policy {
        "keep_or_adhoc" if failing.is_empty() => Ok(false),
        "keep_or_adhoc" => {
            for file in files {
                sign_adhoc(file)?;
            }
            for file in files {
                if !verifies(file)? {
                    bail!("{} does not verify after ad hoc signing", file.display());
                }
            }
            Ok(true)
        }
        "adhoc_pipeline" if failing.is_empty() => Ok(false),
        "adhoc_pipeline" => bail!(
            "the pipeline left files that do not verify: {}",
            failing.join(", ")
        ),
        other => bail!("unknown signing_policy `{other}`"),
    }
}

fn sign_adhoc(path: &Path) -> Result<()> {
    let output = Command::new(CODESIGN)
        .args(["--force", "--sign", "-"])
        .arg(path)
        .output()
        .with_context(|| format!("cannot run {CODESIGN}"))?;
    if !output.status.success() {
        bail!(
            "codesign cannot sign {} ad hoc: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_kinds_come_from_authority_and_flags() {
        let go = "Executable=/x/nats-server\nIdentifier=a.out\nCodeDirectory v=20400 size=136990 flags=0x20002(adhoc,linker-signed) hashes=4278+0 location=embedded\nSignature=adhoc\nTeamIdentifier=not set\n";
        assert_eq!(
            parse_display(go),
            Observed {
                kind: "linker_signed".to_owned(),
                authority: None,
                team_id: None
            }
        );
        let adhoc = go.replace("(adhoc,linker-signed)", "(adhoc)");
        assert_eq!(parse_display(&adhoc).kind, "adhoc");
        let oracle = "CodeDirectory v=20500 size=1 flags=0x10000(runtime) hashes=1+7\nAuthority=Developer ID Application: Oracle America, Inc. (VB5E2TV963)\nAuthority=Developer ID Certification Authority\nAuthority=Apple Root CA\nTeamIdentifier=VB5E2TV963\n";
        assert_eq!(
            parse_display(oracle),
            Observed {
                kind: "developer_id".to_owned(),
                authority: Some(
                    "Developer ID Application: Oracle America, Inc. (VB5E2TV963)".to_owned()
                ),
                team_id: Some("VB5E2TV963".to_owned()),
            }
        );
        assert_eq!(
            parse_display("/x: code object is not signed at all\n").kind,
            "unsigned"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_unsigned_copy_is_signed_ad_hoc_and_then_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("true");
        std::fs::copy("/usr/bin/true", &copy).unwrap();
        let status = Command::new(CODESIGN)
            .args(["--remove-signature"])
            .arg(&copy)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(observe(&copy).unwrap().kind, "unsigned");
        assert!(apply_policy("keep_or_adhoc", &[copy.as_path()]).unwrap());
        assert_eq!(observe(&copy).unwrap().kind, "adhoc");
        assert!(!apply_policy("keep_or_adhoc", &[copy.as_path()]).unwrap());
    }
}
