import Foundation

/// The limits one secret value must meet, checked in the client to hold the main button
/// back (spec 0011 AC-19). The daemon checks the same limits again and stays the judge.
/// The order is the daemon's (spec 0011 AC-22): the base rules of AC-7 first (not empty, no
/// NUL, at most 65536 UTF-8 bytes), then the length of the field in characters, then, for a
/// `printableOnly` field, control characters.
nonisolated public struct SecretLimits: Sendable, Equatable {
    /// The most UTF-8 bytes any secret value may have (spec 0011 AC-7).
    public static let byteLimit = 65_536

    /// The limits of a field that declares none: only the base rules apply.
    public static let base = SecretLimits()

    /// The fewest characters, or `nil` for no minimum.
    public var minLength: Int?
    /// The most characters, or `nil` for no maximum.
    public var maxLength: Int?
    /// Whether control characters (U+0000 to U+001F and U+007F to U+009F) are refused.
    public var printableOnly: Bool

    /// Why a value does not fit.
    public enum Violation: Equatable, Sendable {
        /// Nothing was typed.
        case empty
        /// The value holds a NUL character.
        case containsNUL
        /// The value is longer than ``SecretLimits/byteLimit`` bytes.
        case tooManyBytes
        /// The value has fewer characters than the field needs.
        case tooShort(minimum: Int)
        /// The value has more characters than the field allows.
        case tooLong(maximum: Int)
        /// The field is `printableOnly` and the value holds a control character.
        case controlCharacters

        /// A short sentence for the field, in English.
        public var message: String {
            switch self {
            case .empty: "Enter a value."
            case .containsNUL: "The value can’t contain a NUL character."
            case .tooManyBytes: "The value is too long. The limit is 65,536 bytes."
            case .tooShort(let minimum): "Use at least \(minimum) characters."
            case .tooLong(let maximum): "Use at most \(maximum) characters."
            case .controlCharacters: "Remove tabs, line breaks, and other control characters."
            }
        }
    }

    /// Creates limits.
    /// - Parameters:
    ///   - minLength: The fewest characters, or `nil`.
    ///   - maxLength: The most characters, or `nil`.
    ///   - printableOnly: Whether control characters are refused.
    public init(minLength: Int? = nil, maxLength: Int? = nil, printableOnly: Bool = false) {
        self.minLength = minLength
        self.maxLength = maxLength
        self.printableOnly = printableOnly
    }

    /// The first rule `text` breaks, or `nil` when it fits.
    /// - Parameter text: The value as typed.
    public func violation(for text: String) -> Violation? {
        if text.isEmpty { return .empty }
        if text.unicodeScalars.contains(where: { $0.value == 0 }) { return .containsNUL }
        if text.utf8.count > Self.byteLimit { return .tooManyBytes }
        let length = text.unicodeScalars.count
        if let minLength, length < minLength { return .tooShort(minimum: minLength) }
        if let maxLength, length > maxLength { return .tooLong(maximum: maxLength) }
        if printableOnly,
            text.unicodeScalars.contains(where: { $0.properties.generalCategory == .control })
        {
            return .controlCharacters
        }
        return nil
    }

    /// A one line hint about the length, such as “12 to 64 characters”, or `nil` when the
    /// field has no length limit.
    public var lengthHint: String? {
        switch (minLength, maxLength) {
        case (let low?, let high?): "\(low) to \(high) characters"
        case (let low?, nil): "At least \(low) characters"
        case (nil, let high?): "At most \(high) characters"
        case (nil, nil): nil
        }
    }
}
