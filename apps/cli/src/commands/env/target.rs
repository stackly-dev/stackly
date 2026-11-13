//! An environment named on the command line: a whole UUID, or a name that
//! is matched exactly against `env.list` (spec 0044, *CLI*, AC-17).

use std::str::FromStr;

use stackly_protocol::env::{EnvironmentList, EnvironmentSummary};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use crate::context::Context;
use crate::failure::CliFailure;
use crate::session::CallSpec;

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

#[cfg(test)]
mod tests {
    use super::*;

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
