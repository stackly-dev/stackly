import AppKit
import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The whole onboarding walked with the real views on screen, over fakes and a temporary home
/// (spec 0016, AC-3): showing, moving, skipping, and finishing change nothing. The agent's
/// controller sees only `status`, the temporary home stays as it was, and the daemon link
/// sends no call, which is where every mutation goes.
@Suite(.timeLimit(.minutes(2)))
struct OnboardingWalkTests {
    /// Everything a walk watches.
    final class Walk {
        let runtime: ShellRuntime
        let controller = FakeBackgroundAgentController()
        let link = ShellRuntimeTests.CountingLink()
        let home: CommandLineToolTests.Home
        let store = InMemoryOnboardingRecordStore()
        let feedback: FeedbackCenter
        var spoken: [String] = []
        let homeBefore: Set<String>
        private var window: NSWindow?

        init() throws {
            home = try CommandLineToolTests.Home()
            let bundle = try home.bundle("walk")
            let controller = controller
            let agent = BackgroundAgentModel(
                controller: controller, launchRecord: FakeLaunchRecordReader(.missing),
                rootOverrideActive: false, initialProbe: nil, initialLaunchRecord: .missing)
            let model = DaemonModel(
                link: link.handle, signals: nil, endpoint: nil,
                launchedAt: DaemonModelTests.launch, announce: { _ in }, agent: agent)
            runtime = ShellRuntime(
                model: model, closeNotice: CloseNotice(presentsPanel: false),
                commandLineTool: CommandLineToolModel(controller: home.tool(for: bundle)),
                onboardingStore: store)
            let box = SpokenBox()
            feedback = FeedbackCenter(announce: { box.texts.append($0) })
            spokenBox = box
            homeBefore = home.paths()
        }

        private let spokenBox: SpokenBox
        var announcements: [String] { spokenBox.texts }

        /// Shows the onboarding in a window, as the main window does.
        func show() {
            let flow = OnboardingFlow(onboarding: runtime.onboarding)
                .environment(runtime.model)
                .environment(runtime.commandLineTool)
                .environment(\.feedbackCenter, feedback)
                .frame(width: 900, height: 600)
            let controller = NSHostingController(rootView: flow)
            let window = NSWindow(contentViewController: controller)
            window.setContentSize(CGSize(width: 900, height: 600))
            self.window = window
        }

        /// Lets SwiftUI lay out and run the tasks of the step on screen.
        func settle() async throws {
            let content = try #require(window?.contentView)
            for _ in 0..<6 {
                content.layoutSubtreeIfNeeded()
                try await Task.sleep(for: .milliseconds(15))
            }
        }

        func close() {
            window?.close()
            runtime.willTerminate()
        }

        /// Whether nothing changed outside the finished record.
        func expectNoSideEffects(_ context: String) {
            #expect(
                controller.calls.allSatisfy { $0 == .status }, "\(context): \(controller.calls)")
            #expect(home.paths() == homeBefore, "\(context): the home changed")
            #expect(link.calls.withLock { $0 } == 0, "\(context): the link sent a call")
        }
    }

    final class SpokenBox {
        var texts: [String] = []
    }

    // MARK: - Continue, Not Now, Back, the dots, and Get Started

    @Test func aFullWalkWithTheButtonsChangesNothingAndEndsInTheShell() async throws {
        let walk = try Walk()
        defer { walk.close() }
        walk.show()
        let onboarding = walk.runtime.onboarding
        try await walk.settle()
        // Forward with the primary button: Continue, Not Now, Continue, Continue.
        for expected in OnboardingStep.allCases.dropFirst() {
            OnboardingActions.primary(onboarding: onboarding, feedback: walk.feedback)
            #expect(onboarding.step == expected)
            try await walk.settle()
        }
        #expect(walk.controller.count(of: .status) >= 1, "the agent step read the status")
        // Back twice, then every dot.
        onboarding.back()
        onboarding.back()
        #expect(onboarding.step == .storage)
        for step in OnboardingStep.allCases {
            onboarding.go(to: step)
            try await walk.settle()
        }
        walk.expectNoSideEffects("before Get Started")
        #expect(walk.store.writes == 0)
        #expect(onboarding.isPresented)
        // Get Started writes the record and shows the good news once.
        OnboardingActions.primary(onboarding: onboarding, feedback: walk.feedback)
        #expect(onboarding.isPresented == false)
        #expect(walk.store.writes == 1)
        #expect(walk.announcements == ["Setup complete."])
        walk.expectNoSideEffects("after Get Started")
    }

    // MARK: - Escape on every step

    @Test(arguments: OnboardingStep.allCases)
    func escapeOnAnyStepChangesNothingAndShowsNoFeedback(step: OnboardingStep) async throws {
        let walk = try Walk()
        defer { walk.close() }
        walk.show()
        let onboarding = walk.runtime.onboarding
        onboarding.go(to: step)
        try await walk.settle()
        OnboardingActions.skip(onboarding: onboarding)
        try await walk.settle()
        #expect(onboarding.isPresented == false)
        #expect(walk.store.writes == 1)
        #expect(walk.feedback.current == nil)
        #expect(walk.announcements.isEmpty)
        walk.expectNoSideEffects("Escape on \(step)")
    }

    // MARK: - Help > Welcome to Stackly

    @Test func helpOpensTheOnboardingAgainFromTheShellAtStepOneWithoutAnySideEffect() async throws {
        let walk = try Walk()
        defer { walk.close() }
        walk.runtime.onboarding.finish(.skipped)
        let welcome = try #require(StacklyCommands.appActions(runtime: walk.runtime)[.welcome])
        welcome()
        #expect(walk.runtime.onboarding.isPresented)
        #expect(walk.runtime.onboarding.step == .welcome)
        walk.show()
        try await walk.settle()
        walk.expectNoSideEffects("Help")
        #expect(walk.store.writes == 1, "only the first finish wrote")
        // While it shows, Help only keeps the step.
        walk.runtime.onboarding.go(to: .storage)
        welcome()
        #expect(walk.runtime.onboarding.step == .storage)
    }

    // MARK: - The shell section and the file the onboarding may write

    @Test func theOnboardingFolderNeverTouchesTheShellSectionOrTheDaemonRoots() throws {
        for file in try SourceTree.swiftFiles() where file.path.contains("/StacklyUI/Onboarding/") {
            let code = file.codeLines.joined(separator: "\n")
            for token in ["shell.section", "SceneStorage", "SceneStorage(", "FileManager"] {
                #expect(code.contains(token) == false, "\(file.path) uses \(token)")
            }
        }
    }
}
