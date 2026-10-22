import AppKit
import SwiftUI

/// The main window's content in the running app: ``AppShell`` fed from the runtime's live
/// ``DaemonModel``.
///
/// It also puts the model in the environment, so the pages that later features build can
/// read the daemon's state and send calls. ``AppShell`` sets ``MutationAvailability`` from
/// the model's presentation. It also creates the window's one ``FeedbackCenter`` and puts
/// it in the environment, where ``AppShell`` shows its card. It also builds the window's New
/// Instance sheet over the model's stores and link. The runtime, not the window,
/// owns the model, so closing the window stops nothing.
public struct LiveShell: View {
    @State private var feedbackCenter = FeedbackCenter()
    @State private var newInstance: NewInstancePresenter

    private let runtime: ShellRuntime
    private let windowWillClose: (NSWindow) -> Void

    /// Creates the content.
    ///
    /// - Parameters:
    ///   - runtime: The shell runtime the app delegate holds for the whole launch.
    ///   - windowWillClose: Called with the main window right before it closes.
    public init(runtime: ShellRuntime, windowWillClose: @escaping (NSWindow) -> Void) {
        self.runtime = runtime
        self.windowWillClose = windowWillClose
        _newInstance = State(initialValue: Self.makeNewInstance(runtime.model))
    }

    /// The New Instance sheet of the window over the model's stores and its link.
    private static func makeNewInstance(_ model: DaemonModel) -> NewInstancePresenter {
        let path = MutationPath(model: model)
        return NewInstancePresenter.make(
            runtimes: model.runtimes, catalog: model.catalog, instances: model.instances,
            service: LiveCreateInstanceService(model: model),
            send: { call, key in await path.perform(call, idempotencyKey: key) },
            updates: { model.operations.updates(for: $0) })
    }

    /// The shell, wired to the model.
    public var body: some View {
        let model = runtime.model
        AppShell(
            presentation: model.presentation,
            diagnostics: model.diagnostics,
            counts: model.sidebarCounts,
            instances: model.instances,
            catalog: model.catalog,
            runtimes: model.runtimes,
            operations: model.operations,
            capacity: model.capacity,
            instanceActions: model.instanceActions,
            newInstance: newInstance,
            completedRounds: model.sync.completedRounds,
            windowOpener: runtime.windowOpener,
            windowWillClose: windowWillClose,
            reconnect: { model.reconnect() },
            refreshDiagnostics: { model.refreshDiagnostics() }
        )
        .environment(model)
        .environment(runtime.commandLineTool)
        .environment(\.feedbackCenter, feedbackCenter)
    }
}
