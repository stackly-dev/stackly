import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// A loaded instance table stays below the window banner (spec 0013 AC-10).
struct ShellBannerLayoutTests {
    @Test(arguments: [CGFloat(664), 1044], [false, true])
    func theFirstRowStaysBelowTheWindowBanner(width: CGFloat, showsInspector: Bool) async throws {
        let rig = PageRig(instances: [FixtureScenario.instanceCases[0]])
        await rig.loadInstances()
        let marker = NSView()
        let layout = ShellPageLayout(
            page: rig.instancesPage(hasConnection: false, showsInspector: showsInspector),
            banner: BannerMarker(view: marker).frame(height: 120))
        let hosting = NSHostingView(rootView: layout)
        let size = CGSize(width: width, height: 600)
        let window = SnapshotWindow(size: size)
        window.contentView = hosting
        defer { window.close() }
        hosting.frame = CGRect(origin: .zero, size: size)
        for _ in 0..<10 {
            hosting.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(20))
        }
        let table = try #require(
            Self.descendants(of: hosting).compactMap { $0 as? NSTableView }.first)
        try #require(table.numberOfRows == 1)
        let header = try #require(table.headerView)
        let bannerFrame = marker.convert(marker.bounds, to: hosting)
        let headerFrame = header.convert(header.bounds, to: hosting)
        let rowFrame = table.convert(table.rect(ofRow: 0), to: hosting)
        #expect(headerFrame.minY >= bannerFrame.maxY)
        #expect(rowFrame.minY >= headerFrame.maxY)
        #expect(rowFrame.maxY <= hosting.bounds.maxY)
    }

    private static func descendants(of view: NSView) -> [NSView] {
        [view] + view.subviews.flatMap { descendants(of: $0) }
    }

    /// Reports the banner's frame without adding identifiers to product views.
    private struct BannerMarker: NSViewRepresentable {
        let view: NSView

        func makeNSView(context: Context) -> NSView { view }

        func updateNSView(_ nsView: NSView, context: Context) {}
    }
}
