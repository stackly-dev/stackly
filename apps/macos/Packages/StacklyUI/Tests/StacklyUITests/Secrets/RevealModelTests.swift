import Foundation
import StacklyClient
import StacklyTestSupport
import Synchronization
import Testing

@testable import StacklyUI

/// Show and Copy of `RevealableSecretText` (spec 0011 AC-19, AC-24): the 30 second limit,
/// hiding on every other exit, and all or nothing across references.
@Suite(.timeLimit(.minutes(1)))
struct RevealModelTests {
    private static let user = "local:erp/postgres/user"
    private static let password = "local:erp/postgres/password"

    /// Records each reveal and answers from `values`; a reference without a value fails.
    final class Reveals: Sendable {
        private let log = Mutex<[(String, RevealPurpose)]>([])
        private let values: [String: String]

        init(_ values: [String: String]) {
            self.values = values
        }

        var made: [(reference: String, purpose: RevealPurpose)] {
            log.withLock { $0 }.map { (reference: $0.0, purpose: $0.1) }
        }

        var reveal: RevealModel.Reveal {
            { [self] reference, purpose throws(ClientError) in
                log.withLock { $0.append((reference, purpose)) }
                guard let value = values[reference] else {
                    throw SecretsScript.rejection(
                        "secret.store_locked", category: .unavailable,
                        message: "the keychain is locked")
                }
                return RevealedSecret(reference: reference, value: value)
            }
        }
    }

    private struct Fixture {
        let model: RevealModel
        let reveals: Reveals
        let sleep: ManualSleep
        let pasteboard: FakePasteboard
        let concealed: ConcealedPasteboard
    }

    private static func make(
        references: [String] = [user, password],
        values: [String: String],
        compose: @escaping RevealModel.Compose = { values in
            let user = values[RevealModelTests.user]?.value ?? "?"
            let secret = values[RevealModelTests.password]?.value ?? "?"
            return "postgres://\(user):\(secret)@127.0.0.1:5432/app"
        }
    ) -> Fixture {
        let reveals = Reveals(values)
        let sleep = ManualSleep()
        let pasteboard = FakePasteboard()
        let concealed = ConcealedPasteboard(
            pasteboard: pasteboard, sleep: { try await sleep.sleep(for: $0) })
        let model = RevealModel(
            references: references, reveal: reveals.reveal, compose: compose,
            pasteboard: concealed, sleep: { try await sleep.sleep(for: $0) })
        return Fixture(
            model: model, reveals: reveals, sleep: sleep, pasteboard: pasteboard,
            concealed: concealed)
    }

    @Test func showRevealsEveryReferenceForShowAndComposesTheText() async {
        let credential = FixtureCredential()
        let f = Self.make(values: [Self.user: "admin", Self.password: credential.value])

        await f.model.show()

        #expect(f.reveals.made.map(\.reference) == [Self.user, Self.password])
        #expect(f.reveals.made.allSatisfy { $0.purpose == .show })
        #expect(f.model.shownText == "postgres://admin:\(credential.value)@127.0.0.1:5432/app")
        #expect(f.model.isShown)
        #expect(f.model.phase == .idle)
        #expect(f.pasteboard.changeCount == 0)
    }

    @Test func aShownValueHidesAgainAfterThirtySeconds() async {
        let f = Self.make(values: [Self.user: "admin", Self.password: "fixture-secret-0009"])
        await f.model.show()
        await f.sleep.waitUntilSleeping()
        #expect(f.sleep.durations == [.seconds(30)])
        #expect(RevealModel.showDuration == .seconds(30))

        f.sleep.fire()
        while f.model.isShown { await Task.yield() }

        #expect(f.model.shownText == nil)
    }

    @Test func hideViewDisappearAndAnInactiveAppEachHideTheValue() async {
        let f = Self.make(values: [Self.user: "admin", Self.password: "fixture-secret-0010"])
        let exits: [(String, (RevealModel) -> Void)] = [
            ("Hide", { $0.hide() }), ("the view disappearing", { $0.viewDisappeared() }),
            ("the app becoming inactive", { $0.appBecameInactive() }),
        ]
        for (name, exit) in exits {
            await f.model.show()
            #expect(f.model.isShown, "\(name): the value shows first")
            exit(f.model)
            #expect(f.model.shownText == nil, "\(name) hides the value")
        }
    }

    @Test func hidingCancelsTheThirtySecondTimer() async {
        let f = Self.make(values: [Self.user: "admin", Self.password: "fixture-secret-0011"])
        await f.model.show()
        await f.sleep.waitUntilSleeping()
        f.model.hide()
        while f.sleep.waiting > 0 { await Task.yield() }
        #expect(f.sleep.waiting == 0)
    }

    @Test func oneFailedRevealShowsNothingAndDropsWhatWasReceived() async {
        let credential = FixtureCredential()
        let composed = Mutex(false)
        let f = Self.make(
            values: [Self.user: credential.value],
            compose: { _ in
                composed.withLock { $0 = true }
                return "unexpected"
            })

        await f.model.show()

        #expect(f.reveals.made.map(\.reference) == [Self.user, Self.password])
        #expect(f.model.shownText == nil)
        #expect(composed.withLock { $0 } == false)
        guard case .failed(let message) = f.model.phase else {
            Issue.record("expected a failure")
            return
        }
        #expect(message == "The keychain is locked.")
        credential.assertAbsent(in: message, what: "the failure message")
    }

    @Test func copyRevealsForCopyAndWritesTheComposedTextToTheConcealedPasteboard() async {
        let credential = FixtureCredential()
        let f = Self.make(values: [Self.user: "admin", Self.password: credential.value])

        await f.model.copy()

        #expect(f.reveals.made.allSatisfy { $0.purpose == .copy })
        #expect(f.pasteboard.holds("postgres://admin:\(credential.value)@127.0.0.1:5432/app"))
        #expect(f.pasteboard.preparedOptions == [.currentHostOnly])
        #expect(f.model.phase == .copied)
        #expect(f.model.shownText == nil)
        #expect(f.concealed.hasPendingClear)
    }

    @Test func oneFailedRevealCopiesNothing() async {
        let f = Self.make(values: [Self.password: "fixture-secret-0012"])
        await f.model.copy()
        #expect(f.pasteboard.changeCount == 0)
        #expect(f.pasteboard.strings.isEmpty)
        #expect(!f.concealed.hasPendingClear)
        if case .failed = f.model.phase {} else { Issue.record("expected a failure") }
    }

    @Test func aCompositionThatThrowsShowsAndCopiesNothing() async {
        struct Broken: Error {}
        let f = Self.make(
            values: [Self.user: "admin", Self.password: "fixture-secret-0013"],
            compose: { _ in throw Broken() })

        await f.model.show()
        #expect(f.model.shownText == nil)
        await f.model.copy()
        #expect(f.pasteboard.changeCount == 0)
        #expect(f.model.phase == .failed("Couldn’t build the text from the revealed values."))
    }

    @Test func aRepeatedReferenceIsRevealedOnce() async {
        let f = Self.make(
            references: [Self.password, Self.password],
            values: [Self.password: "fixture-secret-0014"],
            compose: { $0[Self.password]?.value ?? "?" })
        await f.model.show()
        #expect(f.model.references == [Self.password])
        #expect(f.reveals.made.count == 1)
        #expect(f.model.shownText == "fixture-secret-0014")
    }

    /// A reveal that waits until the test lets it answer, like a daemon behind a Keychain
    /// dialog.
    private final class Gate: Sendable {
        private let continuation = Mutex<CheckedContinuation<Void, Never>?>(nil)

        var isWaiting: Bool { continuation.withLock { $0 != nil } }

        func reveal(value: String) -> RevealModel.Reveal {
            { [self] reference, _ throws(ClientError) in
                await withCheckedContinuation { waiting in
                    continuation.withLock { $0 = waiting }
                }
                return RevealedSecret(reference: reference, value: value)
            }
        }

        func waitUntilWaiting() async {
            while !isWaiting { await Task.yield() }
        }

        func open() {
            let waiting = continuation.withLock { current in
                defer { current = nil }
                return current
            }
            waiting?.resume()
        }
    }

    private static func gated(
        _ gate: Gate, value: String, sleep: ManualSleep = ManualSleep(),
        pasteboard: FakePasteboard = FakePasteboard()
    ) -> RevealModel {
        RevealModel(
            references: [password], reveal: gate.reveal(value: value),
            compose: { $0[password]?.value ?? "?" },
            pasteboard: ConcealedPasteboard(
                pasteboard: pasteboard, sleep: { try await sleep.sleep(for: $0) }),
            sleep: { try await sleep.sleep(for: $0) })
    }

    @Test func aHideWhileShowWaitsExplainsItAndNeverShowsTheValue() async {
        let gate = Gate()
        let sleep = ManualSleep()
        let model = Self.gated(gate, value: "fixture-secret-0015", sleep: sleep)

        let showing = Task { await model.show() }
        await gate.waitUntilWaiting()
        model.hide()
        #expect(model.phase == .failed("Hidden before the value arrived. Press Show to try again."))
        gate.open()
        await showing.value

        #expect(model.shownText == nil)
        #expect(model.phase == .failed("Hidden before the value arrived. Press Show to try again."))
        #expect(sleep.durations.isEmpty, "a dropped value arms no timer")
    }

    @Test func theAppTurningInactiveWhileShowWaitsExplainsItAndShowsNothing() async {
        // The first reveal on the login Keychain opens a SecurityAgent dialog, which makes
        // Stackly inactive while the daemon still waits for Allow.
        let gate = Gate()
        let model = Self.gated(gate, value: "fixture-secret-0016")

        let showing = Task { await model.show() }
        await gate.waitUntilWaiting()
        model.appBecameInactive()
        gate.open()
        await showing.value

        #expect(model.shownText == nil)
        #expect(
            model.phase
                == .failed(
                    "Hidden because Stackly went to the background. Press Show to try again."))
    }

    @Test func aShowCancelledByTheAppTurningInactiveCanBeStartedAgain() async {
        let gate = Gate()
        let model = Self.gated(gate, value: "fixture-secret-0017")
        let first = Task { await model.show() }
        await gate.waitUntilWaiting()
        model.appBecameInactive()
        gate.open()
        await first.value

        let second = Task { await model.show() }
        await gate.waitUntilWaiting()
        gate.open()
        await second.value

        #expect(model.shownText == "fixture-secret-0017")
        #expect(model.phase == .idle)
    }

    @Test func theThirtySecondTimerFiringWhileCopyWaitsStillCopies() async {
        let credential = FixtureCredential()
        let gate = Gate()
        let sleep = ManualSleep()
        let pasteboard = FakePasteboard()
        let model = Self.gated(gate, value: credential.value, sleep: sleep, pasteboard: pasteboard)

        // Show first (answered at once), so the 30 second timer is armed.
        let showing = Task { await model.show() }
        await gate.waitUntilWaiting()
        gate.open()
        await showing.value
        await sleep.waitUntilSleeping()
        #expect(model.isShown)

        // Copy at second 29, then the timer fires before the daemon answers.
        let copying = Task { await model.copy() }
        while !gate.isWaiting { await Task.yield() }
        sleep.fire()
        while model.isShown { await Task.yield() }
        #expect(model.phase == .working)
        gate.open()
        await copying.value

        #expect(model.shownText == nil, "the timer still hides the shown text")
        #expect(model.phase == .copied)
        #expect(pasteboard.holds(credential.value))
    }

    @Test func theAppTurningInactiveWhileCopyWaitsStillCopies() async {
        let credential = FixtureCredential()
        let gate = Gate()
        let pasteboard = FakePasteboard()
        let model = Self.gated(gate, value: credential.value, pasteboard: pasteboard)

        let copying = Task { await model.copy() }
        await gate.waitUntilWaiting()
        model.appBecameInactive()
        #expect(model.phase == .working)
        gate.open()
        await copying.value

        #expect(model.phase == .copied)
        #expect(pasteboard.holds(credential.value))
        #expect(model.shownText == nil)
    }

    @Test func aCopyThatArrivesAfterTheClipboardChangedLeavesItAlone() async {
        let gate = Gate()
        let pasteboard = FakePasteboard()
        let model = Self.gated(gate, value: "fixture-secret-0018", pasteboard: pasteboard)

        let copying = Task { await model.copy() }
        await gate.waitUntilWaiting()
        // The person copies something else while the Keychain dialog is still open.
        _ = pasteboard.clearContents()
        _ = pasteboard.setString("something else", forType: .string)
        gate.open()
        await copying.value

        #expect(pasteboard.strings[.string] == "something else")
        #expect(!pasteboard.holds("fixture-secret-0018"))
        #expect(
            model.phase
                == .failed("Not copied because the clipboard changed. Press Copy to try again."))
    }
}
