import Foundation
import Testing

@testable import StacklyUI

/// VoiceOver labels and keyboard reach of the detail page, checked on the source (spec 0021
/// AC-16, *Aksesibilitas*): a hosted view builds no accessibility tree without a running
/// assistive client, so the labels are read from the code, as for the secret views. The manual
/// run with VoiceOver on this Mac is step 11 and is recorded `NOT_RUN`.
struct InstanceDetailAccessibilityTests {
    private static let folder = "apps/macos/Packages/StacklyUI/Sources/StacklyUI/InstanceDetail"

    private func code(_ name: String) throws -> String {
        let path = Self.folder + "/" + name
        return SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
        ).codeLines.joined(separator: "\n")
    }

    @Test func processAndReadinessAreTwoRowsOfTheirOwn() throws {
        let card = try code("EndpointsCard.swift")
        #expect(card.contains(#"LabeledContent("Process")"#))
        #expect(card.contains(#"LabeledContent("Readiness")"#))
        #expect(card.contains("ProcessText.text(for: summary)"))
        #expect(card.contains("ReadinessText.text(for: summary)"))
        #expect(card.contains("AccessibilityID.InstanceDetail.process"))
        #expect(card.contains("AccessibilityID.InstanceDetail.readiness"))
    }

    @Test func everyEndpointCopyButtonSaysWhatItCopiesAndAnnouncesIt() throws {
        let card = try code("EndpointsCard.swift")
        #expect(card.contains(#"what: "\(endpoint.name) address""#))
        #expect(card.contains(#"announcement: "Address copied.""#))
        #expect(card.contains("style: .icon"))
    }

    @Test func recalculateAndCopyIDAreLabelled() throws {
        let card = try code("DetailsCard.swift")
        #expect(card.contains(#".accessibilityLabel("Recalculate data size")"#))
        #expect(card.contains(#"what: "instance ID""#))
        #expect(card.contains(#"announcement: "ID copied.""#))
    }

    @Test func eachConnectionEntryIsAGroupNamedByItsTitleAndAStringWithoutASecretAnnouncesItsCopy()
        throws
    {
        let card = try code("ConnectionCard.swift")
        #expect(card.contains(".accessibilityLabel(connection.title)"))
        #expect(card.contains(#"announcement: "\(connection.title) copied.""#))
        for label in ["Show Password", "Copy Password"] {
            #expect(card.contains("\"\(label)\""), "missing the label \(label)")
        }
    }

    @Test func aStringWithASecretNeverHoldsItsValueInTheCard() throws {
        let card = try code("ConnectionCard.swift")
        #expect(card.contains("RevealableSecretText("))
        #expect(card.contains("ConnectionComposer.composer(for: connection.parts)"))
        #expect(card.contains("PlainPasteboard") == false)
        #expect(card.contains(".expose") == false)
    }

    @Test func theMaskedStringIsSpokenWithPasswordHidden() {
        #expect(
            DetailFormat.spoken(masked: "postgresql://postgres:****@127.0.0.1:5432/postgres")
                == "postgresql://postgres:password hidden@127.0.0.1:5432/postgres")
    }

    @Test func theDetailPageNeverReadsOrWritesAPasteboardItself() throws {
        for file in [
            "InstanceDetailPage.swift", "SummaryTab.swift", "DetailsCard.swift",
            "EndpointsCard.swift", "InstanceDetailModel.swift",
        ] {
            let source = try code(file)
            #expect(source.contains("NSPasteboard") == false, "\(file)")
            #expect(source.contains("revealSecret") == false, "\(file)")
        }
    }

    @Test func theTabPickerShowsOnlyWithMoreThanOneTab() throws {
        let page = try code("InstanceDetailPage.swift")
        #expect(page.contains("tabs.count > 1"))
        #expect(InstanceTab.allCases == [.summary])
    }
}
