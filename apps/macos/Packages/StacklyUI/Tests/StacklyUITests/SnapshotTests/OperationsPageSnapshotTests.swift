import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Operations page (spec 0035, AC-11): a list with a running start, an install past the
/// step that can’t be undone, a failure, an operation that needs recovery, and one that is
/// done; the empty page; and an older daemon. Step 9 of spec 0035 adds the full set of AC-19
/// (fixtures `FixtureOperations`, the inspector, offline, cancel `continuing`).
extension SnapshotTests {
    struct OperationsPageSnapshotTests {
        /// The page area with the inspector open, narrow at the 900 pt window.
        private static let withInspector = SnapshotSize.fitting(width: 1_044, narrowWidth: 664)

        @MainActor
        private static func page(
            _ operations: [OperationSummary], center: OperationCenter = OperationsFake.center()
        ) async -> some View {
            let fake = OperationsFake()
            fake.pages[nil] = OperationPage(items: operations, next: nil)
            let model = OperationsPageModel(service: fake.service, center: center)
            await model.history.reload()
            return OperationsPage(
                model: model, operations: center, waiting: nil, hasConnection: true,
                reconnect: {}, openDiagnostics: {}
            )
            .frame(height: 748)
        }

        @MainActor
        private static var rows: [OperationSummary] {
            [
                OperationSample.make(
                    6, kind: .startInstance, state: .running, stage: "spawn", cancel: .available),
                OperationSample.make(
                    5, kind: .installRuntime, instance: nil, state: .running, stage: "commit",
                    progress: .counted(CountedProgress(done: 1, total: 2)),
                    cancel: .pastPointOfNoReturn, pointOfNoReturnAt: OperationSample.base),
                OperationSample.make(
                    4, kind: .startInstance, state: .failed, cancel: CancelAvailability.none,
                    failure: OperationSample.failure(
                        "supervisor.endpoint_in_use", "port 5432 is held by another process")),
                OperationSample.make(
                    3, kind: .restoreInstance, state: .needsRecovery,
                    cancel: CancelAvailability.none,
                    recovery: OperationSample.recovery(.resume, actions: [.resume, .abandon])),
                OperationSample.make(
                    2, kind: .stopInstance, state: .succeeded, cancel: CancelAvailability.none),
            ]
        }

        @Test(
            .evidence(
                covers: ["REQ-054", "NFR-012"],
                expected:
                    "the Operations page shows each row's note, time and ID, badge, progress, and an off Cancel past the point of no return"
            ))
        @MainActor
        func operationsWithEveryKindOfRow() async throws {
            try await assertSnapshot(
                await Self.page(Self.rows), named: "operations-list", size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"], expected: "the empty Operations page says No Operations"))
        @MainActor
        func operationsWithNone() async throws {
            try await assertSnapshot(
                await Self.page([]), named: "operations-empty", size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "an older daemon without operation.list shows Operations Unavailable"))
        @MainActor
        func operationsOfAnOlderDaemon() async throws {
            let center = OperationCenter(
                fetchPage: { _ throws(ClientError) in
                    throw .rejected(OperationsFake.error("request.unknown_method"))
                },
                fetchOne: { _ throws(ClientError) in nil }, sleep: { _ in })
            await center.store.refresh()
            try await assertSnapshot(
                await Self.page([], center: center), named: "operations-unsupported",
                size: Self.withInspector)
        }
    }
}
