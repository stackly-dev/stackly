import SwiftUI

/// Who publishes a set of commands: the shell, or one page of it.
public enum CommandLevel: Hashable, Sendable {
    /// ``AppShell``, for the commands of scope ``CommandScope/shell``.
    case shell
    /// The page of a section, for the commands of scope ``CommandScope/page(_:)``.
    case page(SidebarSection)
}

/// The actions a window publishes for the menu bar (spec 0069).
/// A menu item does not own any logic: it reads the ``StacklyAction`` published here, and
/// it is enabled exactly when the action is, with the gate of the publisher's own window.
@MainActor public struct PublishedCommands {
    /// Who published.
    public let level: CommandLevel
    /// The actions by command. A command with no entry here is disabled.
    public let actions: [CommandID: StacklyAction]
    /// The toggle items that show a checkmark.
    public let checked: Set<CommandID>
    /// The host of the publisher's window; it holds the gate and shows confirmations.
    let host: ActionHost
    /// Whether the publisher's window is the key window.
    let isWindowKey: Bool
    /// The key state of that window, read again when an item runs.
    let windowKey: MainWindowKey?

    /// Creates the published set.
    init(
        level: CommandLevel,
        actions: [CommandID: StacklyAction],
        checked: Set<CommandID> = [],
        host: ActionHost,
        isWindowKey: Bool,
        windowKey: MainWindowKey? = nil
    ) {
        self.level = level
        self.actions = actions
        self.checked = checked
        self.host = host
        self.isWindowKey = isWindowKey
        self.windowKey = windowKey
    }
}

extension FocusedValues {
    /// The commands the shell of the focused window publishes.
    @Entry var shellCommands: PublishedCommands? = nil
    /// The commands the page of the focused window publishes.
    @Entry var pageCommands: PublishedCommands? = nil
}

extension View {
    /// Publishes the actions the menu commands of `level` run, with this view's action host
    /// and window key state.
    /// Put it inside `.actionHost()` and inside the environment values that set the
    /// mutation gate and the window key, so the commands read the gate and the key state of
    /// this window, not the defaults. Without an action host it publishes nothing.
    /// - Parameters:
    ///   - level: Who publishes.
    ///   - actions: The actions by command.
    ///   - checked: The toggle items that show a checkmark.
    public func publishesCommands(
        _ level: CommandLevel,
        _ actions: [CommandID: StacklyAction],
        checked: Set<CommandID> = []
    ) -> some View {
        modifier(PublishCommandsModifier(level: level, actions: actions, checked: checked))
    }
}

/// Reads the host and the window key state from the environment and publishes them with
/// the actions as a focused scene value.
struct PublishCommandsModifier: ViewModifier {
    @Environment(\.actionHost) private var host
    @Environment(\.mainWindowKey) private var windowKey

    let level: CommandLevel
    let actions: [CommandID: StacklyAction]
    let checked: Set<CommandID>
    /// Called with what is published; a test reads the host and the gate through it.
    var onPublish: (@MainActor (PublishedCommands) -> Void)?

    func body(content: Content) -> some View {
        let published = publishedCommands
        let _ = published.map { onPublish?($0) }
        let keyPath: WritableKeyPath<FocusedValues, PublishedCommands?> =
            level == .shell ? \.shellCommands : \.pageCommands
        content.focusedSceneValue(keyPath, published)
    }

    private var publishedCommands: PublishedCommands? {
        guard let host else {
            assertionFailure("menu commands need .actionHost()")
            return nil
        }
        return PublishedCommands(
            level: level, actions: actions, checked: checked, host: host,
            isWindowKey: windowKey?.isKey ?? false, windowKey: windowKey)
    }
}
