import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Ports sheet of an instance an environment's lease holds (spec 0047, AC-23) in light,
/// dark, and narrow: the caution that names the environment and Change Anyway.
extension SnapshotTests {
    struct EnvironmentOverrideSnapshotTests {
        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Ports sheet of an instance erp-development holds shows the caution that names it, and its button reads Change Anyway"
            ))
        @MainActor
        func portsSheetOfALeasedInstance() async throws {
            let model = await FixtureConfiguration.endpointsModel(
                typing: "5440",
                leases: [
                    InstanceEnvironment(
                        environmentId: FixtureEnvironments.erpID, name: "erp-development",
                        alias: "postgres", ownership: .dedicated, leaseActive: true)
                ])
            try await assertSnapshot(
                EndpointsSheet(model: model, context: InstanceActionContext(report: { _ in })),
                named: "ports-sheet-leased",
                size: SnapshotSize.fitting(width: 520, narrowWidth: 480))
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Ports sheet of an instance two environments hold names both in the plural caution, those environments, and its button reads Change Anyway"
            ))
        @MainActor
        func portsSheetOfAnInstanceTwoEnvironmentsHold() async throws {
            let model = await FixtureConfiguration.endpointsModel(
                typing: "5440",
                leases: [
                    InstanceEnvironment(
                        environmentId: FixtureEnvironments.erpID, name: "erp-development",
                        alias: "postgres", ownership: .dedicated, leaseActive: true),
                    InstanceEnvironment(
                        environmentId: UUID(), name: "reporting", alias: "db",
                        ownership: .shared, leaseActive: true),
                ])
            try await assertSnapshot(
                EndpointsSheet(model: model, context: InstanceActionContext(report: { _ in })),
                named: "ports-sheet-leased-two",
                size: SnapshotSize.fitting(width: 520, narrowWidth: 480))
        }
    }
}
