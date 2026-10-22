import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The first frame of the delayed spinner under `loadingDelay` (spec 0012, State patterns;
/// spec 0068, `loadingDelay`).
@Suite(.timeLimit(.minutes(1)))
struct DelayedProgressTests {
    /// The first frame of a spinner that waits `delay`, captured before any task ran.
    @MainActor
    private func firstFrame(delay: Duration) throws -> SnapshotImage {
        let view = DelayedProgress(delay: delay)
            .frame(width: 80, height: 60)
            .background(Color.white)
        let host = NSHostingView(rootView: view)
        let window = SnapshotWindow(size: CGSize(width: 80, height: 60))
        window.appearance = NSAppearance(named: .aqua)
        window.contentView = host
        defer { window.close() }
        window.setContentSize(CGSize(width: 80, height: 60))
        host.frame = CGRect(x: 0, y: 0, width: 80, height: 60)
        host.layoutSubtreeIfNeeded()
        return try SnapshotImage.capture(host)
    }

    @Test @MainActor
    func aZeroDelayDrawsTheSpinnerInTheFirstFrame() throws {
        let immediate = try firstFrame(delay: .zero)
        let defaulted = try firstFrame(delay: DelayedVisibility.defaultDelay)
        let slow = try firstFrame(delay: .seconds(3600))
        #expect(immediate.difference(from: defaulted).pixelCount > 0, "the spinner is drawn")
        #expect(
            defaulted.difference(from: slow).pixelCount == 0,
            "no delay but zero shows the spinner in the first frame")
    }
}
