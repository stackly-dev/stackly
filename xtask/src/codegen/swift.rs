//! Swift files written next to the typeshare output: `OpenEnums.swift` and
//! `ContractInfo.swift` (spec 0006, *Codegen dan Swift*).

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
}
