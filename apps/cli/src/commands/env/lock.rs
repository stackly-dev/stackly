//! `stackly env lock [--file <path> | <environment>] [--update]
//! [--service <alias>]...` (spec 0045, *CLI*, AC-16): asks the daemon's
//! `env.lock` to write `stackly.lock` next to the manifest. Without
//! `--update` every entry that is still good is kept, `--update` alone
//! picks every entry again, and `--update` with `--service` picks only
//! those aliases again. One line per service goes to stderr, then
//! "Locked <n> services in <file>." or "<file> is up to date." to stdout.
//! A manifest that is not valid exits 2 with `cli.env_invalid`, blockers
//! exit 3 with `cli.env_lock_blocked`, an unknown alias exits 10 with the
//! daemon's `env.service_not_found`; the lock result is the result each
//! time.

use std::path::PathBuf;

use clap::Args;
use serde::Serialize;
use stackly_protocol::env::{
    EnvLockParams, EnvLockResult, EnvSource, LockChange, LockRefresh, LockedService,
};
use stackly_protocol::{Call, Reply};

use super::plan::file_source;
use super::render::{blocker_lines, counts, findings};
use super::target::EnvironmentTarget;
use crate::commands::RunCommand;
use crate::commands::path::home_masked;
use crate::commands::provider::local::{esc, shows_text, to_stderr};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render, wire_name};
use crate::session::CallSpec;

/// `stackly env lock`.
#[derive(Debug, Args)]
pub struct LockArgs {
    /// The manifest; a relative path starts at the current folder
    /// [default: ./stackly.yaml]
    #[arg(long, value_name = "PATH", conflicts_with = "environment")]
    pub file: Option<PathBuf>,
    /// A registered environment, by name or whole ID; its manifest is locked
    #[arg(value_name = "ENVIRONMENT")]
    pub environment: Option<EnvironmentTarget>,
    /// Pick versions again instead of keeping the ones already locked
    #[arg(long)]
    pub update: bool,
    /// With --update, pick only this service again; repeat for more
    #[arg(long = "service", value_name = "ALIAS", requires = "update")]
    pub services: Vec<String>,
}

impl LockArgs {
    /// What `env.lock` picks again.
    fn refresh(&self) -> LockRefresh {
        match (self.update, self.services.is_empty()) {
            (false, _) => LockRefresh::None,
            (true, true) => LockRefresh::All,
            (true, false) => LockRefresh::Services(self.services.clone()),
        }
    }
}

/// The `env lock` result: the daemon's lock result as JSON, and one line
/// about the file in text.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Locked(pub EnvLockResult);

impl Render for Locked {
    fn render(&self, out: &mut HumanWriter) {
        let result = &self.0;
        if !result.validation.valid || !result.blockers.is_empty() {
            return;
        }
        let file = esc(&home_masked(
            &result.lock_file,
            std::env::home_dir().as_deref(),
        ));
        if result.changed {
            let locked = result
                .services
                .iter()
                .filter(|service| service.change != LockChange::Removed)
                .count();
            out.line(&format!("Locked {locked} services in {file}."));
        } else {
            out.line(&format!("{file} is up to date."));
        }
    }
}

/// `<change> <alias> <provider> <version>`, the version of an `updated`
/// service as `<old> -> <new>` and of a `removed` one as the version it
/// had.
#[must_use]
pub fn service_line(service: &LockedService) -> String {
    let version = match (service.change, &service.version, &service.previous_version) {
        (LockChange::Updated, Some(new), Some(old)) => format!("{} -> {}", esc(old), esc(new)),
        (_, Some(version), _) | (LockChange::Removed, None, Some(version)) => esc(version),
        _ => "-".to_owned(),
    };
    format!(
        "{} {} {} {version}",
        wire_name(&service.change),
        esc(&service.alias),
        esc(&service.provider)
    )
}

/// `env.lock` for `source` with `refresh`.
///
/// # Errors
///
/// The daemon's error, such as `env.service_not_found` or
/// `env.lock_unwritable`.
pub async fn lock(
    cx: &mut Context,
    source: EnvSource,
    refresh: LockRefresh,
) -> Result<EnvLockResult, CliFailure> {
    let call = Call::EnvLock(EnvLockParams { source, refresh });
    match cx.call(call, CallSpec::control()).await? {
        Reply::EnvLock(result) => Ok(*result),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

impl RunCommand for LockArgs {
    type Output = Locked;

    async fn run(self, cx: &mut Context) -> CommandOutput<Locked> {
        let refresh = self.refresh();
        let source = match self.environment {
            Some(target) => match target.resolve(cx).await {
                Ok(summary) => EnvSource::Registered(summary.id),
                Err(failure) => return CommandOutput::failed(failure),
            },
            None => match file_source(self.file) {
                Ok(source) => source,
                Err(failure) => return CommandOutput::failed(failure),
            },
        };
        let result = match lock(cx, source, refresh).await {
            Ok(result) => result,
            Err(failure) => return CommandOutput::failed(failure),
        };
        if shows_text(cx, false) {
            to_stderr(cx, |out| {
                findings(out, &result.validation);
                if result.validation.valid && result.blockers.is_empty() {
                    for service in &result.services {
                        out.line(&service_line(service));
                    }
                }
                blocker_lines(out, &result.blockers);
            });
        }
        if !result.validation.valid {
            let (errors, warnings) = counts(&result.validation);
            return CommandOutput::partial(
                Locked(result),
                CliFailure::env_invalid(errors, warnings),
            );
        }
        if !result.blockers.is_empty() {
            let blockers = result.blockers.len() as u64;
            return CommandOutput::partial(Locked(result), CliFailure::env_lock_blocked(blockers));
        }
        CommandOutput::ok(Locked(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(change: LockChange, version: Option<&str>, previous: Option<&str>) -> LockedService {
        LockedService {
            alias: "postgres".to_owned(),
            provider: "official/postgresql".to_owned(),
            selector: "18".to_owned(),
            version: version.map(str::to_owned),
            artifact_id: None,
            change,
            previous_version: previous.map(str::to_owned),
        }
    }

    #[test]
    fn a_service_line_shows_the_change_and_the_version_it_ends_with() {
        assert_eq!(
            service_line(&service(LockChange::Added, Some("18.6"), None)),
            "added postgres official/postgresql 18.6"
        );
        assert_eq!(
            service_line(&service(LockChange::Updated, Some("18.6"), Some("18.5"))),
            "updated postgres official/postgresql 18.5 -> 18.6"
        );
        assert_eq!(
            service_line(&service(LockChange::Removed, None, Some("18.5"))),
            "removed postgres official/postgresql 18.5"
        );
        assert_eq!(
            service_line(&service(LockChange::Kept, None, None)),
            "kept postgres official/postgresql -"
        );
    }

    #[test]
    fn update_picks_everything_or_the_named_services_and_nothing_without_it() {
        let args = |update: bool, services: &[&str]| LockArgs {
            file: None,
            environment: None,
            update,
            services: services.iter().map(|alias| (*alias).to_owned()).collect(),
        };
        assert_eq!(args(false, &[]).refresh(), LockRefresh::None);
        assert_eq!(args(true, &[]).refresh(), LockRefresh::All);
        assert_eq!(
            args(true, &["db", "cache"]).refresh(),
            LockRefresh::Services(vec!["db".to_owned(), "cache".to_owned()])
        );
    }
}
