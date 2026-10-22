import Foundation
import StacklyClient

/// The title of an operation in a row, an item, and a feedback card (spec 0022, *Seksi
/// Operations*). A pure function that the Operations page of feature 35 uses too.
nonisolated public enum OperationTitle {
    /// The runtime names an operation title can use. ``init(store:)`` fills them from the
    /// `RuntimeStore`; without one both tables are empty and the titles say "runtime".
    public struct RuntimeNames: Equatable, Sendable {
        /// "{display name} {version}" of the runtime an `install_runtime` operation installs,
        /// by the ID of that operation (the `installing` rows of spec 0018).
        public var installing: [UUID: String]
        /// "{display name} {version}" of an installed runtime, by the runtime ID.
        public var installed: [UUID: String]

        /// Creates the tables.
        public init(installing: [UUID: String] = [:], installed: [UUID: String] = [:]) {
            self.installing = installing
            self.installed = installed
        }

        /// Reads the names the `RuntimeStore` holds: the installs that have not ended by the
        /// ID of their operation, and the runtimes by their ID. A `nil` store knows none.
        ///
        /// - Parameter store: The store of feature 18, or `nil`.
        @MainActor public init(store: RuntimeStore?) {
            guard let store else {
                self.init()
                return
            }
            self.init(
                installing: Dictionary(
                    store.installing.map { ($0.operationId, "\($0.displayName) \($0.version)") },
                    uniquingKeysWith: { first, _ in first }),
                installed: Dictionary(
                    store.runtimes.map { ($0.id, RuntimeCopy.name(of: $0)) },
                    uniquingKeysWith: { first, _ in first }))
        }

        /// No runtime is known.
        public static var none: RuntimeNames { RuntimeNames() }
    }

    /// The title of an operation.
    ///
    /// - Parameters:
    ///   - operation: The operation.
    ///   - instanceNames: The names of the known instances by ID.
    ///   - runtimes: The names of the known runtimes.
    public static func title(
        for operation: OperationSummary,
        instanceNames: [UUID: String],
        runtimes: RuntimeNames = .none
    ) -> String {
        let instance = resourceID(operation, kind: .instance).flatMap { instanceNames[$0] }
        let subject = instance ?? "instance"
        switch operation.kind {
        case .installRuntime:
            return runtimes.installing[operation.id].map { "Install \($0)" } ?? "Install runtime"
        case .startInstance: return "Start \(subject)"
        case .stopInstance: return "Stop \(subject)"
        case .restartInstance: return "Restart \(subject)"
        case .reloadInstance: return "Reload \(subject)"
        case .createInstance: return "Create \(subject)"
        case .removeInstance: return "Remove \(subject)"
        case .purgeInstance: return "Purge \(subject)"
        case .backupInstance: return "Back up \(subject)"
        case .restoreInstance: return "Restore \(subject)"
        case .renameInstance: return "Rename \(subject)"
        case .migrateData: return "Migrate data of \(subject)"
        case .rebindRuntime: return "Change runtime of \(subject)"
        case .removeRuntime:
            let runtime = resourceID(operation, kind: .runtime).flatMap { runtimes.installed[$0] }
            return runtime.map { "Remove \($0)" } ?? "Remove runtime"
        case .registerExternalRuntime: return "Register external runtime"
        case .environmentUp: return "Bring up environment"
        case .environmentDown: return "Bring down environment"
        case .updateProviderRevision: return "Update provider"
        case .updateManager: return "Update Stackly"
        case .unrecognized: return "Operation"
        }
    }

    /// The ID of the operation's resource when it has the given kind.
    private static func resourceID(_ operation: OperationSummary, kind: ResourceKind) -> UUID? {
        guard let resource = operation.resource, resource.kind == kind else { return nil }
        return UUID(uuidString: resource.id)
    }
}
