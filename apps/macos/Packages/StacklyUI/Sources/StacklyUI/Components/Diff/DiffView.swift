import SwiftUI

/// A list of changed lines (spec 0067, AC-17): config changes and drift.
///
/// Each line has a sign column that always shows ("+" or "−", empty for an unchanged
/// line), so a change is never only a color. A changed line sits on a tint of its tone and
/// has primary text; an unchanged line has secondary text and no tint. Text wraps and is
/// selectable. Without lines the view shows `emptyText`. VoiceOver reads "Added: …",
/// "Removed: …", or "Unchanged: …" per line.
public struct DiffView: View {
    private let lines: [DiffLine]
    private let emptyText: String

    /// Creates a diff view.
    ///
    /// - Parameters:
    ///   - lines: The lines, in order.
    ///   - emptyText: What the view says when there are no lines, from the screen.
    public init(_ lines: [DiffLine], emptyText: String) {
        self.lines = lines
        self.emptyText = emptyText
    }

    /// The diff's content.
    public var body: some View {
        Group {
            if lines.isEmpty {
                Text(emptyText)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(Spacing.s8)
            } else {
                LazyVStack(alignment: .leading, spacing: Spacing.s2) {
                    ForEach(lines) { line in
                        DiffRow(line: line)
                    }
                }
            }
        }
        .padding(Spacing.s8)
        .cardSurface()
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(AccessibilityID.Diff.list)
    }
}

/// One line: the sign column and the text.
private struct DiffRow: View {
    let line: DiffLine

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Radius.small.value)
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            ZStack {
                Text("+").hidden()
                Text(line.kind.mark ?? "")
                    .foregroundStyle(markStyle)
            }
            .accessibilityHidden(true)
            Text(line.text)
                .foregroundStyle(textStyle)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
        }
        .font(.callout.monospaced())
        .padding(.vertical, 3)
        .padding(.horizontal, Spacing.s8)
        .modifier(DiffTint(tone: line.kind.tone, shape: shape))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(line.accessibilityLabel)
    }

    private var markStyle: AnyShapeStyle {
        if let tone = line.kind.tone {
            AnyShapeStyle(tone.text)
        } else {
            AnyShapeStyle(StacklyColor.textSecondary)
        }
    }

    private var textStyle: AnyShapeStyle {
        if line.kind == .unchanged {
            AnyShapeStyle(StacklyColor.textSecondary)
        } else {
            AnyShapeStyle(.primary)
        }
    }
}

/// The tone tint of a changed line, and nothing for an unchanged one.
private struct DiffTint: ViewModifier {
    let tone: Tone?
    let shape: RoundedRectangle

    @ViewBuilder
    func body(content: Content) -> some View {
        if let tone {
            content.toneSurface(tone, on: .card, strength: .banner, in: shape)
        } else {
            content
        }
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { DiffViewGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { DiffViewGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { DiffViewGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
