import StacklyClient

/// One reference a start, env up, or restore needs and cannot read, as the secret entry
/// sheet lists it (spec 0011, *GUI*).
nonisolated public struct SecretNeed: Identifiable, Sendable, Equatable {
    /// The reference, which is also the row's identity.
    public let reference: String
    /// Why it cannot be read.
    public let reason: NeedsSecretReason
    /// Whether a generated value would work for it.
    public let generateAllowed: Bool
    /// The limits of the field behind the reference.
    public let limits: SecretLimits

    /// The row's identity: the reference.
    public var id: String { reference }

    /// Creates a need.
    /// - Parameters:
    ///   - reference: The reference.
    ///   - reason: Why it cannot be read.
    ///   - generateAllowed: Whether a generated value would work.
    ///   - limits: The limits of the field behind the reference.
    public init(
        reference: String, reason: NeedsSecretReason, generateAllowed: Bool,
        limits: SecretLimits = .base
    ) {
        self.reference = reference
        self.reason = reason
        self.generateAllowed = generateAllowed
        self.limits = limits
    }

    /// A need from an instance blocker.
    public init(item: NeedsSecretItem, limits: SecretLimits = .base) {
        self.init(
            reference: item.reference, reason: item.reason, generateAllowed: item.generateAllowed,
            limits: limits)
    }

    /// The needs in a `secret.needs_secret` error, in the order the daemon sent them.
    /// Items of another kind, and items without a reference, are skipped. A reason this
    /// build does not know reads as `.unrecognized`.
    /// - Parameter error: The error body.
    public static func needs(in error: ErrorBody) -> [SecretNeed] {
        guard error.code == "secret.needs_secret" else { return [] }
        return (error.items ?? []).compactMap { item in
            guard item.kind == "reference" else { return nil }
            let facts = Dictionary(
                (item.fields ?? []).map { ($0.key, $0.value) },
                uniquingKeysWith: { first, _ in first })
            guard let reference = facts["reference"] else { return nil }
            return SecretNeed(
                reference: reference,
                reason: facts["reason"].flatMap(NeedsSecretReason.init(rawValue:)) ?? .unrecognized,
                generateAllowed: facts["generateAllowed"] == "true")
        }
    }

    /// The reason in a short sentence, in English.
    public var reasonText: String {
        switch reason {
        case .missing: "No value is stored."
        case .storeLocked: "The Keychain is locked."
        case .accessDenied: "Access to the Keychain was denied."
        case .storeBusy: "The Keychain is busy."
        case .storeUnsupported: "This Mac has no supported secret store."
        case .unrecognized: "The value can’t be read."
        }
    }
}
