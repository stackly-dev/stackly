//! An instance named on the command line: a whole UUID, or a name that is
//! looked up in every page of `instance.list` (spec 0022, *CLI*).

use std::str::FromStr;

use clap::Args;
use stackly_protocol::instance::{InstanceSummary, ListInstancesParams};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use crate::context::Context;
use crate::failure::CliFailure;
use crate::session::CallSpec;

/// The page size of every `instance.list` the CLI sends: the most the
/// contract allows, so the number of instances stays a few calls.
const PAGE_LIMIT: u32 = 200;

/// What the command line named: a whole UUID in either case, or else a
/// name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceTarget {
    /// A 36 character hyphenated UUID, lower cased.
    Id(Uuid),
    /// Any other argument, matched against `name` exactly.
    Name(String),
}

impl FromStr for InstanceTarget {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        if text.is_empty() {
            return Err("expected an instance name or id".to_owned());
        }
        if text.len() == 36
            && let Ok(id) = Uuid::try_parse(text)
        {
            return Ok(Self::Id(id));
        }
        Ok(Self::Name(text.to_owned()))
    }
}

/// An instance argument that takes a name or an ID.
#[derive(Debug, Args)]
pub struct TargetArg {
    /// The instance name, or its whole ID
    #[arg(value_name = "INSTANCE")]
    pub target: InstanceTarget,
}

impl InstanceTarget {
    /// The ID of the instance: the one that was given, or the one whose
    /// `name` is the same.
    ///
    /// # Errors
    ///
    /// Returns `cli.instance_not_found` when no instance has the name, and
    /// the daemon's error when a page cannot be read.
    pub async fn resolve(&self, cx: &mut Context) -> Result<Uuid, CliFailure> {
        match self {
            Self::Id(id) => Ok(*id),
            Self::Name(name) => all_instances(cx)
                .await?
                .into_iter()
                .find(|instance| instance.name == *name)
                .map(|instance| instance.id)
                .ok_or_else(|| CliFailure::instance_not_found(name)),
        }
    }
}

impl InstanceTarget {
    /// The ID of the instance and its name when the lookup already showed
    /// it: a name gives both, an ID gives only the ID.
    ///
    /// # Errors
    ///
    /// The same as [`resolve`](Self::resolve).
    pub async fn resolve_named(
        &self,
        cx: &mut Context,
    ) -> Result<(Uuid, Option<String>), CliFailure> {
        match self {
            Self::Id(id) => Ok((*id, None)),
            Self::Name(name) => all_instances(cx)
                .await?
                .into_iter()
                .find(|instance| instance.name == *name)
                .map(|instance| (instance.id, Some(instance.name)))
                .ok_or_else(|| CliFailure::instance_not_found(name)),
        }
    }
}

/// Every instance the daemon lists, page after page until `next` is null.
///
/// # Errors
///
/// Returns the error of the page that failed, with nothing from the pages
/// before it.
pub async fn all_instances(cx: &mut Context) -> Result<Vec<InstanceSummary>, CliFailure> {
    let mut items = Vec::new();
    let mut before: Option<String> = None;
    loop {
        let params = ListInstancesParams {
            limit: Some(PAGE_LIMIT),
            before: before.clone(),
        };
        let page = match cx
            .call(Call::InstanceList(params), CallSpec::control())
            .await?
        {
            Reply::InstanceList(page) => *page,
            _ => return Err(CliFailure::unexpected_reply(None)),
        };
        items.extend(page.items);
        match page.next {
            // A cursor that does not move would ask for the same page for
            // ever.
            Some(next) if before.as_deref() != Some(next.as_str()) => before = Some(next),
            _ => return Ok(items),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0192f3a5-0000-7000-8000-000000000001";

    #[test]
    fn a_whole_uuid_in_either_case_is_an_id_and_anything_else_is_a_name() {
        let lower: InstanceTarget = ID.parse().unwrap();
        let upper: InstanceTarget = ID.to_uppercase().parse().unwrap();
        assert_eq!(lower, upper);
        assert_eq!(lower, InstanceTarget::Id(Uuid::try_parse(ID).unwrap()));
        for text in [
            "orders-db",
            "0192f3a5",
            "0192f3a5000070008000000000000001",
            "0192f3a5-0000-7000-8000-00000000000g",
        ] {
            assert_eq!(
                text.parse::<InstanceTarget>(),
                Ok(InstanceTarget::Name(text.to_owned())),
                "{text}"
            );
        }
        assert!("".parse::<InstanceTarget>().is_err());
    }
}
