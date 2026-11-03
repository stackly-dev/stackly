import AppKit
import SwiftUI

/// The three template images of the menu bar item (spec 0032, AC-1). They are black shapes on
/// clear, so macOS draws them for light and dark menu bars and for a selected item. The
/// state never rests on color: the failed variant adds a dot to the shape and the offline
/// variant a slash.
@MainActor
public enum MenuBarIcon {
    /// The side of the image in points; the menu bar scales it.
    static let side: CGFloat = 18

    /// The template image of `variant`.
    public static func image(_ variant: MenuBarIconVariant) -> NSImage {
        let image = NSImage(size: NSSize(width: side, height: side), flipped: false) { _ in
            draw(variant)
            return true
        }
        image.isTemplate = true
        return image
    }

    /// Three stacked bars, the shape of the app icon.
    private static func drawBars() {
        NSColor.black.setFill()
        for index in 0..<3 {
            let bar = NSBezierPath(
                roundedRect: NSRect(x: 2, y: 2.5 + CGFloat(index) * 5, width: 14, height: 3.4),
                xRadius: 1.7, yRadius: 1.7)
            bar.fill()
        }
    }

    private static func draw(_ variant: MenuBarIconVariant) {
        drawBars()
        switch variant {
        case .normal:
            break
        case .failed:
            // Clears a ring around the dot, then draws it at the top right corner.
            let center = NSPoint(x: 14.2, y: 14.2)
            NSGraphicsContext.current?.compositingOperation = .clear
            NSBezierPath(
                ovalIn: NSRect(x: center.x - 4.2, y: center.y - 4.2, width: 8.4, height: 8.4)
            ).fill()
            NSGraphicsContext.current?.compositingOperation = .sourceOver
            NSColor.black.setFill()
            NSBezierPath(
                ovalIn: NSRect(x: center.x - 2.8, y: center.y - 2.8, width: 5.6, height: 5.6)
            ).fill()
        case .offline:
            // Clears a channel next to the slash, then draws the slash corner to corner.
            let slash = NSBezierPath()
            slash.move(to: NSPoint(x: 2, y: 2))
            slash.line(to: NSPoint(x: 16, y: 16))
            slash.lineCapStyle = .round
            NSGraphicsContext.current?.compositingOperation = .clear
            slash.lineWidth = 4.6
            slash.stroke()
            NSGraphicsContext.current?.compositingOperation = .sourceOver
            NSColor.black.setStroke()
            slash.lineWidth = 1.6
            slash.stroke()
        }
    }
}

/// The label of the `MenuBarExtra`: the template image of the model's variant. SwiftUI keeps
/// the label alive for the whole launch, so it also hands the scene's `openWindow` action to
/// the ``WindowOpener``: the main window can open from the popover even when the app
/// started with no window.
public struct MenuBarLabel: View {
    private let model: MenuBarModel
    private let windowOpener: WindowOpener
    @Environment(\.openWindow) private var openWindow

    /// Creates the label.
    /// - Parameters:
    ///   - model: The menu bar model.
    ///   - windowOpener: The opener that keeps `openWindow`.
    public init(model: MenuBarModel, windowOpener: WindowOpener) {
        self.model = model
        self.windowOpener = windowOpener
    }

    /// The icon.
    public var body: some View {
        Image(nsImage: MenuBarIcon.image(model.iconVariant))
            .onAppear { windowOpener.capture(openWindow) }
    }
}
