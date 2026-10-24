import SwiftUI

/// How strongly an ``ActionButton`` stands out.
public enum ActionProminence {
    /// `.borderedProminent`: the one main action of a place.
    case primary
    /// `.bordered`: every other action.
    case secondary
}

/// The native button of a ``StacklyAction`` (spec 0067, AC-7).
/// A primary action is `.borderedProminent` and a secondary one `.bordered`. A destructive
/// action is `role: .destructive` and `.bordered`, never prominent and never the default.
/// It runs through the ``ActionHost`` of the window or sheet, so a confirmation shows once.
/// - Disabled for a reason of the action's own: the button is disabled with the reason as   its help, and with `showsReason` the same reason shows as ``DisabledReasonText`` right   under it.
/// An unsupported capability is a disabled button with a reason, never a hidden one.
public struct ActionButton: View {
    @Environment(\.actionHost) private var host
    @Environment(\.mutationAvailability) private var gate

    private let action: StacklyAction
    private let prominence: ActionProminence
    private let showsReason: Bool

    /// Creates a button.
    /// - Parameters:
    ///   - action: The action.
    ///   - prominence: How strongly the button stands out.
    ///   - showsReason: Whether the reason the action cannot run shows under the button.
    ///     `false` for a button in a toolbar.
    public init(
        _ action: StacklyAction,
        prominence: ActionProminence = .secondary,
        showsReason: Bool = true
    ) {
        self.action = action
        self.prominence = prominence
        self.showsReason = showsReason
    }

    /// The button's content.
    public var body: some View {
        ActionButtonFace(
            action: action,
            prominence: prominence,
            showsReason: showsReason,
            availability: action.availability(gate: gate),
            run: { action.run(host: host, gate: gate) }
        )
    }
}

/// The look of an action button for a given availability, so the gallery can show every
/// state without a gate or a running action.
struct ActionButtonFace: View {
    let action: StacklyAction
    let prominence: ActionProminence
    let showsReason: Bool
    let availability: ActionAvailability
    let run: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            HStack(spacing: Spacing.s8) {
                button
                if availability == .busy {
                    ProgressView()
                        .controlSize(.small)
                        .transition(StacklyMotion.crossfade)

                }
            }
            if showsReason, case .disabled(let reason, true) = availability {
                DisabledReasonText(reason)
            }
        }
    }

    private var button: some View {
        Button(role: action.role.isDestructive ? .destructive : nil, action: run) {
            if let symbol = action.symbol {
                Label(action.title, systemImage: symbol)
            } else {
                Text(action.title)
            }
        }
        .modifier(
            ActionButtonStyle(isProminent: prominence == .primary && !action.role.isDestructive)
        )
        .disabled(availability == .busy)
        .disabled(reason: disabledHelp)
        .modifier(BusyValue(isBusy: availability == .busy))

    }

    /// The reason that becomes the help of a disabled button: the action's own or the
    /// gate's. Not set while busy, because that has no reason to write.
    private var disabledHelp: String? {
        if case .disabled(let reason, _) = availability { reason } else { nil }
    }
}

private struct ActionButtonStyle: ViewModifier {
    let isProminent: Bool

    @ViewBuilder func body(content: Content) -> some View {
        if isProminent {
            content.buttonStyle(.borderedProminent)
        } else {
            content.buttonStyle(.bordered)
        }
    }
}

private struct BusyValue: ViewModifier {
    let isBusy: Bool

    @ViewBuilder func body(content: Content) -> some View {
        if isBusy {
            content
        } else {
            content
        }
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { ActionButtonGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { ActionButtonGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { ActionButtonGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
