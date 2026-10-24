import Foundation
import Testing

@testable import StacklyUI

/// The row of the Command Line step over the real tool in a temporary home: the switch and
/// Repair… only ask, and only the action button of the confirmation writes (spec 0016, AC-8).
@Suite(.timeLimit(.minutes(1)))
struct CommandLineFlowTests {
    typealias Home = CommandLineToolTests.Home

    private static func model(_ home: Home, bundle: URL) async -> CommandLineToolModel {
        let model = CommandLineToolModel(controller: home.tool(for: bundle))
        await model.refresh()
        return model
    }

    private static func content(_ model: CommandLineToolModel) -> CommandLineStepContent {
        CommandLineStepContent.make(
            status: model.status.command, bundledTool: model.bundledTool,
            lastError: model.lastError, isWorking: model.isWorking)
    }

    private static func settle(_ model: CommandLineToolModel) async throws {
        try await waitUntil { model.isWorking == false }
    }

    @Test func turningTheSwitchOnOnlyAsksAndCancelWritesNothing() async throws {
        let home = try Home()
        let model = await Self.model(home, bundle: try home.bundle("a"))
        let before = home.paths()
        model.requestToggle(true)
        #expect(model.pendingPrompt?.title == "Install the stackly command?")
        #expect(model.pendingPrompt?.actionTitle == "Install")
        #expect(home.paths() == before)
        model.pendingPrompt = nil
        for _ in 0..<20 { await Task.yield() }
        #expect(home.paths() == before)
        #expect(Self.content(model).control == .toggle(isOn: false, disabledReason: nil))
    }

    @Test func installThroughTheConfirmationLinksAndTheSwitchFollows() async throws {
        let home = try Home()
        let bundle = try home.bundle("a")
        let model = await Self.model(home, bundle: bundle)
        model.requestToggle(true)
        model.pendingPrompt?.perform()
        try await waitUntil { model.status.command == .installed }
        try await Self.settle(model)
        #expect(home.paths().contains(".local/bin/stackly"))
        let content = Self.content(model)
        #expect(content.control == .toggle(isOn: true, disabledReason: nil))
        #expect(
            content.runDirectlyPath == bundle.appending(path: "Contents/MacOS/stackly-cli").path)
        #expect(content.detail == "~/.local/bin/stackly points to this copy of Stackly.")
    }

    @Test func turningTheSwitchOffAsksToRemoveAndOnlyTheLinksGo() async throws {
        let home = try Home()
        let model = await Self.model(home, bundle: try home.bundle("a"))
        model.requestToggle(true)
        model.pendingPrompt?.perform()
        try await waitUntil { model.status.command == .installed }
        try await Self.settle(model)
        model.requestToggle(false)
        #expect(model.pendingPrompt?.title == "Remove the stackly command?")
        #expect(home.paths().contains(".local/bin/stackly"))
        model.pendingPrompt?.perform()
        try await waitUntil { model.status.command == .notInstalled }
        try await Self.settle(model)
        #expect(home.paths().contains(".local/bin/stackly") == false)
        #expect(Self.content(model).control == .toggle(isOn: false, disabledReason: nil))
    }

    @Test func aStaleLinkIsRepairedThroughTheConfirmation() async throws {
        let home = try Home()
        let old = try home.bundle("old")
        let new = try home.bundle("new")
        try await home.tool(for: old).install()
        let model = await Self.model(home, bundle: new)
        #expect(model.status.command == .stale)
        #expect(Self.content(model).control == .repair(disabledReason: nil))
        model.requestRepair()
        #expect(model.pendingPrompt?.actionTitle == "Repair")
        model.pendingPrompt?.perform()
        try await waitUntil { model.status.command == .installed }
        try await Self.settle(model)
        #expect(Self.content(model).control == .toggle(isOn: true, disabledReason: nil))
    }

    @Test func aBrokenLinkIsRepaired() async throws {
        let home = try Home()
        let gone = try home.bundle("gone")
        try await home.tool(for: gone).install()
        try FileManager.default.removeItem(at: gone.deletingLastPathComponent())
        let model = await Self.model(home, bundle: try home.bundle("new"))
        #expect(model.status.command == .broken)
        #expect(Self.content(model).control == .repair(disabledReason: nil))
        model.requestRepair()
        model.pendingPrompt?.perform()
        try await waitUntil { model.status.command == .installed }
    }

    @Test func aFileThatIsNotOursDisablesTheSwitchAndStaysUntouched() async throws {
        let home = try Home()
        let manager = FileManager.default
        let link = home.commandLink
        try manager.createDirectory(
            at: link.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("mine".utf8).write(to: link)
        let model = await Self.model(home, bundle: try home.bundle("a"))
        #expect(model.status.command == .conflict)
        guard case .toggle(let isOn, let reason) = Self.content(model).control else {
            Issue.record("a conflict shows the switch")
            return
        }
        #expect(isOn == false)
        #expect(reason == OnboardingCopy.commandLineConflict)
        #expect(try Data(contentsOf: link) == Data("mine".utf8))
    }

    @Test func aBuildWithoutTheCommandCanStillRemove() async throws {
        // A bundle without the command cannot own an installed link, so the status comes from
        // the fake, which the model reads the same way.
        let fake = FakeCommandLineTool(
            bundledTool: nil,
            status: CommandLineToolStatus(
                command: .installed, commandTarget: "/x/Stackly.app/Contents/MacOS/stackly-cli"))
        let model = CommandLineToolModel(controller: fake)
        await model.refresh()
        #expect(model.bundledTool == nil)
        let content = Self.content(model)
        #expect(content.runDirectlyPath == nil)
        #expect(content.showsPathNote)
        #expect(content.control == .toggle(isOn: true, disabledReason: nil))
        model.requestToggle(false)
        model.pendingPrompt?.perform()
        try await waitUntil { model.status.command == .notInstalled }
        try await Self.settle(model)
        #expect(
            Self.content(model).control
                == .toggle(isOn: false, disabledReason: CommandLineToolModel.missingToolReason))
    }

    @Test func anUnsafeFolderShowsTheErrorAndTheSwitchGoesBack() async throws {
        let home = try Home()
        let bin = home.commandLink.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        try FileManager.default.setAttributes([.posixPermissions: 0o777], ofItemAtPath: bin.path)
        let model = await Self.model(home, bundle: try home.bundle("a"))
        model.requestToggle(true)
        model.pendingPrompt?.perform()
        try await waitUntil { model.lastError != nil }
        try await Self.settle(model)
        let content = Self.content(model)
        #expect(content.lastError?.contains("must belong to you") == true)
        #expect(content.control == .toggle(isOn: false, disabledReason: nil))
    }

    @Test func theAppBecomingActiveReadsTheStatusAgain() async throws {
        let home = try Home()
        let bundle = try home.bundle("a")
        let tool = home.tool(for: bundle)
        let model = CommandLineToolModel(controller: tool)
        let link = ShellRuntimeTests.CountingLink()
        let daemon = DaemonModel(
            link: link.handle, signals: nil, endpoint: nil, launchedAt: DaemonModelTests.launch)
        let runtime = ShellRuntime(
            model: daemon, closeNotice: CloseNotice(presentsPanel: false), commandLineTool: model,
            onboardingStore: InMemoryOnboardingRecordStore())
        try await tool.install()
        runtime.didBecomeActive()
        try await waitUntil { model.status.command == .installed }
    }
}
