import Foundation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// The Stop All flow and the Services section of the menu bar model (spec 0032 AC-3, AC-6 to
/// AC-8): the model sends exactly the IDs of the preview it showed, reads the preview again
/// when the daemon says the list changed, and never asks while the daemon is away.
@MainActor
struct MenuBarModelTests {
    private let first = UUID(uuidString: "0192f3a5-0000-7000-8000-000000000001") ?? UUID()
    private let second = UUID(uuidString: "0192f3a5-0000-7000-8000-000000000002") ?? UUID()

    private func item(_ id: UUID, _ name: String) -> StopAllItem {
        StopAllItem(
            instanceId: id, name: name, lifecycle: .running, demand: [.manual],
            autostart: InstanceAutostart(policy: .manual, session: .none))
    }

    private func preview(_ ids: [(UUID, String)]) -> StopAllPreview {
        StopAllPreview(
            items: ids.map { item($0.0, $0.1) }, alreadyStopping: [], skipped: [],
            atLoginCount: 0)
    }

    /// The previews the daemon answers with, one per read; an empty one when they run out.
    private final class Previews: Sendable {
        private let queue: Mutex<[StopAllPreview]>

        init(_ previews: [StopAllPreview]) {
            queue = Mutex(previews)
        }

        func next() -> StopAllPreview? {
            queue.withLock { $0.isEmpty ? nil : $0.removeFirst() }
        }
    }

    private func model(
        connected: Bool = true,
        previews: Previews,
        stop: @escaping @Sendable ([UUID], String) throws(ClientError) -> StopAllResult
    ) -> MenuBarModel {
        let rig = DaemonModelTests.Fixture()
        return MenuBarModel(
            daemon: rig.model, connected: { connected },
            previewStopAll: { () throws(ClientError) in
                previews.next() ?? self.preview([])
            },
            stopAll: stop)
    }

    private func settle(_ model: MenuBarModel, until done: (MenuBarModel.StopAllPhase) -> Bool)
        async throws
    {
        for _ in 0..<200 where done(model.phase) == false {
            try await Task.sleep(for: .milliseconds(5))
        }
        #expect(done(model.phase), "stuck in \(model.phase)")
    }

    @Test func confirmingSendsTheIDsOfThePreviewWithAKeyAndShowsTheResult() async throws {
        let shown = preview([(first, "alpha"), (second, "bravo")])
        let sent = Mutex<[([UUID], String)]>([])
        let result = StopAllResult(stopped: [first, second], failed: [], skipped: [])
        let model = model(previews: Previews([shown])) { ids, key throws(ClientError) in
            sent.withLock { $0.append((ids, key)) }
            return result
        }

        model.beginStopAll()
        try await settle(model) { if case .preview = $0 { true } else { false } }
        model.confirmStopAll()
        try await settle(model) { if case .result = $0 { true } else { false } }

        let call = try #require(sent.withLock { $0.first })
        #expect(call.0 == [first, second])
        #expect(call.1.isEmpty == false)
        #expect(model.phase == .result(shown, result))
    }

    @Test func aListThatChangedIsReadAgainAndStopsNothing() async throws {
        let old = preview([(first, "alpha")])
        let new = preview([(first, "alpha"), (second, "bravo")])
        let model = model(previews: Previews([old, new])) { _, _ throws(ClientError) in
            throw .rejected(
                ErrorBody(
                    code: "instance.stop_all_changed", category: .conflict,
                    message: "the instances to stop changed since the preview",
                    retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
                    operationId: nil, rule: nil, items: nil))
        }

        model.beginStopAll()
        try await settle(model) { if case .preview = $0 { true } else { false } }
        model.confirmStopAll()
        try await settle(model) {
            if case .preview(_, let changed) = $0 { changed } else { false }
        }

        #expect(model.phase == .preview(new, changed: true))
    }

    @Test func anotherRefusalEndsInASentenceNotARetry() async throws {
        let shown = preview([(first, "alpha")])
        let model = model(previews: Previews([shown])) { _, _ throws(ClientError) in
            throw .notSent(ClientErrorCatalog.notConnected())
        }

        model.beginStopAll()
        try await settle(model) { if case .preview = $0 { true } else { false } }
        model.confirmStopAll()
        try await settle(model) { if case .failed = $0 { true } else { false } }

        #expect(model.phase == .failed(MutationFeedback.notSent))
    }

    @Test func nothingRunningCannotBeConfirmed() async throws {
        let sent = Mutex(0)
        let model = model(previews: Previews([preview([])])) { _, _ throws(ClientError) in
            sent.withLock { $0 += 1 }
            return StopAllResult(stopped: [], failed: [], skipped: [])
        }

        model.beginStopAll()
        try await settle(model) { if case .preview = $0 { true } else { false } }
        model.confirmStopAll()

        #expect(sent.withLock { $0 } == 0)
        #expect(model.phase == .preview(preview([]), changed: false))
    }

    @Test func whileTheDaemonIsAwayStopAllCannotBeAsked() {
        let model = model(connected: false, previews: Previews([])) { _, _ throws(ClientError) in
            StopAllResult(stopped: [], failed: [], skipped: [])
        }

        model.beginStopAll()

        #expect(model.phase == .idle)
        #expect(model.canBeginStopAll == false)
        #expect(model.stopAllDisabledReason == "stacklyd isn’t reachable.")
        #expect(model.iconVariant == .offline)
    }

    @Test func cancelClosesThePreviewWithoutStopping() async throws {
        let sent = Mutex(0)
        let model = model(previews: Previews([preview([(first, "alpha")])])) {
            _, _ throws(ClientError) in
            sent.withLock { $0 += 1 }
            return StopAllResult(stopped: [], failed: [], skipped: [])
        }

        model.beginStopAll()
        try await settle(model) { if case .preview = $0 { true } else { false } }
        model.dismissStopAll()

        #expect(model.phase == .idle)
        #expect(sent.withLock { $0 } == 0)
    }

    @Test func theSectionShowsEightRowsAndNamesTheRest() throws {
        let rig = DaemonModelTests.Fixture()
        let menuBar = MenuBarModel(daemon: rig.model, connected: { true })
        let base = try InstanceContractFixture.mixed()[0]
        for index in 0..<12 {
            rig.model.instances.offer(
                base.testing(
                    id: UUID(), name: String(format: "svc%02d", index), lifecycle: .running))
        }

        #expect(menuBar.rows.count == 8)
        #expect(menuBar.hiddenCount == 4)
        #expect(menuBar.totalCount == 12)
        #expect(MenuBarCopy.showAll(menuBar.totalCount) == "Show all 12 in Stackly…")
    }
}
