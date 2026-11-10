import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Operations page in each state of spec 0035 AC-19, from ``FixtureOperations``: the list
/// with a start running through its stages, an install past the step that can’t be undone with
/// Cancel off, a cancel that finishes safely, a failed start with Retry, a restore that needs
/// recovery, a cancelled create, and a stop that is done; the inspector open on the restore
/// with Resume and Abandon; the empty page; a first load that failed; an older daemon; and the
/// list while stacklyd is unreachable, with every mutation off.
extension SnapshotTests {
    struct OperationsPageSnapshotTests {
        /// The page area with the inspector open, narrow at the 900 pt window.
        private static let withInspector = SnapshotSize.fitting(width: 1_044, narrowWidth: 664)

        /// The page of a fixture over the instances of a ``PageRig``, so a failed start offers
        /// Retry, with the operation `selected` open in the inspector, `height` points tall.
        @MainActor
        private static func page(
            _ fixture: FixtureOperations.Case, selected: UUID? = nil, height: CGFloat = 748
        ) async -> some View {
            let rig = PageRig()
            await rig.loadInstances()
            let center = FixtureOperations.center(fixture)
            await center.store.refresh()
            let model = OperationsPageModel(
                service: FixtureOperations.service(fixture), center: center)
            await model.history.reload()
            if let selected {
                await model.detail.load(selected, revision: nil)
            }
            let offline = fixture == .offline
            return OperationsPage(
                model: model, operations: center, instances: rig.instances,
                actions: rig.actions,
                waiting: offline
                    ? FailureContent(
                        title: "Waiting for stacklyd", message: "stacklyd is unavailable.",
                        code: nil)
                    : nil,
                hasConnection: offline == false, reconnect: {}, openDiagnostics: {},
                selection: selected?.uuidString.lowercased() ?? ""
            )
            .environment(
                \.mutationAvailability,
                offline
                    ? .blocked(reason: "stacklyd is unavailable", since: FixtureClock.now)
                    : .available
            )
            .actionHost()
            .frame(height: height)
        }

        @Test(
            .evidence(
                covers: ["REQ-054", "NFR-012"],
                expected:
                    "the list shows a start with its stages, an install with an off Cancel past the point of no return, Finishing safely, a failed start with Retry, a restore that needs recovery, a cancelled create, and a finished stop"
            ))
        @MainActor
        func operationsWithEveryKindOfRow() async throws {
            try await assertSnapshot(
                await Self.page(.list), named: FixtureOperations.Case.list.rawValue,
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["REQ-054", "NFR-012"],
                expected:
                    "the inspector of a restore that needs recovery shows its steps, created items, recovery, Resume, and Abandon"
            ))
        @MainActor
        func operationsWithTheInspectorOpen() async throws {
            try await assertSnapshot(
                await Self.page(
                    .inspector, selected: FixtureOperations.recoveryID, height: 1_240),
                named: FixtureOperations.Case.inspector.rawValue, size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["REQ-054", "NFR-012"],
                expected: "the empty Operations page says No Operations"
            ))
        @MainActor
        func operationsWithNone() async throws {
            try await assertSnapshot(
                await Self.page(.empty), named: FixtureOperations.Case.empty.rawValue,
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["REQ-054", "NFR-012"],
                expected: "a first load that failed shows Couldn’t Load Operations with Retry"))
        @MainActor
        func operationsThatFailedToLoad() async throws {
            try await assertSnapshot(
                await Self.page(.loadFailed), named: FixtureOperations.Case.loadFailed.rawValue,
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["REQ-054", "NFR-012"],
                expected: "an older daemon without operation.list shows Operations Unavailable"))
        @MainActor
        func operationsOfAnOlderDaemon() async throws {
            try await assertSnapshot(
                await Self.page(.unsupported), named: FixtureOperations.Case.unsupported.rawValue,
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["REQ-054", "NFR-012"],
                expected:
                    "while stacklyd is unreachable the last list stays with Cancel and Retry off"))
        @MainActor
        func operationsWhileOffline() async throws {
            try await assertSnapshot(
                await Self.page(.offline), named: FixtureOperations.Case.offline.rawValue,
                size: Self.withInspector)
        }
    }
}
