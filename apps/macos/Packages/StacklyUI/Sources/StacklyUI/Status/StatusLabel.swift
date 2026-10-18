import SwiftUI

/// Where a status label sits, which decides its text colors.
nonisolated public enum StatusContext: CaseIterable, Hashable, Sendable {
    /// Cards, banners, sheets, and panels: the label takes the `critical` tone text or
    /// ``StacklyColor/textSecondary``, and the detail takes its own tone text.
    case standard
    /// `List` and `Table` rows, the sidebar, menus, and popovers: the label and detail
    /// use `.primary`, because an inactive selection or glass behind the row cannot be
    /// guaranteed for tone text. While the row is selected (`backgroundProminence` is
    /// `.increased`), the dot and symbols use `.primary` too.
    case row
}

/// The color source of one part of a status label.
nonisolated enum StatusInk: Hashable, Sendable {
    /// The system primary label color, which follows a row selection.
    case primary
    /// The text color of a tone, ``Tone/text``.
    case text(Tone)
    /// The fill of a tone, ``Tone/fill``: for dots and symbols only, never for text.
    case fill(Tone)

    /// The style to draw with.
    var style: AnyShapeStyle {
        switch self {
        case .primary: AnyShapeStyle(.primary)
        case .text(let tone): AnyShapeStyle(tone.text)
        case .fill(let tone): AnyShapeStyle(tone.fill)
        }
    }
}

extension StatusContext {
    /// The ink of the status label text.
    func labelInk(for tone: Tone) -> StatusInk {
        switch self {
        case .standard: tone == .critical ? .text(.critical) : .text(.neutral)
        case .row: .primary
        }
    }

    /// The ink of the detail text.
    func detailInk(for tone: Tone) -> StatusInk {
        switch self {
        case .standard: .text(tone)
        case .row: .primary
        }
    }

    /// The ink of the status dot, the status symbol, and the detail symbol.
    func indicatorInk(for tone: Tone, prominence: BackgroundProminence) -> StatusInk {
        if self == .row, prominence == .increased {
            return .primary
        }
        return .fill(tone)
    }
}

extension StatusAppearance {
    /// What VoiceOver reads for a status label: "Status: <label>", then ", <detail>".
    var spokenStatus: String {
        guard let detail else {
            return "Status: \(label)"
        }
        return "Status: \(label), \(detail.text)"
    }
}

extension EnvironmentValues {
    /// Draws status labels and tone badges as if Differentiate Without Color were on.
    /// Only the design gallery sets it, because the system value cannot be set in a
    /// preview.
    @Entry var showsStatusSymbols = false
}

/// A status: a colored dot, the label, and an optional detail with its own tone.
///
/// The label is always shown and never cut; the detail truncates first. While
/// Differentiate Without Color is on, the dot becomes the status symbol and the detail
/// starts with its tone symbol. VoiceOver reads one element, "Status: <label>" plus the
/// detail. A change of status crossfades with ``StacklyMotion/crossfade``.
///
/// The label takes the font around it.
public struct StatusLabel: View {
    @Environment(\.accessibilityDifferentiateWithoutColor) private var differentiateWithoutColor
    @Environment(\.showsStatusSymbols) private var showsStatusSymbols
    @Environment(\.backgroundProminence) private var prominence
    @ScaledMetric(.statusDot) private var dotSize

    private let appearance: StatusAppearance
    private let context: StatusContext

    /// Creates a status label.
    ///
    /// - Parameters:
    ///   - appearance: The status to show, usually from
    ///     ``StatusAppearance/instance(lifecycle:readiness:health:)``.
    ///   - context: Where the label sits. Use `.row` in `List` and `Table` rows, the
    ///     sidebar, menus, and popovers.
    public init(_ appearance: StatusAppearance, context: StatusContext = .standard) {
        self.appearance = appearance
        self.context = context
    }

    /// The label's content.
    public var body: some View {
        ZStack(alignment: .leading) {
            content
                .id(appearance)
                .transition(StacklyMotion.crossfade)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(appearance.spokenStatus)
    }

    private var usesSymbols: Bool {
        differentiateWithoutColor || showsStatusSymbols
    }

    private var content: some View {
        HStack(spacing: Spacing.s6) {
            indicator
            Text(appearance.label)
                .foregroundStyle(context.labelInk(for: appearance.tone).style)
                .fixedSize(horizontal: true, vertical: false)
                .layoutPriority(1)
            if let detail = appearance.detail {
                Text(verbatim: "·")
                    .foregroundStyle(context.labelInk(for: appearance.tone).style)
                    .fixedSize()
                    .accessibilityHidden(true)
                detailView(detail)
            }
        }
    }

    @ViewBuilder private var indicator: some View {
        let ink = context.indicatorInk(for: appearance.tone, prominence: prominence).style
        Group {
            if usesSymbols {
                Image(systemName: appearance.symbol)
                    .foregroundStyle(ink)
            } else {
                Circle()
                    .fill(ink)
                    .frame(width: dotSize, height: dotSize)
            }
        }
        .accessibilityHidden(true)
    }

    private func detailView(_ detail: StatusDetail) -> some View {
        HStack(spacing: Spacing.s4) {
            if usesSymbols {
                Image(systemName: detail.tone.symbol)
                    .foregroundStyle(
                        context.indicatorInk(for: detail.tone, prominence: prominence).style
                    )
                    .accessibilityHidden(true)
            }
            Text(detail.text)
                .foregroundStyle(context.detailInk(for: detail.tone).style)
                .lineLimit(1)
                .truncationMode(.tail)
        }
    }
}
