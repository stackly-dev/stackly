import AppKit
import SwiftUI

/// A sheet with a header, a scrolling body, and a fixed footer (spec 0067, AC-12).
///
/// The sheet is opaque (`surfaceWindow`). The header has an optional ``ProviderTile``, the
/// title, and a subtitle. The body scrolls only when it is taller than the visible screen
/// minus 160 pt. The footer holds the reason the primary action cannot run on the left,
/// Cancel, and the primary action.
///
/// - Return runs the primary action, except when it is destructive (then Return does
///   nothing) or when it is disabled or busy.
/// - Escape runs Cancel. A sheet closes itself on Escape without running Cancel unless
///   `.interactiveDismissDisabled()` is set, so the sheet sets it (design.md, Sheet).
/// - The sheet sets its own ``SwiftUI/View/actionHost()``, so a confirmation from inside
///   it shows on top of this sheet.
/// - A view inside the sheet never sends to the window's `FeedbackCenter`; it shows its
///   result with a `StateBanner`, and the screen that owns the sheet sends feedback after
///   the sheet closes.
public struct StacklySheet<Content: View>: View {
    private let title: String
    private let subtitle: String?
    private let tile: TileSpec?
    private let primary: StacklyAction
    private let cancel: StacklyAction?
    private let minWidth: CGFloat
    private let content: Content

    /// Creates a sheet.
    ///
    /// - Parameters:
    ///   - title: The title.
    ///   - subtitle: A line under the title, or `nil`.
    ///   - tile: The provider tile of the header, or `nil`.
    ///   - primary: The main action of the footer.
    ///   - cancel: The Cancel action, or `nil` for a "Cancel" that closes the sheet. Give
    ///     one when Cancel means something else, such as cancelling an operation.
    ///   - minWidth: The minimum width of the sheet.
    ///   - content: The body.
    public init(
        title: String,
        subtitle: String? = nil,
        tile: TileSpec? = nil,
        primary: StacklyAction,
        cancel: StacklyAction? = nil,
        minWidth: CGFloat = 480,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.subtitle = subtitle
        self.tile = tile
        self.primary = primary
        self.cancel = cancel
        self.minWidth = minWidth
        self.content = content()
    }

    /// The sheet's content.
    public var body: some View {
        SheetLayout(
            title: title, subtitle: subtitle, tile: tile, primary: primary, cancel: cancel,
            content: content
        )
        .frame(minWidth: minWidth)
        .background(StacklyColor.surfaceWindow)
        .presentationBackground(StacklyColor.surfaceWindow)
        .interactiveDismissDisabled()
        .actionHost()
    }
}

/// The measurements of a sheet.
nonisolated enum SheetMetrics {
    /// What the body leaves out of the visible screen height: the header, the footer, and
    /// the room around the sheet.
    static let reservedHeight: CGFloat = 160

    /// The height of the body: its content height, never more than the visible screen
    /// height minus ``reservedHeight``, so a short sheet does not scroll and a long one
    /// scrolls without cutting text.
    ///
    /// - Parameters:
    ///   - content: The height of the content.
    ///   - visibleScreen: The height of the visible screen area.
    static func bodyHeight(content: CGFloat, visibleScreen: CGFloat) -> CGFloat {
        min(content, max(0, visibleScreen - reservedHeight))
    }
}

/// The reason the footer shows: the primary action's own reason, or the gate's reason
/// when the action mutates and the daemon cannot be reached (the sheet covers the window
/// banner, so the footer writes it).
nonisolated enum SheetFooterReason {
    /// The reason for `availability`, or `nil` when the action can run or is busy.
    static func text(for availability: ActionAvailability) -> String? {
        if case .disabled(let reason, _) = availability { reason } else { nil }
    }
}

/// The header, body, and footer of a ``StacklySheet``. It sits below `.actionHost()`, so
/// it reads the sheet's own host.
private struct SheetLayout<Content: View>: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.actionHost) private var host
    @Environment(\.mutationAvailability) private var gate
    @State private var contentHeight: CGFloat = 0

    let title: String
    let subtitle: String?
    let tile: TileSpec?
    let primary: StacklyAction
    let cancel: StacklyAction?
    let content: Content

    var body: some View {
        VStack(spacing: 0) {
            header
            ScrollView {
                VStack(spacing: Spacing.s16) {
                    content
                }
                .frame(maxWidth: .infinity)
                .onGeometryChange(for: CGFloat.self) {
                    $0.size.height
                } action: {
                    contentHeight = $0
                }
            }
            .frame(height: bodyHeight)
            footer
        }
    }

    private var bodyHeight: CGFloat {
        let screen = NSApp.keyWindow?.screen ?? NSScreen.main
        return SheetMetrics.bodyHeight(
            content: contentHeight + Spacing.s12 + Spacing.s16,
            visibleScreen: screen?.visibleFrame.height ?? 800
        )
    }

    private var header: some View {
        HStack(spacing: Spacing.s12) {
            if let tile {
                ProviderTile(tile, size: ProviderTile.mediumSize)
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text(title)
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(.primary)
                    .accessibilityAddTraits(.isHeader)
                if let subtitle {
                    Text(subtitle)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.top, Spacing.s22 - Spacing.s2)
        .padding(.horizontal, Spacing.s22)
        .padding(.bottom, Spacing.s6)
        .accessibilityElement(children: .contain)
    }

    private var footer: some View {
        let cancelAction =
            cancel
            ?? StacklyAction(
                id: "cancel", title: "Cancel", mutates: false, perform: { dismiss() })
        let availability = primary.availability(gate: gate)
        return HStack(spacing: Spacing.s12) {
            if let reason = SheetFooterReason.text(for: availability) {
                DisabledReasonText(reason)
            }
            Spacer(minLength: Spacing.s12)
            SheetFooterButton(
                action: cancelAction, availability: cancelAction.availability(gate: gate),
                role: .cancel, host: host, gate: gate)
            SheetFooterButton(
                action: primary, availability: availability, role: .primary, host: host,
                gate: gate)
        }
        .padding(.top, Spacing.s12)
        .padding(.horizontal, Spacing.s22)
        .padding(.bottom, Spacing.s16 + Spacing.s2)
    }
}

/// The two buttons of the footer. Only these two carry a keyboard shortcut: Escape for
/// Cancel and Return for a primary action that is not destructive.
private struct SheetFooterButton: View {
    enum Role {
        case cancel
        case primary
    }

    let action: StacklyAction
    let availability: ActionAvailability
    let role: Role
    let host: ActionHost?
    let gate: MutationAvailability

    var body: some View {
        HStack(spacing: Spacing.s8) {
            if availability == .busy {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityHidden(true)
            }
            button
        }
    }

    @ViewBuilder private var button: some View {
        switch role {
        case .cancel:
            plainButton
                .buttonStyle(.bordered)
                .keyboardShortcut(.cancelAction)
                .disabled(availability != .enabled)
                .accessibilityIdentifier(AccessibilityID.Sheet.cancel)
        case .primary:
            if action.role.isDestructive {
                plainButton
                    .buttonStyle(.bordered)
                    .disabled(availability != .enabled)
                    .disabled(reason: disabledHelp)
                    .modifier(BusyValue(isBusy: availability == .busy))
                    .accessibilityIdentifier(AccessibilityID.Sheet.primary)
            } else {
                plainButton
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(availability != .enabled)
                    .disabled(reason: disabledHelp)
                    .modifier(BusyValue(isBusy: availability == .busy))
                    .accessibilityIdentifier(AccessibilityID.Sheet.primary)
            }
        }
    }

    private var plainButton: some View {
        Button(role: action.role.isDestructive ? .destructive : nil) {
            action.run(host: host, gate: gate)
        } label: {
            if let symbol = action.symbol {
                Label(action.title, systemImage: symbol)
            } else {
                Text(action.title)
            }
        }
    }

    /// The reason that becomes the help of a disabled primary action.
    private var disabledHelp: String? {
        SheetFooterReason.text(for: availability)
    }
}

private struct BusyValue: ViewModifier {
    let isBusy: Bool

    @ViewBuilder func body(content: Content) -> some View {
        if isBusy {
            content.accessibilityValue("In progress")
        } else {
            content
        }
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { SheetGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { SheetGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { SheetGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
