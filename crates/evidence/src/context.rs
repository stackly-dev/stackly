//! The context of one run: commit, host, and toolchain.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Kind;

/// What the wrapper knows about a run, computed once at its start.
///
/// `cargo xtask evidence run` writes it to `run.json` and copies it into
/// every record of `report.json`; `cargo xtask evidence new` fills it for a
/// manual record. Raw record files written by a test carry none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RunContext {
    /// The commit that was tested, from `git rev-parse HEAD`.
    pub source_commit: String,
    /// True when `git status --porcelain --untracked-files=normal` was not
    /// empty at the start of the run. A dirty run is never release evidence.
    pub source_dirty: bool,
    /// The run ID from the wrapper, or null for a manual record.
    #[serde(default)]
    pub run_id: Option<Uuid>,
    /// The kind of the run, or null for a manual record.
    #[serde(default)]
    pub run_kind: Option<Kind>,
    /// The argv the wrapper ran, or null for a manual record.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    /// The machine the run happened on.
    pub host: Host,
    /// How the host compares with `evidence/baseline-host.json`, or null
    /// while that file does not exist.
    #[serde(default)]
    pub baseline: Option<BaselineMatch>,
    /// Toolchain versions found on the host.
    pub toolchain: Toolchain,
}

impl RunContext {
    /// Creates the context of a run with the required values; the run ID,
    /// kind, command, and baseline start as null.
    #[must_use]
    pub fn new(
        source_commit: impl Into<String>,
        source_dirty: bool,
        host: Host,
        toolchain: Toolchain,
    ) -> Self {
        Self {
            source_commit: source_commit.into(),
            source_dirty,
            run_id: None,
            run_kind: None,
            command: None,
            host,
            baseline: None,
            toolchain,
        }
    }
}

/// The machine a run or a baseline was recorded on.
///
/// Holds no host name and no user name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Host {
    /// `std::env::consts::OS`, for example `macos` or `linux`.
    pub os: String,
    /// `std::env::consts::ARCH`, for example `aarch64`.
    pub arch: String,
    /// `sw_vers -productVersion` on macOS, `uname -r` on Linux.
    #[serde(default)]
    pub os_version: Option<String>,
    /// `sw_vers -buildVersion` on macOS, null elsewhere.
    #[serde(default)]
    pub os_build: Option<String>,
    /// `sysctl -n hw.model`, null outside macOS.
    #[serde(default)]
    pub hardware_model: Option<String>,
    /// `sysctl -n machdep.cpu.brand_string`, null outside macOS.
    #[serde(default)]
    pub chip: Option<String>,
    /// `sysctl -n hw.memsize`, null outside macOS.
    #[serde(default)]
    pub memory_bytes: Option<u64>,
    /// `sysctl -n hw.perflevel0.physicalcpu`, null when the key is missing.
    #[serde(default)]
    pub performance_cores: Option<u32>,
    /// `sysctl -n hw.perflevel1.physicalcpu`, null when the key is missing.
    #[serde(default)]
    pub efficiency_cores: Option<u32>,
    /// True when `sysctl -n kern.hv_vmm_present` is 1, null outside macOS.
    #[serde(default)]
    pub virtualized: Option<bool>,
}

impl Host {
    /// Creates a host with its OS and architecture; every probed value
    /// starts as null.
    #[must_use]
    pub fn new(os: impl Into<String>, arch: impl Into<String>) -> Self {
        Self {
            os: os.into(),
            arch: arch.into(),
            os_version: None,
            os_build: None,
            hardware_model: None,
            chip: None,
            memory_bytes: None,
            performance_cores: None,
            efficiency_cores: None,
            virtualized: None,
        }
    }
}

/// How a run's host compares with the recorded baseline host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BaselineMatch {
    /// True when model, chip, memory, core counts, and `virtualized` are all
    /// present and equal.
    pub matches: bool,
    /// True when the macOS build also matches, tracked apart because it
    /// changes more often than the hardware.
    pub os_build_matches: bool,
}

/// Toolchain versions found on the host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Toolchain {
    /// The second token of `rustc --version`, for example `1.99.1`.
    #[serde(default)]
    pub rustc: Option<String>,
    /// The two lines of `xcodebuild -version` joined, for example
    /// `27.0 (27A266a)`, or null without Xcode.
    #[serde(default)]
    pub xcode: Option<String>,
    /// The version number from `swift --version`, for example `6.4`, or
    /// null without Swift.
    #[serde(default)]
    pub swift: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_v1_context_that_still_holds_the_removed_ci_field_decodes() {
        let json = r#"{
            "sourceCommit": "4f3a818",
            "sourceDirty": false,
            "host": { "os": "macos", "arch": "aarch64" },
            "toolchain": {},
            "ci": null
        }"#;
        let context: RunContext = serde_json::from_str(json).unwrap();
        assert_eq!(context.source_commit, "4f3a818");
    }
}
