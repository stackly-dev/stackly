import Foundation
import Testing

@testable import StacklyUI

/// The command line tool model over the in-memory fake: the prompts, Cancel, the actions,
/// the errors, and the lifecycle hooks (spec 0015, AC-14). The model never reads
/// `Bundle.main` or the user's home.
@Suite(.timeLimit(.minutes(1)))
struct CommandLineToolModelTests {
    func rig(
        bundledTool: URL? = FakeCommandLineTool.fixtureTool
    ) -> (FakeCommandLineTool, CommandLineToolModel) {
        let fake = FakeCommandLineTool(bundledTool: bundledTool)
        return (fake, CommandLineToolModel(controller: fake))
    }

    @Test func theModelTakesHomeAndToolFromTheControllerOnly() {
        let (_, model) = rig()
        #expect(model.home == URL(filePath: "/fixture/home"))
        #expect(model.bundledTool == FakeCommandLineTool.fixtureTool)
        #expect(model.canLink)
        #expect(rig(bundledTool: nil).1.canLink == false)
    }

    @Test func theThreeRequestsOnlyFillThePendingPrompt() async {
        let (fake, model) = rig()
        model.requestInstall()
        #expect(model.pendingPrompt?.title == "Install the stackly command?")
        model.requestRepair()
        #expect(model.pendingPrompt?.title == "Repair the stackly command?")
        model.requestRemove()
        #expect(model.pendingPrompt?.title == "Remove the stackly command?")
        #expect(fake.calls.isEmpty)
        #expect(model.status.command == .notInstalled)
    }

    @Test func cancelChangesNothing() async {
        let (fake, model) = rig()
        model.requestInstall()
        model.pendingPrompt = nil
        for _ in 0..<20 { await Task.yield() }
        #expect(fake.calls.isEmpty)
        #expect(model.status.command == .notInstalled)
    }

    @Test func confirmingInstallLinksAndReadsTheStatusAgain() async throws {
        let (fake, model) = rig()
        model.requestInstall()
        model.pendingPrompt?.perform()
        try await waitUntil { model.status.command == .installed && model.isWorking == false }
        #expect(fake.calls == [.install, .status])
        #expect(model.lastError == nil)
        model.requestRemove()
        model.pendingPrompt?.perform()
        try await waitUntil { model.status.command == .notInstalled && model.isWorking == false }
        #expect(fake.calls == [.install, .status, .remove, .status])
    }

    @Test func aFailedActionShowsItsMessageAndKeepsTheStatus() async throws {
        let (fake, model) = rig()
        fake.failNext(.folderNotSafe(path: "~/.local/bin"))
        model.requestInstall()
        model.pendingPrompt?.perform()
        try await waitUntil { model.lastError != nil && model.isWorking == false }
        #expect(
            model.lastError?.message
                == "~/.local/bin must belong to you, and other users must not be able to write to it."
        )
        #expect(model.status.command == .notInstalled)
        // The next action clears it.
        model.requestInstall()
        model.pendingPrompt?.perform()
        try await waitUntil { model.status.command == .installed && model.isWorking == false }
        #expect(model.lastError == nil)
    }

    @Test func refreshReadsTheStatus() async {
        let (fake, model) = rig()
        fake.setStatus(CommandLineToolStatus(command: .stale, commandTarget: "/old/Stackly.app/x"))
        await model.refresh()
        #expect(model.status.command == .stale)
    }

    @Test func theRuntimeRefreshesTheToolWhenTheAppBecomesActive() async throws {
        let fake = FakeCommandLineTool()
        let folder = try TemporaryFolder()
        let parts = ShellBootstrap.assemble(
            rootOverride: folder.url.path, clientVersion: "0.1.0", commandLineTool: fake)
        defer { parts.runtime.willTerminate() }
        parts.runtime.didBecomeActive()
        try await waitUntil { fake.calls.contains(.status) }
        #expect(parts.runtime.commandLineTool?.home == URL(filePath: "/fixture/home"))
    }

    // MARK: - The words

    @Test func thePromptsHaveTheWordsOfSpec0016() {
        let install = CommandLineToolPrompts.install {}
        #expect(install.title == "Install the stackly command?")
        #expect(
            install.message
                == "Stackly creates a link at ~/.local/bin/stackly and links its manual pages in ~/.local/share/man/man1. Your PATH and shell files stay as they are."
        )
        #expect(install.actionTitle == "Install")
        let repair = CommandLineToolPrompts.repair {}
        #expect(repair.title == "Repair the stackly command?")
        #expect(
            repair.message
                == "Stackly points ~/.local/bin/stackly and its manual pages to this copy of Stackly. Your PATH and shell files stay as they are."
        )
        #expect(repair.actionTitle == "Repair")
        let remove = CommandLineToolPrompts.remove {}
        #expect(remove.title == "Remove the stackly command?")
        #expect(
            remove.message
                == "Stackly deletes only the links it made in ~/.local/bin and ~/.local/share/man/man1. Stackly itself stays installed."
        )
        #expect(remove.actionTitle == "Remove")
        for request in [install, repair, remove] {
            #expect(request.role == .standard)
            #expect(request.copyProblems.isEmpty)
        }
    }

    @Test func everyErrorHasItsMessage() {
        #expect(
            CommandLineToolError.noBundledTool.message
                == "This build doesn’t include the stackly command.")
        #expect(
            CommandLineToolError.conflict(path: "~/.local/bin/stackly").message
                == "~/.local/bin/stackly already exists and wasn’t made by Stackly. Stackly won’t change it."
        )
        #expect(
            CommandLineToolError.io(path: "~/x", reason: "boom").message
                == "Stackly couldn’t change ~/x: boom.")
    }

    @Test func theMissingToolReasonIsTheSpecText() {
        #expect(
            CommandLineToolModel.missingToolReason
                == "This build doesn’t include the stackly command. Build the app with cargo xtask build-app."
        )
    }

    // MARK: - Diagnostics

    @Test func theRowsFollowTheStatusAndWriteThePathsWithATilde() {
        let facts = CommandLineToolDiagnostics(
            command: .installed,
            target: "/Users/a/Applications/Stackly.app/Contents/MacOS/stackly-cli",
            home: "/Users/a")
        let values = Dictionary(uniqueKeysWithValues: facts.rows.map { ($0.label, $0.value) })
        #expect(values["Status"] == "Installed")
        #expect(values["Command"] == "~/.local/bin/stackly")
        #expect(values["Points to"] == "~/Applications/Stackly.app/Contents/MacOS/stackly-cli")
        #expect(
            values["PATH note"]
                == "If Terminal can’t find stackly, add this line to your shell profile:")
        #expect(CommandLineToolDiagnostics.pathLine == #"export PATH="$HOME/.local/bin:$PATH""#)
        let none = CommandLineToolDiagnostics(command: .notInstalled, target: nil, home: "/Users/a")
        #expect(none.rows.first { $0.label == "Points to" }?.value == "Nothing")
    }

    @Test func eachStateOffersItsActions() {
        func actions(_ state: CommandLineLinkState) -> [CommandLineToolDiagnostics.Action] {
            CommandLineToolDiagnostics(command: state, target: nil, home: "/h").actions
        }
        #expect(actions(.notInstalled) == [.install])
        #expect(actions(.installed) == [.remove])
        #expect(actions(.stale) == [.repair, .remove])
        #expect(actions(.broken) == [.repair, .remove])
        #expect(actions(.conflict).isEmpty)
    }

    @Test func theFakeKeepsItsStatusWhenAnErrorIsQueued() async throws {
        let fake = FakeCommandLineTool()
        fake.failNext(.noBundledTool)
        await #expect(throws: CommandLineToolError.noBundledTool) { try await fake.install() }
        #expect(await fake.status().command == .notInstalled)
        try await fake.install()
        let status = await fake.status()
        #expect(status.command == .installed)
        #expect(status.manPages.allSatisfy { $0.state == .installed })
        try await fake.remove()
        #expect(await fake.status().command == .notInstalled)
    }
}
