import SwiftUI

/// A scrolling log: one line per ``LogLine`` in monospace on the console surface
/// (spec 0067, AC-16).
///
/// The time and source columns have the same minimum width in every row, taken from a
/// hidden template, so the text lines up; a longer source shows in full and pushes the
/// text of its own line. A warning or error line always has its symbol and its tone, not
/// only while Differentiate Without Color is on. Text wraps and is selectable per line.
/// VoiceOver reads each line as one element.
///
/// With `follow` on, a new line scrolls the view to the last line. A scroll by the user
/// that ends more than 24 pt above the bottom turns `follow` off; turning it on scrolls
/// to the bottom. Scrolling back to the bottom does not turn it on again: the toggle
/// of feature 27 does. The screen sets the height of the view.
public struct LogView: View {
    private let lines: [LogLine]
    @Binding private var follow: Bool
    private let accessibilityLabel: String
    private let timeTemplate: String
    private let sourceTemplate: String

    @State private var position = ScrollPosition(edge: .bottom)
    @State private var distanceFromBottom: CGFloat = 0

    /// Creates a log view.
    ///
    /// - Parameters:
    ///   - lines: The lines, oldest first.
    ///   - follow: Whether the view follows the newest line.
    ///   - accessibilityLabel: The VoiceOver label of the list.
    ///   - timeTemplate: Text as wide as the widest time, which sets the time column's
    ///     minimum width.
    ///   - sourceTemplate: Text as wide as the usual source, which sets the source
    ///     column's minimum width.
    public init(
        _ lines: [LogLine], follow: Binding<Bool>, accessibilityLabel: String = "Log",
        timeTemplate: String = "00:00:00.000", sourceTemplate: String = "stderr"
    ) {
        self.lines = lines
        _follow = follow
        self.accessibilityLabel = accessibilityLabel
        self.timeTemplate = timeTemplate
        self.sourceTemplate = sourceTemplate
    }

    /// The log's content.
    public var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(lines) { line in
                    LogRow(
                        line: line,
                        timeTemplate: showsTime ? timeTemplate : nil,
                        sourceTemplate: showsSource ? sourceTemplate : nil)
                }
            }
            .scrollTargetLayout()
        }
        .contentMargins(.vertical, Spacing.s12, for: .scrollContent)
        .scrollPosition($position)
        .onScrollGeometryChange(for: CGFloat.self) { geometry in
            max(
                0,
                geometry.contentSize.height + geometry.contentInsets.bottom
                    - geometry.contentOffset.y - geometry.containerSize.height)
        } action: { _, distance in
            distanceFromBottom = distance
        }
        .onScrollPhaseChange { old, new in
            if LogFollow.shouldStopFollowing(
                from: old, to: new, distanceFromBottom: distanceFromBottom)
            {
                follow = false
            }
        }
        .onChange(of: lines.last?.id) {
            if follow { scrollToLastLine() }
        }
        .onChange(of: follow) {
            if follow { scrollToLastLine() }
        }
        .onAppear {
            if follow { scrollToLastLine() }
        }
        .background { Color.clear.consoleSurface() }
        .clipShape(RoundedRectangle(cornerRadius: Radius.medium.value))
        .accessibilityLabel(accessibilityLabel)
        .accessibilityIdentifier(AccessibilityID.Log.list)
    }

    /// Whether any line has a time, so the time column is drawn.
    private var showsTime: Bool {
        lines.contains { !($0.timeText ?? "").isEmpty }
    }

    /// Whether any line has a source, so the source column is drawn.
    private var showsSource: Bool {
        lines.contains { !($0.source ?? "").isEmpty }
    }

    /// Jumps to the last line without animation.
    private func scrollToLastLine() {
        var transaction = Transaction()
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            if let last = lines.last?.id {
                position.scrollTo(id: last, anchor: .bottom)
            } else {
                position.scrollTo(edge: .bottom)
            }
        }
    }
}

/// One line: the time, the source, the level symbol, and the text.
struct LogRow: View {
    let line: LogLine
    let timeTemplate: String?
    let sourceTemplate: String?

    /// A row with the default columns, for tests of the row alone.
    init(
        line: LogLine, timeTemplate: String? = "00:00:00.000",
        sourceTemplate: String? = "stderr"
    ) {
        self.line = line
        self.timeTemplate = timeTemplate
        self.sourceTemplate = sourceTemplate
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            if let timeTemplate {
                column(line.timeText, template: timeTemplate)
            }
            if let sourceTemplate {
                column(line.source, template: sourceTemplate)
            }
            symbolBox
            Text(line.text)
                .font(.callout.monospaced())
                .foregroundStyle(textStyle)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.vertical, Spacing.s2)
        .padding(.horizontal, Spacing.s14)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(line.accessibilityLabel)
    }

    /// A column as wide as its hidden template at least, and as wide as `text` when that
    /// is wider.
    private func column(_ text: String?, template: String) -> some View {
        ZStack(alignment: .leading) {
            Text(template).hidden()
            Text(text ?? "")
        }
        .font(.callout.monospaced())
        .foregroundStyle(StacklyColor.textSecondary)
        .lineLimit(1)
        .fixedSize(horizontal: true, vertical: false)
        .accessibilityHidden(true)
    }

    /// The symbol of a warning or error, or an empty box as wide as the symbol, so the
    /// text of every line starts at the same place.
    private var symbolBox: some View {
        ZStack {
            Image(systemName: LogLevel.warning.symbol ?? "").hidden()
            if let symbol = line.level.symbol, let tone = line.level.tone {
                Image(systemName: symbol)
                    .foregroundStyle(tone.fill)
            }
        }
        .font(.callout)
        .accessibilityHidden(true)
    }

    private var textStyle: AnyShapeStyle {
        if let tone = line.level.tone, line.level != .normal {
            AnyShapeStyle(tone.text)
        } else {
            AnyShapeStyle(.primary)
        }
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { LogViewGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { LogViewGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { LogViewGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
