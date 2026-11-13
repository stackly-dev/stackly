//! `environment::export` (spec 0045, *Variabel export*): the host and port
//! of every endpoint of an environment's instances as environment
//! variables.
//!
//! An export holds only what the Summary tab of an instance already shows:
//! no secret value, reference, connection string, path, or PID, and no
//! option to add one (AC-15). Every value is checked for its characters
//! and passed through the [`Redactor`]; a value the redactor changes is
//! never printed.

use std::collections::BTreeSet;
use std::net::IpAddr;

use stackly_domain::{EndpointName, MemberAlias, Port};

use crate::secret::Redactor;

/// How `text` is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    /// `NAME=value` per line.
    Dotenv,
    /// `export NAME='value'` per line.
    Shell,
}

impl ExportFormat {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Dotenv => "dotenv",
            Self::Shell => "shell",
        }
    }
}

/// What a variable holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportValueKind {
    /// The host of the endpoint.
    Host,
    /// Its port.
    Port,
    /// `host:port`, IPv6 in brackets.
    Address,
}

impl ExportValueKind {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Port => "port",
            Self::Address => "address",
        }
    }

    const fn suffix(self) -> &'static str {
        match self {
            Self::Host => "HOST",
            Self::Port => "PORT",
            Self::Address => "ADDRESS",
        }
    }
}

/// Why a service or endpoint is not in the export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportSkipReason {
    /// The service has no instance yet.
    NotCreated,
    /// An `auto` endpoint that never had a port.
    PortNotAssigned,
    /// A Unix socket endpoint.
    UnixEndpoint,
    /// The pinned package can't be read.
    PackageUnavailable,
    /// The instance is retained.
    Retained,
    /// An earlier variable has the name.
    NameCollision,
    /// The redactor changed the value.
    Redacted,
}

impl ExportSkipReason {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NotCreated => "not_created",
            Self::PortNotAssigned => "port_not_assigned",
            Self::UnixEndpoint => "unix_endpoint",
            Self::PackageUnavailable => "package_unavailable",
            Self::Retained => "retained",
            Self::NameCollision => "name_collision",
            Self::Redacted => "redacted",
        }
    }
}

/// One endpoint of an instance, in the order of its pinned package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportEndpoint {
    /// An IP endpoint.
    Ip {
        /// The endpoint.
        name: EndpointName,
        /// Its effective host.
        host: IpAddr,
        /// Its effective port, when it has one.
        port: Option<Port>,
        /// Whether it asks for `auto`.
        auto: bool,
    },
    /// A Unix socket endpoint.
    Unix {
        /// The endpoint.
        name: EndpointName,
    },
}

/// Where the values of one service come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSource {
    /// The service has no instance.
    NotCreated,
    /// The instance is retained.
    Retained,
    /// The pinned package of the instance can't be read.
    PackageUnavailable,
    /// The instance and its endpoints.
    Instance {
        /// Whether its lifecycle is `running`.
        running: bool,
        /// Its endpoints, in package order.
        endpoints: Vec<ExportEndpoint>,
    },
}

/// One service of the snapshot, in snapshot order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportService {
    /// The alias.
    pub alias: MemberAlias,
    /// Its values.
    pub source: ExportSource,
}

/// One variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportVariable {
    /// The name, such as `STACKLY_POSTGRES_CLIENT_PORT`.
    pub name: String,
    /// The value.
    pub value: String,
    /// The service.
    pub service: MemberAlias,
    /// The endpoint.
    pub endpoint: EndpointName,
    /// What it holds.
    pub kind: ExportValueKind,
    /// Whether the port may change at the next start: an `auto` endpoint
    /// of an instance that is not running.
    pub may_change: bool,
}

/// One service or endpoint left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSkipped {
    /// The service.
    pub service: MemberAlias,
    /// The endpoint, when only one is left out.
    pub endpoint: Option<EndpointName>,
    /// Why.
    pub reason: ExportSkipReason,
}

/// What [`collect`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Collected {
    /// The variables that are printed.
    pub variables: Vec<ExportVariable>,
    /// What is left out.
    pub skipped: Vec<ExportSkipped>,
}

/// A value with a character an export never prints.
#[derive(Debug, thiserror::Error)]
#[error("an export value holds a character outside letters, digits, '.', ':', '[' and ']'")]
#[non_exhaustive]
pub struct UnsafeValue;

/// `<ALIAS>` or `<ENDPOINT>`: capital letters, and `_` for every
/// character but `A-Z` and `0-9`.
fn name_part(text: &str) -> String {
    text.chars()
        .map(|c| {
            let upper = c.to_ascii_uppercase();
            if upper.is_ascii_uppercase() || upper.is_ascii_digit() {
                upper
            } else {
                '_'
            }
        })
        .collect()
}

fn safe(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '[' | ']'))
}

/// `export::collect` (AC-14, AC-15): the variables of every service in
/// snapshot order, then every endpoint in package order. Each value is
/// checked for its characters, then passed through `redactor`; a value it
/// changes is left out as `redacted`.
///
/// # Errors
///
/// Returns [`UnsafeValue`] when a value holds a character outside letters,
/// digits, `.`, `:`, `[`, and `]`, so a strange value never reaches a
/// shell.
pub fn collect(services: &[ExportService], redactor: &Redactor) -> Result<Collected, UnsafeValue> {
    let mut out = Collected::default();
    let mut names: BTreeSet<String> = BTreeSet::new();
    for service in services {
        let skip_service = |out: &mut Collected, reason| {
            out.skipped.push(ExportSkipped {
                service: service.alias.clone(),
                endpoint: None,
                reason,
            });
        };
        let (running, endpoints) = match &service.source {
            ExportSource::NotCreated => {
                skip_service(&mut out, ExportSkipReason::NotCreated);
                continue;
            }
            ExportSource::Retained => {
                skip_service(&mut out, ExportSkipReason::Retained);
                continue;
            }
            ExportSource::PackageUnavailable => {
                skip_service(&mut out, ExportSkipReason::PackageUnavailable);
                continue;
            }
            ExportSource::Instance { running, endpoints } => (*running, endpoints),
        };
        for endpoint in endpoints {
            let skip = |out: &mut Collected, name: &EndpointName, reason| {
                out.skipped.push(ExportSkipped {
                    service: service.alias.clone(),
                    endpoint: Some(name.clone()),
                    reason,
                });
            };
            let (name, host, port, auto) = match endpoint {
                ExportEndpoint::Unix { name } => {
                    skip(&mut out, name, ExportSkipReason::UnixEndpoint);
                    continue;
                }
                ExportEndpoint::Ip {
                    name, port: None, ..
                } => {
                    skip(&mut out, name, ExportSkipReason::PortNotAssigned);
                    continue;
                }
                ExportEndpoint::Ip {
                    name,
                    host,
                    port: Some(port),
                    auto,
                } => (name, *host, *port, *auto),
            };
            let address = match host {
                IpAddr::V4(v4) => format!("{v4}:{port}"),
                IpAddr::V6(v6) => format!("[{v6}]:{port}"),
            };
            let values = [
                (ExportValueKind::Host, host.to_string()),
                (ExportValueKind::Port, port.to_string()),
                (ExportValueKind::Address, address),
            ];
            for (kind, value) in values {
                if !safe(&value) {
                    return Err(UnsafeValue);
                }
                let variable = format!(
                    "STACKLY_{}_{}_{}",
                    name_part(service.alias.as_str()),
                    name_part(name.as_str()),
                    kind.suffix()
                );
                if names.contains(&variable) {
                    skip(&mut out, name, ExportSkipReason::NameCollision);
                    continue;
                }
                if redactor.redact(&value) != value.as_str() {
                    skip(&mut out, name, ExportSkipReason::Redacted);
                    continue;
                }
                names.insert(variable.clone());
                out.variables.push(ExportVariable {
                    name: variable,
                    value,
                    service: service.alias.clone(),
                    endpoint: name.clone(),
                    kind,
                    may_change: auto && !running,
                });
            }
        }
    }
    Ok(out)
}

/// `export::render`: `text` from the variables that passed, one line each
/// with `\n` after it, no comment and no blank line.
#[must_use]
pub fn render(variables: &[ExportVariable], format: ExportFormat) -> String {
    let mut text = String::new();
    for variable in variables {
        match format {
            ExportFormat::Dotenv => {
                text.push_str(&variable.name);
                text.push('=');
                text.push_str(&variable.value);
            }
            ExportFormat::Shell => {
                text.push_str("export ");
                text.push_str(&variable.name);
                text.push_str("='");
                text.push_str(&variable.value);
                text.push('\'');
            }
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_part_is_capital_letters_digits_and_underscores() {
        assert_eq!(name_part("web-api"), "WEB_API");
        assert_eq!(name_part("client2"), "CLIENT2");
    }
}
