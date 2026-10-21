import SwiftUI

/// How a menu item says why it is disabled.
enum MenuReasonPath {
    /// The reason is the item's subtitle. macOS 27 draws it under the title in a toolbar
    /// menu, a context menu, and the main menu, but it does not show up in the item's
    /// accessibility tree (spec 0067 rationale, *Temuan langkah 1*).
    case subtitle
    /// The reason is a disabled item of its own, right under the item, so it is both
    /// visible and read by VoiceOver.
    case extraItem

    /// The path in use. Change it to ``subtitle`` once VoiceOver is shown to read the
    /// subtitle; no API changes.
    static let current: MenuReasonPath = .extraItem
}

/// The More menu of a toolbar (spec 0067, AC-8): a native `Menu` with an `ellipsis.circle`
/// icon, labeled "More" for VoiceOver, holding sections of actions separated by dividers.
public struct ActionMenu: View {
    private let sections: [[StacklyAction]]
    private let title: String
    private let symbol: String
    private let help: String?

    /// Creates the menu.
    ///
    /// - Parameters:
    ///   - sections: The actions, in sections separated by a divider. Put the section with
    ///     a destructive action last.
    ///   - title: The label for VoiceOver and the default help.
    ///   - symbol: The SF Symbol of the icon.
    ///   - help: The tooltip, or `nil` for the title. A page adds the shortcut with
    ///     `CommandMap.helpText(for:)`.
    public init(
        _ sections: [[StacklyAction]],
        title: String = "More",
        symbol: String = "ellipsis.circle",
        help: String? = nil
    ) {
        self.sections = sections
        self.title = title
        self.symbol = symbol
        self.help = help
    }

    /// The menu's content.
    public var body: some View {
        Menu {
            ActionMenuItems(sections)
        } label: {
            Label(title, systemImage: symbol)
                .labelStyle(.iconOnly)
        }
        .help(help ?? title)
        .accessibilityIdentifier(AccessibilityID.Action.more)
    }
}

/// The items of a menu of actions, for `.contextMenu` and for ``ActionMenu`` (spec 0067,
/// AC-8): sections separated by dividers, a destructive role for a destructive action, and
/// no keyboard shortcut (feature 69 owns them).
///
/// An item that is busy, or disabled for a reason of its own, is disabled and says why (or
/// "In progress") the way ``MenuReasonPath/current`` decides. An item the mutation gate
/// blocks is disabled with no reason, because the window banner writes it.
public struct ActionMenuItems: View {
    @Environment(\.actionHost) private var host
    @Environment(\.mutationAvailability) private var gate

    private let sections: [[StacklyAction]]

    /// Creates the items.
    ///
    /// - Parameter sections: The actions, in sections separated by a divider.
    public init(_ sections: [[StacklyAction]]) {
        self.sections = sections
    }

    /// The items.
    public var body: some View {
        ForEach(Array(sections.enumerated()), id: \.offset) { index, section in
            if index > 0 {
                Divider()
            }
            ForEach(section) { action in
                item(action)
            }
        }
    }

    @ViewBuilder private func item(_ action: StacklyAction) -> some View {
        let availability = action.availability(gate: gate)
        let reason = availability.menuReason
        switch (MenuReasonPath.current, reason) {
        case (.subtitle, let reason?):
            button(action, availability) {
                title(of: action)
                Text(reason)
            }
        case (.extraItem, let reason?):
            button(action, availability) { title(of: action) }
            Button(reason) {}
                .disabled(true)
        default:
            button(action, availability) { title(of: action) }
        }
    }

    private func button<Label: View>(
        _ action: StacklyAction,
        _ availability: ActionAvailability,
        @ViewBuilder label: () -> Label
    ) -> some View {
        Button(role: action.role.isDestructive ? .destructive : nil) {
            action.run(host: host, gate: gate)
        } label: {
            label()
        }
        .disabled(availability != .enabled)
    }

    @ViewBuilder private func title(of action: StacklyAction) -> some View {
        if let symbol = action.symbol {
            Label(action.title, systemImage: symbol)
        } else {
            Text(action.title)
        }
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { ActionMenuGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { ActionMenuGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { ActionMenuGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
