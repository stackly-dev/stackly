import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// A feedback card in the detail column keeps the shell inside the window (spec 0067 AC-9,
/// AC-11).
struct ShellFeedbackLayoutTests {
    /// The window sizes: the mockup's window and `AppShell.minimumSize`, written out since
    /// test arguments are read outside the main actor.
    private nonisolated static let sizes = [
        CGSize(width: 1280, height: 800), CGSize(width: 900, height: 600),
    ]

    /// The shortest a card with its 16 pt bottom margin can be: the 24 pt Dismiss target,
    /// its padding, and the margin.
    private static let minimumCardSpace: CGFloat = 40

    /// Every sample card (the long ones and the one with an action included), with and
    /// without the window banner and the inspector, in a large and in the smallest window.
    @Test(arguments: sizes, [false, true])
    func aFeedbackCardKeepsTheSplitViewTheSizeOfTheWindow(
        size: CGSize, showsInspector: Bool
    ) async throws {
        let rig = PageRig(instances: [FixtureScenario.instanceCases[0]])
        await rig.loadInstances()
        for feedback in ComponentSamples.feedbacks {
            for showsBanner in [false, true] {
                try await expectSplitFillsWindow(
                    rig: rig, feedback: feedback, size: size, showsBanner: showsBanner,
                    showsInspector: showsInspector)
            }
        }
    }

    private func expectSplitFillsWindow(
        rig: PageRig, feedback: Feedback, size: CGSize, showsBanner: Bool, showsInspector: Bool
    ) async throws {
        let center = FeedbackCenter()
        center.show(feedback)
        let marker = NSView()
        let view = NavigationSplitView {
            List { Text("Overview") }
        } detail: {
            ShellPageLayout(
                page: rig.instancesPage(hasConnection: true, showsInspector: showsInspector)
                    .background(PageMarker(view: marker)),
                banner: Color.clear.frame(height: showsBanner ? 120 : 0)
            )
            .feedbackPresenter()
            .navigationSplitViewColumnWidth(
                min: AppShell.contentMinWidth, ideal: AppShell.contentIdealWidth)
        }
        .environment(\.feedbackCenter, center)
        let hosting = NSHostingView(rootView: view)
        let window = SnapshotWindow(size: size)
        window.contentView = hosting
        defer { window.close() }
        hosting.frame = CGRect(origin: .zero, size: size)
        let context =
            "“\(feedback.message)” in \(Int(size.width)) × \(Int(size.height)) (banner: \(showsBanner), inspector: \(showsInspector))"

        // Waits until the card takes its space below the page: the bottom inset lifts the
        // page's lower edge.
        var timer = ResponsiveTimer(maxCharge: .milliseconds(40))
        while Self.spaceBelowPage(marker: marker, in: hosting) < Self.minimumCardSpace {
            timer.lap()
            guard timer.counted < .seconds(3) else {
                Issue.record("the card never took space below the page with \(context)")
                return
            }
            hosting.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(20))
        }
        hosting.layoutSubtreeIfNeeded()

        let split = try #require(
            Self.descendants(of: hosting).compactMap { $0 as? NSSplitView }.first,
            "no split view with \(context)")
        #expect(
            split.frame.height == size.height,
            "the split view is \(split.frame.height) pt tall with \(context)")
        #expect(
            hosting.fittingSize.height <= size.height,
            "the content asks for \(hosting.fittingSize.height) pt with \(context)")
    }

    /// The distance from the page's lower edge to the window's lower edge.
    private static func spaceBelowPage(marker: NSView, in hosting: NSView) -> CGFloat {
        let page = marker.convert(marker.bounds, to: hosting)
        guard page.height > 0 else { return 0 }
        return hosting.isFlipped ? hosting.bounds.maxY - page.maxY : page.minY
    }

    private static func descendants(of view: NSView) -> [NSView] {
        [view] + view.subviews.flatMap { descendants(of: $0) }
    }

    /// Reports the page's frame without adding identifiers to product views.
    private struct PageMarker: NSViewRepresentable {
        let view: NSView

        func makeNSView(context: Context) -> NSView { view }

        func updateNSView(_ nsView: NSView, context: Context) {}
    }
}
