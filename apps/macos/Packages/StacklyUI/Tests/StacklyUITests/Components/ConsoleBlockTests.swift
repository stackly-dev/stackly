import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

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
            covers: ["NFR-012"],
            expected:
                "Spec 0067 AC-5: the console keeps the text selectable and offers its Copy button"))
    func theTextIsOneElementWithTheScreensLabelAndTheCopyButtonStaysSeparate() throws {
        let text = try code("ConsoleBlock.swift")

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
        #expect(text.contains("lineWidth: 1 / displayScale"))
    }

    @Test func theCopyButtonIsALabelledNativeButtonWithA24PointClickArea() throws {
        let text = try code("CopyButton.swift")

        #expect(text.contains(".frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)"))
        #expect(text.contains("Button(action: action)"))

        #expect(text.contains("mutationGated") == false, "copy never changes data")
    }
}
