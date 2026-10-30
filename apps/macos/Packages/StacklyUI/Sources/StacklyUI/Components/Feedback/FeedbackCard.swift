import SwiftUI

nonisolated struct FeedbackHold: Equatable {
    /// The pointer is over the card.
    var isHovered: Bool
    /// The window is the key or the active window.
    var isWindowActive: Bool

    /// Whether the wait runs.
    var countsDown: Bool {
        !isHovered && isWindowActive
    }
}

extension View {
    /// Shows the window's ``FeedbackCenter/current`` as a card at the bottom of this view,
    /// in a bottom safe area inset, so the content scrolls above it and a focused control
    /// is never covered. Put it on the detail column of the window. Without a center in
    /// the environment it draws nothing.
    public func feedbackPresenter() -> some View {
        modifier(FeedbackPresenter())
    }
}

private struct FeedbackPresenter: ViewModifier {
    @Environment(\.feedbackCenter) private var center

    func body(content: Content) -> some View {
        content.safeAreaInset(edge: .bottom, spacing: 0) {
            Group {
                if let center, let feedback = center.current {
                    FeedbackCard(feedback: feedback, center: center)
                        .id(feedback.id)
                        .transition(StacklyMotion.crossfade)
                }
            }
            .animation(StacklyMotion.crossfadeAnimation, value: center?.current?.id)
        }
    }
}

/// One feedback as an opaque card: the tone symbol, the message, the detail, the code, at
/// most one action, and Dismiss.
struct FeedbackCard: View {

    /// What restarts the countdown task: a new card, or a change of whether it may run.
    private struct CountdownKey: Equatable {
        let id: Feedback.ID
        let countsDown: Bool
    }

    private let symbolSize = ScaledSize.bannerSymbol.base
    @Environment(\.controlActiveState) private var controlActiveState
    @State private var isHovered = false

    let feedback: Feedback
    let center: FeedbackCenter

    private var hold: FeedbackHold {
        FeedbackHold(
            isHovered: isHovered,

            isWindowActive: controlActiveState == .key || controlActiveState == .active
        )
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
            Image(systemName: feedback.tone.symbol)
                .stacklySymbol(.bannerSymbol)
                .foregroundStyle(feedback.tone.fill)

            texts
            if let action = feedback.action {
                ActionButton(action, showsReason: false)
                    .controlSize(.small)

            }
            dismissButton
        }
        .padding(.vertical, Spacing.s12)
        .padding(.horizontal, Spacing.s16)
        .toneSurface(
            feedback.tone,
            on: .card,
            strength: .banner,
            in: RoundedRectangle(cornerRadius: Radius.large.value)
        )
        .shadow(color: .black.opacity(0.18), radius: 12, y: 4)
        .frame(maxWidth: Self.maxWidth)
        .padding(.horizontal, Spacing.s22)
        .padding(.bottom, Spacing.s16)
        .frame(maxWidth: .infinity)
        .onHover { isHovered = $0 }
        .task(id: CountdownKey(id: feedback.id, countsDown: hold.countsDown)) {
            if hold.countsDown {
                await center.countdown(feedback.id)
            }
        }

    }

    /// The widest the card gets: 520 pt.
    static let maxWidth: CGFloat = 520

    /// The narrowest the text column gets: 200 pt. A split view asks the detail column for
    /// its size at a width near zero, where text with a fixed vertical size is hundreds of
    /// points tall; through the bottom inset that height would become the window's.
    static let textMinWidth: CGFloat = 200

    private var texts: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            Text(feedback.message)
                .font(.body)
                .foregroundStyle(.primary)
            if let detail = feedback.detail {
                Text(detail)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            if let code = feedback.code {
                Text(code)
                    .font(.callout.monospaced())
                    .foregroundStyle(StacklyColor.textSecondary)
                    .textSelection(.enabled)
            }
        }
        .frame(minWidth: Self.textMinWidth, alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, alignment: .leading)

    }

    private var dismissButton: some View {
        Button {
            center.dismiss()
        } label: {
            Image(systemName: "xmark")
                .frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)
                .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)

        .help("Dismiss")

    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { FeedbackGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { FeedbackGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { FeedbackGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
