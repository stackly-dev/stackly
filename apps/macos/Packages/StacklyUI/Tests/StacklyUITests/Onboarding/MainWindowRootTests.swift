import AppKit
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// What the main window shows and what Help > Welcome to Stackly does (spec 0016, AC-1 and
/// AC-3).
struct MainWindowRootTests {
    /// A runtime over a link that never connects, with the given record.
    private static func runtime(store: InMemoryOnboardingRecordStore) -> ShellRuntime {
        let link = ShellRuntimeTests.CountingLink()
        let model = DaemonModel(
            link: link.handle, signals: nil, endpoint: nil, launchedAt: DaemonModelTests.launch,
            announce: { _ in })
        return ShellRuntime(
            model: model, closeNotice: CloseNotice(presentsPanel: false), onboardingStore: store)
    }

    /// Whether the content of a window that fills `MainWindowRoot` holds a split view, which
    /// only the shell draws.
    private static func showsShell(of runtime: ShellRuntime) async throws -> Bool {
        let root = MainWindowRoot(runtime: runtime, windowWillClose: { _ in })
        let controller = NSHostingController(rootView: root)
        let window = NSWindow(contentViewController: controller)
        defer { window.close() }
        window.setContentSize(AppShell.minimumSize)
        let content = try #require(window.contentView)
        for _ in 0..<10 {
            content.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(20))
        }
        return hasSplitView(content)
    }

    private static func hasSplitView(_ view: NSView) -> Bool {
        view is NSSplitView || view.subviews.contains(where: hasSplitView)
    }

    // MARK: - The window content (AC-1)

    @Test func theWindowShowsTheOnboardingAsWholeContentUntilItIsFinished() async throws {
        let runtime = Self.runtime(store: InMemoryOnboardingRecordStore())
        #expect(try await Self.showsShell(of: runtime) == false)
    }

    @Test func theWindowShowsTheShellWhenTheRecordSaysFinished() async throws {
        let runtime = Self.runtime(store: InMemoryOnboardingRecordStore(isFinished: true))
        #expect(try await Self.showsShell(of: runtime))
    }

    @Test func theWindowSwitchesToTheShellWhenTheOnboardingFinishes() async throws {
        let runtime = Self.runtime(store: InMemoryOnboardingRecordStore())
        let root = MainWindowRoot(runtime: runtime, windowWillClose: { _ in })
        let controller = NSHostingController(rootView: root)
        let window = NSWindow(contentViewController: controller)
        defer { window.close() }
        window.setContentSize(AppShell.minimumSize)
        let content = try #require(window.contentView)
        runtime.onboarding.finish(.skipped)
        for _ in 0..<20 {
            content.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(20))
        }
        #expect(Self.hasSplitView(content))
    }

    @Test func aWindowThatOpensAgainReadsTheSameStepFromTheRuntime() {
        let runtime = Self.runtime(store: InMemoryOnboardingRecordStore())
        runtime.onboarding.go(to: .storage)
        // The window and its views are gone; the runtime is what the next window reads.
        #expect(runtime.onboarding.isPresented)
        #expect(runtime.onboarding.step == .storage)
    }

    // MARK: - Help > Welcome to Stackly (AC-3)

    @Test func welcomeShowsTheOnboardingAtStepOneAndTouchesNothingElse() throws {
        let store = InMemoryOnboardingRecordStore(isFinished: true)
        let runtime = Self.runtime(store: store)
        let welcome = try #require(StacklyCommands.appActions(runtime: runtime)[.welcome])
        welcome()
        #expect(runtime.onboarding.isPresented)
        #expect(runtime.onboarding.step == .welcome)
        #expect(store.writes == 0)
    }

    @Test func welcomeWhileTheOnboardingShowsKeepsItsStep() throws {
        let runtime = Self.runtime(store: InMemoryOnboardingRecordStore())
        runtime.onboarding.go(to: .catalogTrust)
        let welcome = try #require(StacklyCommands.appActions(runtime: runtime)[.welcome])
        welcome()
        #expect(runtime.onboarding.step == .catalogTrust)
    }

    @Test func welcomeIsABuiltItemOfTheHelpMenuWithoutAShortcut() {
        let entry = CommandMap.entry(for: .welcome)
        #expect(entry.status == .built)
        #expect(entry.menu == .help)
        #expect(entry.title == "Welcome to Stackly")
        #expect(entry.shortcut == nil)
        #expect(entry.title == OnboardingCopy.helpMenuItem)
    }
}
