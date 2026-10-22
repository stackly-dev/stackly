import StacklyClient
import SwiftUI

/// What the Instances page publishes to the menu bar (spec 0069, *Penerbit*, and spec 0022,
/// *Aksesibilitas*): Find, the inspector toggle, the seven sort rows, and Start and Stop for
/// the selected instance.
enum InstancesCommandActions {
    /// The sort command of a column.
    static func command(for key: InstanceSortKey) -> CommandID {
        switch key {
        case .name: .sortInstancesByName
        case .provider: .sortInstancesByProvider
        case .version: .sortInstancesByVersion
        case .endpoints: .sortInstancesByEndpoints
        case .status: .sortInstancesByStatus
        }
    }

    /// The commands the page publishes and the toggle rows that show a checkmark.
    ///
    /// - Parameters:
    ///   - showsInspector: Whether the inspector is open.
    ///   - toggleInspector: Shows or hides the inspector.
    ///   - sort: The comparator that orders the table now.
    ///   - setSort: Orders the table by another comparator.
    ///   - focusSearch: Puts the cursor in the search field.
    ///   - target: The main action of the selected instance and what it stands for, or `nil`
    ///     when nothing is selected. Start and Retry are `startInstance`, Stop is
    ///     `stopInstance`; the other command is not published, so it shows disabled.
    static func publish(
        showsInspector: Bool,
        toggleInspector: @escaping @MainActor () -> Void,
        sort: InstanceSortComparator,
        setSort: @escaping @MainActor (InstanceSortComparator) -> Void,
        focusSearch: @escaping @MainActor () -> Void,
        target: (action: StacklyAction, verb: InstanceVerb?)?
    ) -> (actions: [CommandID: StacklyAction], checked: Set<CommandID>) {
        var actions: [CommandID: StacklyAction] = [:]
        actions[.find] = StacklyAction(
            id: CommandID.find.rawValue, title: CommandMap.entry(for: .find).title, mutates: false,
            perform: focusSearch)
        actions[.toggleInspector] = StacklyAction(
            id: CommandID.toggleInspector.rawValue,
            title: showsInspector ? "Hide Inspector" : "Show Inspector", mutates: false,
            perform: toggleInspector)
        for key in InstanceSortKey.allCases {
            let command = command(for: key)
            actions[command] = StacklyAction(
                id: command.rawValue, title: CommandMap.entry(for: command).title, mutates: false,
                perform: { setSort(InstanceSortComparator(key, order: sort.order)) })
        }
        for (command, order) in [
            (CommandID.sortInstancesAscending, SortOrder.forward),
            (.sortInstancesDescending, .reverse),
        ] {
            actions[command] = StacklyAction(
                id: command.rawValue, title: CommandMap.entry(for: command).title, mutates: false,
                perform: { setSort(InstanceSortComparator(sort.key, order: order)) })
        }
        if let target, let verb = target.verb {
            let command: CommandID = verb == .start ? .startInstance : .stopInstance
            var action = target.action
            action.id = command.rawValue
            action.title = CommandMap.entry(for: command).title
            actions[command] = action
        }
        let checked: Set<CommandID> = [
            command(for: sort.key),
            sort.order == .forward ? .sortInstancesAscending : .sortInstancesDescending,
        ]
        return (actions, checked)
    }
}
