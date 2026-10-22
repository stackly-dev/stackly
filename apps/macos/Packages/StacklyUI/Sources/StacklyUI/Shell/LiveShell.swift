import AppKit
import SwiftUI

/// The main window's content in the running app: ``AppShell`` fed from the runtime's live
/// ``DaemonModel``.
///
/// It also puts the model in the environment, so the pages that later features build can
/// read the daemon's state and send calls. ``AppShell`` sets ``MutationAvailability`` from
/// the model's presentation. It also creates the window's one ``FeedbackCenter`` and puts
/// it in the environment, where ``AppShell`` shows its card. The runtime, not the window,
/// owns the model, so closing the window stops nothing.
public struct LiveShell: View {
    @State private var feedbackCenter = FeedbackCenter()

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
            windowOpener: runtime.windowOpener,
            windowWillClose: windowWillClose,
            reconnect: { model.reconnect() },
            refreshDiagnostics: { model.refreshDiagnostics() }
        )
        .environment(model)
        .environment(\.feedbackCenter, feedbackCenter)
    }
}
