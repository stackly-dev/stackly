import AppKit
import Foundation
import Testing

@testable import StacklyUI

/// The pasteboard write and its clear (spec 0011 AC-19, *Reveal dan copy*).
@Suite(.timeLimit(.minutes(1)))
struct ConcealedPasteboardTests {
    private struct Fixture {
        let concealed: ConcealedPasteboard
        let pasteboard: FakePasteboard
        let sleep: ManualSleep
        let center: NotificationCenter
    }

    private static func make() -> Fixture {
        let pasteboard = FakePasteboard()
        let sleep = ManualSleep()
        let center = NotificationCenter()
        let concealed = ConcealedPasteboard(
            pasteboard: pasteboard, sleep: { try await sleep.sleep(for: $0) },
            clearsOnTerminationOf: center)
        return Fixture(concealed: concealed, pasteboard: pasteboard, sleep: sleep, center: center)
    }

    @Test func aCopyStaysOnThisHostAndCarriesTheConcealedAndTransientMarkers() {
        let f = Self.make()
        f.concealed.copy("fixture-secret-0016")

        #expect(f.pasteboard.preparedOptions == [.currentHostOnly])
        #expect(f.pasteboard.strings[.string] == "fixture-secret-0016")
        let concealed = NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")
        let transient = NSPasteboard.PasteboardType("org.nspasteboard.TransientType")
        #expect(f.pasteboard.strings[concealed] != nil)
        #expect(f.pasteboard.strings[transient] != nil)
        #expect(ConcealedPasteboard.concealedType == concealed)
        #expect(ConcealedPasteboard.transientType == transient)
    }

    @Test func theClearDelayIsNinetySeconds() async {
        let f = Self.make()
        f.concealed.copy("fixture-secret-0017")
        await f.sleep.waitUntilSleeping()
        #expect(ConcealedPasteboard.clearDelay == .seconds(90))
        #expect(f.sleep.durations == [.seconds(90)])
    }

    @Test func theValueIsClearedAfterTheDelayWhenNothingElseWasCopied() async {
        let f = Self.make()
        f.concealed.copy("fixture-secret-0018")
        await f.sleep.waitUntilSleeping()

        f.sleep.fire()
        while f.pasteboard.holds("fixture-secret-0018") { await Task.yield() }

        #expect(f.pasteboard.strings.isEmpty)
        #expect(!f.concealed.hasPendingClear)
    }

    @Test func somethingCopiedSinceStaysOnThePasteboard() async {
        let f = Self.make()
        f.concealed.copy("fixture-secret-0019")
        await f.sleep.waitUntilSleeping()
        // Another app copies: the change count moves on.
        _ = f.pasteboard.clearContents()
        _ = f.pasteboard.setString("something else", forType: .string)

        f.sleep.fire()
        while f.concealed.hasPendingClear { await Task.yield() }

        #expect(f.pasteboard.strings[.string] == "something else")
    }

    @Test func quittingTheAppClearsAnUnchangedPasteboardAtOnce() {
        let f = Self.make()
        f.concealed.copy("fixture-secret-0020")

        f.center.post(name: NSApplication.willTerminateNotification, object: nil)

        #expect(f.pasteboard.strings.isEmpty)
        #expect(!f.concealed.hasPendingClear)
    }

    @Test func quittingTheAppLeavesAChangedPasteboardAlone() {
        let f = Self.make()
        f.concealed.copy("fixture-secret-0021")
        _ = f.pasteboard.clearContents()
        _ = f.pasteboard.setString("something else", forType: .string)

        f.center.post(name: NSApplication.willTerminateNotification, object: nil)

        #expect(f.pasteboard.strings[.string] == "something else")
    }

    @Test func aSecondCopyReplacesTheFirstAndRestartsTheDelay() async {
        let f = Self.make()
        f.concealed.copy("fixture-secret-0022")
        await f.sleep.waitUntilSleeping()
        f.concealed.copy("fixture-secret-0023")
        await f.sleep.waitUntilSleeping()

        #expect(f.pasteboard.strings[.string] == "fixture-secret-0023")
        #expect(f.sleep.durations == [.seconds(90), .seconds(90)])
        f.sleep.fire()
        while f.concealed.hasPendingClear { await Task.yield() }
        #expect(f.pasteboard.strings.isEmpty)
    }

    @Test func clearingWithNothingCopiedDoesNothing() {
        let f = Self.make()
        f.concealed.clearIfUnchanged()
        #expect(f.pasteboard.changeCount == 0)
    }
}
