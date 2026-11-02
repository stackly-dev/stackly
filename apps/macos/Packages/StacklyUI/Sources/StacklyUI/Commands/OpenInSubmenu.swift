import SwiftUI

/// The "Open In" submenu of the Instance menu (spec 0070, AC-5), next to Copy Connection
/// String. Its items are the provider actions of the instance in view, which the Instances
/// page publishes with ``PublishedCommands/openIn``; it is not in the command map because its
/// items are not fixed. It has no shortcut.
struct OpenInSubmenu: View {
    let context: CommandContext

    /// What the focused window published, when it is the Instances page of the key window;
    /// `nil` turns the submenu off.
    static func published(shell: PublishedCommands?, page: PublishedCommands?) -> (
        page: PublishedCommands, content: OpenInContent
    )? {
        guard let shell, shell.isWindowKey, let page, case .page(.instances) = page.level,
            let content = page.openIn
        else { return nil }
        return (page, content)
    }

    var body: some View {
        if let published = Self.published(shell: context.shell, page: context.page) {
            switch published.content {
            case .hidden:
                EmptyView()
            case .noSelection:
                disabled
            case .loading:
                Menu(OpenInMenu.title) {
                    Button(OpenInMenu.loadingText) {}
                        .disabled(true)
                }
            case .sections(let sections):
                Menu(OpenInMenu.title) {
                    ForEach(Array(sections.enumerated()), id: \.offset) { index, section in
                        if index > 0 {
                            Divider()
                        }
                        ForEach(section) { action in
                            item(action, in: published.page)
                        }
                    }
                }
            }
        } else {
            disabled
        }
    }

    private var disabled: some View {
        Menu(OpenInMenu.title) {
            Button(OpenInMenu.loadingText) {}
        }
        .disabled(true)
    }

    @ViewBuilder private func item(_ action: StacklyAction, in page: PublishedCommands)
        -> some View
    {
        let availability = action.availability(gate: page.host.gate)
        Button(action.title) {
            CommandItemState.run(action, in: page)
        }
        .disabled(availability != .enabled)
        if let reason = availability.menuReason, MenuReasonPath.current == .extraItem {
            Button(reason) {}
                .disabled(true)
        }
    }
}
