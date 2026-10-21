import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// What `AppShell` publishes to the menu bar (spec 0069, AC-5, AC-7, AC-8).
struct ShellCommandsTests {
    nonisolated static let rows = ShellPresentationTests.rows

    /// Records what the commands asked for.
    private final class Calls {
        var selected: [SidebarSection] = []
        var reconnects = 0
    }

    private let calls = Calls()

    private func actions(_ presentation: ShellPresentation) -> [CommandID: StacklyAction] {
        let calls = calls
        return AppShell.shellActions(
            presentation: presentation, select: { calls.selected.append($0) },
            reconnect: { calls.reconnects += 1 })
    }

    // MARK: sections

    @Test func theSectionCommandsFollowTheSidebarOrder() {
        #expect(AppShell.sectionCommands.map(\.section) == SidebarSection.allCases)
        #expect(
            AppShell.sectionCommands.map(\.command) == [
                .showOverview, .showInstances, .showEnvironments, .showCatalog, .showRuntimes,
                .showOperations, .showDiagnostics,
            ])
    }

    @Test(arguments: rows)
    func everySectionCommandIsPublishedWhateverTheDaemonState(row: ShellPresentationTests.Expected)
    {
        let published = actions(ShellPresentation(status: row.status))
        for (command, _) in AppShell.sectionCommands {
            let action = published[command]
            #expect(action != nil, "\(row.name): \(command)")
            #expect(action?.mutates == false)
            #expect(
                action?.availability(
                    gate: ShellPresentation(status: row.status).mutationAvailability) == .enabled)
        }
    }

    @Test func aSectionCommandSelectsItsSection() {
        let published = actions(ShellPresentation(status: .connected(.ready)))
        for (command, section) in AppShell.sectionCommands {
            published[command]?.perform()
            #expect(calls.selected.last == section)
        }
        #expect(calls.selected == SidebarSection.allCases)
    }

    @Test func aSectionCommandTitleIsTheMapsTitle() {
        let published = actions(ShellPresentation(status: .connected(.ready)))
        for (command, _) in AppShell.sectionCommands {
            #expect(published[command]?.title == CommandMap.entry(for: command).title)
        }
    }

    // MARK: reconnect

    @Test(arguments: rows)
    func reconnectIsPublishedExactlyWhileThereIsNoConnection(
        row: ShellPresentationTests.Expected
    ) {
        let presentation = ShellPresentation(status: row.status)
        let published = actions(presentation)
        #expect(
            (published[.reconnect] != nil) == (presentation.hasConnection == false), "\(row.name)")
    }

    @Test func reconnectIsEnabledWhileUnreachable() {
        let presentation = ShellPresentation(
            status: .unreachable(lastKnownAt: nil, since: ShellPresentationTests.since))
        let action = actions(presentation)[.reconnect]
        #expect(action?.availability(gate: presentation.mutationAvailability) == .enabled)
        action?.perform()
        #expect(calls.reconnects == 1)
    }

    @Test func reconnectIsBusyWhileAnAttemptRuns() {
        let presentation = ShellPresentation(
            status: .unreachable(lastKnownAt: nil, since: ShellPresentationTests.since),
            isReconnecting: true)
        let action = actions(presentation)[.reconnect]
        #expect(action?.availability(gate: presentation.mutationAvailability) == .busy)
        #expect(action?.mutates == false)
    }

    @Test func reconnectIsDisabledWithTheDiagnosticsReasonWhileMisconfigured() {
        let presentation = ShellPresentation(
            status: .misconfigured(.rootEmpty, since: ShellPresentationTests.since))
        let action = actions(presentation)[.reconnect]
        #expect(
            action?.availability(gate: presentation.mutationAvailability)
                == .disabled(
                    reason: "Relaunch Stackly after fixing the problem.", showsReason: true))
    }

    @Test func noRowHasAnActionThatChangesData() {
        for row in Self.rows {
            for action in actions(ShellPresentation(status: row.status)).values {
                #expect(action.mutates == false)
            }
        }
    }

    @Test func everyPublishedActionHasABuiltShellRowInTheMap() {
        for row in Self.rows {
            for id in actions(ShellPresentation(status: row.status)).keys {
                let entry = CommandMap.entry(for: id)
                #expect(entry.status == .built, "\(id)")
                #expect(entry.scope == .shell, "\(id)")
                #expect(entry.requirements.contains(.daemon) == false, "\(id)")
            }
        }
    }

    // MARK: the app items

    @Test func everyBuiltAppButtonHasAnAction() async throws {
        let (runtime, _) = try await ShellRuntimeTests.connectedRuntime()
        let appActions = StacklyCommands.appActions(runtime: runtime)
        for entry in CommandMap.entries
        where entry.scope == .app && entry.kind == .button && entry.status == .built {
            #expect(appActions[entry.id] != nil, "\(entry.id)")
        }
    }

    // MARK: the window chain (G10)

    /// The hosts' gates and key state `AppShell` publishes when it fills a window.
    private func published(_ status: ShellStatus) async throws -> PublishedCommands {
        let presentation = ShellPresentation(status: status)
        var shell = AppShell(
            presentation: presentation,
            diagnostics: ConnectionDiagnostics(status: status, appProtocol: "1.3"),
            windowOpener: WindowOpener(),
            windowWillClose: { _ in },
            reconnect: {},
            refreshDiagnostics: {}
        )
        var latest: PublishedCommands?
        shell.onPublishCommands = { latest = $0 }
        let controller = NSHostingController(rootView: shell)
        let window = NSWindow(contentViewController: controller)
        defer { window.close() }
        window.setContentSize(AppShell.minimumSize)
        let content = try #require(window.contentView)
        for _ in 0..<10 {
            content.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(20))
        }
        return try #require(latest)
    }

    @Test(arguments: rows)
    func theShellPublishesWithTheGateOfItsOwnPresentation(
        row: ShellPresentationTests.Expected
    ) async throws {
        let published = try await published(row.status)
        #expect(published.level == .shell)
        #expect(published.host.gate == ShellPresentation(status: row.status).mutationAvailability)
        #expect(published.windowKey != nil)
    }
}
