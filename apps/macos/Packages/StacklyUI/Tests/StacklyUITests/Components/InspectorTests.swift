import AppKit
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// The inspector components (spec 0067, AC-18, AC-19): the column measures, the toggle's
/// label, help, and identifier, and that header and placeholder wrap instead of cutting
/// text at the narrowest column.
struct InspectorTests {
    /// The height a view takes at `width`.
    private func height(of view: some View, width: CGFloat) -> CGFloat {
        let host = NSHostingView(rootView: view.frame(width: width))
        host.frame = CGRect(x: 0, y: 0, width: width, height: 10)
        host.layoutSubtreeIfNeeded()
        return host.fittingSize.height
    }

    /// The inner width of the narrowest inspector column.
    private var narrowContentWidth: CGFloat {
        InspectorMetrics.minWidth - 2 * InspectorMetrics.padding
    }

    // MARK: - Column

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Spec 0067 AC-18: the column widths are the spec measures"))
    func columnWidthsAreTheSpecMeasures() {
        #expect(InspectorMetrics.minWidth == 240)
        #expect(InspectorMetrics.idealWidth == 280)
        #expect(InspectorMetrics.maxWidth == 360)
        #expect(InspectorMetrics.padding == 16)
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Spec 0067 AC-18: the maximum width is 280 below 1000 pt and 360 from there"))
    func maxWidthIs280BelowAThousandPointsAndAfterwards360() {
        #expect(StacklyInspectorWidth.compactWindowWidth == 1000)
        #expect(StacklyInspectorWidth.maxWidth(windowWidth: 900) == 280)
        #expect(StacklyInspectorWidth.maxWidth(windowWidth: 999) == 280)
        #expect(StacklyInspectorWidth.maxWidth(windowWidth: 1000) == 360)
        #expect(StacklyInspectorWidth.maxWidth(windowWidth: 1280) == 360)
    }

    @Test func maxWidthIs360WhenTheWindowIsNotKnown() {
        #expect(StacklyInspectorWidth.maxWidth(windowWidth: nil) == 360)
        #expect(EnvironmentValues().stacklyWindowWidth == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-18: the page column at the smallest window is never below 384 pt"))
    func pageColumnAtTheSmallestWindowIsNeverBelow384() {
        let page =
            AppShell.minimumSize.width - AppShell.sidebarWidth.ideal
            - StacklyInspectorWidth.maxWidth(windowWidth: AppShell.minimumSize.width)
        #expect(page == 384)
    }

    /// Records the background material where it stands, so a body that never ran cannot
    /// pass for no material.
    private struct MaterialSpy: View {
        final class Seen {
            var evaluated = false
            var material: Material?
        }
        let seen: Seen
        @Environment(\.backgroundMaterial) private var material

        var body: some View {
            seen.evaluated = true
            seen.material = material
            return Color.clear
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-18: the inspector content draws on the opaque column without the glass material of the system's column"
        ))
    func columnContentDrawsWithoutTheGlassMaterial() async throws {
        for scheme in [ColorScheme.light, .dark] {
            let seen = MaterialSpy.Seen()
            _ = try await SnapshotRender.render(
                Color.clear.stacklyInspector(isPresented: .constant(true)) {
                    MaterialSpy(seen: seen)
                },
                width: 900,
                height: .fixed(300),
                scheme: scheme
            )
            #expect(seen.evaluated, "the column content was never evaluated in \(scheme)")
            #expect(
                seen.material == nil,
                "the column content reads a background material in \(scheme), so its buttons, symbols, and text draw vibrant"
            )
        }
    }

    /// A page with a note that is thousands of points tall at a width of 0, which is where
    /// the split view of `.inspector` measures a page.
    private struct WrappingPage: View {
        final class Seen {
            var size: CGSize?
        }
        let seen: Seen

        var body: some View {
            VStack(alignment: .leading) {
                Text(String(repeating: "A note that wraps at the width of the page. ", count: 8))
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
            }
            .onGeometryChange(for: CGSize.self) {
                $0.size
            } action: {
                seen.size = $0
            }
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-18: the page under the inspector takes the size the column offers, open or closed, also with text that wraps"
        ))
    func pageTakesTheSizeTheColumnOffers() async throws {
        for isOpen in [false, true] {
            let state = isOpen ? "open" : "closed"
            let seen = WrappingPage.Seen()
            _ = try await SnapshotRender.render(
                WrappingPage(seen: seen).stacklyInspector(isPresented: .constant(isOpen)) {
                    Color.clear
                },
                width: 900,
                height: .fixed(500),
                scheme: .light
            )
            let size = try #require(seen.size, "the page was never laid out (\(state))")
            #expect(
                size.height == 500,
                "the page is \(size.height) pt tall in a 500 pt window with the inspector \(state)")
            #expect(size.width <= 900, "the page is \(size.width) pt wide (\(state))")
        }
    }

    // MARK: - Toggle

    @Test(
        .evidence(
            covers: ["NFR-012"], expected: "Spec 0067 AC-18: the toggle label follows the binding"))
    func toggleLabelFollowsTheBinding() {
        #expect(InspectorToggle.label(isPresented: true) == "Hide Inspector")
        #expect(InspectorToggle.label(isPresented: false) == "Show Inspector")
    }

    @Test func toggleHelpDefaultsToItsLabelAndTakesTheScreensText() {
        #expect(InspectorToggle.helpText(isPresented: true, help: nil) == "Hide Inspector")
        #expect(InspectorToggle.helpText(isPresented: false, help: nil) == "Show Inspector")
        #expect(
            InspectorToggle.helpText(isPresented: false, help: "Show Inspector (⌥⌘I)")
                == "Show Inspector (⌥⌘I)")
    }

    @Test func toggleUsesTheTrailingSidebarSymbol() {
        #expect(InspectorToggle.symbol == "sidebar.trailing")

    }

    // MARK: - Header

    @Test func longTitleAndSubtitleWrapAtTheNarrowestColumn() {
        let short = InspectorHeader(title: "pg17-dev", subtitle: "PostgreSQL 17.4")
        let long = InspectorHeader(
            title: "pg17-analytics-replica-eu-central-staging",
            subtitle: "PostgreSQL 17.4 with the replication extension")
        let shortHeight = height(of: short, width: narrowContentWidth)
        let longHeight = height(of: long, width: narrowContentWidth)
        #expect(shortHeight > 0)
        #expect(longHeight > shortHeight, "long texts take more lines, they are not cut")
    }

    @Test func tileAddsToTheHeaderHeight() {
        let plain = InspectorHeader(title: "pg17-dev")
        let tiled = InspectorHeader(
            title: "pg17-dev", tile: TileSpec(mark: "Pg", tile: .blue, name: "PostgreSQL"))
        #expect(
            height(of: tiled, width: narrowContentWidth)
                >= height(of: plain, width: narrowContentWidth) + ProviderTile.mediumSize)
    }

    @Test func actionsStackWhenTheyDoNotFitSideBySide() {
        func header(width: CGFloat) -> CGFloat {
            height(
                of: InspectorHeader(title: "pg17-dev") {
                    Button("Show Operations Log") {}
                    Button("Remove Instance…") {}
                },
                width: width)
        }
        #expect(
            header(width: narrowContentWidth) > header(width: 480),
            "two wide buttons stack at the narrowest column and sit in a row when there is room")
    }

    @Test func headerWithoutActionsIsShorterThanWithActions() {
        let without = InspectorHeader(title: "pg17-dev")
        let with = InspectorHeader(title: "pg17-dev") { Button("Stop") {} }
        #expect(
            height(of: with, width: narrowContentWidth)
                > height(of: without, width: narrowContentWidth))
    }

    // MARK: - Placeholder

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Spec 0067 AC-18: the placeholder defaults to No Selection"))
    func placeholderDefaultsToNoSelection() {
        let placeholder = InspectorPlaceholder()
        #expect(placeholder.title == "No Selection")
        #expect(placeholder.message == nil)
    }

    @Test func placeholderMessageWrapsAtTheNarrowestColumn() {
        let message = "Select an instance in the list to see its details, status, and endpoint."
        let without = InspectorPlaceholder()
        let with = InspectorPlaceholder(message: message)
        let withoutHeight = height(of: without, width: narrowContentWidth)
        let withHeight = height(of: with, width: narrowContentWidth)
        #expect(withHeight > withoutHeight)
        #expect(withHeight > 0)
    }
}
