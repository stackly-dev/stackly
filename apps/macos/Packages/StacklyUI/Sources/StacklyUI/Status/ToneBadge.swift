import SwiftUI

/// A status tag: the label in a capsule tinted with its tone, for example an operation
/// state from ``StatusAppearance/operation(state:partial:)``.
///
/// The label is `.subheadline.weight(.semibold)` in the tone text color on
/// `toneSurface(tone, strength: .badge)`, which adds a 1 pt tone border while Increase
/// Contrast is on. While Differentiate Without Color is on, the tone symbol comes before
/// the label, in the same tone text color, so it keeps AA contrast on the tint. VoiceOver
/// reads the label.
public struct ToneBadge: View {
    /// The horizontal padding inside the capsule: 9 pt, from the mockup tag.
    static let horizontalPadding: CGFloat = 9

    @Environment(\.accessibilityDifferentiateWithoutColor) private var differentiateWithoutColor
    @Environment(\.showsStatusSymbols) private var showsStatusSymbols
    @ScaledMetric(.badgeHeight) private var minHeight

    private let appearance: StatusAppearance

    /// Creates a tone badge.
    ///
    /// - Parameter appearance: The status to show. The badge uses its label and tone.
    public init(_ appearance: StatusAppearance) {
        self.appearance = appearance
    }

    /// The badge's content.
    public var body: some View {
        HStack(spacing: Spacing.s4) {
            if differentiateWithoutColor || showsStatusSymbols {
                Image(systemName: appearance.tone.symbol)
                    .accessibilityHidden(true)
            }
            Text(appearance.label)
        }
        .font(.subheadline.weight(.semibold))
        .foregroundStyle(appearance.tone.text)
        .lineLimit(1)
        .fixedSize()
        .padding(.horizontal, Self.horizontalPadding)
        .frame(minHeight: minHeight)
        .toneSurface(appearance.tone, strength: .badge)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(appearance.label)
    }
}
