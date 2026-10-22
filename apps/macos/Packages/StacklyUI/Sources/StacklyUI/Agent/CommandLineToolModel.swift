import Foundation
import Observation

/// The state of the `stackly` command and the three things the user can do about it:
/// Install, Repair, and Remove (spec 0015, *GUI*). ``ShellRuntime`` owns it, and
/// Diagnostics and the Command Line step of onboarding read it.
///
/// The three `request` methods only fill ``pendingPrompt``. Only the action button of the
/// confirmation calls the tool, and Cancel changes nothing.
@Observable
public final class CommandLineToolModel {
    /// The state of the command and of each manual page.
    public private(set) var status: CommandLineToolStatus
    /// The command inside the running bundle, or `nil` for a build without it. Install and
    /// Repair need it; Remove does not.
    public let bundledTool: URL?
    /// What the last action failed with; every action clears it first.
    public private(set) var lastError: CommandLineToolError?
    /// Whether an action runs.
    public private(set) var isWorking = false
    /// The confirmation to show, if any.
    public var pendingPrompt: ConfirmationRequest?

    /// Why Install and Repair are disabled in a build without the command.
    public static let missingToolReason =
        "This build doesn’t include the stackly command. Build the app with cargo xtask build-app."

    @ObservationIgnored private let controller: any CommandLineToolControlling

    /// Creates the model.
    ///
    /// - Parameters:
    ///   - controller: The tool.
    ///   - initialStatus: The status shown until the first ``refresh()``; a fixture gives
    ///     the status its fake answers, so a snapshot does not wait for it.
    init(
        controller: any CommandLineToolControlling,
        initialStatus: CommandLineToolStatus = .notInstalled
    ) {
        self.controller = controller
        self.status = initialStatus
        self.bundledTool = controller.bundledTool
    }

    /// The user's home, for `UserHome.tildePath`.
    var home: URL { controller.home }

    /// Whether Install and Repair can run: the build has the command.
    public var canLink: Bool { bundledTool != nil }

    /// Reads the status again. Called when a view that shows it appears, after every action,
    /// and when the app becomes active.
    public func refresh() async {
        status = await controller.status()
    }

    /// Asks to install the command; the confirmation does the rest.
    public func requestInstall() {
        pendingPrompt = CommandLineToolPrompts.install { [weak self] in
            self?.run { (tool) async throws(CommandLineToolError) in try await tool.install() }
        }
    }

    /// Asks to repair the links; the confirmation does the rest.
    public func requestRepair() {
        pendingPrompt = CommandLineToolPrompts.repair { [weak self] in
            self?.run { (tool) async throws(CommandLineToolError) in try await tool.repair() }
        }
    }

    /// Asks to remove the links; the confirmation does the rest.
    public func requestRemove() {
        pendingPrompt = CommandLineToolPrompts.remove { [weak self] in
            self?.run { (tool) async throws(CommandLineToolError) in try await tool.remove() }
        }
    }

    /// Runs an action in a task and reads the status after it, success or not.
    private func run(
        _ action:
            @escaping (any CommandLineToolControlling) async throws(CommandLineToolError) ->
            Void
    ) {
        guard isWorking == false else { return }
        isWorking = true
        lastError = nil
        Task { [weak self] in
            guard let self else { return }
            do throws(CommandLineToolError) {
                try await action(self.controller)
            } catch {
                self.lastError = error
            }
            await self.refresh()
            self.isWorking = false
        }
    }
}
