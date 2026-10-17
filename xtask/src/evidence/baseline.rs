//! `cargo xtask evidence baseline`: records the Mac it runs on as the
//! benchmark baseline in `evidence/baseline-host.json` (spec 0002,
//! "Baseline benchmark").
//!
//! It refuses to run outside macOS and in a VM. The storage values describe
//! the volume that holds `std::env::temp_dir()`, where test roots and
//! benchmark data go.

use std::env;
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use stackly_evidence::{BaselineHost, Storage, write_json_atomic};

use super::context::{self, BASELINE_FILE};

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    if let Some(extra) = args.first() {
        bail!("`evidence baseline` takes no arguments, found `{extra}`");
    }
    if env::consts::OS != "macos" {
        bail!("the benchmark baseline is a Mac; run `cargo xtask evidence baseline` on it");
    }
    let host = context::probe_host();
    match host.virtualized {
        Some(false) => {}
        Some(true) => bail!("this is a VM (kern.hv_vmm_present is 1); the baseline is a real Mac"),
        None => bail!("cannot tell whether this is a VM; `sysctl -n kern.hv_vmm_present` failed"),
    }
    let storage = probe_storage(&env::temp_dir())?;
    let baseline = BaselineHost::new(Timestamp::now(), host, storage);
    baseline
        .validate()
        .context("this Mac cannot be recorded as the baseline")?;

    let path = root.join(BASELINE_FILE);
    let replaced = path.exists();
    write_json_atomic(&path, &baseline).with_context(|| format!("cannot write {BASELINE_FILE}"))?;
    println!("{BASELINE_FILE}");
    eprintln!(
        "evidence baseline: {} {BASELINE_FILE} for {}; commit it",
        if replaced { "replaced" } else { "wrote" },
        describe(&baseline),
    );
    Ok(ExitCode::SUCCESS)
}

/// One line naming the hardware, memory, macOS, and volume.
fn describe(baseline: &BaselineHost) -> String {
    let host = &baseline.host;
    let unknown = || "?".to_owned();
    let gib = host
        .memory_bytes
        .map_or_else(unknown, |bytes| (bytes / (1 << 30)).to_string());
    let cores = |count: Option<u32>| count.map_or_else(unknown, |n| n.to_string());
    format!(
        "{} ({}), {gib} GiB, {}P+{}E cores, macOS {} ({}), {} on {}",
        host.hardware_model.clone().unwrap_or_else(unknown),
        host.chip.clone().unwrap_or_else(unknown),
        cores(host.performance_cores),
        cores(host.efficiency_cores),
        host.os_version.clone().unwrap_or_else(unknown),
        host.os_build.clone().unwrap_or_else(unknown),
        baseline.storage.filesystem,
        baseline.storage.volume,
    )
}

/// Reads `MountPoint`, `FilesystemType`, `Internal`, and `SolidState` of the
/// volume that holds `dir`. `diskutil info` takes a device or a mount point,
/// not any path, so `stat -f %Sd` names the device first.
fn probe_storage(dir: &Path) -> Result<Storage> {
    let device = command("stat", &["-f", "%Sd"], Some(dir), None)?;
    let device = String::from_utf8_lossy(&device).trim().to_owned();
    let plist = command("diskutil", &["info", "-plist", &device], None, None)?;
    Ok(Storage {
        volume: plist_value(&plist, "MountPoint")?,
        filesystem: plist_value(&plist, "FilesystemType")?,
        internal: parse_bool("Internal", &plist_value(&plist, "Internal")?)?,
        solid_state: parse_bool("SolidState", &plist_value(&plist, "SolidState")?)?,
    })
}

/// One top level value of a property list, through `plutil -extract`.
fn plist_value(plist: &[u8], key: &str) -> Result<String> {
    let value = command(
        "plutil",
        &["-extract", key, "raw", "-o", "-", "-"],
        None,
        Some(plist),
    )
    .with_context(|| format!("`diskutil info -plist` has no {key}"))?;
    Ok(String::from_utf8_lossy(&value).trim().to_owned())
}

fn parse_bool(key: &str, value: &str) -> Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        other => bail!("{key} from `diskutil info -plist` is `{other}`, not true or false"),
    }
}

/// Runs a probe command and returns its stdout; a failure is an error,
/// because every baseline value is required.
fn command(
    program: &str,
    args: &[&str],
    path: Option<&Path>,
    stdin: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(path) = path {
        command.arg(path);
    }
    let mut child = command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot run `{program}`"))?;
    if let Some(bytes) = stdin {
        let mut pipe = child.stdin.take().context("stdin is piped above")?;
        pipe.write_all(bytes)
            .with_context(|| format!("cannot write to `{program}`"))?;
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("cannot wait for `{program}`"))?;
    if !output.status.success() {
        bail!(
            "`{program} {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diskutil_booleans_are_true_or_false() {
        assert!(parse_bool("Internal", "true").unwrap());
        assert!(!parse_bool("Internal", "false").unwrap());
        assert!(parse_bool("Internal", "yes").is_err());
    }

    #[test]
    fn the_summary_names_hardware_memory_and_volume() {
        let mut host = stackly_evidence::Host::new("macos", "aarch64");
        host.hardware_model = Some("Mac13,1".to_owned());
        host.chip = Some("Apple M1 Max".to_owned());
        host.memory_bytes = Some(34_359_738_368);
        host.performance_cores = Some(8);
        host.efficiency_cores = Some(2);
        host.os_version = Some("27.0".to_owned());
        host.os_build = Some("26A428".to_owned());
        let storage = Storage {
            volume: "/System/Volumes/Data".to_owned(),
            filesystem: "apfs".to_owned(),
            internal: true,
            solid_state: true,
        };
        let baseline = BaselineHost::new(Timestamp::now(), host, storage);
        assert_eq!(
            describe(&baseline),
            "Mac13,1 (Apple M1 Max), 32 GiB, 8P+2E cores, macOS 27.0 (26A428), apfs on /System/Volumes/Data"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn storage_of_the_temporary_folder_names_a_mounted_volume() {
        let storage = probe_storage(&env::temp_dir()).unwrap();
        assert!(storage.volume.starts_with('/'));
        assert!(!storage.filesystem.is_empty());
    }
}
