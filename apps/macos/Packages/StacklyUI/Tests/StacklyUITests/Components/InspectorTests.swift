import AppKit
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

    @Test func columnWidthsAreTheSpecMeasures() {
        #expect(InspectorMetrics.minWidth == 240)
        #expect(InspectorMetrics.idealWidth == 280)
        #expect(InspectorMetrics.maxWidth == 360)
        #expect(InspectorMetrics.padding == 16)
    }

    @Test func maxWidthIs280BelowAThousandPointsAndAfterwards360() {
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

    @Test func pageColumnAtTheSmallestWindowIsNeverBelow384() {
        let page =
            AppShell.minimumSize.width - AppShell.sidebarWidth.ideal
            - StacklyInspectorWidth.maxWidth(windowWidth: AppShell.minimumSize.width)
        #expect(page == 384)
    }

    // MARK: - Toggle

    @Test func toggleLabelFollowsTheBinding() {
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
        #expect(
            NSImage(systemSymbolName: InspectorToggle.symbol, accessibilityDescription: nil) != nil)
    }

    @Test func inspectorIdentifiersAreTheAgreedOnes() {
        #expect(AccessibilityID.Inspector.toggle == "inspector.toggle")
        #expect(AccessibilityID.Inspector.placeholder == "inspector.placeholder")
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

    @Test func placeholderDefaultsToNoSelection() {
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
