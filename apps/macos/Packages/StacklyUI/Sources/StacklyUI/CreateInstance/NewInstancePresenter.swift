import Foundation
import Observation
import StacklyClient
import SwiftUI

/// Presents the New Instance sheet for a window (spec 0020, *Titik masuk*): every entry point
/// (the toolbar and empty state buttons through ``SwiftUI/EnvironmentValues/newInstanceAction``,
/// the Catalog detail panel, and ⌘N) calls ``present(software:)``, and Duplicate… (spec 0024)
/// calls ``presentDuplicate(of:)``, and the shell shows the
/// sheet while ``isPresented`` is true.
///
/// The presenter owns the one ``CreateInstanceModel`` of the window, so a create that is
/// running survives the sheet closing: the model keeps following the operation, and the
/// shell hears of the end through ``finishHandler``.
@MainActor
@Observable
public final class NewInstancePresenter {
    /// Whether the sheet shows.
    public var isPresented = false {
        didSet {
            if oldValue, !isPresented { didClose() }
        }
    }

    /// The model of the sheet.
    public let model: CreateInstanceModel

    /// Called once when a create succeeded, with what the window says and where it goes: the
    /// shell sets it to close the sheet, show the new instance, and report the result.
    @ObservationIgnored public var finishHandler: (@MainActor (CreateOutcome) -> Void)?

    /// Creates a presenter around `model`.
    ///
    /// - Parameter model: The model of the sheet. Its `onFinish` should call
    ///   ``finished(_:)``; ``makeModel(runtimes:catalog:instances:service:send:updates:)``
    ///   builds one that does.
    public init(model: CreateInstanceModel) {
        self.model = model
    }

    /// Opens the sheet.
    ///
    /// - Parameter software: The software to start on, or `nil` for the first runtime.
    public func present(software: String? = nil) {
        guard !isPresented else { return }
        switch model.phase {
        case .submitting, .creating:
            // A create that runs keeps its model; the sheet comes back to follow it.
            isPresented = true
            return
        default:
            break
        }
        model.begin(software: software)
        isPresented = true
    }

    /// Opens the sheet on the settings of an instance (spec 0024).
    ///
    /// - Parameter source: The instance to copy the settings of.
    public func presentDuplicate(of source: DuplicateSourceRef) {
        guard !isPresented else { return }
        switch model.phase {
        case .submitting, .creating:
            // A create that runs keeps its model; the sheet comes back to follow it.
            isPresented = true
            return
        default:
            break
        }
        model.beginDuplicate(of: source)
        isPresented = true
    }

    /// The create succeeded: closes the sheet and tells the shell.
    public func finished(_ outcome: CreateOutcome) {
        isPresented = false
        finishHandler?(outcome)
    }

    private func didClose() {
        // A create that is still running keeps its model; anything else is dropped.
        if case .creating = model.phase { return }
        model.end()
    }

    /// The action of the Duplicate… item and command.
    public var duplicateAction: DuplicateInstanceAction {
        DuplicateInstanceAction { [self] in presentDuplicate(of: $0) }
    }

    /// The action of the New Instance button, command, and empty state.
    public var action: NewInstanceAction {
        NewInstanceAction { [self] in present() }
    }
}

extension NewInstancePresenter {
    /// Builds the presenter of a window over the shell's stores and the daemon.
    ///
    /// - Parameters:
    ///   - runtimes: The runtimes the picker offers.
    ///   - catalog: The catalog, for the category and the tile.
    ///   - instances: The instances, for the labels of Copy from.
    ///   - service: What the sheet reads from the daemon.
    ///   - send: Sends one mutation with its idempotency key.
    ///   - updates: The updates of an operation, ending after its final state.
    public static func make(
        runtimes: RuntimeStore, catalog: CatalogStore?, instances: InstanceStore,
        service: any CreateInstanceService,
        send: @escaping @MainActor (Call, String) async -> MutationOutcome,
        updates: @escaping @MainActor (UUID) -> AsyncStream<OperationSnapshot>
    ) -> NewInstancePresenter {
        var presenter: NewInstancePresenter?
        let model = CreateInstanceModel(
            runtimes: runtimes, catalog: catalog, instances: instances, service: service,
            send: send, updates: updates,
            onFinish: { outcome in presenter?.finished(outcome) })
        let made = NewInstancePresenter(model: model)
        presenter = made
        return made
    }
}

extension EnvironmentValues {
    /// Opens the New Instance sheet on a software, for the Catalog detail panel, or `nil`
    /// where the window has no sheet.
    @Entry public var newInstancePresenter: NewInstancePresenter? = nil
}

/// Shows the New Instance sheet of a presenter on the view it modifies.
struct NewInstanceSheetModifier: ViewModifier {
    let presenter: NewInstancePresenter?

    @ViewBuilder func body(content: Content) -> some View {
        if let presenter {
            content.sheet(
                isPresented: Binding(
                    get: { presenter.isPresented },
                    set: { presenter.isPresented = $0 })
            ) {
                NewInstanceSheet(model: presenter.model)
            }
        } else {
            content
        }
    }
}
