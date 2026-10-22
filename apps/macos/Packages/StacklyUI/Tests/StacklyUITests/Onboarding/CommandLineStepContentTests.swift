import Foundation
import Testing

@testable import StacklyUI

/// Every row of the *Kontrol Command Line* table (spec 0016, AC-8).
struct CommandLineStepContentTests {
    static let tool = URL(filePath: "/Applications/Stackly.app/Contents/MacOS/stackly-cli")
    static let missingTool = CommandLineToolModel.missingToolReason

    static func make(
        _ status: CommandLineLinkState, tool: URL? = tool, error: CommandLineToolError? = nil,
        isWorking: Bool = false
    ) -> CommandLineStepContent {
        CommandLineStepContent.make(
            status: status, bundledTool: tool, lastError: error, isWorking: isWorking)
    }

    @Test func notInstalledHasAnOffToggle() {
        let content = Self.make(.notInstalled)
        #expect(content.title == "Install stackly in ~/.local/bin")
        #expect(
            content.detail
                == "No password needed. Stackly doesn’t change your PATH or shell files.")
        #expect(content.control == .toggle(isOn: false, disabledReason: nil))
        #expect(content.showsPathNote == false)
        #expect(content.runDirectlyPath == nil)
    }

    @Test func installedHasAnOnToggleThePathNoteAndTheFullPath() {
        let content = Self.make(.installed)
        #expect(content.detail == "~/.local/bin/stackly points to this copy of Stackly.")
        #expect(content.control == .toggle(isOn: true, disabledReason: nil))
        #expect(content.showsPathNote)
        #expect(content.runDirectlyPath == Self.tool.path)
        #expect(content.runDirectlyPath?.hasSuffix("Contents/MacOS/stackly-cli") == true)
    }

    @Test(arguments: [CommandLineLinkState.stale, .broken])
    func aStaleOrBrokenLinkHasRepair(state: CommandLineLinkState) {
        let content = Self.make(state)
        #expect(content.control == .repair(disabledReason: nil))
        #expect(content.showsPathNote == false)
        #expect(
            content.detail
                == (state == .stale
                    ? "~/.local/bin/stackly points to another copy of Stackly."
                    : "~/.local/bin/stackly points to a copy of Stackly that’s gone."))
    }

    @Test func aConflictHasADisabledOffToggleWithTheDetailAsReason() {
        let content = Self.make(.conflict)
        let detail =
            "~/.local/bin/stackly already exists and wasn’t made by Stackly. Stackly won’t change it."
        #expect(content.detail == detail)
        #expect(content.control == .toggle(isOn: false, disabledReason: detail))
    }

    @Test func aBuildWithoutTheCommandDisablesInstallAndRepairWithTheReason() {
        #expect(
            Self.make(.notInstalled, tool: nil).control
                == .toggle(isOn: false, disabledReason: Self.missingTool))
        #expect(Self.make(.stale, tool: nil).control == .repair(disabledReason: Self.missingTool))
        #expect(Self.make(.broken, tool: nil).control == .repair(disabledReason: Self.missingTool))
        #expect(
            Self.missingTool
                == "This build doesn’t include the stackly command. Build the app with cargo xtask build-app."
        )
    }

    @Test func aBuildWithoutTheCommandStillShowsTheTrueStatusAndCanRemove() {
        let content = Self.make(.installed, tool: nil)
        #expect(content.control == .toggle(isOn: true, disabledReason: nil))
        #expect(content.showsPathNote)
        #expect(content.runDirectlyPath == nil)
    }

    @Test(arguments: [CommandLineLinkState.notInstalled, .installed, .stale, .broken])
    func workingDisablesTheControlWithTheWorkingReason(state: CommandLineLinkState) {
        let content = Self.make(state, isWorking: true)
        #expect(content.isWorking)
        switch content.control {
        case .toggle(_, let reason), .repair(let reason):
            #expect(reason == "Working…")
        }
    }

    @Test func theErrorOfTheLastActionShowsItsMessage() {
        let content = Self.make(.notInstalled, error: .folderNotSafe(path: "~/.local/bin"))
        #expect(
            content.lastError
                == "~/.local/bin must belong to you, and other users must not be able to write to it."
        )
    }

    @Test func theExampleBlockHoldsOnlyTheCommandsOfTheCopy() {
        #expect(
            OnboardingCopy.commandExamples == ["stackly daemon status", "stackly provider list"])
        #expect(
            OnboardingCopy.commandExamplesText == "$ stackly daemon status\n$ stackly provider list"
        )
        #expect(OnboardingCopy.commandExamplesLabel == "Example commands")
    }

    @Test func thePathTextsEqualTheOnesOfDiagnostics() {
        #expect(OnboardingCopy.commandLinePathLine == CommandLineToolDiagnostics.pathLine)
        #expect(OnboardingCopy.commandLinePathNote == CommandLineToolDiagnostics.pathNote)
        #expect(OnboardingCopy.commandLinePathLine == #"export PATH="$HOME/.local/bin:$PATH""#)
        #expect(OnboardingCopy.commandLineRepair == "Repair…")
    }
}
