import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The key value row and the card section (spec 0067, AC-3): when the value drops below
/// the label, and how tall a card is.
struct KeyValueLayoutTests {
    /// The height a view takes at `width`.
    private func height(of view: some View, width: CGFloat) -> CGFloat {
        let host = NSHostingView(rootView: view.frame(width: width))
        host.frame = CGRect(x: 0, y: 0, width: width, height: 10)
        host.layoutSubtreeIfNeeded()
        return host.fittingSize.height
    }

    private func row(_ value: String) -> some View {
        LabeledContent("Data directory", value: value)
            .labeledContentStyle(.stackly)
    }

    @Test func shortValueStaysOnOneLineWithItsLabel() {
        let single = height(of: row("127.0.0.1"), width: 400)
        let longer = height(of: row(String(repeating: "x", count: 60)), width: 400)
        #expect(single > 0)
        #expect(longer > single, "a value that does not fit moves below the label")
    }

    @Test func longValueWrapsInsteadOfBeingCut() {
        let oneLine = height(of: row(String(repeating: "x", count: 60)), width: 400)
        let wrapped = height(of: row(String(repeating: "word ", count: 60)), width: 400)
        #expect(wrapped > oneLine)
    }

    @Test func cardRowsAreAtLeastTheRowMinimumHigh() {
        let card = CardSection("Summary") {
            LabeledContent("A", value: "1")
            LabeledContent("B", value: "2")
            LabeledContent("C", value: "3")
        }
        .labeledContentStyle(.stackly)
        let one = CardSection("Summary") {
            LabeledContent("A", value: "1")
        }
        .labeledContentStyle(.stackly)
        let minimum = ScaledSize.rowMinHeight.base
        #expect(height(of: one, width: 400) >= minimum)
        #expect(height(of: card, width: 400) >= 3 * minimum)
        #expect(height(of: card, width: 400) > height(of: one, width: 400) + 2 * minimum - 1)
    }

    @Test func forEachMakesOneRowPerElement() {
        let rows = ["a", "b", "c", "d"]
        let looped = CardSection {
            ForEach(rows, id: \.self) { Text($0) }
        }
        let written = CardSection {
            Text("a")
            Text("b")
            Text("c")
            Text("d")
        }
        #expect(height(of: looped, width: 400) == height(of: written, width: 400))
    }

    @Test func aCardWithATitleAndFooterIsTallerThanTheCardAlone() {
        let bare = CardSection { Text("a") }
        let full = CardSection("Title", footer: "Footer") { Text("a") }
        #expect(height(of: full, width: 400) > height(of: bare, width: 400))
    }

    @Test func voiceOverReadsTheRowAsOneElementBuiltFromLabelAndValue() throws {
        let path =
            "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/KeyValue/StacklyLabeledContentStyle.swift"
        let code = SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
        )
        .codeLines.joined(separator: "\n")
        #expect(code.contains(".accessibilityRepresentation {"))
        #expect(code.contains(".accessibilityElement(children: .combine)"))
        #expect(code.contains(".textSelection(.enabled)"))
    }
}
