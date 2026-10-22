import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The shell keeps its split view inside the window with every window banner (spec 0013
/// AC-14, AC-21).
///
/// A banner's text wraps at the width it is offered, and the split view once measured the
/// banner at a tiny width: the text wrapped one character per line and the split view
/// grew to more than 1500 pt, out of a 600 pt window, so the sidebar and the page slid
/// out of view whenever stacklyd was unreachable.
struct ShellLayoutTests {
    nonisolated static let now = Date(timeIntervalSince1970: 1_792_300_000)

    /// The banners with the longest copy: the offline banner with its time, a critical
    /// banner with a reason, and the longest misconfigured message.
    nonisolated static let statuses: [ShellStatus] = [
        .unreachable(lastKnownAt: now.addingTimeInterval(-300), since: now),
        .untrusted(.endpoint(reason: "parent_group_or_other_access"), since: now),
        .misconfigured(.rootTooLong, since: now),
    ]

    @Test(arguments: statuses)
    func theSplitViewFillsTheSmallestWindowWithABanner(status: ShellStatus) async throws {
        let presentation = ShellPresentation(status: status, lastErrorCode: "ipc.endpoint_missing")
        #expect(presentation.banner != nil)
        let frames = try await Self.splitViewFrames(of: presentation, in: AppShell.minimumSize)
        #expect(frames == [CGRect(origin: .zero, size: AppShell.minimumSize)])
    }

    @Test func theSplitViewFillsTheDefaultWindowWithTheOfflineBanner() async throws {
        let presentation = ShellPresentation(status: Self.statuses[0])
        let size = CGSize(width: 1280, height: 800)
        let frames = try await Self.splitViewFrames(of: presentation, in: size)
        #expect(frames == [CGRect(origin: .zero, size: size)])
    }

    /// The frames of the split views when the shell fills a window of `size`.
    static func splitViewFrames(
        of presentation: ShellPresentation,
        in size: CGSize
    ) async throws -> [CGRect] {
        let shell = AppShell(
            presentation: presentation,
            diagnostics: ConnectionDiagnostics(status: presentation.status, appProtocol: "1.3"),
            reconnect: {},
            refreshDiagnostics: {}
        )
        let controller = NSHostingController(rootView: shell)
        controller.sizingOptions = [.minSize]
        let window = NSWindow(contentViewController: controller)
        defer { window.close() }
        window.setContentSize(size)
        let content = try #require(window.contentView)
        // SwiftUI settles the split view over a few passes.
        for _ in 0..<10 {
            content.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(20))
        }
        return splitViews(in: content).map(\.frame)
    }

    private static func splitViews(in view: NSView) -> [NSSplitView] {
        var found: [NSSplitView] = []
        if let split = view as? NSSplitView { found.append(split) }
        for child in view.subviews { found += splitViews(in: child) }
        return found
    }
}
