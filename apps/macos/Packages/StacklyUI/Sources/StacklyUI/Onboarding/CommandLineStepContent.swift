import Foundation

/// What the row of the Command Line step shows for one status (spec 0016, *Kontrol Command
/// Line*). Pure, so every row of the table is tested without a model.
struct CommandLineStepContent: Equatable {
    /// The control on the right of the row.
    enum Control: Equatable {
        /// A switch that shows `isOn`. It never changes by itself: turning it on or off only
        /// asks for a confirmation. `disabledReason` is why it cannot be used, if it cannot.
        case toggle(isOn: Bool, disabledReason: String?)
        /// The Repair… button, with the reason it cannot be used, if it cannot.
        case repair(disabledReason: String?)
    }

    /// The row title.
    let title: String
    /// The row detail.
    let detail: String
    /// The control.
    let control: Control
    /// Whether the PATH note shows, which only an installed command gets.
    let showsPathNote: Bool
    /// The full path of the command inside the app, for "Or run it directly", or `nil` when
    /// the status is not installed or the build has no command.
    let runDirectlyPath: String?
    /// The text of the last failed action, shown under the row, or `nil`.
    let lastError: String?
    /// Whether an action runs.
    let isWorking: Bool

    /// Builds the content.
    ///
    /// - Parameters:
    ///   - status: The status of the `stackly` command.
    ///   - bundledTool: The command inside the running bundle, or `nil` for a build without it.
    ///   - lastError: What the last action failed with.
    ///   - isWorking: Whether an action runs.
    static func make(
        status: CommandLineLinkState, bundledTool: URL?, lastError: CommandLineToolError?,
        isWorking: Bool
    ) -> CommandLineStepContent {
        let missingTool = bundledTool == nil ? CommandLineToolModel.missingToolReason : nil
        let working = isWorking ? OnboardingCopy.working : nil
        let detail: String
        let control: Control
        switch status {
        case .notInstalled:
            detail = OnboardingCopy.commandLineNotInstalled
            control = .toggle(isOn: false, disabledReason: working ?? missingTool)
        case .installed:
            detail = OnboardingCopy.commandLineInstalled
            control = .toggle(isOn: true, disabledReason: working)
        case .stale:
            detail = OnboardingCopy.commandLineStale
            control = .repair(disabledReason: working ?? missingTool)
        case .broken:
            detail = OnboardingCopy.commandLineBroken
            control = .repair(disabledReason: working ?? missingTool)
        case .conflict:
            detail = OnboardingCopy.commandLineConflict
            control = .toggle(isOn: false, disabledReason: OnboardingCopy.commandLineConflict)
        }
        return CommandLineStepContent(
            title: OnboardingCopy.commandLineRowTitle, detail: detail, control: control,
            showsPathNote: status == .installed,
            runDirectlyPath: status == .installed ? bundledTool?.path : nil,
            lastError: lastError?.message, isWorking: isWorking)
    }
}
