//! Probes the run context once, at the start of a run.
//!
//! `sourceCommit`, `sourceDirty`, `host.os`, and `host.arch` are required:
//! the run fails when one cannot be read. Every other value comes from one
//! command; when it fails, the value is null and one warning line is printed.

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use stackly_evidence::{BaselineHost, BaselineMatch, Host, Kind, RunContext, Toolchain};
use uuid::Uuid;

/// The baseline host file, relative to the repository root.
const BASELINE_FILE: &str = "evidence/baseline-host.json";

/// Computes the context of a run.
pub fn probe(root: &Path, run_id: Uuid, kind: Kind, command: &[String]) -> Result<RunContext> {
    let source_commit = git(root, &["rev-parse", "HEAD"])
        .context("cannot read the source commit with `git rev-parse HEAD`")?;
    let status = git(root, &["status", "--porcelain", "--untracked-files=normal"])
        .context("cannot read whether the checkout is dirty with `git status --porcelain`")?;
    let host = probe_host();
    let baseline = probe_baseline(root, &host);
    let mut context = RunContext::new(
        source_commit,
        !status.is_empty(),
        host,
        probe_toolchain(root),
    );
    context.run_id = Some(run_id);
    context.run_kind = Some(kind);
    context.command = Some(command.to_vec());
    context.baseline = baseline;
    Ok(context)
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .context("cannot run `git`")?;
    if !output.status.success() {
        bail!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned())
}

fn probe_host() -> Host {
    let mut host = Host::new(env::consts::OS, env::consts::ARCH);
    match env::consts::OS {
        "macos" => {
            host.os_version = command_output("the OS version", "sw_vers", &["-productVersion"]);
            host.os_build = command_output("the OS build", "sw_vers", &["-buildVersion"]);
            host.hardware_model = sysctl("hw.model");
            host.chip = sysctl("machdep.cpu.brand_string");
            host.memory_bytes = sysctl("hw.memsize").and_then(|value| number("hw.memsize", &value));
            host.performance_cores = sysctl("hw.perflevel0.physicalcpu")
                .and_then(|value| number("hw.perflevel0.physicalcpu", &value));
            host.efficiency_cores = sysctl("hw.perflevel1.physicalcpu")
                .and_then(|value| number("hw.perflevel1.physicalcpu", &value));
            host.virtualized = sysctl("kern.hv_vmm_present").map(|value| value == "1");
        }
        "linux" => host.os_version = command_output("the kernel version", "uname", &["-r"]),
        _ => {}
    }
    host
}

fn sysctl(key: &str) -> Option<String> {
    command_output(key, "sysctl", &["-n", key])
}

fn number<T: std::str::FromStr>(what: &str, value: &str) -> Option<T> {
    let parsed = value.trim().parse().ok();
    if parsed.is_none() {
        warn(what, &format!("`{value}` is not a number"));
    }
    parsed
}

fn probe_baseline(root: &Path, host: &Host) -> Option<BaselineMatch> {
    let path = root.join(BASELINE_FILE);
    if !path.exists() {
        return None;
    }
    let baseline = fs::read(&path)
        .map_err(|err| err.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<BaselineHost>(&bytes).map_err(|err| err.to_string())
        })
        .and_then(|baseline| {
            baseline
                .validate()
                .map(|()| baseline)
                .map_err(|err| err.to_string())
        });
    match baseline {
        Ok(baseline) => Some(baseline.compare(host)),
        Err(err) => {
            warn(BASELINE_FILE, &err);
            None
        }
    }
}

fn probe_toolchain(root: &Path) -> Toolchain {
    let rustc = Command::new("rustc")
        .arg("--version")
        // Run from the repository so rustup picks the pinned toolchain.
        .current_dir(root)
        .output();
    let rustc = match rustc {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let version = text.split_whitespace().nth(1).map(str::to_owned);
            if version.is_none() {
                warn("the rustc version", "unexpected `rustc --version` output");
            }
            version
        }
        Ok(output) => {
            warn(
                "the rustc version",
                &format!("`rustc` exited with {}", output.status),
            );
            None
        }
        Err(err) => {
            warn("the rustc version", &err.to_string());
            None
        }
    };
    let xcode = if env::consts::OS == "macos" {
        command_output("the Xcode version", "xcodebuild", &["-version"])
            .and_then(|text| parsed("the Xcode version", parse_xcode(&text)))
    } else {
        None
    };
    let swift = tool_output("the Swift version", "swift", &["--version"])
        .and_then(|text| parsed("the Swift version", parse_swift(&text)));
    let mut toolchain = Toolchain::default();
    toolchain.rustc = rustc;
    toolchain.xcode = xcode;
    toolchain.swift = swift;
    toolchain
}

/// Passes a parsed value through, warning when the output had an
/// unexpected shape.
fn parsed(what: &str, value: Option<String>) -> Option<String> {
    if value.is_none() {
        warn(what, "the command printed an unexpected format");
    }
    value
}

/// `Xcode 27.0` and `Build version 27A266a` become `27.0 (27A266a)`.
fn parse_xcode(text: &str) -> Option<String> {
    let mut lines = text.lines();
    let version = lines.next()?.trim().strip_prefix("Xcode ")?;
    let build = lines.next()?.trim().strip_prefix("Build version ")?;
    Some(format!("{version} ({build})"))
}

/// The token after `Swift version`, for example `6.4`.
fn parse_swift(text: &str) -> Option<String> {
    let (_, rest) = text.split_once("Swift version ")?;
    rest.split_whitespace().next().map(str::to_owned)
}

/// Runs one probe command. On failure, prints one warning and returns null.
fn command_output(what: &str, program: &str, args: &[&str]) -> Option<String> {
    run_probe(what, program, args, true)
}

/// Like [`command_output`], but a missing program means the tool is not installed,
/// which is expected and needs no warning.
fn tool_output(what: &str, program: &str, args: &[&str]) -> Option<String> {
    run_probe(what, program, args, false)
}

fn run_probe(what: &str, program: &str, args: &[&str], warn_if_missing: bool) -> Option<String> {
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if text.is_empty() {
                warn(what, &format!("`{program}` printed nothing"));
                return None;
            }
            Some(text)
        }
        Ok(output) => {
            warn(what, &format!("`{program}` exited with {}", output.status));
            None
        }
        Err(err) if err.kind() == ErrorKind::NotFound && !warn_if_missing => None,
        Err(err) => {
            warn(what, &format!("cannot run `{program}`: {err}"));
            None
        }
    }
}

fn warn(what: &str, why: &str) {
    eprintln!("warning: evidence context: cannot read {what}: {why}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xcode_version_joins_the_version_and_build_lines() {
        let text = "Xcode 27.0\nBuild version 27A266a\n";
        assert_eq!(parse_xcode(text).as_deref(), Some("27.0 (27A266a)"));
        assert_eq!(parse_xcode("Xcode 27.0"), None);
    }

    #[test]
    fn swift_version_is_the_token_after_swift_version() {
        let apple = "swift-driver version: 1.120 Apple Swift version 6.4 (swiftlang-6.4.0.1)\nTarget: arm64-apple-macosx27.0";
        assert_eq!(parse_swift(apple).as_deref(), Some("6.4"));
        let linux = "Swift version 6.1 (swift-6.1-RELEASE)";
        assert_eq!(parse_swift(linux).as_deref(), Some("6.1"));
        assert_eq!(parse_swift("no version here"), None);
    }
}
