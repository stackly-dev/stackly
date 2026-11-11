//! What a validated package may run and hand to a process, read from its
//! checked manifest and templates, for the permission summary of an
//! imported package (spec 0037, *Ringkasan izin*). The summary itself
//! lives in `catalog::local::permissions`; this module only reaches the
//! checked manifest, whose parts are private to the provider engine.

use std::collections::{BTreeMap, BTreeSet};

use super::interpolate::{ExecRef, Interp, Token};
use super::manifest::{CommandSpec, InitStepSpec, ProbeDef, ReloadActionSpec, StdinSpec, StopSpec};
use super::open_with::OpenWithAction;
use super::validate::ValidatedPackage;

/// Where an executable is run, in the summary's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum UseSite {
    Start,
    Initialize,
    Stop,
    Reload,
    Probe,
    VersionCheck,
    OpenWith,
}

/// One executable: `runtime` or an alias, its path under the runtime root,
/// and where it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutableFact {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) uses: BTreeSet<UseSite>,
}

/// The name of the runtime executable in the summary.
pub(crate) const RUNTIME: &str = "runtime";

/// Returns `runtime.executable`, then each `additionalExecutables` alias in
/// alias order, with where the manifest runs it.
pub(crate) fn executables(package: &ValidatedPackage) -> Vec<ExecutableFact> {
    let manifest = &package.manifest;
    let mut uses: BTreeMap<String, BTreeSet<UseSite>> = BTreeMap::new();
    let mut note = |name: String, site: UseSite| {
        uses.entry(name).or_default().insert(site);
    };
    note(exec_name(&manifest.start.executable), UseSite::Start);
    for step in &manifest.initialize {
        if let InitStepSpec::RunExecutable { command, .. } = step {
            note(exec_name(&command.executable), UseSite::Initialize);
        }
    }
    if let StopSpec::Command { command, .. } = &manifest.stop {
        note(exec_name(&command.executable), UseSite::Stop);
    }
    if let Some(reload) = &manifest.reload
        && let ReloadActionSpec::Run { command, .. } = &reload.action
    {
        note(exec_name(&command.executable), UseSite::Reload);
    }
    for probe in [&manifest.readiness.probe, &manifest.health.probe] {
        if let ProbeDef::Exec { command, .. } = probe {
            note(exec_name(&command.executable), UseSite::Probe);
        }
    }
    if manifest.version_check.is_some() {
        note(RUNTIME.to_owned(), UseSite::VersionCheck);
    }
    for entry in &manifest.open_with {
        if let OpenWithAction::Terminal { executable, .. } = &entry.action {
            for name in interp_executables(executable) {
                note(name, UseSite::OpenWith);
            }
        }
    }
    let mut facts = vec![ExecutableFact {
        name: RUNTIME.to_owned(),
        path: manifest.executable.join("/"),
        uses: uses.remove(RUNTIME).unwrap_or_default(),
    }];
    for (alias, path) in &manifest.aliases {
        facts.push(ExecutableFact {
            name: alias.clone(),
            path: path.join("/"),
            uses: uses.remove(alias).unwrap_or_default(),
        });
    }
    facts
}

fn exec_name(executable: &ExecRef) -> String {
    match executable {
        ExecRef::Runtime => RUNTIME.to_owned(),
        ExecRef::Alias(alias) => alias.clone(),
    }
}

fn interp_executables(interp: &Interp) -> Vec<String> {
    interp
        .tokens()
        .filter_map(|token| match token {
            Token::RuntimeExecutable => Some(RUNTIME.to_owned()),
            Token::RuntimeAlias(alias) => Some(alias.clone()),
            _ => None,
        })
        .collect()
}

fn interp_secrets(interp: &Interp, into: &mut BTreeSet<String>) {
    for token in interp.tokens() {
        if let Token::Secret(name) = token {
            into.insert(name.to_string());
        }
    }
}

fn command_secrets(command: &CommandSpec, into: &mut BTreeSet<String>) {
    for arg in &command.args {
        interp_secrets(arg, into);
    }
    for (_, value) in &command.env {
        interp_secrets(value, into);
    }
}

/// Returns the names of the secret fields the package hands to a process:
/// through the arguments or environment of a command, the stdin of an
/// initialize step, a materialized secret file, or a rendered config file.
pub(crate) fn secret_fields(package: &ValidatedPackage) -> BTreeSet<String> {
    let manifest = &package.manifest;
    let mut secrets = BTreeSet::new();
    command_secrets(&manifest.start, &mut secrets);
    for step in &manifest.initialize {
        match step {
            InitStepSpec::RunExecutable { command, stdin, .. } => {
                command_secrets(command, &mut secrets);
                if let Some(StdinSpec::Secret(name)) = stdin {
                    secrets.insert(name.to_string());
                }
            }
            InitStepSpec::MaterializeSecret { secret, .. } => {
                secrets.insert(secret.to_string());
            }
            InitStepSpec::CreateManagedDirectories | InitStepSpec::RenderConfig(_) => {}
        }
    }
    if let StopSpec::Command { command, .. } = &manifest.stop {
        command_secrets(command, &mut secrets);
    }
    if let Some(reload) = &manifest.reload
        && let ReloadActionSpec::Run { command, .. } = &reload.action
    {
        command_secrets(command, &mut secrets);
    }
    for probe in [&manifest.readiness.probe, &manifest.health.probe] {
        if let ProbeDef::Exec { command, .. } = probe {
            command_secrets(command, &mut secrets);
        }
    }
    for spec in manifest.config_files.values() {
        if let Some(template) = package.templates.get(&spec.template) {
            for (name, _) in template.secrets() {
                secrets.insert(name.to_string());
            }
        }
    }
    secrets
}

/// Returns the target of an `openWith` entry, in manifest order: the first
/// bundle ID of an `app` entry, the executable of a `terminal` entry, and
/// none for a `browser` or `reveal` entry.
pub(crate) fn open_with_targets(package: &ValidatedPackage) -> Vec<Option<String>> {
    package
        .manifest
        .open_with
        .iter()
        .map(|entry| match &entry.action {
            OpenWithAction::App { bundle_ids, .. } => bundle_ids.first().cloned(),
            OpenWithAction::Terminal { executable, .. } => {
                interp_executables(executable).into_iter().next()
            }
            OpenWithAction::Browser { .. } | OpenWithAction::Reveal { .. } => None,
        })
        .collect()
}
