//! An environment named on the command line: a whole UUID, or a name that
//! is matched exactly against `env.list` (spec 0044, *CLI*, AC-17); and the
//! target of `plan`, `up`, and `status`, which may also be a path (spec
//! 0046, *CLI*, *Target*, AC-17).

use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::Args;
use stackly_protocol::env::{EnvSource, EnvironmentList, EnvironmentSummary};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use super::DEFAULT_FILE;
use crate::commands::path::absolute;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::session::CallSpec;

/// The manifest name a folder given as the target stands for.
pub const MANIFEST_NAME: &str = "stackly.yaml";

/// What the command line named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentTarget {
    /// A 36 character UUID, in either case.
    Id(Uuid),
    /// Any other argument, matched against `name` exactly.
    Name(String),
}

impl std::fmt::Display for EnvironmentTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id(id) => write!(f, "{id}"),
            Self::Name(name) => f.write_str(name),
        }
    }
}

impl FromStr for EnvironmentTarget {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        if text.is_empty() {
            return Err("expected an environment name or id".to_owned());
        }
        if text.len() == 36
            && let Ok(id) = Uuid::try_parse(text)
        {
            return Ok(Self::Id(id));
        }
        Ok(Self::Name(text.to_owned()))
    }
}

impl EnvironmentTarget {
    /// The summary in `items` this target names.
    ///
    /// # Errors
    ///
    /// `cli.environment_not_found` when no item has the ID or the name.
    pub fn pick(&self, items: Vec<EnvironmentSummary>) -> Result<EnvironmentSummary, CliFailure> {
        items
            .into_iter()
            .find(|item| match self {
                Self::Id(id) => item.id == *id,
                Self::Name(name) => item.name == *name,
            })
            .ok_or_else(|| CliFailure::environment_not_found(&self.to_string()))
    }

    /// The summary this target names, read from `env.list`.
    ///
    /// # Errors
    ///
    /// `cli.environment_not_found`, or the daemon's error.
    pub async fn resolve(&self, cx: &mut Context) -> Result<EnvironmentSummary, CliFailure> {
        self.pick(list(cx).await?.items)
    }
}

/// Every registered environment, from `env.list`.
///
/// # Errors
///
/// The daemon's error.
pub async fn list(cx: &mut Context) -> Result<EnvironmentList, CliFailure> {
    match cx.call(Call::EnvList, CallSpec::control()).await? {
        Reply::EnvList(list) => Ok(list),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// A positional target of `plan`, `up`, or `status`: a path or an
/// environment (spec 0046, *Target*). An argument is a path when it holds
/// a `/`, starts with `.` or `~`, or ends in `.yaml` or `.yml`; an
/// environment name never holds a `/` or a `.`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestTarget {
    /// A file, or a folder that stands for its `stackly.yaml`.
    Path(PathBuf),
    /// A registered environment.
    Environment(EnvironmentTarget),
}

impl FromStr for ManifestTarget {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        if looks_like_path(text) {
            return Ok(Self::Path(PathBuf::from(text)));
        }
        text.parse().map(Self::Environment)
    }
}

/// Whether a positional argument names a path (spec 0046, *Target*).
#[must_use]
pub fn looks_like_path(text: &str) -> bool {
    text.contains('/')
        || text.starts_with('.')
        || text.starts_with('~')
        || text.ends_with(".yaml")
        || text.ends_with(".yml")
}

/// `--file <path>` or one positional target, never both; neither means
/// `./stackly.yaml`.
#[derive(Debug, Clone, Args)]
pub struct TargetArgs {
    /// The manifest; a relative path starts at the current folder
    /// [default: ./stackly.yaml]
    #[arg(long, value_name = "PATH", conflicts_with = "target")]
    pub file: Option<PathBuf>,
    /// A registered environment by name or whole ID, or a path: one with a
    /// /, starting with . or ~, or ending in .yaml or .yml; a folder stands
    /// for its stackly.yaml
    #[arg(value_name = "ENVIRONMENT|PATH")]
    pub target: Option<ManifestTarget>,
}

/// Where the manifest of a target is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// The absolute path of a file.
    File(String),
    /// A registered environment, read from `env.list`.
    Registered(Box<EnvironmentSummary>),
}

impl Resolved {
    /// The source `env.plan` and `env.up` take.
    #[must_use]
    pub fn source(&self) -> EnvSource {
        match self {
            Self::File(path) => EnvSource::File(path.clone()),
            Self::Registered(summary) => EnvSource::Registered(summary.id),
        }
    }
}

impl TargetArgs {
    /// The absolute manifest path, when the target is a file: `--file` as
    /// given, a positional path with `~` expanded and a folder standing for
    /// its `stackly.yaml`, or `./stackly.yaml`. `None` for an environment.
    ///
    /// # Errors
    ///
    /// `cli.usage` when the current folder or the home folder cannot be
    /// read.
    pub fn file(&self) -> Result<Option<String>, CliFailure> {
        let path = match (&self.file, &self.target) {
            (Some(file), _) => absolute(file)?,
            (None, Some(ManifestTarget::Path(path))) => {
                let path = absolute(&expand_home(path)?)?;
                if path.is_dir() {
                    path.join(MANIFEST_NAME)
                } else {
                    path
                }
            }
            (None, Some(ManifestTarget::Environment(_))) => return Ok(None),
            (None, None) => absolute(Path::new(DEFAULT_FILE))?,
        };
        Ok(Some(path.display().to_string()))
    }

    /// The manifest the target names: a file, or a registered environment
    /// looked up in `env.list`.
    ///
    /// # Errors
    ///
    /// `cli.usage` as for [`Self::file`], `cli.environment_not_found`, or
    /// the daemon's error.
    pub async fn resolve(&self, cx: &mut Context) -> Result<Resolved, CliFailure> {
        match &self.target {
            Some(ManifestTarget::Environment(target)) if self.file.is_none() => target
                .resolve(cx)
                .await
                .map(|summary| Resolved::Registered(Box::new(summary))),
            _ => self
                .file()?
                .map(Resolved::File)
                .ok_or_else(|| CliFailure::unexpected_reply(None)),
        }
    }
}

/// `path` with a leading `~` or `~/` written as the home folder.
fn expand_home(path: &Path) -> Result<PathBuf, CliFailure> {
    let Some(text) = path.to_str() else {
        return Ok(path.to_path_buf());
    };
    let rest = match text.strip_prefix('~') {
        Some("") => "",
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/'),
        _ => return Ok(path.to_path_buf()),
    };
    let home = std::env::home_dir()
        .filter(|home| !home.as_os_str().is_empty())
        .ok_or_else(|| CliFailure::usage("invalid_value", "cannot read the home folder"))?;
    Ok(home.join(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slash_a_leading_dot_or_tilde_or_a_yaml_ending_makes_a_path() {
        for text in [
            "~/Code/erp",
            "~",
            "./stackly.yaml",
            ".",
            "..",
            "../erp",
            "erp/stackly.yaml",
            "/Users/dev/erp",
            "stackly.yaml",
            "erp.yml",
        ] {
            assert_eq!(
                text.parse::<ManifestTarget>().unwrap(),
                ManifestTarget::Path(PathBuf::from(text)),
                "{text}"
            );
        }
        assert_eq!(
            "erp-development".parse::<ManifestTarget>().unwrap(),
            ManifestTarget::Environment(EnvironmentTarget::Name("erp-development".to_owned()))
        );
    }

    #[test]
    fn a_folder_stands_for_its_stackly_yaml_and_a_file_is_kept() {
        let dir = std::env::temp_dir().join(format!("stackly-target-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("other.yaml");
        std::fs::write(&file, "x").unwrap();
        let args = |target: &Path| TargetArgs {
            file: None,
            target: Some(ManifestTarget::Path(target.to_path_buf())),
        };
        assert_eq!(
            args(&dir).file().unwrap(),
            Some(dir.join(MANIFEST_NAME).display().to_string())
        );
        assert_eq!(
            args(&file).file().unwrap(),
            Some(file.display().to_string())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_leading_tilde_is_the_home_folder() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(
            expand_home(Path::new("~/Code/erp")).unwrap(),
            home.join("Code/erp")
        );
        assert_eq!(expand_home(Path::new("~")).unwrap(), home.join(""));
        assert_eq!(
            expand_home(Path::new("~other/x")).unwrap(),
            PathBuf::from("~other/x")
        );
    }

    #[test]
    fn a_whole_uuid_is_an_id_and_anything_else_is_a_name() {
        let id = "0192F3A7-0000-7000-8000-0000000000E1";
        assert_eq!(
            id.parse::<EnvironmentTarget>().unwrap(),
            EnvironmentTarget::Id(Uuid::try_parse(id).unwrap())
        );
        assert_eq!(
            "erp-development".parse::<EnvironmentTarget>().unwrap(),
            EnvironmentTarget::Name("erp-development".to_owned())
        );
        assert_eq!(
            "0192f3a7-0000-7000-8000-0000000000e"
                .parse::<EnvironmentTarget>()
                .unwrap(),
            EnvironmentTarget::Name("0192f3a7-0000-7000-8000-0000000000e".to_owned())
        );
        assert!("".parse::<EnvironmentTarget>().is_err());
    }
}
