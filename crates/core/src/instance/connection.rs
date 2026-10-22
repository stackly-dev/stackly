//! Rendering the connection strings of an instance (spec 0021, AC-2): the
//! `connection` block of the pinned package, rendered with the same
//! interpolation engine and `PlanContext` as a plan, so host, port, and
//! socket are the ones the process binds.
//!
//! The result never holds a secret value. A secret in a template is a
//! [`ConnectionPart::Secret`] with its field and its reference, and the
//! client joins the string once the user asks for the value
//! (`stackly_protocol::instance::connection::compose`). Nothing here reads
//! the Keychain or the filesystem.

use stackly_domain::{
    Endpoint, Instance, InstanceLifecycle, PortRequest, SecretFieldName, SecretRef,
};

use crate::provider::{
    PlanContext, RenderFailure, RenderedEntry, RenderedPiece, ValidatedPackage, render_entries,
};

/// What a masked secret looks like, always four stars.
const MASK: &str = "****";

/// One `connection` entry of the package, rendered for one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderedConnection {
    /// The entry name from the manifest.
    pub name: String,
    /// The label the GUI shows.
    pub title: String,
    /// The string, or why there is none.
    pub state: ConnectionState,
    /// True when an endpoint the entry names asks for an `Auto` port and
    /// the instance is not running, so the port may differ the next time
    /// it starts.
    pub port_may_change: bool,
}

/// Whether an entry could be rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionState {
    /// The string is ready except for its secrets.
    Available {
        /// The string with every secret as four stars.
        masked: String,
        /// The string in order: encoded text and secret references.
        parts: Vec<ConnectionPart>,
    },
    /// The string cannot be built now.
    Unavailable(ConnectionUnavailable),
}

/// One piece of an available connection string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionPart {
    /// Text that is already percent encoded. Two texts never follow each
    /// other.
    Text(String),
    /// A secret the client fetches: the settings field and the reference to
    /// read, never the value.
    Secret {
        /// The secret settings field.
        field: SecretFieldName,
        /// The reference `secret.reveal` takes.
        reference: SecretRef,
    },
}

/// Why an entry has no string, in the order the reasons are tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConnectionUnavailable {
    /// The instance is retained.
    Retained,
    /// The pinned package is not the one the instance was created with, or
    /// the instance does not fit it (for example a secret without a use).
    PackageUnavailable,
    /// An endpoint the entry names has no effective port yet.
    PortNotAssigned,
}

/// Renders every `connection` entry of `package` for `instance`, in manifest
/// order.
///
/// `context` is the plan context the daemon builds for a start, with the
/// effective port of every endpoint; settings are not read. `secret_refs`
/// are the secret uses of the instance. An entry is `Retained` when the
/// instance is retained; else `PackageUnavailable` when `package` is not the
/// pinned revision of `instance` (the digest differs) or does not fit
/// `context`; else `PortNotAssigned` when an endpoint it names has no port;
/// else available.
#[must_use]
pub fn render(
    package: &ValidatedPackage,
    context: &PlanContext,
    instance: &Instance,
    secret_refs: &[(SecretFieldName, SecretRef)],
) -> Vec<RenderedConnection> {
    let retained = instance.lifecycle() == InstanceLifecycle::Retained;
    let pinned = package.key() == instance.provider_revision();
    let running = instance.lifecycle() == InstanceLifecycle::Running;
    render_entries(package.manifest(), context)
        .into_iter()
        .map(|entry| {
            let port_may_change = !running && names_an_auto_endpoint(&entry, instance);
            let state = if retained {
                ConnectionState::Unavailable(ConnectionUnavailable::Retained)
            } else if !pinned {
                ConnectionState::Unavailable(ConnectionUnavailable::PackageUnavailable)
            } else {
                state_of(&entry, secret_refs)
            };
            RenderedConnection {
                name: entry.name,
                title: entry.title,
                state,
                port_may_change,
            }
        })
        .collect()
}

/// Whether the entry names an endpoint that asks for an `Auto` port.
fn names_an_auto_endpoint(entry: &RenderedEntry, instance: &Instance) -> bool {
    instance.endpoints().iter().any(|endpoint| {
        matches!(endpoint, Endpoint::Ip(ip) if ip.request() == PortRequest::Auto)
            && entry.endpoints.contains(endpoint.name())
    })
}

/// The state of one entry of a pinned package of a live instance.
fn state_of(
    entry: &RenderedEntry,
    secret_refs: &[(SecretFieldName, SecretRef)],
) -> ConnectionState {
    let pieces = match &entry.pieces {
        Ok(pieces) => pieces,
        Err(RenderFailure::PortNotAssigned) => {
            return ConnectionState::Unavailable(ConnectionUnavailable::PortNotAssigned);
        }
        Err(RenderFailure::Unresolvable) => {
            return ConnectionState::Unavailable(ConnectionUnavailable::PackageUnavailable);
        }
    };
    let mut masked = String::new();
    let mut parts = Vec::with_capacity(pieces.len());
    for piece in pieces {
        match piece {
            RenderedPiece::Text(text) => {
                masked.push_str(text);
                parts.push(ConnectionPart::Text(text.clone()));
            }
            RenderedPiece::Secret(field) => {
                let Some((name, reference)) = secret_refs
                    .iter()
                    .find(|(name, _)| name.as_str() == field.as_str())
                else {
                    return ConnectionState::Unavailable(ConnectionUnavailable::PackageUnavailable);
                };
                masked.push_str(MASK);
                parts.push(ConnectionPart::Secret {
                    field: name.clone(),
                    reference: reference.clone(),
                });
            }
        }
    }
    ConnectionState::Available { masked, parts }
}
