import SwiftUI

/// What the menu items read: the commands the focused window published and the actions of
/// the items that are always on (spec 0069).
struct CommandContext {
    /// What the shell of the focused window published, or `nil`.
    var shell: PublishedCommands?
    /// What the page of the focused window published, or `nil`.
    var page: PublishedCommands?
    /// The actions of the `app` scope items of kind `button`.
    var appActions: [CommandID: @MainActor () -> Void] = [:]
}

/// When a disabled item has a reason of its own, the way the reason shows follows
/// ``MenuReasonPath/current``: as a disabled item right under it.
struct CommandItem: View {
    private let entry: CommandEntry
    private let context: CommandContext

    /// Creates the item.
    /// - Parameters:
    ///   - id: The command.
    ///   - context: What the focused window published.
    init(_ id: CommandID, context: CommandContext) {
        entry = CommandMap.entry(for: id)
        self.context = context
    }

    private var state: CommandItemState {
        if entry.scope == .app {
            let run = context.appActions[entry.id]
            return CommandItemState(
                title: entry.title, isEnabled: run != nil, subtitle: nil, isChecked: false,
                run: run)
        }
        return CommandItemState.resolve(entry, shell: context.shell, page: context.page)
    }

    var body: some View {
        let state = state
        switch entry.kind {
        case .button:
            Button(state.title) { state.run?() }

                .disabled(state.isEnabled == false)
        case .toggle:
            Toggle(
                state.title,
                isOn: Binding(get: { state.isChecked }, set: { _ in state.run?() })
            )

            .disabled(state.isEnabled == false)
        case .system:
            EmptyView()
        }
        if let reason = state.subtitle, MenuReasonPath.current == .extraItem {
            Button(reason) {}
                .disabled(true)
        }
    }
}

/// A divider between two groups of a menu, drawn only when `group` has a built command and
/// an earlier group has one too, so the menu never shows a double or a hanging divider
/// (spec 0069, Peta perintah).
struct GroupDivider: View {
    private let shown: Bool

    /// Creates the divider before `group`.
    /// - Parameters:
    ///   - menu: The menu.
    ///   - submenu: The submenu, or `nil` for the menu itself.
    ///   - group: The group the divider sits in front of.
    init(_ menu: CommandMenuID, submenu: CommandSubmenuID? = nil, before group: Int) {
        shown = Self.shows(
            CommandMap.layout(menu: menu, submenu: submenu), before: group)
    }

    /// Whether a divider belongs in front of `group` in this layout.
    static func shows(_ layout: [[CommandEntry]], before group: Int) -> Bool {
        let groups = layout.compactMap(\.first?.group)
        return groups.contains(group) && groups.contains { $0 < group }
    }

    var body: some View {
        if shown {
            Divider()
        }
    }
}
