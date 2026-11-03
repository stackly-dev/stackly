import StacklyClient
import Testing

@testable import StacklyUI

/// The sentence of a start that waits for a locked Keychain (spec 0031, AC-15).
@Suite(.timeLimit(.minutes(1)))
struct KeychainWaitTests {
    private func need(_ reason: NeedsSecretReason) -> SecretNeed {
        SecretNeed(reference: "local:erp/db", reason: reason, generateAllowed: false)
    }

    @Test func aLockedOrBusyKeychainSaysStacklyTriesAgain() {
        let words = "The Keychain is locked. Stackly tries again every 30 seconds."
        #expect(SecretNeed.keychainWaitNote(for: [need(.storeLocked)]) == words)
        #expect(SecretNeed.keychainWaitNote(for: [need(.storeBusy), need(.storeLocked)]) == words)
    }

    @Test func aReasonThatNeedsThePersonHasNoWaitSentence() {
        #expect(SecretNeed.keychainWaitNote(for: [need(.missing)]) == nil)
        #expect(SecretNeed.keychainWaitNote(for: [need(.accessDenied)]) == nil)
        #expect(SecretNeed.keychainWaitNote(for: [need(.storeLocked), need(.missing)]) == nil)
        #expect(SecretNeed.keychainWaitNote(for: []) == nil)
    }
}
