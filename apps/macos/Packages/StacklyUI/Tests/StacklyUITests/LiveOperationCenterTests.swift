import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The live operation center against a real `stacklyd` (spec 0013 AC-13 and AC-18):
/// snapshots, events from another client, and updates through completion.
@Suite(.tags(.daemon), .timeLimit(.minutes(1)))
struct LiveOperationCenterTests {
    /// What a stream of operation updates gave, and whether it ended.
    final class Collected {
        var values: [OperationSnapshot] = []
        var ended = false
    }

    @Test func theOperationCenterLoadsTheDaemonsActiveOperations() async throws {
        let daemon = try LiveDaemon(recordsEvidence: false)
        try await daemon.start()
        let parts = ShellEndToEndTests.shell(
            for: daemon)
        let model = parts.runtime.model
        try await ShellEndToEndTests.waitUntilRefreshed(model, "the shell to connect and refresh")
        let probe = await model.read(
            .operationList(
                ListOperationsParams(activeOnly: true, resource: nil, limit: nil, before: nil)))
        guard case .success(.operationList) = probe else {
            Issue.record("operation.list failed or returned the wrong reply: \(probe)")
            await parts.shutDown()
            await daemon.stop()
            return
        }
        try await waitFor("the operation snapshot", seconds: 20) {
            model.operations.runningCount != nil
        }
        // A fresh root has no operation, so nothing runs.
        #expect(model.operations.active.isEmpty)
        #expect(model.operations.runningCount == 0)
        #expect(model.operations.subtitle == "No running operations")
        #expect(model.sidebarCounts.runningOperations == 0)

        // An operation the daemon does not know ends its updates without a value.
        let unknown = try #require(UUID(uuidString: "0192f3a4-0000-7000-8000-00000000ffff"))
        let collected = Collected()
        let stream = model.operations.updates(for: unknown)
        Task {
            for await value in stream { collected.values.append(value) }
            collected.ended = true
        }
        try await waitFor("the updates of an unknown operation to end", seconds: 20) {
            collected.ended
        }
        #expect(collected.values.isEmpty)
        await parts.shutDown()
        await daemon.stop()
    }

    @Test func anotherClientsOperationUpdatesTheBadgeAndFinishesItsStream() async throws {
        try await SkeletonRig.run(recordsEvidence: false) { rig in
            let parts = ShellEndToEndTests.shell(for: rig.daemon)
            let model = parts.runtime.model
            let center = model.operations
            let collected = Collected()
            var watchedID: UUID?
            var watcher: Task<Void, Never>?
            let didApply = center.store.didApply
            defer {
                center.store.didApply = didApply
                watcher?.cancel()
            }
            do {
                try await ShellEndToEndTests.waitUntilRefreshed(model, "the shell to refresh")
                let rounds = model.sync.completedRounds
                let loadedAt = center.store.loadedAt

                // Register at the running event itself, before the final event can
                // arrive. No delay in the fake provider decides the test's order.
                center.store.didApply = { change in
                    didApply?(change)
                    guard case .changed(let operation) = change,
                        operation.state == .running, watchedID == nil
                    else { return }
                    watchedID = operation.id
                    #expect(center.active.contains { $0.id == operation.id })
                    #expect(center.runningCount == 1)
                    #expect(center.subtitle == "1 running")
                    #expect(model.sidebarCounts.runningOperations == 1)
                    let stream = center.updates(for: operation.id)
                    watcher = Task {
                        for await value in stream { collected.values.append(value) }
                        collected.ended = true
                    }
                }

                _ = try await rig.create("observed")
                let operationID = try #require(rig.startedOperations.last)
                #expect(watchedID == operationID)
                try await waitFor("the operation stream to finish", seconds: 20) {
                    collected.ended
                }
                #expect(collected.values.first?.state == .running)
                #expect(collected.values.last?.state == .succeeded)
                #expect(collected.values.allSatisfy { $0.id == operationID })
                for (earlier, later) in zip(collected.values, collected.values.dropFirst()) {
                    #expect(earlier.revision < later.revision)
                }
                #expect(center.active.isEmpty)
                #expect(center.recent.contains { $0.id == operationID && $0.state == .succeeded })
                #expect(center.runningCount == 0)
                #expect(center.subtitle == "No running operations")
                #expect(model.sidebarCounts.runningOperations == 0)
                #expect(model.sync.completedRounds == rounds)
                #expect(center.store.loadedAt == loadedAt)
            } catch {
                await parts.shutDown()
                throw error
            }
            await parts.shutDown()
        }
    }
}
