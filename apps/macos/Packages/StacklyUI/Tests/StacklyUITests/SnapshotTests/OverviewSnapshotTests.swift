import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Overview in every variant (spec 0068 AC-8, spec 0022 AC-13): the cards, Needs
/// Attention with every rule and the disk, the instances, the operations, and every state of
/// the page and of its sections. The narrow variant is the 640 pt content column: one column
/// and no cut text.
extension SnapshotTests {
    struct OverviewSnapshotTests {
        /// The page these snapshots show, named so the source rule finds its suite.
        private typealias Page = OverviewPage

        private static let errorBody = PageRig.failure

        @MainActor
        private func rig(
            instances: [InstanceSummary] = FixtureScenario.instanceCases,
            operations: [OperationSummary] = FixtureScenario.operationCases,
            capacity: CapacityReport? = nil, load: Bool = true,
            prepare: (PageRig) -> Void = { _ in }
        ) async -> PageRig {
            let rig = PageRig(instances: instances, operations: operations, capacity: capacity)
            prepare(rig)
            if load { await rig.load() }
            return rig
        }

        private static func report(_ level: CapacityLevel, available: UInt64) -> CapacityReport {
            CapacityReport(
                checkedAt: FixtureClock.now, totalBytes: 994_662_584_320, availableBytes: available,
                usedPercent: UInt8((994_662_584_320 - available) * 100 / 994_662_584_320),
                level: level)
        }

        // MARK: - Loaded

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview of a mixed set of instances renders in every variant"
            ))
        func overviewOfMixedInstances() async throws {
            let rig = await rig(capacity: Self.report(.low, available: 4_100_000_000))
            try await assertSnapshot(rig.overviewPage(), named: "overview-mixed")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview with a critical disk renders in every variant"
            ))
        func overviewWithACriticalDisk() async throws {
            let rig = await rig(
                instances: [FixtureScenario.instanceCases[0], FixtureScenario.instanceCases[3]],
                operations: [],
                capacity: Self.report(.critical, available: 1_700_000_000))
            try await assertSnapshot(rig.overviewPage(), named: "overview-disk-critical")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview of a healthy Mac renders in every variant"
            ))
        func overviewWhereNothingNeedsAttention() async throws {
            let rig = await rig(
                instances: [FixtureScenario.instanceCases[0], FixtureScenario.instanceCases[3]],
                operations: [], capacity: Self.report(.ok, available: 500_000_000_000))
            try await assertSnapshot(rig.overviewPage(), named: "overview-healthy")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview without instances renders in every variant"
            ))
        func overviewWithoutInstances() async throws {
            let rig = await rig(
                instances: [], operations: [],
                capacity: Self.report(.ok, available: 500_000_000_000))
            try await assertSnapshot(rig.overviewPage(), named: "overview-empty")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Overview of a daemon that cannot report the disk renders in every variant"
            ))
        func overviewWhereTheDiskIsUnavailable() async throws {
            let rig = await rig(
                instances: [FixtureScenario.instanceCases[0]], operations: [], capacity: nil)
            try await assertSnapshot(rig.overviewPage(), named: "overview-disk-unavailable")
        }

        // MARK: - States of the page

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview before the first snapshot renders in every variant"
            ))
        func overviewBeforeTheFirstSnapshot() async throws {
            let rig = await rig(load: false)
            try await assertSnapshot(
                rig.overviewPage(), named: "overview-loading", contrastInvariant: true,
                contrastInvariantReason:
                    "only a system spinner, which Increase Contrast does not change")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview that failed to load renders in every variant"
            ))
        func overviewThatFailedToLoad() async throws {
            let rig = await rig(load: false) {
                $0.script.instanceFailure = .rejected(Self.errorBody)
            }
            await rig.load()
            try await assertSnapshot(rig.overviewPage(), named: "overview-failed")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview while the daemon cannot be reached renders in every variant"
            ))
        func overviewWhileWaitingForTheDaemon() async throws {
            let rig = await rig(load: false)
            try await assertSnapshot(
                rig.overviewPage(
                    isConnected: false,
                    waiting: FailureContent(
                        title: "Waiting for stacklyd",
                        message: "Stackly shows this page once the daemon is reachable.",
                        code: "ipc.endpoint_missing"),
                    hasConnection: false),
                named: "overview-waiting")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Overview of a daemon that cannot list instances renders in every variant"
            ))
        func overviewOfADaemonWithoutTheMethod() async throws {
            let rig = await rig(load: false) {
                $0.script.instanceFailure = .rejected(PageRig.unknownMethod)
            }
            await rig.load()
            try await assertSnapshot(rig.overviewPage(), named: "overview-daemon-unsupported")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview after a failed refresh renders in every variant"
            ))
        func overviewAfterAFailedRefresh() async throws {
            let rig = await rig(capacity: Self.report(.ok, available: 500_000_000_000))
            rig.script.instanceFailure = .rejected(Self.errorBody)
            await rig.instances.store.refresh()
            try await assertSnapshot(rig.overviewPage(), named: "overview-stale")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Overview with an instance that could not be refreshed renders in every variant"
            ))
        func overviewWithAnInstanceThatCouldNotBeRefreshed() async throws {
            let rig = await rig(capacity: Self.report(.ok, available: 500_000_000_000))
            rig.script.oneFailure = .rejected(Self.errorBody)
            let stranger = InstanceSummary.fixture(number: 40, name: "newcomer")
            rig.instances.store.receive(
                Event(
                    subscriptionId: 1, sequence: 1, at: FixtureClock.now,
                    resource: ResourceRef(kind: .instance, id: stranger.id.uuidString.lowercased()),
                    revision: 9,
                    kind: .instanceHealthChanged(
                        InstanceHealthChanged(
                            instanceId: stranger.id, statusSeq: 50, readiness: stranger.readiness,
                            health: stranger.health, blocker: nil, restart: stranger.restart,
                            process: nil, logsUnavailable: false))))
            try await waitUntil { rig.instances.store.itemFailures.isEmpty == false }
            try await assertSnapshot(rig.overviewPage(), named: "overview-partial")
        }

        // MARK: - States of a section

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview whose operations failed to load renders in every variant"
            ))
        func overviewWhoseOperationsFailedToLoad() async throws {
            let rig = await rig(load: false) {
                $0.script.operationFailure = .rejected(Self.errorBody)
            }
            await rig.load()
            try await assertSnapshot(rig.overviewPage(), named: "overview-operations-failed")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Overview of a daemon that cannot list operations renders in every variant"
            ))
        func overviewOfADaemonWithoutOperations() async throws {
            let rig = await rig(load: false) {
                $0.script.operationFailure = .rejected(PageRig.unknownMethod)
            }
            await rig.load()
            try await assertSnapshot(rig.overviewPage(), named: "overview-operations-unsupported")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Overview with many running operations renders in every variant"
            ))
        func overviewWithManyOperations() async throws {
            var many: [OperationSummary] = []
            for number in 1...7 {
                let installs = number % 2 == 0
                let state: OperationState = number == 3 ? .cancelRequested : .running
                let progress: OperationProgress =
                    installs ? .counted(CountedProgress(done: 30, total: 100)) : .indeterminate
                many.append(
                    .fixture(
                        number: UInt8(number), kind: installs ? .installRuntime : .startInstance,
                        state: state, stage: installs ? "verify_digest" : nil, progress: progress))
            }
            let rig = await rig(
                instances: Array(FixtureScenario.instanceCases.prefix(3)), operations: many,
                capacity: Self.report(.ok, available: 500_000_000_000))
            try await assertSnapshot(rig.overviewPage(), named: "overview-operations-many")
        }
    }
}
