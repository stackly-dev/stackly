//! The benchmark baseline host `stackly.evidence-baseline/v1`.

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::rules::Violations;
use crate::{BASELINE_SCHEMA, BaselineMatch, Host, RuleViolations};

/// The fingerprint of the development Mac that runs the benchmarks, committed
/// as `evidence/baseline-host.json` by `cargo xtask evidence baseline`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct BaselineHost {
    /// Always [`BASELINE_SCHEMA`].
    pub schema: String,
    /// When the baseline was recorded.
    pub recorded_at: Timestamp,
    /// The same fields as `context.host`.
    pub host: Host,
    /// The volume that holds the temporary directory.
    pub storage: Storage,
}

/// The volume that holds test roots and benchmark data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    /// `MountPoint` from `diskutil info -plist`.
    pub volume: String,
    /// `FilesystemType` from `diskutil info -plist`.
    pub filesystem: String,
    /// `Internal` from `diskutil info -plist`.
    pub internal: bool,
    /// `SolidState` from `diskutil info -plist`.
    pub solid_state: bool,
}

impl BaselineHost {
    /// Creates the baseline of `host`, recorded at `recorded_at`.
    #[must_use]
    pub fn new(recorded_at: Timestamp, host: Host, storage: Storage) -> Self {
        Self {
            schema: BASELINE_SCHEMA.to_owned(),
            recorded_at,
            host,
            storage,
        }
    }

    /// Checks that the baseline names a real, non virtual Mac with every
    /// field the comparison needs.
    ///
    /// # Errors
    ///
    /// Returns every violated rule at once.
    pub fn validate(&self) -> Result<(), RuleViolations> {
        let host = &self.host;
        let mut v = Violations::default();
        v.check(self.schema == BASELINE_SCHEMA, || {
            format!(
                "schema must be `{BASELINE_SCHEMA}`, found `{}`",
                self.schema
            )
        });
        v.check(host.os == "macos", || {
            "the baseline host runs macOS".to_owned()
        });
        v.check(host.virtualized == Some(false), || {
            "the baseline host is a real Mac, not a VM".to_owned()
        });
        v.check(
            host.hardware_model.is_some()
                && host.chip.is_some()
                && host.memory_bytes.is_some()
                && host.performance_cores.is_some()
                && host.efficiency_cores.is_some()
                && host.os_build.is_some(),
            || "the baseline host needs model, chip, memory, core counts, and OS build".to_owned(),
        );
        v.into_result()
    }

    /// Compares a run's host with this baseline.
    #[must_use]
    pub fn compare(&self, host: &Host) -> BaselineMatch {
        let base = &self.host;
        let same = |a: Option<&str>, b: Option<&str>| a.is_some() && a == b;
        let matches = same(
            base.hardware_model.as_deref(),
            host.hardware_model.as_deref(),
        ) && same(base.chip.as_deref(), host.chip.as_deref())
            && base.memory_bytes.is_some()
            && base.memory_bytes == host.memory_bytes
            && base.performance_cores.is_some()
            && base.performance_cores == host.performance_cores
            && base.efficiency_cores.is_some()
            && base.efficiency_cores == host.efficiency_cores
            && base.virtualized.is_some()
            && base.virtualized == host.virtualized;
        BaselineMatch {
            matches,
            os_build_matches: same(base.os_build.as_deref(), host.os_build.as_deref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac() -> Host {
        Host {
            os: "macos".to_owned(),
            arch: "aarch64".to_owned(),
            os_version: Some("27.0".to_owned()),
            os_build: Some("26A428".to_owned()),
            hardware_model: Some("Mac13,1".to_owned()),
            chip: Some("Apple M1 Max".to_owned()),
            memory_bytes: Some(34_359_738_368),
            performance_cores: Some(8),
            efficiency_cores: Some(2),
            virtualized: Some(false),
        }
    }

    fn baseline() -> BaselineHost {
        BaselineHost {
            schema: BASELINE_SCHEMA.to_owned(),
            recorded_at: Timestamp::now(),
            host: mac(),
            storage: Storage {
                volume: "/".to_owned(),
                filesystem: "apfs".to_owned(),
                internal: true,
                solid_state: true,
            },
        }
    }

    #[test]
    fn a_complete_mac_baseline_is_valid() {
        assert!(baseline().validate().is_ok());
    }

    #[test]
    fn a_vm_cannot_be_the_baseline() {
        let mut vm = baseline();
        vm.host.virtualized = Some(true);
        assert!(vm.validate().is_err());
    }

    #[test]
    fn same_hardware_matches_and_os_build_is_tracked_apart() {
        let mut host = mac();
        host.os_build = Some("26A500".to_owned());
        let result = baseline().compare(&host);
        assert!(result.matches);
        assert!(!result.os_build_matches);
    }

    #[test]
    fn a_missing_field_never_matches() {
        let mut host = mac();
        host.performance_cores = None;
        assert!(!baseline().compare(&host).matches);
    }
}
