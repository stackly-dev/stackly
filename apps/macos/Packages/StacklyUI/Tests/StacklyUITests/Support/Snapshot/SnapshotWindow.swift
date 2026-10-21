import AppKit

/// The off screen window snapshots render in: borderless, never ordered in, never key,
/// at scale 1 in sRGB, so the same view gives the same pixels on any display.
final class SnapshotWindow: NSWindow {
    override var backingScaleFactor: CGFloat { 1 }
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    /// Creates a window whose content area is `size` points.
    init(size: CGSize) {
        super.init(
            contentRect: CGRect(origin: .zero, size: size),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        colorSpace = .sRGB
        isReleasedWhenClosed = false
    }
}
