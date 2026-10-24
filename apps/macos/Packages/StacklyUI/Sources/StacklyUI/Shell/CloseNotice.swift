import AppKit
import SwiftUI

/// The notice that tells the user Stackly keeps running after the main window closes.
/// It shows once per launch, on the first close of the main window that is not part of quitting: an opaque card in a borderless panel at the bottom center of the screen the window was on. SwiftUI has no presentation that outlives its window, so the panel is AppKit.
public final class CloseNotice {
    /// The title.
    static let title = "Stackly is still running"
    /// The message, which points to the Dock.
    // provisional: F32 (the menu bar copy of the mockup replaces the Dock sentence)
    static let message =
        "Closing the window doesn’t stop stacklyd or your instances. Click Stackly in the Dock to reopen it."
    /// How long the notice stays.
    static let displayDuration: Duration = .seconds(4)
    /// The gap between the notice and the bottom of the screen's visible area.
    static let bottomOffset: CGFloat = 80

    /// Whether the notice was shown in this launch.
    private(set) var hasShown = false

    private let presentsPanel: Bool
    private var panel: NSPanel?
    private var dismissTask: Task<Void, Never>?

    /// Creates the notice. Keep one for the whole launch.
    public convenience init() {
        self.init(presentsPanel: true)
    }

    /// Creates the notice. With `presentsPanel` false, it only decides, for tests.
    init(presentsPanel: Bool) {
        self.presentsPanel = presentsPanel
    }

    /// Call it when the main window is about to close. It shows the notice the first time
    /// in this launch, unless the app is quitting.
    /// - Parameters:
    ///   - isTerminating: Whether the window closes because the app quits.
    ///   - screen: The screen the window was on, or `nil` for the main screen.
    /// - Returns: Whether the notice shows.
    @discardableResult
    public func mainWindowWillClose(isTerminating: Bool, screen: NSScreen?) -> Bool {
        guard isTerminating == false, hasShown == false else { return false }
        hasShown = true
        if presentsPanel {
            show(on: screen ?? NSScreen.main)
        }
        return true
    }

    private func show(on screen: NSScreen?) {
        let hosting = FirstMouseHostingView(
            rootView: CloseNoticeCard { [weak self] in self?.dismiss() }
        )
        let size = hosting.fittingSize
        let panel = NSPanel(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.becomesKeyOnlyIfNeeded = true
        panel.collectionBehavior = [.canJoinAllSpaces, .transient, .ignoresCycle]
        panel.contentView = hosting
        if let visible = screen?.visibleFrame {
            panel.setFrameOrigin(
                NSPoint(x: visible.midX - size.width / 2, y: visible.minY + Self.bottomOffset)
            )
        }
        panel.alphaValue = 0
        panel.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.2
            panel.animator().alphaValue = 1
        }
        self.panel = panel
        dismissTask = Task { [weak self] in
            try? await Task.sleep(for: Self.displayDuration)
            guard Task.isCancelled == false else { return }
            self?.dismiss()
        }
    }

    private func dismiss() {
        dismissTask?.cancel()
        dismissTask = nil
        guard let panel else { return }
        self.panel = nil
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.2
            panel.animator().alphaValue = 0
        } completionHandler: {
            MainActor.assumeIsolated {
                panel.orderOut(nil)
            }
        }
    }
}

/// The card inside the notice panel: the title and the message, centered, on an opaque
/// card. A click anywhere dismisses it.
struct CloseNoticeCard: View {
    /// The width of the text, so the card is 360 pt wide with its insets (mockup).
    static let textWidth: CGFloat = 328

    let dismiss: () -> Void

    var body: some View {
        VStack(spacing: Spacing.s2) {
            Text(CloseNotice.title)
                .font(.body.weight(.semibold))
            Text(CloseNotice.message)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .multilineTextAlignment(.center)
        .frame(width: Self.textWidth)
        .padding(Spacing.cardInsets)
        .cardSurface(radius: .large)
        .contentShape(Rectangle())
        .onTapGesture(perform: dismiss)

    }
}

/// A hosting view that takes the first click, so a click on the notice dismisses it
/// without activating the app first.
private final class FirstMouseHostingView<Content: View>: NSHostingView<Content> {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        true
    }
}
