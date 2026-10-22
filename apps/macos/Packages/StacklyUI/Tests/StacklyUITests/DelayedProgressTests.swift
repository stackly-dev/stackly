import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The first frame of the delayed spinner under `loadingDelay` (spec 0012, State patterns;
/// spec 0068, `loadingDelay`).
@Suite(.timeLimit(.minutes(1)))
struct DelayedProgressTests {
    /// The first frame of a spinner under `loadingDelay`, captured before any task ran.
    @MainActor
    private func firstFrame(
        loadingDelay: Duration?, explicit: Duration? = nil
    ) throws -> SnapshotImage {
        let spinner: AnyView =
            explicit.map { AnyView(DelayedProgress(delay: $0)) } ?? AnyView(DelayedProgress())
        let view =
            spinner
            .environment(\.loadingDelay, loadingDelay)
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
    func aZeroLoadingDelayDrawsTheSpinnerInTheFirstFrame() throws {
        let immediate = try firstFrame(loadingDelay: .zero)
        let defaulted = try firstFrame(loadingDelay: nil)
        let slow = try firstFrame(loadingDelay: .seconds(3600))
        #expect(immediate.difference(from: defaulted).pixelCount > 0, "the spinner is drawn")
        #expect(
            defaulted.difference(from: slow).pixelCount == 0,
            "no delay but zero shows the spinner in the first frame")
    }

    @Test @MainActor
    func anExplicitDelayWinsOverTheEnvironment() throws {
        let hidden = try firstFrame(loadingDelay: .zero, explicit: .seconds(3600))
        let slow = try firstFrame(loadingDelay: .seconds(3600))
        #expect(hidden.difference(from: slow).pixelCount == 0)
        let shown = try firstFrame(loadingDelay: .seconds(3600), explicit: .zero)
        #expect(shown.difference(from: slow).pixelCount > 0)
    }
}
