import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The action descriptor, its availability, invoke, and host (spec 0067, AC-6 to AC-8).
struct StacklyActionTests {
    private static let blocked = MutationAvailability.blocked(
        reason: "stacklyd is unavailable.", since: Date(timeIntervalSince1970: 0))

    private static let removal = ActionConfirmation(
        title: "Remove “pg17-dev”?",
        message: "Data is kept.",
        actionTitle: "Remove"
    )

    /// Counts the runs of an action.
    private final class Runs {
        var count = 0
    }

    private func action(
        mutates: Bool = true,
        reason: String? = nil,
        busy: Bool = false,
        role: ActionRole = .standard(confirmation: nil),
        runs: Runs = Runs()
    ) -> StacklyAction {
        StacklyAction(
            id: "test",
            title: role.confirmation == nil ? "Stop" : "Remove…",
            role: role,
            mutates: mutates,
            disabledReason: reason,
            isBusy: busy,
            perform: { runs.count += 1 }
        )
    }

    // MARK: - availability

    @Test func anActionWithNothingInTheWayIsEnabled() {
        #expect(action().availability(gate: .available) == .enabled)
    }

    @Test(
        .evidence(
            covers: ["REQ-026"],
            expected: "Spec 0067 AC-6: the action's own reason comes first and is shown"))
    func theActionsOwnReasonComesFirstAndIsShown() {
        let gated = action(reason: "This instance isn’t initialized.", busy: true)
        let expected = ActionAvailability.disabled(
            reason: "This instance isn’t initialized.", showsReason: true)
        #expect(gated.availability(gate: .available) == expected)
        #expect(gated.availability(gate: Self.blocked) == expected)
    }

    @Test(
        .evidence(
            covers: ["REQ-026"],
            expected:
                "Spec 0067 AC-6: a blocked gate disables only a mutating action and hides its reason"
        ))
    func aBlockedGateDisablesOnlyAMutatingActionAndHidesItsReason() {
        let blocked = ActionAvailability.disabled(
            reason: "stacklyd is unavailable.", showsReason: false)
        #expect(action(mutates: true).availability(gate: Self.blocked) == blocked)
        #expect(action(mutates: false).availability(gate: Self.blocked) == .enabled)
        #expect(action(mutates: true, busy: true).availability(gate: Self.blocked) == blocked)
    }

    @Test func aBusyActionIsBusyOnlyWhenNothingElseIsInTheWay() {
        #expect(action(busy: true).availability(gate: .available) == .busy)
        #expect(action(mutates: false, busy: true).availability(gate: Self.blocked) == .busy)
    }

    @Test(
        .evidence(
            covers: ["REQ-026"],
            expected: "Spec 0067 AC-8: a menu item shows its own reason or In progress"))
    func menuReasonsAreTheOwnReasonOrInProgress() {
        #expect(ActionAvailability.enabled.menuReason == nil)
        #expect(ActionAvailability.busy.menuReason == "In progress")
        #expect(
            ActionAvailability.disabled(reason: "No backups.", showsReason: true).menuReason
                == "No backups.")
        #expect(
            ActionAvailability.disabled(reason: "stacklyd is unavailable.", showsReason: false)
                .menuReason == nil)
    }

    // MARK: - invoke

    @Test func invokeRunsAnOrdinaryActionAtOnce() {
        let runs = Runs()
        var presented = 0
        action(runs: runs).invoke(gate: .available) { _ in presented += 1 }
        #expect(runs.count == 1)
        #expect(presented == 0)
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "Spec 0067 AC-6: invoke does nothing while the action cannot run"))
    func invokeDoesNothingWhileTheActionCannotRun() {
        let runs = Runs()
        var presented = 0
        let cannot = [
            action(reason: "No backups.", runs: runs),
            action(busy: true, runs: runs),
            action(role: .destructive(Self.removal), runs: runs),
        ]
        for item in cannot.prefix(2) {
            item.invoke(gate: .available) { _ in presented += 1 }
        }
        cannot[2].invoke(gate: Self.blocked) { _ in presented += 1 }
        #expect(runs.count == 0)
        #expect(presented == 0)
    }

    @Test func invokePresentsAStandardConfirmationAndRunsOnlyWhenItIsConfirmed() throws {
        let runs = Runs()
        let confirmation = ActionConfirmation(
            title: "Stop “pg17-dev”?", message: "Clients disconnect.", actionTitle: "Stop")
        let stop = StacklyAction(
            id: "stop", title: "Stop…", role: .standard(confirmation: confirmation),
            mutates: true, perform: { runs.count += 1 })
        var request: ConfirmationRequest?
        stop.invoke(gate: .available) { request = $0 }
        let shown = try #require(request)
        #expect(shown.title == "Stop “pg17-dev”?")
        #expect(shown.message == "Clients disconnect.")
        #expect(shown.actionTitle == "Stop")
        #expect(shown.role == .standard)
        #expect(runs.count == 0, "the dialog runs it, not invoke")
        shown.perform()
        #expect(runs.count == 1)
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "Spec 0067 AC-6: a destructive role presents a destructive confirmation"))
    func invokePresentsADestructiveConfirmationForADestructiveRole() throws {
        let runs = Runs()
        var request: ConfirmationRequest?
        action(role: .destructive(Self.removal), runs: runs).invoke(gate: .available) {
            request = $0
        }
        let shown = try #require(request)
        #expect(shown.role == .destructive)
        #expect(shown.copyProblems.isEmpty)
        shown.perform()
        #expect(runs.count == 1)
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "Spec 0067 AC-6: a destructive role always carries a confirmation"))
    func aDestructiveRoleAlwaysCarriesAConfirmation() {
        #expect(action(role: .destructive(Self.removal)).confirmation == Self.removal)
        #expect(action(role: .standard(confirmation: nil)).confirmation == nil)
        #expect(ActionRole.destructive(Self.removal).isDestructive)
        #expect(ActionRole.standard(confirmation: Self.removal).isDestructive == false)
    }

    // MARK: - host

    @Test func theHostRunsWithItsOwnGateAndConfirmation() throws {
        let runs = Runs()
        var request: ConfirmationRequest?
        let open = ActionHost(gate: .available) { request = $0 }
        let closed = ActionHost(gate: Self.blocked) { request = $0 }

        open.run(action(runs: runs))
        #expect(runs.count == 1)
        closed.run(action(runs: runs))
        #expect(runs.count == 1, "a blocked gate stops a mutating action")
        closed.run(action(mutates: false, runs: runs))
        #expect(runs.count == 2, "a read only action ignores the gate")

        open.run(action(role: .destructive(Self.removal), runs: runs))
        #expect(try #require(request).title == Self.removal.title)
        #expect(runs.count == 2)
    }

    @Test func withoutAHostAnOrdinaryActionStillRunsThroughTheEnvironmentGate() {
        let runs = Runs()
        action(runs: runs).run(host: nil, gate: .available)
        action(runs: runs).run(host: nil, gate: Self.blocked)
        #expect(runs.count == 1)
    }

    // MARK: - source

    @Test func theViewsStayNativeAndKeepKeyboardShortcutsToFeature69() throws {
        let folder = "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/Actions"
        for name in try SourceTree.files(under: folder).filter({ $0.hasSuffix(".swift") }) {
            let file = SourceFile(
                path: name, text: try String(contentsOf: SourceTree.url(of: name), encoding: .utf8))
            let code = file.codeLines.joined(separator: "\n")
            #expect(code.contains("keyboardShortcut") == false, "\(name) sets a shortcut")
            #expect(code.contains("onTapGesture") == false, "\(name) taps without a button")
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-026"], expected: "Spec 0067 AC-7: a destructive button is never prominent"
        ))
    func aDestructiveButtonIsNeverProminent() throws {
        let path =
            "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/Actions/ActionButton.swift"
        let code = SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
        )
        .codeLines.joined(separator: "\n")
        #expect(code.contains("prominence == .primary && !action.role.isDestructive"))
        #expect(code.contains("Button(role: action.role.isDestructive ? .destructive : nil"))
        #expect(code.contains(".accessibilityValue(\"In progress\")"))
        #expect(code.contains("AccessibilityID.Action.button(action.id)"))
    }

    @Test func theMenuUsesTheReasonPathTheStepOneCheckChose() {
        #expect(MenuReasonPath.current == .extraItem)
    }
}
