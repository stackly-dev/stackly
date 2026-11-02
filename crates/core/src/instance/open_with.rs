//! Rendering the Open In actions of an instance (spec 0070, AC-2): the
//! `openWith` block of the pinned package, rendered with the same
//! interpolation engine and `PlanContext` as the connection strings of spec
//! 0021, so host, port, socket, and runtime path are the ones the process
//! uses.
//!
//! The result never holds a secret value: `copySecret` is carried as the
//! field and its reference, and the GUI asks `secret.reveal` for the value
//! when the user picks the action. Nothing here reads the Keychain or the
//! filesystem.

use std::path::PathBuf;

use stackly_domain::{Instance, InstanceLifecycle, SecretFieldName, SecretRef};

use crate::instance::connection::ConnectionUnavailable;
use crate::provider::{
    OpenWithKind, PlanContext, RenderFailure, RenderedOpenEntry, RenderedOpenValue,
    ValidatedPackage, render_open_entries,
};

/// One `openWith` entry of the package, rendered for one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderedOpenAction {
    /// The entry name from the manifest.
    pub name: String,
    /// The title the label is built from.
    pub title: String,
    /// The kind of the entry.
    pub kind: OpenWithKind,
    /// The rendered action, or why there is none.
    pub state: OpenActionState,
    /// The secret the GUI copies before it opens the action: the field and
    /// the reference `secret.reveal` takes, never the value. Set only while
    /// the action is available.
    pub copy_secret: Option<(SecretFieldName, SecretRef)>,
}

/// Whether an entry could be rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenActionState {
    /// The action is ready to open.
    Available(OpenTarget),
    /// The action cannot be opened now, for the same reasons a connection
    /// string cannot be built (spec 0021).
    Unavailable(ConnectionUnavailable),
}

/// What an available action opens.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenTarget {
    /// A URL opened with the first installed app of `bundle_ids`.
    App {
        /// The bundle IDs in priority order.
        bundle_ids: Vec<String>,
        /// The URL, every token value percent encoded.
        url: String,
    },
    /// A runtime executable the Terminal script runs.
    Terminal(OpenCommand),
    /// An `http` or `https` URL opened in the default browser.
    Browser {
        /// The URL, every token value percent encoded.
        url: String,
    },
    /// A config file shown in Finder.
    Reveal {
        /// The absolute path of the rendered file.
        path: PathBuf,
    },
}

/// The command of a `terminal` action, every value inserted as is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenCommand {
    /// The absolute path of the executable in the instance's runtime.
    pub executable: String,
    /// The arguments.
    pub args: Vec<String>,
    /// The env pairs from the manifest, in name order.
    pub env: Vec<(String, String)>,
}

/// Renders every `openWith` entry of `package` for `instance`, in manifest
/// order.
///
/// `context` is the plan context of
/// [`connection::render`](crate::instance::connection::render) and
/// `secret_refs` the secret uses of the instance. The state is chosen in the order of spec 0021, AC-2:
/// `Retained` when the instance is retained (every kind, `reveal`
/// included); else `PackageUnavailable` when `package` is not the pinned
/// revision, does not fit `context`, or the instance has no use of the
/// `copySecret` field; else `PortNotAssigned` when an endpoint the entry
/// names has no port; else available.
#[must_use]
pub fn render(
    package: &ValidatedPackage,
    context: &PlanContext,
    instance: &Instance,
    secret_refs: &[(SecretFieldName, SecretRef)],
) -> Vec<RenderedOpenAction> {
    let retained = instance.lifecycle() == InstanceLifecycle::Retained;
    let pinned = package.key() == instance.provider_revision();
    render_open_entries(package.manifest(), context)
        .into_iter()
        .map(|entry| {
            let (state, copy_secret) = if retained {
                unavailable(ConnectionUnavailable::Retained)
            } else if !pinned {
                unavailable(ConnectionUnavailable::PackageUnavailable)
            } else {
                state_of(&entry, secret_refs)
            };
            RenderedOpenAction {
                name: entry.name,
                title: entry.title,
                kind: entry.kind,
                state,
                copy_secret,
            }
        })
        .collect()
}

/// An unavailable state, which carries no secret reference.
const fn unavailable(
    reason: ConnectionUnavailable,
) -> (OpenActionState, Option<(SecretFieldName, SecretRef)>) {
    (OpenActionState::Unavailable(reason), None)
}

/// The state of one entry of a pinned package of a live instance.
fn state_of(
    entry: &RenderedOpenEntry,
    secret_refs: &[(SecretFieldName, SecretRef)],
) -> (OpenActionState, Option<(SecretFieldName, SecretRef)>) {
    let value = match &entry.value {
        Ok(value) => value,
        Err(RenderFailure::PortNotAssigned) => {
            return unavailable(ConnectionUnavailable::PortNotAssigned);
        }
        Err(RenderFailure::Unresolvable) => {
            return unavailable(ConnectionUnavailable::PackageUnavailable);
        }
    };
    let copy_secret = match &entry.copy_secret {
        None => None,
        Some(field) => {
            let Some(found) = secret_refs
                .iter()
                .find(|(name, _)| name.as_str() == field.as_str())
            else {
                return unavailable(ConnectionUnavailable::PackageUnavailable);
            };
            Some(found.clone())
        }
    };
    let target = match value {
        RenderedOpenValue::App { bundle_ids, url } => OpenTarget::App {
            bundle_ids: bundle_ids.clone(),
            url: url.clone(),
        },
        RenderedOpenValue::Terminal {
            executable,
            args,
            env,
        } => OpenTarget::Terminal(OpenCommand {
            executable: executable.clone(),
            args: args.clone(),
            env: env.clone(),
        }),
        RenderedOpenValue::Browser { url } => OpenTarget::Browser { url: url.clone() },
        RenderedOpenValue::Reveal { path } => OpenTarget::Reveal { path: path.clone() },
    };
    (OpenActionState::Available(target), copy_secret)
}
