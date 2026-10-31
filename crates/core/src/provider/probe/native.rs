//! The registry of native probes (spec 0023, *Jenis probe native*): the
//! closed set of probe names a manifest may use with `type: native`, what
//! each one takes, and the typed probe a validated, interpolated set of
//! params becomes.
//!
//! This is the only engine module that names a concrete native probe.
//! Validation, the manifest model, and plans work with the generic types
//! here, so a probe that is added needs one variant, one descriptor, one
//! `build` arm, and one runner arm in the supervisor, and the compiler
//! rejects any of them missing.

use serde::Serialize;

use crate::provider::EndpointTransport;

/// A native probe name in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NativeProbeKind {
    /// `nats-readiness`: speaks the NATS client protocol to the `client`
    /// endpoint and checks identity, JetStream mode, and a PING.
    NatsReadiness,
}

/// What a native probe takes in a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeProbeDescriptor {
    /// The registry name, matching `^[a-z][a-z0-9-]{0,31}$`.
    pub name: &'static str,
    /// The transports its `endpoint` may have.
    pub endpoint_transports: &'static [EndpointTransport],
    /// Its params, all required, in the order the descriptor lists them.
    pub params: &'static [NativeParamDescriptor],
}

/// One param of a native probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeParamDescriptor {
    /// The param name in `params`, camelCase.
    pub name: &'static str,
    /// What value it takes.
    pub kind: NativeParamKind,
}

/// The type of a native probe param.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeParamKind {
    /// Text of 1 to `max_bytes` bytes without control characters.
    Text {
        /// The most bytes after interpolation.
        max_bytes: usize,
    },
    /// `true` or `false`.
    Boolean,
}

/// A param value after validation and interpolation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeParamValue {
    /// A [`NativeParamKind::Text`] value.
    Text(String),
    /// A [`NativeParamKind::Boolean`] value.
    Boolean(bool),
}

/// A native probe with its params, ready to run.
///
/// `Serialize` writes `{"name":"nats-readiness","params":{...}}` with the
/// param names of the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "name", content = "params", rename_all_fields = "camelCase")]
#[non_exhaustive]
pub enum NativeProbe {
    /// The `nats-readiness` probe.
    #[serde(rename = "nats-readiness")]
    NatsReadiness {
        /// The `server_name` the instance's server must report.
        server_name: String,
        /// Whether JetStream must be on.
        jetstream: bool,
    },
}

const NATS_READINESS: NativeProbeDescriptor = NativeProbeDescriptor {
    name: "nats-readiness",
    endpoint_transports: &[EndpointTransport::Tcp],
    params: &[
        NativeParamDescriptor {
            name: "serverName",
            kind: NativeParamKind::Text { max_bytes: 255 },
        },
        NativeParamDescriptor {
            name: "jetstream",
            kind: NativeParamKind::Boolean,
        },
    ],
};

impl NativeProbeKind {
    /// Every probe in the registry.
    pub const ALL: &'static [NativeProbeKind] = &[NativeProbeKind::NatsReadiness];

    /// Returns what the probe takes.
    #[must_use]
    pub const fn descriptor(self) -> &'static NativeProbeDescriptor {
        match self {
            Self::NatsReadiness => &NATS_READINESS,
        }
    }

    /// Returns the probe with this registry name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.descriptor().name == name)
    }
}

/// Turns validated, interpolated params into the typed probe.
///
/// Validation and the plan builder check every param against the
/// descriptor first, so `params` holds each descriptor param once with its
/// declared type.
///
/// # Panics
///
/// Panics when a param the descriptor lists is missing or has another
/// type, which the callers rule out.
#[must_use]
pub fn build(kind: NativeProbeKind, params: &[(&str, NativeParamValue)]) -> NativeProbe {
    let find = |name: &str| {
        params
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    };
    let text = |name: &str| match find(name) {
        Some(NativeParamValue::Text(text)) => text.clone(),
        _ => panic!("a validated native probe has the text param {name}"),
    };
    let boolean = |name: &str| match find(name) {
        Some(NativeParamValue::Boolean(flag)) => *flag,
        _ => panic!("a validated native probe has the boolean param {name}"),
    };
    match kind {
        NativeProbeKind::NatsReadiness => NativeProbe::NatsReadiness {
            server_name: text("serverName"),
            jetstream: boolean("jetstream"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_names_are_unique_and_lowercase_with_hyphens() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in NativeProbeKind::ALL {
            let name = kind.descriptor().name;
            assert!(seen.insert(name), "{name} is listed twice");
            assert!(
                (1..=32).contains(&name.len())
                    && name.starts_with(|c: char| c.is_ascii_lowercase())
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{name} breaks ^[a-z][a-z0-9-]{{0,31}}$"
            );
            assert_eq!(NativeProbeKind::from_name(name), Some(*kind));
        }
    }

    // covers: AC-2
    #[test]
    fn the_registry_holds_exactly_nats_readiness() {
        assert_eq!(NativeProbeKind::ALL, [NativeProbeKind::NatsReadiness]);
        assert_eq!(
            NativeProbeKind::from_name("nats-readiness"),
            Some(NativeProbeKind::NatsReadiness)
        );
    }

    // covers: AC-2
    #[test]
    fn the_generic_provider_modules_never_name_the_nats_probe() {
        let modules = [
            ("validate.rs", include_str!("../validate.rs")),
            ("manifest.rs", include_str!("../manifest.rs")),
            ("plan.rs", include_str!("../plan.rs")),
        ];
        for (file, source) in modules {
            for line in source.lines() {
                // The `natsConf` config format is a different feature.
                let probe_mention = line
                    .replace("natsConf", "")
                    .replace("nats_conf", "")
                    .to_ascii_lowercase()
                    .contains("nats");
                assert!(
                    !probe_mention,
                    "{file} names NATS, the probe registry belongs in native.rs: {line}"
                );
            }
        }
    }

    #[test]
    fn a_name_outside_the_registry_has_no_probe() {
        assert_eq!(NativeProbeKind::from_name("mystery"), None);
        assert_eq!(NativeProbeKind::from_name("NATS-READINESS"), None);
    }

    #[test]
    fn nats_readiness_takes_a_server_name_and_a_jetstream_flag_on_a_tcp_endpoint() {
        let descriptor = NativeProbeKind::NatsReadiness.descriptor();

        assert_eq!(descriptor.name, "nats-readiness");
        assert_eq!(descriptor.endpoint_transports, [EndpointTransport::Tcp]);
        let params: Vec<_> = descriptor
            .params
            .iter()
            .map(|param| (param.name, param.kind))
            .collect();
        assert_eq!(
            params,
            [
                ("serverName", NativeParamKind::Text { max_bytes: 255 }),
                ("jetstream", NativeParamKind::Boolean),
            ]
        );
    }

    #[test]
    fn build_maps_the_params_to_a_typed_probe() {
        let probe = build(
            NativeProbeKind::NatsReadiness,
            &[
                ("jetstream", NativeParamValue::Boolean(true)),
                ("serverName", NativeParamValue::Text("golden".into())),
            ],
        );

        assert_eq!(
            probe,
            NativeProbe::NatsReadiness {
                server_name: "golden".into(),
                jetstream: true,
            }
        );
    }

    #[test]
    fn a_native_probe_serializes_with_its_name_and_camel_case_params() {
        let probe = NativeProbe::NatsReadiness {
            server_name: "golden".into(),
            jetstream: false,
        };

        assert_eq!(
            serde_json::to_string(&probe).unwrap(),
            r#"{"name":"nats-readiness","params":{"serverName":"golden","jetstream":false}}"#
        );
    }
}
