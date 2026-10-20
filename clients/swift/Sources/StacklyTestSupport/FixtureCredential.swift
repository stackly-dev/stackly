import Foundation
public import Testing

/// A credential made for one test, never a real one (spec 0011 AC-18, spec 0002 *Fixture
/// credential*).
///
/// Its value is `stackly-fixture-credential-<32 lowercase hex>` from a random UUID, so the
/// scan for ``prefix`` in a log, a database, a frame recording, or CLI output finds a leak of
/// it. It prints as `FixtureCredential(****)`, so a failing test never shows the value.
public struct FixtureCredential: Sendable, CustomStringConvertible, CustomDebugStringConvertible,
    CustomReflectable
{
    /// The prefix every fixture credential starts with; the same text as
    /// `FIXTURE_CREDENTIAL_PREFIX` in `stackly-evidence`.
    public static let prefix = "stackly-fixture-credential-"

    /// The value to hand to the code under test.
    public let value: String

    /// Creates a new random fixture credential.
    public init() {
        value = Self.prefix + UUID().uuidString.lowercased().filter { $0 != "-" }
    }

    /// `FixtureCredential(****)`.
    public var description: String { "FixtureCredential(****)" }

    /// The same text as ``description``.
    public var debugDescription: String { description }

    /// A mirror with the mask only, so `dump` never reaches the value.
    public var customMirror: Mirror {
        Mirror(self, children: ["value": "****"], displayStyle: .struct)
    }

    /// Records an issue when ``prefix`` appears in `haystack`. The message names `what` and
    /// never the value.
    ///
    /// - Parameters:
    ///   - haystack: The text to scan: a log, a dump, a description.
    ///   - what: What `haystack` is, for the message.
    ///   - sourceLocation: Where the issue is reported.
    public static func assertAbsent(
        in haystack: String, what: String, sourceLocation: SourceLocation = #_sourceLocation
    ) {
        if haystack.contains(prefix) {
            Issue.record("fixture credential found in \(what)", sourceLocation: sourceLocation)
        }
    }

    /// Records an issue when this credential's prefix appears in `haystack`.
    ///
    /// - Parameters:
    ///   - haystack: The text to scan.
    ///   - what: What `haystack` is, for the message.
    ///   - sourceLocation: Where the issue is reported.
    public func assertAbsent(
        in haystack: String, what: String, sourceLocation: SourceLocation = #_sourceLocation
    ) {
        Self.assertAbsent(in: haystack, what: what, sourceLocation: sourceLocation)
    }
}
