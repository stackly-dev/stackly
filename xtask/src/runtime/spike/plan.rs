//! The plan the driver hands the worker, and the layout of a spike root.
//!
//! launchd does not pass the test's environment on, so everything the
//! worker needs travels in `plan.json`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::inventory::Artifact;

/// The `schema` value of `plan.json`.
pub const PLAN_SCHEMA: &str = "stackly.runtime-spike-plan/v1";
/// The folder inside the root that every artifact is extracted under. The
/// space and the `Ü` prove the runtime copes with such paths (AC-5).
const SPIKE_DIR: &str = "Stackly Spike Ü";

/// Everything the worker needs, written by the driver.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Always [`PLAN_SCHEMA`].
    pub schema: String,
    /// The run ID, also part of the root folder name.
    pub run_id: Uuid,
    /// The inventory entry under test.
    pub artifact: PlanArtifact,
    /// The cached download, already checked by the driver.
    pub cache_file: PathBuf,
    /// The spike root `<TMPDIR>/stackly-runtime-spike-<runId>/`.
    pub root: PathBuf,
    /// `gui/<uid>`.
    pub domain: String,
    /// Whether this run probes a quarantined copy (AC-9).
    pub quarantine: bool,
    /// The evidence folder that already holds the quarantine result for
    /// this digest, when `quarantine` is false.
    pub quarantine_recorded_in: Option<String>,
    /// How long the worker may take in total, in seconds.
    pub worker_timeout_secs: u64,
    /// Text every string in the result is cleaned of: `(from, to)`.
    pub redactions: Vec<Redaction>,
}

/// One replacement applied to every string of the spike result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Redaction {
    /// The text to replace, such as the root path.
    pub from: String,
    /// What it becomes, such as `$ROOT`.
    pub to: String,
    /// Replace only where `from` is not part of a longer word.
    pub whole_word: bool,
}

/// The inventory fields the spike reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanArtifact {
    /// The artifact ID.
    pub id: String,
    /// The provider, for example `nats`.
    pub provider: String,
    /// The release line.
    pub line: String,
    /// The exact version the running binary must report.
    pub version: String,
    /// `upstream` or `maintainer`.
    pub origin: String,
    /// `tar_gz`, `zip`, or `binary`.
    pub format: String,
    /// The single top folder of the archive, when it has one.
    pub archive_root: Option<String>,
    /// Paths inside the artifact root; the main executable first.
    pub executables: Vec<String>,
    /// `keep_or_adhoc` or `adhoc_pipeline`.
    pub signing_policy: String,
    /// The pinned digest.
    pub sha256: String,
    /// The pinned size in bytes.
    pub size: u64,
    /// The candidate download URLs.
    pub urls: Vec<String>,
    /// Every dependency the inventory lists.
    pub dependencies: Vec<PlanDependency>,
}

/// One `[[artifact.dependency]]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanDependency {
    /// The library file name.
    pub name: String,
    /// `bundled`, `own`, or `system`.
    pub kind: String,
    /// The install name as `otool -L` prints it.
    pub install_name: String,
}

impl PlanArtifact {
    /// Takes the fields the spike needs from an `available` entry that
    /// passed `runtime check`.
    pub fn from_inventory(artifact: &Artifact) -> Result<Self> {
        let text = |value: &Option<String>, field: &str| {
            value
                .clone()
                .filter(|value| !value.trim().is_empty())
                .with_context(|| format!("the inventory entry has no {field}"))
        };
        let size = artifact
            .size
            .and_then(|size| u64::try_from(size).ok())
            .filter(|size| *size > 0)
            .context("the inventory entry has no positive size")?;
        let executables = artifact.executables.clone().unwrap_or_default();
        if executables.is_empty() {
            bail!("the inventory entry lists no executables");
        }
        let dependencies = artifact
            .dependencies
            .iter()
            .map(|dependency| {
                Ok(PlanDependency {
                    name: text(&dependency.name, "dependency name")?,
                    kind: text(&dependency.kind, "dependency kind")?,
                    install_name: text(&dependency.install_name, "dependency install_name")?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            id: text(&artifact.id, "id")?,
            provider: text(&artifact.provider, "provider")?,
            line: text(&artifact.line, "line")?,
            version: text(&artifact.version, "version")?,
            origin: text(&artifact.origin, "origin")?,
            format: text(&artifact.format, "format")?,
            archive_root: artifact.archive_root.clone(),
            executables,
            signing_policy: text(&artifact.signing_policy, "signing_policy")?,
            sha256: text(&artifact.sha256, "sha256")?,
            size,
            urls: artifact.urls.clone().unwrap_or_default(),
            dependencies,
        })
    }

    /// The main executable, relative to the artifact root.
    pub fn main_executable(&self) -> &str {
        &self.executables[0]
    }
}

impl Plan {
    /// Reads `plan.json`.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        let plan: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("{} is not a spike plan", path.display()))?;
        if plan.schema != PLAN_SCHEMA {
            bail!(
                "{} has schema `{}`, not `{PLAN_SCHEMA}`",
                path.display(),
                plan.schema
            );
        }
        Ok(plan)
    }

    /// The layout of this plan's root.
    pub fn layout(&self) -> Layout {
        Layout::new(self.root.clone())
    }
}

/// Where everything lives inside one spike root.
#[derive(Debug, Clone)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    /// The layout of `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The root itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file that marks the root as owned by this user.
    pub fn owner_marker(&self) -> PathBuf {
        self.root.join(super::OWNER_MARKER)
    }

    /// `plan.json`.
    pub fn plan(&self) -> PathBuf {
        self.root.join("plan.json")
    }

    /// The LaunchAgent property list. It never goes to `~/Library`.
    pub fn plist(&self) -> PathBuf {
        self.root.join("agent.plist")
    }

    /// `result.json`, written atomically by the worker.
    pub fn result(&self) -> PathBuf {
        self.root.join("result.json")
    }

    /// The worker's stdout.
    pub fn worker_stdout(&self) -> PathBuf {
        self.root.join("worker.out.log")
    }

    /// The worker's stderr.
    pub fn worker_stderr(&self) -> PathBuf {
        self.root.join("worker.err.log")
    }

    /// `HOME` of the worker.
    pub fn agent_home(&self) -> PathBuf {
        self.root.join("agent-home")
    }

    /// `TMPDIR` of the worker.
    pub fn agent_tmp(&self) -> PathBuf {
        self.root.join("agent-tmp")
    }

    /// The folder with a space and a non ASCII letter in its name.
    pub fn spike_dir(&self) -> PathBuf {
        self.root.join(SPIKE_DIR)
    }

    /// The folder the artifact is extracted into.
    pub fn extract_dir(&self, id: &str) -> PathBuf {
        self.spike_dir().join(id)
    }

    /// The APFS clone that gets the quarantine attribute.
    pub fn quarantine_dir(&self, id: &str) -> PathBuf {
        self.spike_dir().join(format!("{id} quarantined"))
    }

    /// The run folder: working folder, data, configuration, and logs.
    pub fn run_dir(&self) -> PathBuf {
        self.spike_dir().join("run")
    }
}
