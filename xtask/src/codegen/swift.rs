//! Swift files written next to the typeshare output: `OpenEnums.swift` and
//! `ContractInfo.swift` (spec 0006, *Codegen dan Swift*).

use anyhow::{Context, Result, bail};
use stackly_protocol::ProtocolVersion;

use super::GENERATED_HEADER;

/// Names of the enums in typeshare output that are open: `String` backed
/// with a `case unknown`, in the order they appear.
pub fn open_enum_names(swift: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current: Option<&str> = None;
    for line in swift.lines() {
        if let Some(rest) = line.strip_prefix("public enum ") {
            current = rest
                .split_once(": String, Codable")
                .map(|(name, _)| name.trim());
            continue;
        }
        let Some(name) = current else { continue };
        if line.trim() == "case unknown" {
            names.push(name.to_owned());
            current = None;
        } else if line == "}" {
            current = None;
        }
    }
    names
}

/// `OpenEnums.swift`: for every open enum, a decoder that reads a value
/// this build does not know as `.unknown` instead of failing.
pub fn open_enums(swift: &str) -> String {
    let mut out = format!(
        "{GENERATED_HEADER}\n\
         // Open enums read a value from a newer peer as `.unknown` instead of\n\
         // failing to decode (spec 0006). The daemon never sends `unknown`.\n"
    );
    for name in open_enum_names(swift) {
        out.push_str(&format!(
            "\nextension {name} {{\n\
             \tpublic init(from decoder: Decoder) throws {{\n\
             \t\tlet value = try decoder.singleValueContainer().decode(String.self)\n\
             \t\tself = Self(rawValue: value) ?? .unknown\n\
             \t}}\n\
             }}\n"
        ));
    }
    out
}

/// `ContractInfo.swift`: the digest and protocol version the Swift client
/// sends in its handshake.
pub fn contract_info(digest: &str, version: ProtocolVersion) -> String {
    format!(
        "{GENERATED_HEADER}\n\
         /// The contract this client was generated from (spec 0006).\n\
         public enum ContractInfo {{\n\
         \t/// SHA-256 of the normalized contract, sent as `contractDigest` in the handshake.\n\
         \tpublic static let digest = \"{digest}\"\n\
         \t/// The protocol version this client speaks.\n\
         \tpublic static let protocolVersion = ProtocolVersion(major: {}, minor: {})\n\
         }}\n",
        version.major, version.minor
    )
}

/// The `CodingKeys` of `public enum <name>` in typeshare output, as
/// `(swift case, wire name)` pairs in order.
pub fn coding_keys(swift: &str, name: &str) -> Result<Vec<(String, String)>> {
    let header = format!("public enum {name}: ");
    let mut lines = swift.lines().skip_while(|line| !line.starts_with(&header));
    if lines.next().is_none() {
        bail!("typeshare output has no `{header}`");
    }
    let mut lines =
        lines.skip_while(|line| line.trim() != "enum CodingKeys: String, CodingKey, Codable {");
    if lines.next().is_none() {
        bail!("`{name}` in typeshare output has no CodingKeys");
    }
    let body: String = lines
        .take_while(|line| line.trim() != "}")
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    body.trim_start_matches("case ")
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (case, raw) = entry
                .split_once(" = ")
                .with_context(|| format!("`{name}` key `{entry}` has no wire name"))?;
            Ok((
                case.trim().to_owned(),
                raw.trim().trim_matches('"').to_owned(),
            ))
        })
        .collect()
}

/// `ContractNames.swift`: the event kinds the Swift client decodes fully and
/// the method name of every `Call` and `Reply` case (spec 0013), so the
/// client checks that a reply's tag matches its call.
pub fn contract_names(swift: &str, event_kinds: &[&str], methods: &[&str]) -> Result<String> {
    let mut out = format!(
        "{GENERATED_HEADER}\n\
         /// Names from the contract the Swift client checks daemon frames against\n\
         /// (spec 0013).\n\
         public enum ContractNames {{\n\
         \t/// Every event kind this client decodes fully. Another kind is skipped\n\
         \t/// through `EventProbe` and still advances the cursor.\n\
         \tpublic static let eventKinds: Set<String> = [\n"
    );
    for kind in event_kinds {
        out.push_str(&format!("\t\t\"{kind}\",\n"));
    }
    out.push_str("\t]\n}\n");
    for (name, doc) in [
        ("Call", "The method name, such as `daemon.get_info`."),
        ("Reply", "The method this reply answers."),
    ] {
        let keys = coding_keys(swift, name)?;
        let wire: Vec<&str> = keys.iter().map(|(_, raw)| raw.as_str()).collect();
        if wire != methods {
            bail!("`{name}` cases {wire:?} do not match the method table {methods:?}");
        }
        out.push_str(&format!(
            "\nextension {name} {{\n\t/// {doc}\n\tpublic var methodName: String {{\n\t\tswitch self {{\n"
        ));
        for (case, raw) in &keys {
            out.push_str(&format!("\t\tcase .{case}: \"{raw}\"\n"));
        }
        out.push_str("\t\t}\n\t}\n}\n");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
public enum ClientKind: String, Codable, Sendable, Equatable {
\tcase cli
\tcase unknown
}

public enum ErrorCategory: String, Codable, Sendable, Equatable {
\tcase invalidInput = \"invalid_input\"
\tcase internal
}

public enum Call: Codable, Sendable, Equatable {
\tcase unknown
}

public enum HealthStatus: String, Codable, Sendable, Equatable {
\tcase ready
\tcase unknown
}
";

    #[test]
    fn only_string_enums_with_an_unknown_case_are_open() {
        assert_eq!(open_enum_names(SAMPLE), ["ClientKind", "HealthStatus"]);
    }

    #[test]
    fn each_open_enum_gets_a_decoder_that_falls_back_to_unknown() {
        let swift = open_enums(SAMPLE);
        assert!(swift.starts_with(GENERATED_HEADER));
        assert!(swift.contains("extension ClientKind {"));
        assert!(swift.contains("self = Self(rawValue: value) ?? .unknown"));
        assert!(!swift.contains("extension ErrorCategory"));
        assert!(!swift.contains("extension Call"));
    }

    #[test]
    fn contract_info_carries_the_digest_and_version() {
        let swift = contract_info("ab", ProtocolVersion { major: 1, minor: 2 });
        assert!(swift.contains("public static let digest = \"ab\""));
        assert!(swift.contains("ProtocolVersion(major: 1, minor: 2)"));
    }

    const TAGGED: &str = "\
public enum Call: Codable, Sendable, Equatable {
\tcase daemonGetInfo
\tcase eventsSubscribe(SubscribeParams)

\tenum CodingKeys: String, CodingKey, Codable {
\t\tcase daemonGetInfo = \"daemon.get_info\",
\t\t\teventsSubscribe = \"events.subscribe\"
\t}
}

public enum Reply: Codable, Sendable, Equatable {
\tcase daemonGetInfo(DaemonInfo)
\tcase eventsSubscribe(Subscribed)

\tenum CodingKeys: String, CodingKey, Codable {
\t\tcase daemonGetInfo = \"daemon.get_info\",
\t\t\teventsSubscribe = \"events.subscribe\"
\t}
}
";

    #[test]
    fn coding_keys_pair_each_swift_case_with_its_wire_name() {
        assert_eq!(
            coding_keys(TAGGED, "Call").unwrap(),
            [
                ("daemonGetInfo".to_owned(), "daemon.get_info".to_owned()),
                ("eventsSubscribe".to_owned(), "events.subscribe".to_owned()),
            ]
        );
        assert!(coding_keys(TAGGED, "Missing").is_err());
    }

    #[test]
    fn contract_names_list_event_kinds_and_a_method_name_per_case() {
        let swift = contract_names(
            TAGGED,
            &["events.gap"],
            &["daemon.get_info", "events.subscribe"],
        )
        .unwrap();
        assert!(swift.starts_with(GENERATED_HEADER));
        assert!(
            swift.contains(
                "public static let eventKinds: Set<String> = [\n\t\t\"events.gap\",\n\t]"
            )
        );
        assert!(swift.contains("extension Call {"));
        assert!(swift.contains("extension Reply {"));
        assert!(swift.contains("case .eventsSubscribe: \"events.subscribe\""));
    }

    #[test]
    fn contract_names_refuse_cases_that_drift_from_the_method_table() {
        let error = contract_names(TAGGED, &[], &["daemon.get_info"]).unwrap_err();
        assert!(error.to_string().contains("do not match the method table"));
    }
}
