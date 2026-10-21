import AppKit
import SwiftUI

/// How tall a rendered view is.
enum SnapshotHeight: Equatable, Sendable {
    /// A fixed height in points.
    case fixed(CGFloat)
    /// The height the view asks for at the width, from `fittingSize`.
    case fitting
}

/// Renders a SwiftUI view in a ``SnapshotWindow`` and waits until it stops changing.
@MainActor
enum SnapshotRender {
    /// The view must be mounted this long before it can count as settled; longer than the
    /// 0.2 second crossfade of spec 0012.
    static let minimumSettle: Duration = .milliseconds(250)
    /// The time between two captures.
    static let interval: Duration = .milliseconds(50)
    /// The view must settle within this time, or the render fails without a retry.
    static let limit: Duration = .seconds(3)
    /// A fitting height of nothing or more than this fails the render.
    nonisolated static let maximumFittingHeight: CGFloat = 4000

    /// Mounts `view` at `width`, waits until two captures in a row are identical after
    /// ``minimumSettle``, and returns the last one.
    ///
    /// - Throws: ``SnapshotError/didNotSettle(last:)`` after ``limit``, and
    ///   ``SnapshotRenderError`` for a fitting height outside 1 to 4000 pt.
    static func render<V: View>(
        _ view: V,
        width: CGFloat,
        height: SnapshotHeight,
        scheme: ColorScheme
    ) async throws -> SnapshotImage {
        let hosting = NSHostingView(rootView: view)
        let window = SnapshotWindow(size: CGSize(width: width, height: 1))
        window.appearance = NSAppearance(named: scheme == .dark ? .darkAqua : .aqua)
        window.contentView = hosting
        defer { window.close() }

        var timer = ResponsiveTimer(maxCharge: interval + .milliseconds(50))
        var previous: SnapshotImage?
        while true {
            try size(window, hosting: hosting, width: width, height: height)
            hosting.layoutSubtreeIfNeeded()
            let image = try SnapshotImage.capture(hosting)
            timer.lap()
            let elapsed = timer.counted
            if elapsed >= minimumSettle, let previous, previous == image {
                return image
            }
            if elapsed >= limit {
                throw SnapshotError.didNotSettle(last: image)
            }
            previous = image
            try await Task.sleep(for: interval)
        }
    }

    private static func size(
        _ window: SnapshotWindow,
        hosting: NSHostingView<some View>,
        width: CGFloat,
        height: SnapshotHeight
    ) throws {
        let resolved: CGFloat
        switch height {
        case .fixed(let points):
            resolved = points
        case .fitting:
            hosting.frame = CGRect(x: 0, y: 0, width: width, height: 1)
            hosting.layoutSubtreeIfNeeded()
            resolved = hosting.fittingSize.height.rounded(.up)
            guard resolved >= 1, resolved <= maximumFittingHeight else {
                throw SnapshotRenderError.fittingHeight(resolved)
            }
        }
        window.setContentSize(CGSize(width: width, height: resolved))
        hosting.frame = CGRect(x: 0, y: 0, width: width, height: resolved)
    }
}

/// Why a render could not start.
enum SnapshotRenderError: Error, CustomStringConvertible {
    /// The fitting height was zero or taller than the limit.
    case fittingHeight(CGFloat)

    var description: String {
        switch self {
        case .fittingHeight(let points):
            "fitting height \(points) pt is outside 1 to \(Int(SnapshotRender.maximumFittingHeight)) pt"
        }
    }
}
