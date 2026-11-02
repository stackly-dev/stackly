import StacklyClient
import SwiftUI

/// What the Instances page publishes to the menu bar (spec 0069, *Penerbit*, and spec 0022,
/// *Aksesibilitas*): Find, the inspector toggle, the seven sort rows, Start and Stop for the
/// instance in view, Open for the selected row, Back while a detail page is open, and Copy
/// Connection String (spec 0021, spec 0069 AC-11 and AC-12), and Duplicate… (spec 0024).
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
    /// - Parameters:
    ///   - showsInspector: Whether the inspector is open.
    ///   - toggleInspector: Shows or hides the inspector.
    ///   - sort: The comparator that orders the table now.
    ///   - setSort: Orders the table by another comparator.
    ///   - hasSearch: Whether the search field is in the toolbar. It is not while a detail page
    ///     is open, and then Find is not published, so it shows disabled (AC-10).
    ///   - target: The main action of the instance in view and what it stands for, or `nil`
    ///     when there is none. The instance in view is the one whose detail page is open, else
    ///     the selected row. Start and Retry are `startInstance`, Stop is `stopInstance`; the
    ///     other command is not published, so it shows disabled.
    ///   - open: Opens the selected row; `nil` while a detail page is open or nothing is
    ///     selected.
    ///   - back: Closes the detail page; `nil` while the table shows.
    ///   - copyConnection: Copies the first connection string of the instance in view, or
    ///     `nil` when there is none.
    ///   - duplicate: Opens the New Instance sheet on the settings of the instance in view,
    ///     or `nil` when there is none or the window has no sheet (spec 0024 AC-13).
    ///   - rename: Opens the Rename sheet of the instance in view, or `nil` when there is none
    ///     (spec 0029 AC-19).
    ///   - editPorts: Opens the Ports sheet of the instance in view, or `nil` when there is none.
    ///   - lifecycle: Restart, Reload, and Force Stop… of the instance in view (spec 0026), or
    ///     `nil` when there is none.
    static func publish(
        showsInspector: Bool,
        toggleInspector: @escaping @MainActor () -> Void,
        sort: InstanceSortComparator,
        setSort: @escaping @MainActor (InstanceSortComparator) -> Void,

        target: (action: StacklyAction, verb: InstanceVerb?)?,
        open: StacklyAction? = nil,
        back: (@MainActor () -> Void)? = nil,
        copyConnection: StacklyAction? = nil,
        duplicate: StacklyAction? = nil,
        rename: StacklyAction? = nil,
        editPorts: StacklyAction? = nil,
        lifecycle: (restart: StacklyAction, reload: StacklyAction, forceStop: StacklyAction)? =
            nil
    ) -> (actions: [CommandID: StacklyAction], checked: Set<CommandID>) {
        var actions: [CommandID: StacklyAction] = [:]

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
        if let lifecycle {
            // Published also while they cannot run, so the menu writes the reason as the
            // subtitle (spec 0026 AC-15).
            for (command, action) in [
                (CommandID.restartInstance, lifecycle.restart),
                (.reloadInstance, lifecycle.reload),
                (.forceStopInstance, lifecycle.forceStop),
            ] {
                var action = action
                action.id = command.rawValue
                if action.isBusy == false {
                    action.title = CommandMap.entry(for: command).title
                }
                actions[command] = action
            }
        }
        if let target, let verb = target.verb, verb == .start || verb == .stop {
            let command: CommandID = verb == .start ? .startInstance : .stopInstance
            var action = target.action
            action.id = command.rawValue
            action.title = CommandMap.entry(for: command).title
            actions[command] = action
        }
        if let open {
            var action = open
            action.id = CommandID.openInstance.rawValue
            actions[.openInstance] = action
        }
        if let back {
            actions[.back] = StacklyAction(
                id: CommandID.back.rawValue, title: CommandMap.entry(for: .back).title,
                mutates: false, perform: back)
        }
        if let copyConnection {
            var action = copyConnection
            action.id = CommandID.copyConnectionString.rawValue
            actions[.copyConnectionString] = action
        }
        if let duplicate {
            var action = duplicate
            action.id = CommandID.duplicateInstance.rawValue
            actions[.duplicateInstance] = action
        }
        if let rename {
            var action = rename
            action.id = CommandID.renameInstance.rawValue
            actions[.renameInstance] = action
        }
        if let editPorts {
            var action = editPorts
            action.id = CommandID.editPorts.rawValue
            actions[.editPorts] = action
        }
        let checked: Set<CommandID> = [
            command(for: sort.key),
            sort.order == .forward ? .sortInstancesAscending : .sortInstancesDescending,
        ]
        return (actions, checked)
    }
}
