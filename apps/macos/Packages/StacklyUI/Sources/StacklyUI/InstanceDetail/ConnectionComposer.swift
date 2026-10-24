import Foundation
import StacklyClient

/// Joins the parts of a connection string with the secret values the person asked for (spec
/// 0021, AC-3): text parts as they are, secret parts as the percent encoded value of their
/// reference.
/// A pure function, written a second time in Rust (`stackly_protocol::instance::connection::
/// compose`) for the CLI; both give the same string for every case of
/// `protocols/fixtures/instance/connection-compose.json`. A value that is missing is an
/// error, never an empty string.
nonisolated public enum ConnectionComposer {
    /// Why the parts could not be joined.
    public enum Failure: Error, Equatable, Sendable {
        /// A secret part names a reference that `values` does not hold.
        case missingValue(reference: String)
        /// A part whose kind and fields do not match, or whose kind this build does not know.
        case invalidPart
    }

    /// Joins `parts`.
    /// - Parameters:
    ///   - parts: The parts of one connection string, as the daemon sent them.
    ///   - values: The secret values by reference. A value nobody asked for is ignored.
    /// - Returns: The full string.
    /// - Throws: ``Failure/missingValue(reference:)`` for a secret part without a value and
    ///   ``Failure/invalidPart`` for a part of an unknown kind or without the field its kind
    ///   needs.
    public static func compose(
        parts: [ConnectionPart], values: [String: String]
    ) throws(Failure) -> String {
        var text = ""
        for part in parts {
            switch (part.kind, part.text, part.secret) {
            case (.text, let literal?, nil):
                text += literal
            case (.secret, nil, let secret?):
                guard let value = values[secret.reference] else {
                    throw .missingValue(reference: secret.reference)
                }
                text += encoded(value)
            default:
                throw .invalidPart
            }
        }
        return text
    }

    /// The references the parts need, without repeats, in order.
    /// - Parameter parts: The parts of one connection string.
    public static func references(in parts: [ConnectionPart]) -> [String] {
        var seen = Set<String>()
        return parts.compactMap { $0.secret?.reference }.filter { seen.insert($0).inserted }
    }

    /// The function a ``RevealModel`` calls with the revealed values, over `parts`.
    /// It captures the parts, which hold no value, and reads each revealed value only while
    /// it joins them.
    /// - Parameter parts: The parts of one connection string.
    public static func composer(for parts: [ConnectionPart]) -> RevealModel.Compose {
        { revealed in
            let values = revealed.mapValues(\.value)
            return try compose(parts: parts, values: values)
        }
    }

    /// `value` as UTF-8 bytes in upper case hex, except the unreserved characters of RFC 3986
    /// (`A-Z a-z 0-9 - . _ ~`), so a value can never change the structure of the URI.
    static func encoded(_ value: String) -> String {
        let hex = Array("0123456789ABCDEF")
        var text = ""
        for byte in value.utf8 {
            if isUnreserved(byte) {
                text.unicodeScalars.append(Unicode.Scalar(byte))
            } else {
                text.append("%")
                text.append(hex[Int(byte >> 4)])
                text.append(hex[Int(byte & 0x0F)])
            }
        }
        return text
    }

    private static func isUnreserved(_ byte: UInt8) -> Bool {
        switch byte {
        case UInt8(ascii: "A")...UInt8(ascii: "Z"), UInt8(ascii: "a")...UInt8(ascii: "z"),
            UInt8(ascii: "0")...UInt8(ascii: "9"), UInt8(ascii: "-"), UInt8(ascii: "."),
            UInt8(ascii: "_"), UInt8(ascii: "~"):
            true
        default:
            false
        }
    }
}
