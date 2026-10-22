import AppKit
import SwiftUI

/// The main window's content in the running app: the onboarding until it is finished, then
/// ``AppShell`` fed from the runtime's live ``DaemonModel`` (spec 0016, *Tampilan di jendela
/// utama*).
///
/// It is the root view of the window, so it holds what belongs to the window and not to a
/// screen: the window's one ``FeedbackCenter``, the New Instance sheet over the model's
/// stores and link, the ``MainWindowKey`` that follows the key state, the capture of the
/// scene's `openWindow` action into the ``WindowOpener``, the report that the window is about
/// to close, and the minimum size. They stay in place while the onboarding shows, so the
/// close notice and the size rule hold there too. The model, the command line tool, and the
/// feedback center reach both the onboarding and the shell through the environment.
///
/// The runtime, not the window, owns the models, so closing the window stops nothing and
/// opening it again shows the same onboarding step.
public struct MainWindowRoot: View {
    @State private var feedbackCenter = FeedbackCenter()
    @State private var newInstance: NewInstancePresenter
    @State private var windowKey = MainWindowKey()
    @Environment(\.openWindow) private var openWindow

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

    /// The onboarding or the shell, with the window's wiring.
    public var body: some View {
        content
            .animation(StacklyMotion.crossfadeAnimation, value: runtime.onboarding.isPresented)
            .frame(minWidth: AppShell.minimumSize.width, minHeight: AppShell.minimumSize.height)
            .environment(runtime.model)
            .environment(runtime.commandLineTool)
            .environment(\.feedbackCenter, feedbackCenter)
            .environment(\.mainWindowKey, windowKey)
            .background(
                MainWindowAccessor(windowKey: windowKey, windowWillClose: windowWillClose)
            )
            .onAppear {
                runtime.windowOpener.capture(openWindow)
            }
    }

    @ViewBuilder private var content: some View {
        if runtime.onboarding.isPresented {
            OnboardingFlow(onboarding: runtime.onboarding)
                .transition(StacklyMotion.crossfade)
        } else {
            shell
                .transition(StacklyMotion.crossfade)
        }
    }

    private var shell: some View {
        let model = runtime.model
        return AppShell(
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
            reconnect: { model.reconnect() },
            refreshDiagnostics: { model.refreshDiagnostics() }
        )
    }
}
