import AppKit
import Foundation
import Testing

@testable import StacklyUI

/// `CommandItemState.resolve` and its run guard (spec 0069, AC-4 to AC-6).
struct CommandItemStateTests {
    private static let blocked = MutationAvailability.blocked(
        reason: "stacklyd is unavailable.", since: Date(timeIntervalSince1970: 0))

    /// Records what the host and the actions did.
    private final class Log {
        var performed: [String] = []
        var presented: [String] = []
    }

    private let log = Log()

    private func action(
        _ id: CommandID, title: String? = nil, mutates: Bool = false, reason: String? = nil,
        busy: Bool = false, role: ActionRole = .standard(confirmation: nil)
    ) -> StacklyAction {
        let log = log
        return StacklyAction(
            id: id.rawValue, title: title ?? CommandMap.entry(for: id).title, role: role,
            mutates: mutates, disabledReason: reason, isBusy: busy,
            perform: { log.performed.append(id.rawValue) })
    }

    /// A published set with a host that records confirmations.
    private func published(
        _ level: CommandLevel, _ actions: [StacklyAction], checked: Set<CommandID> = [],
        gate: MutationAvailability = .available, key: Bool = true,
        windowKey: MainWindowKey? = nil
    ) -> PublishedCommands {
        let log = log
        let host = ActionHost(gate: gate, present: { log.presented.append($0.title) })
        return PublishedCommands(
            level: level,
            actions: Dictionary(
                uniqueKeysWithValues: actions.map { (CommandID(rawValue: $0.id)!, $0) }),
            checked: checked, host: host, isWindowKey: key,
            windowKey: windowKey ?? MainWindowKey(isKeyWindow: { _ in key }))
    }

    private func resolve(
        _ id: CommandID, shell: PublishedCommands?, page: PublishedCommands? = nil
    ) -> CommandItemState {
        CommandItemState.resolve(CommandMap.entry(for: id), shell: shell, page: page)
    }

    // MARK: scope and key window

    @Test func aShellItemIsOffWithoutAShellPublisher() {
        let state = resolve(.showOverview, shell: nil)
        #expect(state.isEnabled == false)
        #expect(state.subtitle == nil)
        #expect(state.run == nil)
    }

    @Test func aShellItemIsOnOnlyWhileTheWindowIsKey() {
        let actions = [action(.showOverview)]
        #expect(resolve(.showOverview, shell: published(.shell, actions, key: true)).isEnabled)
        #expect(
            resolve(.showOverview, shell: published(.shell, actions, key: false)).isEnabled
                == false)
    }

    @Test func aShellItemWithoutAnActionIsOffWithoutAReason() {
        let state = resolve(.showCatalog, shell: published(.shell, [action(.showOverview)]))
        #expect(state.isEnabled == false)
        #expect(state.subtitle == nil)
    }

    @Test func aPageItemNeedsItsPageToPublish() {
        let shell = published(.shell, [])
        let items = [action(.find)]
        #expect(resolve(.find, shell: shell, page: nil).isEnabled == false)
        #expect(
            resolve(.find, shell: shell, page: published(.page(.catalog), items)).isEnabled)
    }

    @Test func aPageItemIsOffWhenTheSectionIsNotInItsScope() {
        let shell = published(.shell, [])
        let state = resolve(
            .openInstance, shell: shell,
            page: published(.page(.catalog), [action(.openInstance)]))
        #expect(state.isEnabled == false)
    }

    @Test func aPageItemIsOffWhenTheShellIsNotKeyEvenIfThePageIsPublished() {
        let page = published(.page(.catalog), [action(.find)])
        #expect(resolve(.find, shell: nil, page: page).isEnabled == false)
        #expect(
            resolve(.find, shell: published(.shell, [], key: false), page: page).isEnabled
                == false)
    }

    // MARK: availability, gate, busy, reason

    @Test func aMutatingActionIsOffWithNoSubtitleWhileTheGateIsBlocked() {
        let shell = published(.shell, [action(.newInstance, mutates: true)], gate: Self.blocked)
        let state = resolve(.newInstance, shell: shell)
        #expect(state.isEnabled == false)
        #expect(state.subtitle == nil)
        #expect(state.run == nil)
    }

    @Test func aNonMutatingActionIgnoresTheGate() {
        let shell = published(.shell, [action(.showOverview, mutates: false)], gate: Self.blocked)
        #expect(resolve(.showOverview, shell: shell).isEnabled)
    }

    @Test func aBusyActionIsOffAndSaysInProgress() {
        let shell = published(.shell, [action(.reconnect, busy: true)])
        let state = resolve(.reconnect, shell: shell)
        #expect(state.isEnabled == false)
        #expect(state.subtitle == "In progress")
    }

    @Test func anActionsOwnReasonIsTheSubtitle() {
        let shell = published(
            .shell, [action(.reconnect, reason: "Relaunch Stackly after fixing the problem.")])
        let state = resolve(.reconnect, shell: shell)
        #expect(state.isEnabled == false)
        #expect(state.subtitle == "Relaunch Stackly after fixing the problem.")
    }

    @Test func theOwnReasonStillShowsWhileTheGateIsBlocked() {
        let shell = published(
            .shell, [action(.newInstance, mutates: true, reason: "No runtime installed.")],
            gate: Self.blocked)
        #expect(resolve(.newInstance, shell: shell).subtitle == "No runtime installed.")
    }

    // MARK: title and checkmark

    @Test func aRowWithAnAlternateTitleTakesTheActionsTitle() {
        let hide = published(
            .page(.instances), [action(.toggleInspector, title: "Hide Inspector")])
        let state = resolve(.toggleInspector, shell: published(.shell, []), page: hide)
        #expect(state.title == "Hide Inspector")
        let show = published(
            .page(.instances), [action(.toggleInspector, title: "Show Inspector")])
        #expect(
            resolve(.toggleInspector, shell: published(.shell, []), page: show).title
                == "Show Inspector")
    }

    @Test func aRowWithoutAnAlternateTitleKeepsTheMapsTitle() {
        let shell = published(.shell, [action(.showOverview, title: "Something else")])
        #expect(resolve(.showOverview, shell: shell).title == "Overview")
    }

    @Test func aToggleIsCheckedOnlyWhenThePublisherSaysSo() {
        let shell = published(.shell, [])
        let name = action(.sortInstancesByName)
        let on = published(.page(.instances), [name], checked: [.sortInstancesByName])
        let off = published(.page(.instances), [name])
        #expect(resolve(.sortInstancesByName, shell: shell, page: on).isChecked)
        #expect(resolve(.sortInstancesByName, shell: shell, page: off).isChecked == false)
    }

    // MARK: run goes through the host

    @Test func runningAnItemRunsTheActionThroughTheHost() throws {
        let key = MainWindowKey(isKeyWindow: { _ in true })
        let shell = published(.shell, [action(.showOverview)], windowKey: key)
        let run = try #require(resolve(.showOverview, shell: shell).run)
        keyWindow(key)
        run()
        #expect(log.performed == ["showOverview"])
    }

    @Test func runningAnActionThatConfirmsPresentsTheDialogAndDoesNotPerform() throws {
        let confirmation = ActionConfirmation(
            title: "Remove “pg17-dev”?", message: "Data is kept.", actionTitle: "Remove")
        let key = MainWindowKey(isKeyWindow: { _ in true })
        let page = published(
            .page(.instances),
            [
                action(
                    .removeInstance, title: "Remove…", mutates: true,
                    role: .destructive(confirmation))
            ], windowKey: key)
        let run = try #require(
            resolve(.removeInstance, shell: published(.shell, [], windowKey: key), page: page).run)
        keyWindow(key)
        run()
        #expect(log.presented == ["Remove “pg17-dev”?"])
        #expect(log.performed.isEmpty)
    }

    @Test func theRunGuardRefusesWhenTheWindowIsNotKeyAnymore() throws {
        let keyed = Keyed()
        let key = MainWindowKey(isKeyWindow: { _ in keyed.value })
        let shell = published(.shell, [action(.showOverview)], windowKey: key)
        let run = try #require(resolve(.showOverview, shell: shell).run)
        keyWindow(key)
        keyed.value = false
        run()
        #expect(log.performed.isEmpty)
    }

    @Test func aWindowKeyThatNeverTrackedAWindowIsNotKeyNow() throws {
        let shell = published(
            .shell, [action(.showOverview)], windowKey: MainWindowKey())
        let run = try #require(resolve(.showOverview, shell: shell).run)
        run()
        #expect(log.performed.isEmpty)
    }

    // MARK: app scope and system rows

    @Test func anAppItemDoesNotGoThroughResolveAndStaysOn() {
        let state = CommandItemState.resolve(
            CommandMap.entry(for: .welcome), shell: nil, page: nil)
        #expect(state.isEnabled)
        #expect(state.run == nil)
    }

    @Test func aSystemRowHasNothingToRun() {
        let state = CommandItemState.resolve(
            CommandMap.entry(for: .showMainWindow), shell: nil, page: nil)
        #expect(state.isEnabled == false)
        #expect(state.run == nil)
    }

    // MARK: helpers

    private final class Keyed {
        var value = true
    }

    /// Gives the key state a window, so `isKeyNow` consults its test closure.
    private func keyWindow(_ key: MainWindowKey) {
        key.track(NSWindow())
    }
}
