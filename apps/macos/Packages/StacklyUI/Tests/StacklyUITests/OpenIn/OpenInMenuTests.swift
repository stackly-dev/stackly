import AppKit
import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The Open In menu in its three places (spec 0070, AC-5, AC-6): what the Instances page
/// publishes to the Open In submenu of the Instance menu, when that submenu is on, and how
/// the gate of the window reaches an item.
@MainActor
struct OpenInMenuTests {
    private final class Log {
        var performed: [String] = []
    }

    private let log = Log()

    private func item(_ id: String, reason: String? = nil) -> StacklyAction {
        let log = log
        return StacklyAction(
            id: id, title: id, mutates: true, disabledReason: reason,
            perform: { log.performed.append(id) })
    }

    private func published(
        _ level: CommandLevel, openIn: OpenInContent?, gate: MutationAvailability = .available,
        key: Bool = true
    ) -> PublishedCommands {
        let windowKey = MainWindowKey(isKeyWindow: { _ in key })
        windowKey.track(NSWindow())
        return PublishedCommands(
            level: level, actions: [:], openIn: openIn,
            host: ActionHost(gate: gate, present: { _ in }), isWindowKey: key,
            windowKey: windowKey)
    }

    @Test func theSubmenuReadsWhatTheInstancesPagePublishes() throws {
        let content = OpenInContent.sections([[item("openIn.psql")]])
        let page = published(.page(.instances), openIn: content)
        let shell = published(.shell, openIn: nil)

        let found = try #require(OpenInSubmenu.published(shell: shell, page: page))

        guard case .sections(let sections) = found.content else {
            Issue.record("the submenu got no sections")
            return
        }
        #expect(sections.flatMap { $0 }.map(\.id) == ["openIn.psql"])
    }

    @Test func theSubmenuIsOffWithoutAKeyWindowAnInstancesPageOrAnOpenInPublication() {
        let content = OpenInContent.sections([[item("openIn.psql")]])
        let shell = published(.shell, openIn: nil)
        let notKey = published(.shell, openIn: nil, key: false)

        #expect(OpenInSubmenu.published(shell: nil, page: nil) == nil)
        #expect(
            OpenInSubmenu.published(
                shell: shell, page: published(.page(.overview), openIn: content))
                == nil)
        #expect(
            OpenInSubmenu.published(shell: shell, page: published(.page(.instances), openIn: nil))
                == nil)
        #expect(
            OpenInSubmenu.published(
                shell: notKey, page: published(.page(.instances), openIn: content)) == nil)
    }

    @Test func anItemRunsInTheKeyWindowAndNotWhileTheMutationGateIsClosed() {
        let open = published(.page(.instances), openIn: nil)
        let closed = published(
            .page(.instances), openIn: nil,
            gate: .blocked(
                reason: "stacklyd is unavailable.", since: Date(timeIntervalSince1970: 0)))
        let action = item("openIn.psql")

        CommandItemState.run(action, in: closed)
        #expect(log.performed.isEmpty)
        CommandItemState.run(action, in: open)
        #expect(log.performed == ["openIn.psql"])
    }

    @Test func theButtonShowsForLoadingAndForActionsButNotForNothingToShow() {
        #expect(OpenInContent.hidden.showsButton == false)
        #expect(OpenInContent.noSelection.showsButton == false)
        #expect(OpenInContent.loading.showsButton)
        #expect(OpenInContent.sections([[item("openIn.psql")]]).showsButton)
    }
}
