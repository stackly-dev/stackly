import SwiftUI

/// A status tag: the label in a capsule tinted with its tone, for example an operation
/// state from ``StatusAppearance/operation(state:partial:)``.
public struct ToneBadge: View {
    /// The horizontal padding inside the capsule: 9 pt, from the mockup tag.
    static let horizontalPadding: CGFloat = 9

    private let minHeight = ScaledSize.badgeHeight.base

    private let appearance: StatusAppearance

    /// Creates a tone badge.
    /// - Parameter appearance: The status to show. The badge uses its label and tone.
    public init(_ appearance: StatusAppearance) {
        self.appearance = appearance
    }

    /// The badge's content.
    public var body: some View {
        HStack(spacing: Spacing.s4) {

            Text(appearance.label)
        }
        .font(.subheadline.weight(.semibold))
        .foregroundStyle(appearance.tone.text)
        .lineLimit(1)
        .fixedSize()
        .padding(.horizontal, Self.horizontalPadding)
        .frame(minHeight: minHeight)
        .toneSurface(appearance.tone, strength: .badge)

    }
}
