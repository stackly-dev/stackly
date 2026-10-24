import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The live operation center against a real `stacklyd` (spec 0013 AC-18). It cancels
/// with the reason while the daemon answers `operation.list` with
/// `request.unknown_method`, which is the case until the feature 7 handlers land.
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
        if case .failure(let error) = probe, error.body.code == "request.unknown_method" {
            // An older daemon: the center says nothing rather than a false zero.
            #expect(model.operations.store.unsupported)
            #expect(model.operations.subtitle == nil)
            await parts.shutDown()
            await daemon.stop()
            try Test.cancel(
                "operation.list answered request.unknown_method: this stacklyd has no feature 7 operation handlers yet"
            )
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
}
