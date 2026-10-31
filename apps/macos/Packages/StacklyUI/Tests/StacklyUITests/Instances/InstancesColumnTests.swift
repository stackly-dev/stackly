import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The table of the Instances page fits the narrowest page column the spec promises without
/// scrolling: 384 pt next to an open inspector in a 900 pt window (spec 0022 AC-13, *Halaman
/// Instances*: minimum widths Name 120, Status 110, and the action 72 pt).
@MainActor
struct InstancesColumnTests {
    /// The table the page draws in a window of `width`.
    private static func table(at width: CGFloat) async throws -> (NSTableView, NSWindow) {
        let rig = PageRig()
        await rig.load()
        let controller = NSHostingController(
            rootView: rig.instancesPage(showsInspector: false)
                .environment(\.fixtureNow, FixtureClock.now))
        let window = NSWindow(contentViewController: controller)
        window.setContentSize(CGSize(width: width, height: 600))
        let content = try #require(window.contentView)
        for _ in 0..<15 {
            content.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(30))
        }
        let table = try #require(tables(in: content).first)
        return (table, window)
    }

    private static func tables(in view: NSView) -> [NSTableView] {
        var found: [NSTableView] = []
        if let table = view as? NSTableView { found.append(table) }
        for child in view.subviews { found += tables(in: child) }
        return found
    }

    @Test func theNarrowestTableFitsThePageColumnNextToTheInspector() async throws {
        let (table, window) = try await Self.table(at: 384)
        defer { window.close() }
        let columns = table.tableColumns
        #expect(columns.count == 3, "Name, Status, and the action")
        // The table adds its cell spacing and insets to the widths of its columns.
        let chrome = table.frame.width - columns.reduce(0) { $0 + $1.width }
        let narrowest = columns.reduce(0) { $0 + $1.minWidth } + chrome
        let clip = try #require(table.enclosingScrollView).contentView.bounds.width
        #expect(narrowest <= clip, "the table needs \(narrowest) pt in a \(clip) pt column")
    }
}
