//! Lock keys: the names of what an operation holds while it runs (spec
//! 0007, "Lock dan antrian").

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use crate::error::{ValueError, ValueKind};
use crate::id::{EnvironmentId, InstanceId, RuntimeId};
use crate::operation::OperationResource;
use crate::value::{EnvironmentName, InstanceName, Sha256Digest};

/// One thing an operation locks while it runs. Two operations that share a
/// key never run at the same time.
///
/// The text form is `<kind>:<value>`: `runtime:<id>`, `instance:<id>`,
/// `environment:<id>`, `manager:self`, `artifact:<sha256>`,
/// `instance_name:<name>`, `environment_name:<name>`, and
/// `external_root:<path>`. The registry stores it in `operation_locks`
/// only to rebuild the lock table after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum LockKey {
    /// A runtime.
    Runtime(RuntimeId),
    /// An instance.
    Instance(InstanceId),
    /// An environment.
    Environment(EnvironmentId),
    /// Stackly itself.
    Manager,
    /// A downloaded artifact, by its digest.
    Artifact(Sha256Digest),
    /// An instance name, while an operation creates or renames to it.
    InstanceName(InstanceName),
    /// An environment name, while an operation creates or renames to it.
    EnvironmentName(EnvironmentName),
    /// The canonical folder of an external runtime. Build it with
    /// [`LockKey::external_root`], which checks the path.
    ExternalRoot(PathBuf),
}

impl LockKey {
    /// The longest external root path, in bytes of UTF-8.
    pub const EXTERNAL_ROOT_MAX: usize = 1024;

    /// Returns the key of the resource an operation works on.
    #[must_use]
    pub const fn for_resource(resource: OperationResource) -> Self {
        match resource {
            OperationResource::Runtime(id) => Self::Runtime(id),
            OperationResource::Instance(id) => Self::Instance(id),
            OperationResource::Environment(id) => Self::Environment(id),
            OperationResource::Manager => Self::Manager,
        }
    }

    /// Returns the key of an external runtime folder.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when `path` is not absolute, has a `.` or
    /// `..` component, is not UTF-8, or is longer than
    /// [`LockKey::EXTERNAL_ROOT_MAX`] bytes.
    pub fn external_root(path: impl Into<PathBuf>) -> Result<Self, ValueError> {
        let path = path.into();
        check_external_root(&path)?;
        Ok(Self::ExternalRoot(path))
    }

    /// Returns the kind part of the text form, for example `instance`.
    #[must_use]
    pub const fn kind_code(&self) -> &'static str {
        match self {
            Self::Runtime(_) => "runtime",
            Self::Instance(_) => "instance",
            Self::Environment(_) => "environment",
            Self::Manager => "manager",
            Self::Artifact(_) => "artifact",
            Self::InstanceName(_) => "instance_name",
            Self::EnvironmentName(_) => "environment_name",
            Self::ExternalRoot(_) => "external_root",
        }
    }

    /// Returns the text form, or `None` for an external root that is not
    /// UTF-8, which the registry refuses to store.
    #[must_use]
    pub fn to_text(&self) -> Option<String> {
        let value = match self {
            Self::Runtime(id) => id.to_string(),
            Self::Instance(id) => id.to_string(),
            Self::Environment(id) => id.to_string(),
            Self::Manager => "self".to_owned(),
            Self::Artifact(sha256) => sha256.to_string(),
            Self::InstanceName(name) => name.to_string(),
            Self::EnvironmentName(name) => name.to_string(),
            Self::ExternalRoot(path) => path.to_str()?.to_owned(),
        };
        Some(format!("{}:{value}", self.kind_code()))
    }
}

/// The text form; an external root that is not UTF-8 shows lossily.
impl fmt::Display for LockKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self, self.to_text()) {
            (_, Some(text)) => f.write_str(&text),
            (Self::ExternalRoot(path), None) => {
                write!(f, "{}:{}", self.kind_code(), path.display())
            }
            (_, None) => f.write_str(self.kind_code()),
        }
    }
}

impl FromStr for LockKey {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || {
            ValueError::new(
                ValueKind::LockKey,
                "must be <kind>:<value> with a known kind and a valid value",
            )
        };
        let (kind, value) = s.split_once(':').ok_or_else(invalid)?;
        let key = match kind {
            "runtime" => Self::Runtime(value.parse().map_err(|_| invalid())?),
            "instance" => Self::Instance(value.parse().map_err(|_| invalid())?),
            "environment" => Self::Environment(value.parse().map_err(|_| invalid())?),
            "manager" if value == "self" => Self::Manager,
            "artifact" => Self::Artifact(value.parse()?),
            "instance_name" => Self::InstanceName(value.parse()?),
            "environment_name" => Self::EnvironmentName(value.parse()?),
            "external_root" => Self::external_root(value)?,
            _ => return Err(invalid()),
        };
        Ok(key)
    }
}

fn check_external_root(path: &Path) -> Result<(), ValueError> {
    let valid = path.is_absolute()
        && path.to_str().is_some_and(|text| {
            text.len() <= LockKey::EXTERNAL_ROOT_MAX
                && !text
                    .split('/')
                    .any(|segment| segment == "." || segment == "..")
        })
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)));
    if valid {
        Ok(())
    } else {
        Err(ValueError::new(
            ValueKind::LockKey,
            "an external root must be an absolute UTF-8 path of at most 1024 bytes without . or ..",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lock_key_round_trips_through_its_text() {
        let keys = [
            LockKey::Runtime(RuntimeId::generate()),
            LockKey::Instance(InstanceId::generate()),
            LockKey::Environment(EnvironmentId::generate()),
            LockKey::Manager,
            LockKey::Artifact(
                "da578821d15e9c0f59eb8e4672962c6f032074e802684576debc6dfc7f86815f"
                    .parse()
                    .unwrap(),
            ),
            LockKey::InstanceName("orders-db".parse().unwrap()),
            LockKey::EnvironmentName("shop".parse().unwrap()),
            LockKey::external_root("/Users/me/pg/17").unwrap(),
        ];

        for key in keys {
            let text = key.to_text().unwrap();
            assert_eq!(text.parse::<LockKey>().unwrap(), key, "{text}");
            assert_eq!(key.to_string(), text);
            assert!(text.starts_with(&format!("{}:", key.kind_code())));
        }
        assert_eq!(LockKey::Manager.to_text().as_deref(), Some("manager:self"));
    }

    #[test]
    fn a_resource_names_its_own_lock_key() {
        let id = InstanceId::generate();

        assert_eq!(
            LockKey::for_resource(OperationResource::Instance(id)),
            LockKey::Instance(id)
        );
        assert_eq!(
            LockKey::for_resource(OperationResource::Manager),
            LockKey::Manager
        );
    }

    #[test]
    fn invalid_lock_key_text_is_rejected() {
        for text in [
            "",
            "runtime",
            "runtime:not-a-uuid",
            "manager:other",
            "unknown:x",
            "external_root:relative/path",
            "external_root:/a/../b",
            "instance_name:Orders",
        ] {
            assert!(text.parse::<LockKey>().is_err(), "{text:?}");
        }
        let long = format!("/{}", "a".repeat(LockKey::EXTERNAL_ROOT_MAX));
        assert!(LockKey::external_root(long).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn an_external_root_that_is_not_utf8_has_no_text() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let key = LockKey::ExternalRoot(PathBuf::from(OsStr::from_bytes(b"/opt/\xff")));

        assert_eq!(key.to_text(), None);
        assert!(LockKey::external_root(PathBuf::from(OsStr::from_bytes(b"/opt/\xff"))).is_err());
    }
}
