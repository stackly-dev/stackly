import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// How a confirmation shows and answers (spec 0012 AC-15): a sheet on the window of the
/// view that has the modifier, app modal without one, one answer per alert, and the binding
/// reset before `perform` runs.
///
/// These tests drive real `NSAlert`s by clicking their buttons, so an alert appears on
/// screen for a moment without taking focus. Return and Escape act only on a key window,
/// which a test process gets only by activating itself, and activating ends the test run.
/// So `ConfirmationTests` pins the key equivalents, and `/check verify` presses the keys in
/// the Debug app (see `docs/specs/0012-design-system-ui-foundation/verify.md`).
///
/// Main actor tasks run only while a test awaits, so these tests are `async` and wait with
/// ``settle(_:until:)``. The app modal path has its own suite, ``ConfirmationModalTests``.
@Suite(.serialized)
struct ConfirmationPresentationTests {
    // MARK: Sheet on the anchor's window

    /// covers: AC-15
    @Test func requestWithAWindowShowsAsASheetOnThatWindow() async throws {
        let window = Self.makeWindow()
        defer { window.orderOut(nil) }
        let presenter = ConfirmationPresenter()
        presenter.anchor = window.contentView
        let request = ConfirmationTests.removeRequest()

        presenter.show(request) { _ in }

        let sheet = try #require(await Self.sheet(on: window))
        #expect(NSApp.modalWindow == nil)
        #expect(Self.texts(in: sheet).contains(request.title))
        #expect(Self.texts(in: sheet).contains(request.message))
        #expect(presenter.shownID == request.id)
        presenter.close()
    }

    /// One click per role and button, with the answer the click must give.
    nonisolated struct ClickCase: Sendable, CustomTestStringConvertible {
        let role: ConfirmationRequest.Role
        let button: String
        let confirmed: Bool

        var testDescription: String { "\(role) \(button) → \(confirmed)" }
    }

    nonisolated static let clickCases = [
        ClickCase(role: .destructive, button: "Remove", confirmed: true),
        ClickCase(role: .destructive, button: "Cancel", confirmed: false),
        ClickCase(role: .standard, button: "Bring Down", confirmed: true),
        ClickCase(role: .standard, button: "Cancel", confirmed: false),
    ]

    /// covers: AC-15
    @Test(arguments: clickCases)
    func clickingAButtonAnswersOnceAndClosesTheSheet(click: ClickCase) async throws {
        let window = Self.makeWindow()
        defer { window.orderOut(nil) }
        let presenter = ConfirmationPresenter()
        presenter.anchor = window.contentView
        var answers: [Bool] = []
        presenter.show(ConfirmationTests.request(click.role)) { answers.append($0) }
        let sheet = try #require(await Self.sheet(on: window))

        try Self.click(click.button, in: sheet)

        #expect(await Self.settle { answers.isEmpty == false })
        #expect(await Self.settle { window.attachedSheet == nil })
        #expect(answers == [click.confirmed])
        #expect(presenter.shownID == nil)
    }

    /// covers: AC-15
    @Test func replacedRequestNeverAnswersAndTheNewOneShows() async throws {
        let window = Self.makeWindow()
        defer { window.orderOut(nil) }
        let presenter = ConfirmationPresenter()
        presenter.anchor = window.contentView
        var answers: [String] = []
        let first = ConfirmationTests.removeRequest()
        let second = ConfirmationTests.bringDownRequest()
        presenter.show(first) { answers.append("first \($0)") }
        _ = try #require(await Self.sheet(on: window))

        presenter.show(second) { answers.append("second \($0)") }

        #expect(
            await Self.settle {
                window.attachedSheet.map { Self.texts(in: $0).contains(second.title) } ?? false
            })
        #expect(answers.isEmpty)
        #expect(presenter.shownID == second.id)
        let sheet = try #require(window.attachedSheet)
        try Self.click("Cancel", in: sheet)
        #expect(await Self.settle { answers.isEmpty == false })
        #expect(answers == ["second false"])
    }

    /// covers: AC-15
    @Test func closeEndsTheSheetWithoutAnAnswer() async throws {
        let window = Self.makeWindow()
        defer { window.orderOut(nil) }
        let presenter = ConfirmationPresenter()
        presenter.anchor = window.contentView
        var answers: [Bool] = []
        presenter.show(ConfirmationTests.removeRequest()) { answers.append($0) }
        _ = try #require(await Self.sheet(on: window))

        presenter.close()

        #expect(await Self.settle { window.attachedSheet == nil })
        #expect(answers.isEmpty)
        #expect(presenter.shownID == nil)
    }

    // MARK: The modifier

    /// The state a screen keeps for `.stacklyConfirmation(item:)`.
    @Observable final class Screen {
        var request: ConfirmationRequest?
        var showsConfirmingView = true
        var performed: [String] = []
    }

    /// A screen with the modifier, which it can remove to test what happens on disappear.
    struct ScreenView: View {
        @Bindable var screen: Screen

        var body: some View {
            if screen.showsConfirmingView {
                Color.clear
                    .frame(width: 320, height: 200)
                    .stacklyConfirmation(item: $screen.request)
            } else {
                Color.clear
                    .frame(width: 320, height: 200)
            }
        }
    }

    /// Hosts `ScreenView` for `screen` in a window and lets it appear.
    static func host(_ screen: Screen) async -> NSWindow {
        let window = makeWindow()
        window.contentView = NSHostingView(rootView: ScreenView(screen: screen))
        try? await Task.sleep(for: .milliseconds(200))
        return window
    }

    /// covers: AC-15
    @Test func confirmingClearsTheBindingBeforePerformRuns() async throws {
        let screen = Screen()
        let window = await Self.host(screen)
        defer { window.orderOut(nil) }
        screen.request = ConfirmationRequest(
            title: "Bring down “erp-development”?",
            message: "Releases the lease. Data is kept.",
            actionTitle: "Bring Down",
            role: .standard
        ) {
            screen.performed.append("Bring Down, binding is nil: \(screen.request == nil)")
        }
        let sheet = try #require(await Self.sheet(on: window))

        try Self.click("Bring Down", in: sheet)

        #expect(await Self.settle { screen.performed.isEmpty == false })
        #expect(screen.performed == ["Bring Down, binding is nil: true"])
        #expect(screen.request == nil)
        #expect(await Self.settle { window.attachedSheet == nil })
    }

    /// covers: AC-15
    @Test func cancellingClearsTheBindingAndNeverPerforms() async throws {
        let screen = Screen()
        let window = await Self.host(screen)
        defer { window.orderOut(nil) }
        screen.request = ConfirmationTests.removeRequest { screen.performed.append("Remove") }
        let sheet = try #require(await Self.sheet(on: window))

        try Self.click("Cancel", in: sheet)

        #expect(await Self.settle { screen.request == nil })
        #expect(await Self.settle { window.attachedSheet == nil })
        #expect(screen.performed.isEmpty)
    }

    /// covers: AC-15
    @Test func clearingTheBindingClosesTheDialogWithoutPerforming() async throws {
        let screen = Screen()
        let window = await Self.host(screen)
        defer { window.orderOut(nil) }
        screen.request = ConfirmationTests.removeRequest { screen.performed.append("Remove") }
        _ = try #require(await Self.sheet(on: window))

        screen.request = nil

        #expect(await Self.settle { window.attachedSheet == nil })
        #expect(screen.performed.isEmpty)
    }

    /// covers: AC-15
    @Test func removingTheViewClosesTheDialogAndClearsTheBinding() async throws {
        let screen = Screen()
        let window = await Self.host(screen)
        defer { window.orderOut(nil) }
        screen.request = ConfirmationTests.removeRequest { screen.performed.append("Remove") }
        _ = try #require(await Self.sheet(on: window))

        screen.showsConfirmingView = false

        #expect(await Self.settle { window.attachedSheet == nil })
        #expect(await Self.settle { screen.request == nil })
        #expect(screen.performed.isEmpty)
    }

    // MARK: Helpers

    /// A titled window for sheets, transparent so the tests show as little as possible.
    static func makeWindow() -> NSWindow {
        _ = NSApplication.shared
        let window = NSWindow(
            contentRect: CGRect(x: 0, y: 0, width: 480, height: 320),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.alphaValue = 0
        window.orderFrontRegardless()
        return window
    }

    /// Waits, letting main actor tasks and AppKit run, until `condition` holds or `timeout`
    /// passes. Returns whether it holds.
    ///
    /// The timeout counts responsive time only (``ResponsiveTimer``): another test that holds
    /// the main actor in a full run counts as one check, not as the whole stall.
    static func settle(
        _ timeout: Duration = .seconds(3),
        until condition: () -> Bool
    ) async -> Bool {
        var timer = ResponsiveTimer(maxCharge: .milliseconds(70))
        while condition() == false, timer.counted < timeout {
            try? await Task.sleep(for: .milliseconds(20))
            timer.lap()
        }
        return condition()
    }

    /// The sheet attached to `window`, once it appears.
    static func sheet(on window: NSWindow) async -> NSWindow? {
        _ = await settle { window.attachedSheet != nil }
        return window.attachedSheet
    }

    /// Clicks the button titled `title` in `window`.
    static func click(_ title: String, in window: NSWindow) throws {
        let content = try #require(window.contentView)
        let button = try #require(buttons(in: content).first { $0.title == title })
        button.performClick(nil)
    }

    /// Every button under `view`.
    static func buttons(in view: NSView) -> [NSButton] {
        var found = view.subviews.flatMap { buttons(in: $0) }
        if let button = view as? NSButton {
            found.insert(button, at: 0)
        }
        return found
    }

    /// The text of every text field in `window`.
    static func texts(in window: NSWindow) -> [String] {
        window.contentView.map { texts(in: $0) } ?? []
    }

    /// The text of every text field under `view`.
    static func texts(in view: NSView) -> [String] {
        var found = view.subviews.flatMap { texts(in: $0) }
        if let field = view as? NSTextField {
            found.insert(field.stringValue, at: 0)
        }
        return found
    }
}

/// The app modal path of a confirmation without a window (spec 0012 AC-15), such as the
/// menu bar popover of feature 32.
///
/// Off by default. Once `runModal()` has run in a `swift test` process, the next test that
/// lets AppKit run ends the whole test run early with exit code 0 and no summary, which
/// would hide every later test. So run this suite on its own:
/// `STACKLY_UI_MODAL_TESTS=1 swift test --package-path apps/macos/Packages/StacklyUI
/// --filter ConfirmationModalTests`.
@Suite(
    .serialized,
    .enabled(if: ProcessInfo.processInfo.environment["STACKLY_UI_MODAL_TESTS"] == "1")
)
struct ConfirmationModalTests {
    /// What a timer saw and did while the app modal alert ran.
    final class ModalProbe {
        var sawModalAlert = false
        var answers: [Bool] = []
    }

    /// covers: AC-15
    @Test func withoutAWindowTheAlertRunsAppModalAndAnswersTheClick() {
        _ = NSApplication.shared
        let presenter = ConfirmationPresenter()
        let probe = ModalProbe()
        Self.whileModal {
            guard let window = NSApp.modalWindow, let content = window.contentView else { return }
            probe.sawModalAlert = ConfirmationPresentationTests.texts(in: content).contains(
                "Remove “pg17-dev”?")
            ConfirmationPresentationTests.buttons(in: content).first { $0.title == "Remove" }?
                .performClick(nil)
        }

        presenter.show(ConfirmationTests.removeRequest()) { probe.answers.append($0) }

        #expect(presenter.anchor == nil)
        #expect(probe.sawModalAlert)
        #expect(probe.answers == [true])
        #expect(NSApp.modalWindow == nil)
    }

    /// covers: AC-15
    @Test func closeEndsAnAppModalAlertWithoutAnAnswer() {
        _ = NSApplication.shared
        let presenter = ConfirmationPresenter()
        let probe = ModalProbe()
        Self.whileModal {
            probe.sawModalAlert = NSApp.modalWindow != nil
            presenter.close()
        }

        presenter.show(ConfirmationTests.removeRequest()) { probe.answers.append($0) }

        #expect(probe.sawModalAlert)
        #expect(probe.answers.isEmpty)
        #expect(NSApp.modalWindow == nil)
        #expect(presenter.shownID == nil)
    }

    /// Runs `body` once, 0.5 s from now, also while an app modal alert runs.
    static func whileModal(_ body: @escaping @MainActor () -> Void) {
        let timer = Timer(timeInterval: 0.5, repeats: false) { _ in
            MainActor.assumeIsolated { body() }
        }
        RunLoop.main.add(timer, forMode: .common)
    }
}
