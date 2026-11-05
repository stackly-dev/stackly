//! Signing from the inside out, and the proof that it worked (spec 0015,
//! steps 5 and 6). Hardened runtime, no entitlements, no `--deep` while
//! signing, no password prompt, and nothing written to a Keychain. Developer
//! ID, a secure timestamp, and notarization belong to feature 60.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use stackly_platform::bundle::{
    APP_BUNDLE_ID, CLI_EXECUTABLE, CLI_SIGNING_ID, DAEMON_EXECUTABLE, DAEMON_SIGNING_ID, MACOS_DIR,
};

use super::capture;

/// The variable that names the signing identity when `--identity` is not
/// given.
pub const IDENTITY_ENV: &str = "STACKLY_SIGN_IDENTITY";
/// The ad hoc identity.
const AD_HOC: &str = "-";

/// The identity to sign with: the flag, then `STACKLY_SIGN_IDENTITY`, then
/// ad hoc.
#[must_use]
pub fn identity(flag: Option<&str>) -> String {
    pick_identity(flag, std::env::var(IDENTITY_ENV).ok().as_deref())
}

fn pick_identity(flag: Option<&str>, env: Option<&str>) -> String {
    [flag, env]
        .into_iter()
        .flatten()
        .find(|value| !value.is_empty())
        .unwrap_or(AD_HOC)
        .to_owned()
}

/// Signs every Mach-O under `Contents/Frameworks`, then `stacklyd`, then
/// `stackly`, then the bundle.
///
/// # Errors
///
/// A `codesign` call that fails.
pub fn sign(bundle: &Path, identity: &str) -> Result<()> {
    for path in frameworks(bundle)? {
        codesign(identity, None, &path)?;
    }
    let macos = bundle.join(MACOS_DIR);
    codesign(
        identity,
        Some(DAEMON_SIGNING_ID),
        &macos.join(DAEMON_EXECUTABLE),
    )?;
    codesign(identity, Some(CLI_SIGNING_ID), &macos.join(CLI_EXECUTABLE))?;
    codesign(identity, None, bundle)
}

fn codesign(identity: &str, identifier: Option<&str>, path: &Path) -> Result<()> {
    let mut command = Command::new("codesign");
    command.args([
        "--force",
        "--sign",
        identity,
        "--options",
        "runtime",
        "--timestamp=none",
    ]);
    if let Some(identifier) = identifier {
        command.args(["--identifier", identifier]);
    }
    command.arg(path);
    capture(&mut command).with_context(|| format!("cannot sign {}", path.display()))?;
    Ok(())
}

/// Every Mach-O file under `Contents/Frameworks`, deepest first, so a
/// nested one is signed before the one around it.
fn frameworks(bundle: &Path) -> Result<Vec<PathBuf>> {
    let root = bundle.join("Contents/Frameworks");
    let mut found = Vec::new();
    if !root.is_dir() {
        return Ok(found);
    }
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let kind = std::fs::symlink_metadata(&path)?.file_type();
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && is_mach_o(&path)? {
                found.push(path);
            }
        }
    }
    found.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    Ok(found)
}

/// Whether the first four bytes of `path` are a Mach-O or fat magic.
fn is_mach_o(path: &Path) -> Result<bool> {
    let mut magic = [0_u8; 4];
    let mut file = std::fs::File::open(path)?;
    if file.read_exact(&mut magic).is_err() {
        return Ok(false);
    }
    Ok(matches!(
        u32::from_be_bytes(magic),
        0xfeed_facf | 0xcffa_edfe | 0xcafe_babe | 0xbebafeca
    ))
}

/// Checks the signature with `codesign --verify --strict --deep`, then
/// checks the bundle and the two executables against the requirement they
/// must meet (their expected identifier, and the Team ID of a Developer ID
/// `identity`), and prints, for each, the identifier, the Team ID (`ad hoc`
/// without one), and the designated requirement.
///
/// # Errors
///
/// A failed verification, or a signature that does not meet its
/// requirement.
pub fn verify(bundle: &Path, identity: &str) -> Result<()> {
    capture(
        Command::new("codesign")
            .args(["--verify", "--strict", "--deep"])
            .arg(bundle),
    )
    .with_context(|| format!("{} does not verify", bundle.display()))?;
    let macos = bundle.join(MACOS_DIR);
    let signed = [
        (macos.join(DAEMON_EXECUTABLE), DAEMON_SIGNING_ID),
        (macos.join(CLI_EXECUTABLE), CLI_SIGNING_ID),
        (bundle.to_path_buf(), APP_BUNDLE_ID),
    ];
    for (path, identifier) in &signed {
        meets(path, &requirement(identifier, identity))?;
    }
    for (path, _) in signed {
        let details = describe(&path)?;
        println!(
            "signed {}: identifier {}, team {}\n  {}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            details.identifier,
            details.team,
            details.requirement
        );
    }
    Ok(())
}

/// The requirement a file signed by `identity` must meet: its expected
/// `identifier` and, for a `Developer ID Application: <name> (<Team ID>)`
/// identity, an Apple issued leaf certificate of that Team ID. An ad hoc or
/// any other identity is held to the identifier only, since its certificate
/// subject does not name the Team ID reliably.
fn requirement(identifier: &str, identity: &str) -> String {
    let mut requirement = format!("identifier \"{identifier}\"");
    if let Some(team) = developer_id_team(identity) {
        requirement.push_str(&format!(
            " and anchor apple generic and certificate leaf[subject.OU] = \"{team}\""
        ));
    }
    requirement
}

/// The Team ID a `Developer ID Application: <name> (<Team ID>)` identity
/// names, when it is one.
fn developer_id_team(identity: &str) -> Option<&str> {
    let rest = identity.strip_prefix("Developer ID Application: ")?;
    let team = rest.strip_suffix(')')?.rsplit_once(" (")?.1;
    (team.len() == 10
        && team
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
    .then_some(team)
}

/// Checks that the signature of `path` satisfies `requirement`.
fn meets(path: &Path, requirement: &str) -> Result<()> {
    capture(
        Command::new("codesign")
            .args(["--verify", "--strict"])
            .arg(format!("-R={requirement}"))
            .arg(path),
    )
    .with_context(|| {
        format!(
            "{} does not meet the requirement `{requirement}`",
            path.display()
        )
    })?;
    Ok(())
}

/// What `codesign -d` says about a signed file.
#[derive(Debug, PartialEq, Eq)]
struct Details {
    identifier: String,
    team: String,
    requirement: String,
}

fn describe(path: &Path) -> Result<Details> {
    // `codesign -d` writes its report to stderr, and `-r-` its requirement
    // to stdout.
    let report = Command::new("codesign")
        .args(["-d", "--verbose=2"])
        .arg(path)
        .output()
        .context("cannot run codesign")?;
    if !report.status.success() {
        bail!("codesign -d failed for {}", path.display());
    }
    let requirement = capture(
        Command::new("codesign")
            .args(["-d", "-r-"])
            .arg(path)
            .stderr(std::process::Stdio::null()),
    )?;
    Ok(parse_details(
        &String::from_utf8_lossy(&report.stderr),
        &requirement,
    ))
}

fn parse_details(report: &str, requirement: &str) -> Details {
    let field = |name: &str| {
        report
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .map(str::to_owned)
    };
    let team = field("TeamIdentifier=")
        .filter(|team| team != "not set")
        .unwrap_or_else(|| "ad hoc".to_owned());
    let requirement = requirement
        .lines()
        .find_map(|line| line.trim_start_matches("# ").strip_prefix("designated => "))
        .unwrap_or("no designated requirement")
        .to_owned();
    Details {
        identifier: field("Identifier=").unwrap_or_default(),
        team,
        requirement,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_is_the_flag_then_the_variable_then_ad_hoc() {
        assert_eq!(
            pick_identity(Some("Apple Development: A"), Some("Other")),
            "Apple Development: A"
        );
        assert_eq!(pick_identity(None, Some("From Env")), "From Env");
        assert_eq!(pick_identity(Some(""), Some("From Env")), "From Env");
        assert_eq!(pick_identity(None, None), "-");
        assert_eq!(pick_identity(None, Some("")), "-");
    }

    #[test]
    fn an_ad_hoc_report_names_the_identifier_and_says_ad_hoc_for_the_team() {
        let report = "Executable=/x/stacklyd\nIdentifier=com.ojiepermana.stackly.daemon\nFormat=Mach-O thin (arm64)\nSignature=adhoc\nTeamIdentifier=not set\n";
        let details = parse_details(
            report,
            "Executable=/x/stacklyd\n# designated => cdhash H\"abc\"\n",
        );
        assert_eq!(details.identifier, "com.ojiepermana.stackly.daemon");
        assert_eq!(details.team, "ad hoc");
        assert_eq!(details.requirement, "cdhash H\"abc\"");
    }

    #[test]
    fn a_team_id_is_printed_as_it_is() {
        let details = parse_details("Identifier=x\nTeamIdentifier=ABCDE12345\n", "");
        assert_eq!(details.team, "ABCDE12345");
        assert_eq!(details.requirement, "no designated requirement");
    }

    #[test]
    fn an_ad_hoc_or_development_identity_is_held_to_the_identifier_only() {
        assert_eq!(
            requirement(DAEMON_SIGNING_ID, "-"),
            "identifier \"com.ojiepermana.stackly.daemon\""
        );
        assert_eq!(
            requirement(CLI_SIGNING_ID, "Apple Development: A Person (ABCDE12345)"),
            "identifier \"com.ojiepermana.stackly.cli\""
        );
    }

    #[test]
    fn a_developer_id_identity_also_requires_its_team_id() {
        assert_eq!(
            requirement(
                APP_BUNDLE_ID,
                "Developer ID Application: A Person (ABCDE12345)"
            ),
            "identifier \"com.ojiepermana.stackly\" and anchor apple generic and certificate leaf[subject.OU] = \"ABCDE12345\""
        );
        assert_eq!(
            developer_id_team("Developer ID Application: A (abcde12345)"),
            None
        );
        assert_eq!(
            developer_id_team("Developer ID Application: A (ABCDE1234)"),
            None
        );
        assert_eq!(developer_id_team("Developer ID Application: A"), None);
        assert_eq!(
            developer_id_team("Developer ID Installer: A (ABCDE12345)"),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_ad_hoc_signature_meets_only_its_own_identifier() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("stacklyd");
        std::fs::copy("/usr/bin/true", &tool).unwrap();
        codesign(AD_HOC, Some(DAEMON_SIGNING_ID), &tool).unwrap();

        meets(&tool, &requirement(DAEMON_SIGNING_ID, AD_HOC)).unwrap();
        let err = meets(&tool, &requirement(CLI_SIGNING_ID, AD_HOC))
            .err()
            .unwrap();
        assert!(
            err.to_string().ends_with(
                "does not meet the requirement `identifier \"com.ojiepermana.stackly.cli\"`"
            ),
            "{err}"
        );
        let team = requirement(
            DAEMON_SIGNING_ID,
            "Developer ID Application: A Person (ABCDE12345)",
        );
        assert!(meets(&tool, &team).is_err(), "ad hoc has no Team ID");
    }

    #[test]
    fn mach_o_files_are_found_by_magic_and_nested_ones_come_first() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Stackly.app");
        let frameworks = bundle.join("Contents/Frameworks/X.framework/Versions/A");
        std::fs::create_dir_all(&frameworks).unwrap();
        std::fs::write(frameworks.join("X"), [0xcf, 0xfa, 0xed, 0xfe, 0, 0]).unwrap();
        std::fs::write(
            bundle.join("Contents/Frameworks/libY.dylib"),
            [0xcf, 0xfa, 0xed, 0xfe],
        )
        .unwrap();
        std::fs::write(bundle.join("Contents/Frameworks/notes.txt"), b"text").unwrap();

        let found = frameworks_of(&bundle);

        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("Versions/A/X"), "{found:?}");
        assert!(found[1].ends_with("libY.dylib"));
    }

    fn frameworks_of(bundle: &Path) -> Vec<PathBuf> {
        frameworks(bundle).unwrap()
    }

    #[test]
    fn a_bundle_without_a_frameworks_folder_has_nothing_to_sign_first() {
        let dir = tempfile::tempdir().unwrap();
        assert!(frameworks(dir.path()).unwrap().is_empty());
    }
}
