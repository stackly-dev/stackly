import Testing

@testable import StacklyUI

/// The client side limits of a secret value (spec 0011 AC-7, AC-19, AC-22).
struct SecretLimitsTests {
    @Test func aPlainValueFitsTheBaseRules() {
        #expect(SecretLimits.base.violation(for: "fixture-secret-0001") == nil)
        #expect(SecretLimits.base.violation(for: "x") == nil)
    }

    @Test func anEmptyValueAndANULAreRefused() {
        #expect(SecretLimits.base.violation(for: "") == .empty)
        #expect(SecretLimits.base.violation(for: "ab\u{0}cd") == .containsNUL)
    }

    @Test func theByteLimitCountsUTF8Bytes() {
        let atLimit = String(repeating: "a", count: SecretLimits.byteLimit)
        #expect(SecretLimits.base.violation(for: atLimit) == nil)
        #expect(SecretLimits.base.violation(for: atLimit + "a") == .tooManyBytes)
        // Two bytes per character: 32769 of them are 65538 bytes.
        let wide = String(repeating: "é", count: SecretLimits.byteLimit / 2 + 1)
        #expect(SecretLimits.base.violation(for: wide) == .tooManyBytes)
    }

    @Test func lengthCountsCharactersNotBytes() {
        let limits = SecretLimits(minLength: 4, maxLength: 6)
        #expect(limits.violation(for: "éééé") == nil)
        #expect(limits.violation(for: "abc") == .tooShort(minimum: 4))
        #expect(limits.violation(for: "abcdefg") == .tooLong(maximum: 6))
        #expect(limits.violation(for: "abcdef") == nil)
    }

    @Test(arguments: ["a\nb", "a\tb", "a\u{85}b", "a\u{7F}b", "a\u{1F}b", "a\rb"])
    func aPrintableOnlyFieldRefusesControlCharacters(_ value: String) {
        let limits = SecretLimits(printableOnly: true)
        #expect(limits.violation(for: value) == .controlCharacters)
        #expect(SecretLimits.base.violation(for: value) == nil)
    }

    @Test func lengthIsCheckedBeforeControlCharacters() {
        let limits = SecretLimits(minLength: 8, printableOnly: true)
        #expect(limits.violation(for: "a\nb") == .tooShort(minimum: 8))
        #expect(limits.violation(for: "abcd\nefgh") == .controlCharacters)
    }

    @Test func aNULIsNeverReportedAsAControlCharacter() {
        #expect(SecretLimits(printableOnly: true).violation(for: "a\u{0}b") == .containsNUL)
    }

    @Test func theLengthHintReadsInPlainWords() {
        #expect(SecretLimits(minLength: 12, maxLength: 64).lengthHint == "12 to 64 characters")
        #expect(SecretLimits(minLength: 12).lengthHint == "At least 12 characters")
        #expect(SecretLimits(maxLength: 64).lengthHint == "At most 64 characters")
        #expect(SecretLimits.base.lengthHint == nil)
    }

    @Test func everyViolationHasAMessage() {
        let all: [SecretLimits.Violation] = [
            .empty, .containsNUL, .tooManyBytes, .tooShort(minimum: 3), .tooLong(maximum: 9),
            .controlCharacters,
        ]
        for violation in all {
            #expect(!violation.message.isEmpty)
            #expect(violation.message.hasSuffix("."))
        }
    }
}
