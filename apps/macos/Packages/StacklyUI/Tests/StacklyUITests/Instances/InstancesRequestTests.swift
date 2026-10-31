import AppKit
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Instances page applies what the navigator asks also when SwiftUI builds the page a
/// second time right after the first one applied it, as macOS 27 does on the first visit of
/// the page in a window (spec 0022 AC-2, verify step 102).
@MainActor
struct InstancesRequestTests {
    /// The page alone, with an identity the test changes to build it again.
    private struct Host: View {
        let rig: PageRig
        let navigator: ShellNavigator
        let generation: Int

        var body: some View {
            rig.instancesPage(showsInspector: false)
                .id(generation)
                .environment(\.shellNavigator, navigator)
                .environment(\.fixtureNow, FixtureClock.now)
        }
    }

    private static func settle(_ view: NSView) async throws {
        for _ in 0..<15 {
            view.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(30))
        }
    }

    private static func tables(in view: NSView) -> [NSTableView] {
        var found: [NSTableView] = []
        if let table = view as? NSTableView { found.append(table) }
        for child in view.subviews { found += tables(in: child) }
        return found
    }

    /// The page in a window, asked for `destination` before it shows; `check` reads the
    /// window content once the page settled, then again after it was built a second time.
    private func showPage(
        _ destination: ShellDestination, check: (NSView) -> Void
    ) async throws {
        let rig = PageRig()
        await rig.load()
        let navigator = ShellNavigator()
        navigator.show(destination)
        let controller = NSHostingController(
            rootView: Host(rig: rig, navigator: navigator, generation: 0))
        let window = NSWindow(contentViewController: controller)
        defer { window.close() }
        window.setContentSize(CGSize(width: 1044, height: 748))
        let content = try #require(window.contentView)
        try await Self.settle(content)
        check(content)

        controller.rootView = Host(rig: rig, navigator: navigator, generation: 1)
        try await Self.settle(content)
        check(content)
    }

    @Test func aPageBuiltAgainKeepsTheFilterOfTheCard() async throws {
        // Failed holds analytics and queue.
        try await showPage(.instances(filter: .failed)) { content in
            #expect(Self.tables(in: content).map(\.numberOfRows) == [2])
        }
    }

    @Test func aPageBuiltAgainStillShowsTheDetailPageOfTheOpenedInstance() async throws {
        // The detail page is pushed over the table, which leaves the window.
        try await showPage(.instance(FixtureScenario.instanceCases[7].id)) { content in
            #expect(Self.tables(in: content).isEmpty)
        }
    }
}
