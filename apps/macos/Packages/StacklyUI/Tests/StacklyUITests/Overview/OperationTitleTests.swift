import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The title of every operation kind (spec 0022, *Seksi Operations*).
struct OperationTitleTests {
    private let instanceID = FixtureID.uuid(kind: FixtureID.instance, number: 1)
    private let runtimeID = FixtureID.uuid(kind: FixtureID.runtime, number: 1)

    private func title(
        _ kind: OperationKind, resource: ResourceRef? = nil, known: Bool = true,
        runtimes: OperationTitle.RuntimeNames = .none, number: UInt8 = 1
    ) -> String {
        OperationTitle.title(
            for: .fixture(number: number, kind: kind, resource: resource),
            instanceNames: known ? [instanceID: "pg17-dev"] : [:], runtimes: runtimes)
    }

    private var instance: ResourceRef {
        ResourceRef(kind: .instance, id: instanceID.uuidString.lowercased())
    }

    @Test func anInstanceVerbNamesTheInstanceOrTheWordInstance() {
        let verbs: [(OperationKind, String)] = [
            (.startInstance, "Start"), (.stopInstance, "Stop"), (.restartInstance, "Restart"),
            (.reloadInstance, "Reload"), (.createInstance, "Create"),
            (.removeInstance, "Remove"), (.purgeInstance, "Purge"),
            (.backupInstance, "Back up"), (.restoreInstance, "Restore"),
            (.renameInstance, "Rename"),
        ]
        for (kind, word) in verbs {
            #expect(title(kind, resource: instance) == "\(word) pg17-dev", "\(kind)")
            #expect(title(kind, resource: instance, known: false) == "\(word) instance", "\(kind)")
            #expect(title(kind) == "\(word) instance", "\(kind)")
        }
    }

    @Test func migrationAndRebindNameTheInstance() {
        #expect(title(.migrateData, resource: instance) == "Migrate data of pg17-dev")
        #expect(title(.rebindRuntime, resource: instance) == "Change runtime of pg17-dev")
        #expect(title(.migrateData) == "Migrate data of instance")
    }

    @Test func installUsesTheInstallingRowOfTheSameOperation() {
        let id = FixtureID.uuid(kind: FixtureID.operation, number: 1)
        let runtimes = OperationTitle.RuntimeNames(installing: [id: "PostgreSQL 17.2"])
        #expect(title(.installRuntime, runtimes: runtimes) == "Install PostgreSQL 17.2")
        #expect(title(.installRuntime) == "Install runtime")
        #expect(title(.installRuntime, runtimes: runtimes, number: 2) == "Install runtime")
    }

    @Test func removeRuntimeUsesTheInstalledRuntime() {
        let runtimes = OperationTitle.RuntimeNames(installed: [runtimeID: "Valkey 8.1"])
        let resource = ResourceRef(kind: .runtime, id: runtimeID.uuidString.lowercased())
        #expect(
            title(.removeRuntime, resource: resource, runtimes: runtimes) == "Remove Valkey 8.1")
        #expect(title(.removeRuntime, resource: resource) == "Remove runtime")
    }

    @Test func theOtherKindsHaveFixedTitles() {
        #expect(title(.registerExternalRuntime) == "Register external runtime")
        #expect(title(.environmentUp) == "Bring up environment")
        #expect(title(.environmentDown) == "Bring down environment")
        #expect(title(.updateProviderRevision) == "Update provider")
        #expect(title(.updateManager) == "Update Stackly")
        #expect(title(.unrecognized) == "Operation")
    }
}
