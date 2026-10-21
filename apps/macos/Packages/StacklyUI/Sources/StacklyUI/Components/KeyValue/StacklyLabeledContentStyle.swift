import SwiftUI

/// The style of a read only key value row (spec 0067, AC-3): the label in `.body`
/// primary text and the value in `.body` ``StacklyColor/textSecondary``, selectable.
///
/// The row is one line with the value flush right when both fit. Otherwise the label sits
/// above and the value wraps below it, flush left, never cut. VoiceOver reads the row as
/// one element, "{label}, {value}" (checked in the running app, because a selectable value
/// alone makes VoiceOver read the label only).
///
/// Apply it to a ``CardSection`` whose rows are all read only, to an inspector, or to a
/// single read only row inside a `Form`. Never apply it to a `Form` itself: the controls
/// of a `Form` (field, picker, toggle) use the `LabeledContent` style inside it. A screen
/// may give the value its own font, for example `.callout.monospaced()` for an endpoint,
/// a path, or a digest; the color stays `textSecondary`.
public struct StacklyLabeledContentStyle: LabeledContentStyle {
    /// Creates the style.
    public init() {}

    /// The row for `configuration`.
    public func makeBody(configuration: Configuration) -> some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .firstTextBaseline, spacing: 0) {
                label(configuration)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
                Spacer(minLength: Spacing.s12)
                value(configuration)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                label(configuration)
                    .fixedSize(horizontal: false, vertical: true)
                value(configuration)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        // Selectable text breaks `.combine` (VoiceOver would read the label only), so the
        // spoken element is built from the same label and value without the selection.
        .accessibilityRepresentation {
            HStack {
                configuration.label
                configuration.content
            }
            .accessibilityElement(children: .combine)
        }
    }

    private func label(_ configuration: Configuration) -> some View {
        configuration.label
            .font(.body)
            .foregroundStyle(.primary)
    }

    private func value(_ configuration: Configuration) -> some View {
        configuration.content
            .font(.body)
            .foregroundStyle(StacklyColor.textSecondary)
            .textSelection(.enabled)
    }
}

extension LabeledContentStyle where Self == StacklyLabeledContentStyle {
    /// The Stackly key value row style. See ``StacklyLabeledContentStyle``.
    public static var stackly: StacklyLabeledContentStyle {
        StacklyLabeledContentStyle()
    }
}
