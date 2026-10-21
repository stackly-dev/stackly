import AppKit
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The plain pasteboard write of the copy button (spec 0067, AC-4): it replaces the general
/// pasteboard with a plain string. The suite runs serialized and puts back what the
/// pasteboard held, because the general pasteboard is the one thing a test cannot give its
/// own copy of.
@Suite(.serialized)
struct PlainPasteboardTests {
    /// What the general pasteboard held, so a test can put it back.
    private struct Saved {
        let items: [[(type: NSPasteboard.PasteboardType, data: Data)]]

        init() {
            items = (NSPasteboard.general.pasteboardItems ?? []).map { item in
                item.types.compactMap { type in
                    item.data(forType: type).map { (type: type, data: $0) }
                }
            }
        }

        func restore() {
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()
            let restored = items.map { entries in
                let item = NSPasteboardItem()
                for entry in entries {
                    item.setData(entry.data, forType: entry.type)
                }
                return item
            }
            pasteboard.writeObjects(restored)
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "Spec 0067 AC-4: a copy replaces the general pasteboard with one plain string"
        ),
        arguments: [
            "127.0.0.1:5432",
            "stackly instance create --name analytics",
            "line one\nline two",
            "Ünïcode ✓ 日本語",
            "",
        ])
    func writeLeavesExactlyThePlainStringOnThePasteboard(text: String) {
        let saved = Saved()
        defer { saved.restore() }
        PlainPasteboard.write(text)
        let pasteboard = NSPasteboard.general
        #expect(pasteboard.string(forType: .string) == text)
        #expect(pasteboard.pasteboardItems?.count == 1)
        #expect(pasteboard.pasteboardItems?.first?.types == [.string], "no hidden or rich type")
    }

    @Test func aSecondWriteReplacesTheFirstInsteadOfAddingToIt() {
        let saved = Saved()
        defer { saved.restore() }
        PlainPasteboard.write("first")
        PlainPasteboard.write("second")
        let pasteboard = NSPasteboard.general
        #expect(pasteboard.string(forType: .string) == "second")
        #expect(pasteboard.pasteboardItems?.count == 1)
    }

    @Test func aPlainWriteIsNeverMarkedConcealed() {
        let saved = Saved()
        defer { saved.restore() }
        PlainPasteboard.write("not a secret")
        let concealed = NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")
        #expect(NSPasteboard.general.types?.contains(concealed) == false)
    }
}
