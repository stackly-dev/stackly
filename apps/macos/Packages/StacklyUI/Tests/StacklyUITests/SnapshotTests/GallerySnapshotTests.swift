import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The pages of the design gallery in every variant (spec 0068, AC-8): each page at its
/// full height, 1044 pt wide and 640 pt narrow, without the outer scroll view.
///
/// The States and Components pages are taller than a snapshot may be (4000 pt), so they
/// are rendered one group of sections at a time.
extension SnapshotTests {
    struct GallerySnapshotTests {
        private static let size = SnapshotSize.fitting(width: 1044, narrowWidth: 640)

        private func check<Page: View>(_ page: Page, named name: String) async throws {
            try await assertSnapshot(
                page.environment(\.galleryScrollsPage, false), named: name, size: Self.size)
        }

        @Test(
            .evidence(
                covers: ["REQ-027", "NFR-012"],
                expected: "The Tokens gallery page renders in every variant"))
        func tokensPage() async throws {
            try await check(TokensGalleryPage(), named: "tokens")
        }

        @Test(
            .evidence(
                covers: ["REQ-027", "NFR-012"],
                expected: "The Status gallery page renders in every variant"))
        func statusPage() async throws {
            try await check(StatusGalleryPage(), named: "status")
        }

        @Test(
            .evidence(
                covers: ["REQ-024", "REQ-027"],
                expected: "Each group of the States gallery page renders in every variant"),
            arguments: [
                (StatesGalleryPage.Part.shapes, "states-shapes"),
                (.live, "states-live"),
                (.placeholders, "states-placeholders"),
                (.banners, "states-banners"),
            ])
        func statesPage(part: StatesGalleryPage.Part, name: String) async throws {
            try await check(StatesGalleryPage(parts: [part]), named: name)
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected: "The Confirmation gallery page renders in every variant"))
        func confirmationPage() async throws {
            try await check(ConfirmationGalleryPage(), named: "confirmation")
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected: "The Secrets gallery page renders on the fixture secrets client"))
        func secretsPage() async throws {
            let client = SecretsClient.fixture(FixtureScenario(.connectedEmpty))
            try await check(SecretsGalleryPage(client: client), named: "secrets")
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected: "The tile, key value, copy, console, action, and feedback sections render"
            ))
        func componentsBasePage() async throws {
            try await check(
                GalleryPageLayout { BaseComponentsSections() }, named: "components-base")
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected: "The sheet, checklist, and staged progress sections render"))
        func componentsSheetPage() async throws {
            try await check(
                GalleryPageLayout { SheetComponentsSections() }, named: "components-sheet")
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected: "The inspector sections render"))
        func componentsInspectorPage() async throws {
            try await check(
                GalleryPageLayout { InspectorComponentsSections() },
                named: "components-inspector")
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected: "The log and diff sections render"))
        func componentsLogPage() async throws {
            try await check(
                GalleryPageLayout { LogComponentsSections() }, named: "components-log")
        }
    }
}
