import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The lease in the app (spec 0047, AC-22) in light, dark, and narrow: the Lease row of the
/// environment page while the lease holds three instances, and "Shared · lease" in the
/// inspector of an instance an environment holds.
extension SnapshotTests {
    struct EnvironmentLeaseSnapshotTests {
        /// The pages this suite covers, named for the snapshot source rule.
        static let pages = [
            String(describing: EnvironmentDetailPage.self),
            String(describing: InstancesPage.self),
        ]

        /// erp-development with its lease held on three instances since two hours ago.
        private static func leasedDetail() throws -> EnvironmentDetail {
            let base = try #require(FixtureEnvironments.detail(FixtureEnvironments.erpID))
            return EnvironmentDetail(
                summary: base.summary, services: base.services, detached: base.detached,
                manifestState: base.manifestState, problems: base.problems, lock: base.lock,
                local: base.local, lastUp: base.lastUp,
                lease: EnvLeaseInfo(
                    activeCount: 3, heldSince: FixtureClock.now.addingTimeInterval(-7_200)))
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Details section of the environment page has the Lease row: Lease held on 3 instances since a time today, and what Down stops"
            ))
        @MainActor
        func detailWithAHeldLease() async throws {
            let detail = try Self.leasedDetail()
            let rig = PageRig(
                instances: FixtureEnvironments.instances,
                environments: FixtureEnvironments.summaries)
            rig.script.environmentDetails[detail.summary.id] = detail
            await rig.load()
            try await assertSnapshot(
                await rig.environmentsPage(selected: FixtureEnvironments.erpID),
                named: "environment-lease-held")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The inspector of an instance whose environment lease is active says Shared · lease in the Ownership row"
            ))
        @MainActor
        func inspectorWithALease() async throws {
            let instances = FixtureEnvironments.instances.map { summary in
                guard summary.name == "queue" else { return summary }
                return summary.with(environments: [
                    InstanceEnvironment(
                        environmentId: FixtureEnvironments.erpID, name: "erp-development",
                        alias: "queue", ownership: .shared, leaseActive: true)
                ])
            }
            let rig = PageRig(instances: instances)
            await rig.loadInstances()
            let id = FixtureID.uuid(kind: FixtureID.instance, number: 5).uuidString.lowercased()
            try await assertSnapshot(
                rig.instancesPage(selection: id).frame(height: 748),
                named: "instances-inspector-lease",
                size: SnapshotSize.fitting(width: 1_044, narrowWidth: 664))
        }
    }
}
