import StacklyClient

/// Where the value of a secret field comes from, as the person chose it.
nonisolated public enum SecretDraftSource: Hashable, Sendable, CaseIterable {
    /// The daemon generates it.
    case generate
    /// The person types it.
    case enter
    /// The daemon copies it from another reference.
    case copyFrom

    /// The label of the choice.
    public var title: String {
        switch self {
        case .generate: "Generate"
        case .enter: "Enter"
        case .copyFrom: "Copy from"
        }
    }
}

/// What the person has chosen and typed for one secret field, until it is sent.
/// It lives in a view's `@State` and nowhere else: never in an `@Observable` model that
/// outlives the view, in `UserDefaults`, or in a log (spec 0011, *GUI*). It prints as a
/// mask. Call ``clear()`` as soon as the value is sent, cancelled, or the view goes away.
nonisolated public struct SecretDraft: Sendable, CustomStringConvertible,
    CustomDebugStringConvertible, CustomReflectable
{
    /// Where the value comes from.
    public var source: SecretDraftSource
    /// The typed value, used by ``SecretDraftSource/enter``.
    public var text: String
    /// The reference to copy from, used by ``SecretDraftSource/copyFrom``.
    public var copyFromReference: String?

    /// Creates a draft.
    /// - Parameter allowsGenerate: Whether a generated value works for the field. When it
    ///   does, the draft starts on Generate; otherwise on Enter.
    public init(allowsGenerate: Bool = true) {
        source = allowsGenerate ? .generate : .enter
        text = ""
        copyFromReference = nil
    }

    /// Whether the main button may send this draft: Generate always, Enter when the text
    /// fits `limits`, and Copy from once a reference is picked.
    /// - Parameter limits: The limits of the field.
    public func canSubmit(limits: SecretLimits) -> Bool {
        switch source {
        case .generate: true
        case .enter: limits.violation(for: text) == nil
        case .copyFrom: copyFromReference != nil
        }
    }

    /// The input for `secret.set`, or `nil` when ``canSubmit(limits:)`` is `false`.
    /// - Parameter limits: The limits of the field.
    public func input(limits: SecretLimits) -> SecretInput? {
        guard canSubmit(limits: limits) else { return nil }
        switch source {
        case .generate: return .generate
        case .enter: return .value(text)
        case .copyFrom: return copyFromReference.map { .copyFrom($0) }
        }
    }

    /// Forgets the typed value.
    public mutating func clear() {
        text = ""
        copyFromReference = nil
    }

    /// `SecretDraft(<source>, ****)`.
    public var description: String { "SecretDraft(\(source), ****)" }

    /// The same text as ``description``.
    public var debugDescription: String { description }

    /// A mirror with the source and the mask, so `dump` never reaches the typed value.
    public var customMirror: Mirror {
        Mirror(self, children: ["source": "\(source)", "text": "****"], displayStyle: .struct)
    }
}
