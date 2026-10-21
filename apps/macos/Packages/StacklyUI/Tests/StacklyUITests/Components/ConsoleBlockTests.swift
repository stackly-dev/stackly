import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The console block and the copy views: VoiceOver text, keyboard reach, and where the
/// pasteboard is touched (spec 0067, AC-4, AC-5, AC-19). A hosted view builds no
/// accessibility tree without a running assistive client, so labels are read from the
/// source, as the secret views are.
struct ConsoleBlockTests {
    private static let folder = "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/Copy"

    private func code(_ name: String) throws -> String {
        let path = Self.folder + "/" + name
        return SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
        )
        .codeLines.joined(separator: "\n")
    }

    @Test(
        .evidence(
            covers: ["NFR-012"], expected: "Spec 0067 AC-5: a command is spoken without its prompt")
    )
    func aCommandIsSpokenWithoutItsPromptAndTheTextIsOneElement() {
        let block = ConsoleBlock(command: "stackly daemon status")
        #expect(block.spokenText == "stackly daemon status")
    }

    @Test func aTextBlockSpeaksItsTextAndTheDefaultsAreTheSpecCopy() {
        let block = ConsoleBlock(text: "a\nb", accessibilityLabel: "Example")
        #expect(block.spokenText == "a\nb")
        let command = ConsoleBlock(command: "x")
        #expect(command.kind == .command("x", what: "command", announcement: "Command copied."))
    }

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "Spec 0067 AC-5: the text is one VoiceOver element and the Copy button its own"))
    func theTextIsOneElementWithTheScreensLabelAndTheCopyButtonStaysSeparate() throws {
        let text = try code("ConsoleBlock.swift")
        #expect(text.contains(".accessibilityElement(children: .ignore)"))
        #expect(text.contains(".accessibilityLabel(accessibilityLabel)"))
        #expect(text.contains(".accessibilityValue(spokenText)"))
        #expect(text.contains(".accessibilityElement(children: .contain)"))
        #expect(text.contains(".textSelection(.enabled)"))
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Spec 0067 AC-5: the console surface is built from tokens only"))
    func theConsoleSurfaceIsMadeOfTokensOnly() throws {
        let text = try code("ConsoleBlock.swift")
        #expect(text.contains("StacklyColor.surfaceConsole"))
        #expect(text.contains("Radius.medium.value"))
        #expect(text.contains(".separatorColor"))
        #expect(text.contains("contrast == .increased ? 1 : 1 / displayScale"))
    }

    @Test func theCopyButtonIsALabelledNativeButtonWithA24PointClickArea() throws {
        let text = try code("CopyButton.swift")
        #expect(text.contains(#".accessibilityLabel("Copy \(what)")"#))
        #expect(text.contains(".frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)"))
        #expect(text.contains("Button(action: action)"))
        #expect(text.contains("AccessibilityID.Copy.button(key)"))
        #expect(text.contains("mutationGated") == false, "copy never changes data")
    }
}
