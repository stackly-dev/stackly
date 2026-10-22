import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

extension Event {
    /// A `secret.changed` event for `reference`, as the daemon sends it.
    static func secretChanged(
        _ reference: String, status: SecretChangeStatus = .present, sequence: UInt64 = 1
    ) -> Event {
        Event(
            subscriptionId: 1, sequence: sequence, at: Date(timeIntervalSince1970: 0),
            resource: ResourceRef(kind: .secret, id: reference), revision: 1,
            kind: .secretChanged(SecretChanged(status: status, statusObservedAt: nil)))
    }
}

/// `ShellSync.events(kindPrefix:)` (spec 0013 as extended by spec 0021): a view with no store
/// of its own follows the events of one kind.
@MainActor
struct ShellSyncEventsTests {
    private static let reference = "local:instance/0192f000-0000-7000-8000-00000000000a/token"

    @Test func anObserverReceivesTheEventsOfItsPrefixOnly() async {
        let sync = ShellSync()
        var secrets = sync.events(kindPrefix: "secret.").makeAsyncIterator()
        sync.handle(.event(.instanceRemoved(UUID(), revision: 3)))
        sync.handle(.event(.secretChanged(Self.reference)))

        let event = await secrets.next()
        #expect(event?.resource?.id == Self.reference)
        #expect(event?.kind.kindName == "secret.changed")
    }

    @Test func twoObserversEachGetTheirOwnCopy() async {
        let sync = ShellSync()
        var first = sync.events(kindPrefix: "secret.").makeAsyncIterator()
        var second = sync.events(kindPrefix: "secret.changed").makeAsyncIterator()
        sync.handle(.event(.secretChanged(Self.reference)))

        #expect(await first.next()?.resource?.id == Self.reference)
        #expect(await second.next()?.resource?.id == Self.reference)
    }

    @Test func stoppingTheSyncEndsEveryStream() async {
        let sync = ShellSync()
        var secrets = sync.events(kindPrefix: "secret.").makeAsyncIterator()
        sync.stop()

        #expect(await secrets.next() == nil)
    }

    @Test func aDroppedStreamIsNotFedAgain() async throws {
        let sync = ShellSync()
        do {
            _ = sync.events(kindPrefix: "secret.")
        }
        // The observer is removed on the main actor after the stream goes away.
        for _ in 0..<200 where sync.observerCount > 0 {
            try await Task.sleep(for: .milliseconds(5))
        }
        #expect(sync.observerCount == 0)
    }
}
