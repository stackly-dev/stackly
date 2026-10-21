import SwiftUI

/// What one menu item shows and does, worked out from what the focused window published
/// (spec 0069, AC-4 to AC-6). It is a pure function of its inputs, so the tests cover every
/// combination without a menu bar.
@MainActor struct CommandItemState {
    /// The title to show.
    let title: String
    /// Whether the item can be chosen.
    let isEnabled: Bool
    /// Why the item is disabled, when the action says so: its own reason, or "In progress".
    /// Nothing for a reason the window banner already writes. ``CommandItem`` shows it the
    /// way ``MenuReasonPath/current`` decides.
    let subtitle: String?
    /// Whether a toggle item shows its checkmark.
    let isChecked: Bool
    /// What choosing the item does, or `nil` when it does nothing.
    let run: (@MainActor () -> Void)?

    /// Works out the state of a `shell` or `page` item.
    ///
    /// A `system` row and a row of scope ``CommandScope/app`` do not come through here: the
    /// first has no item and the second is always on, so this returns an enabled item with
    /// nothing to run.
    ///
    /// - Parameters:
    ///   - entry: The row of the command.
    ///   - shell: What the shell of the focused window published, or `nil`.
    ///   - page: What the page of the focused window published, or `nil`.
    static func resolve(
        _ entry: CommandEntry, shell: PublishedCommands?, page: PublishedCommands?
    ) -> CommandItemState {
        let off = CommandItemState(
            title: entry.title, isEnabled: false, subtitle: nil, isChecked: false, run: nil)
        if entry.kind == .system { return off }
        let publisher: PublishedCommands?
        switch entry.scope {
        case .app:
            return CommandItemState(
                title: entry.title, isEnabled: true, subtitle: nil, isChecked: false, run: nil)
        case .shell:
            publisher = shell
        case .page(let sections):
            if let page, case .page(let shown) = page.level, sections.contains(shown) {
                publisher = page
            } else {
                publisher = nil
            }
        }
        // A page lives inside the shell, so a shell that is not key switches both off.
        guard let shell, shell.isWindowKey, let publisher,
            let action = publisher.actions[entry.id]
        else { return off }
        let availability = action.availability(gate: publisher.host.gate)
        let isChecked = entry.kind == .toggle && publisher.checked.contains(entry.id)
        let title = entry.alternateTitle != nil ? action.title : entry.title
        guard availability == .enabled else {
            return CommandItemState(
                title: title, isEnabled: false, subtitle: availability.menuReason,
                isChecked: isChecked, run: nil)
        }
        return CommandItemState(
            title: title, isEnabled: true, subtitle: nil, isChecked: isChecked,
            run: { Self.run(action, in: publisher) })
    }

    /// Runs `action` the way its button does, after checking again that the window is still
    /// key and the action is still enabled: the menu may not be recomputed yet.
    private static func run(_ action: StacklyAction, in publisher: PublishedCommands) {
        guard publisher.windowKey?.isKeyNow == true,
            action.availability(gate: publisher.host.gate) == .enabled
        else { return }
        publisher.host.run(action)
    }
}
