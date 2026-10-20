import Foundation

/// One JSON value, for the parts of the contract that carry free form JSON.
///
/// Numbers are `Double`, which holds every integer up to `MAX_SAFE_INTEGER`
/// (2^53 - 1, spec 0006) exactly. Decoding refuses a whole number beyond
/// that range instead of rounding it, so no digit is lost in silence; a
/// fraction keeps `Double` precision. Nesting is not counted here: the
/// daemon refuses a request nested deeper than `ContractLimits.maxJsonDepth`
/// and the frame decoder scans every incoming frame with ``JSONDepth``, and
/// this type only ever travels in a request. Read and write it through
/// ``StacklyJSON`` like any other contract type.
public enum JSONValue: Codable, Sendable, Equatable {
    /// `null`.
    case null
    /// `true` or `false`.
    case bool(Bool)
    /// A number.
    case number(Double)
    /// A string.
    case string(String)
    /// An array.
    case array([JSONValue])
    /// An object.
    case object([String: JSONValue])

    /// The largest whole number the wire carries, 2^53 - 1.
    static let largestSafeInteger = 9_007_199_254_740_991.0

    /// Reads any JSON value.
    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            guard value.rounded() != value || abs(value) <= Self.largestSafeInteger else {
                throw DecodingError.dataCorruptedError(
                    in: container,
                    debugDescription: "integer is above 2^53 - 1")
            }
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else {
            self = .object(try container.decode([String: JSONValue].self))
        }
    }

    /// Writes the value as plain JSON.
    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case .bool(let value): try container.encode(value)
        case .number(let value): try container.encode(value)
        case .string(let value): try container.encode(value)
        case .array(let value): try container.encode(value)
        case .object(let value): try container.encode(value)
        }
    }
}

/// A JSON object: the `SettingsDocument` of `instance.create`, the provider
/// settings without secret values (spec 0010). It decodes only from a JSON
/// object.
public struct JSONObject: Codable, Sendable, Equatable {
    /// The members of the object.
    public var members: [String: JSONValue]

    /// Wraps `members`.
    public init(_ members: [String: JSONValue] = [:]) {
        self.members = members
    }

    /// Reads a JSON object; any other JSON value fails.
    public init(from decoder: Decoder) throws {
        members = try decoder.singleValueContainer().decode([String: JSONValue].self)
    }

    /// Writes the members as a plain JSON object.
    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(members)
    }
}
