/// A value read back from the daemon by `secret.reveal`: the one place the app holds a
/// secret value (spec 0011, *Reveal dan copy*).
///
/// It is hand written, not a generated DTO. It prints as `RevealedSecret(<reference>, ****)`
/// through `description`, `debugDescription`, and its mirror, so `print`, `dump`, string
/// interpolation, and a log line never show the value. It is not `Codable`, `Equatable`, or
/// `Hashable`: nothing should compare, encode, or key on a value. Read ``value`` only at the
/// moment of use (show on screen, write to the pasteboard) and drop the whole struct after.
public struct RevealedSecret: Sendable, CustomStringConvertible, CustomDebugStringConvertible,
    CustomReflectable
{
    /// The mask every text form uses in place of the value.
    static let mask = "****"

    /// The reference the value belongs to.
    public let reference: String
    /// The value. Never print, log, or persist it.
    public let value: String

    /// Creates a revealed secret.
    ///
    /// - Parameters:
    ///   - reference: The reference the value belongs to.
    ///   - value: The value the daemon sent.
    public init(reference: String, value: String) {
        self.reference = reference
        self.value = value
    }

    /// `RevealedSecret(<reference>, ****)`.
    public var description: String {
        "RevealedSecret(\(reference), \(Self.mask))"
    }

    /// The same text as ``description``.
    public var debugDescription: String { description }

    /// A mirror with the reference and the mask, so `dump` never reaches the value.
    public var customMirror: Mirror {
        Mirror(
            self, children: ["reference": reference, "value": Self.mask], displayStyle: .struct)
    }
}
