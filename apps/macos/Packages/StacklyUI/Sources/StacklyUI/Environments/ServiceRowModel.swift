import Foundation
import StacklyClient

/// One row of the Services table of an environment (spec 0044, *Layar*): the words of each
/// column from the service and, when the instance store holds it, the instance summary.
struct ServiceRowModel: Identifiable, Equatable {
    /// The service.
    let service: EnvironmentService
    /// The instance summary from the instance store, when it holds the service's instance.
    let summary: InstanceSummary?

    /// The alias, unique in the environment.
    var id: String { service.alias }
    /// The instance, when it exists.
    var instanceID: UUID? { service.instance?.id }

    /// Service: the alias.
    var alias: String { service.alias }
    /// Selector: `{provider}@{selector}` or `instanceRef: {name}`.
    var selector: String { EnvironmentCopy.selectorText(service) }
    /// Instance: the name, or "Will be created".
    var instanceText: String { service.instance?.name ?? EnvironmentCopy.willBeCreated }
    /// Version: the runtime version, or "—".
    var version: String { service.instance?.runtimeVersion ?? "—" }
    /// Ownership: "Dedicated" or "Shared ref".
    var ownership: String { EnvironmentCopy.ownershipText(service.ownership) }

    /// Status: the instance status of the instance table, or "Not created".
    var appearance: StatusAppearance {
        if let summary {
            return StatusAdapter.appearance(of: summary)
        }
        guard let instance = service.instance else { return .serviceNotCreated }
        return .instance(
            lifecycle: StatusAdapter.lifecycle(instance.lifecycle),
            readiness: StatusAdapter.readiness(instance.readiness),
            health: StatusAdapter.health(instance.health))
    }

    /// The rows of `detail`, in snapshot order.
    static func rows(
        of detail: EnvironmentDetail, instances: [UUID: InstanceSummary]
    ) -> [ServiceRowModel] {
        (detail.services ?? []).map { service in
            ServiceRowModel(
                service: service, summary: service.instance.flatMap { instances[$0.id] })
        }
    }

    /// The services that are not ready, as the items of the partially ready banner:
    /// "{instance} ({alias})" with the reason.
    static func notReadyItems(
        of detail: EnvironmentDetail, open: @escaping @MainActor (UUID) -> Void
    ) -> [PartialItem] {
        (detail.services ?? []).compactMap { service in
            guard let notReady = service.notReady else { return nil }
            let name = service.instance.map { "\($0.name) (\(service.alias))" } ?? service.alias
            let action = service.instance.map { instance in
                StateAction(title: "Show Instance", mutates: false, perform: { open(instance.id) })
            }
            return PartialItem(
                name: name, reason: EnvironmentCopy.notReadyText(notReady), action: action)
        }
    }
}

/// Which columns the Services table shows at a width: Version goes first, then Selector
/// (spec 0044, *Layar*).
nonisolated enum ServiceColumnSet: Hashable, Sendable {
    /// Every column.
    case full
    /// Without Version.
    case withoutVersion
    /// Without Version and Selector.
    case compact

    /// The widest width that hides Version.
    static let fullWidth: CGFloat = 820
    /// The widest width that hides Selector too.
    static let selectorWidth: CGFloat = 640

    /// The columns of a table `width` points wide.
    init(width: CGFloat) {
        if width >= Self.fullWidth {
            self = .full
        } else if width >= Self.selectorWidth {
            self = .withoutVersion
        } else {
            self = .compact
        }
    }

    /// Whether Version shows.
    var showsVersion: Bool { self == .full }
    /// Whether Selector shows.
    var showsSelector: Bool { self != .compact }
}
