import SwiftUI

extension View {
    /// Draws the console surface behind the view: ``StacklyColor/surfaceConsole`` with
    /// ``Radius/medium`` corners, a hairline `separatorColor` border that becomes 1 pt
    /// while Increase Contrast is on, and 12 pt above and below and 14 pt at the sides.
    ///
    /// It only composes tokens of spec 0012, so there is no new contrast pair.
    public func consoleSurface() -> some View {
        modifier(ConsoleSurfaceModifier())
    }
}

private struct ConsoleSurfaceModifier: ViewModifier {
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.displayScale) private var displayScale

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: Radius.medium.value)
        content
            .padding(.vertical, Spacing.s12)
            .padding(.horizontal, Spacing.s14)
            .background(shape.fill(StacklyColor.surfaceConsole))
            .overlay {
                shape.strokeBorder(
                    Color(nsColor: .separatorColor),
                    lineWidth: contrast == .increased ? 1 : 1 / displayScale
                )
            }
    }
}

/// A block of monospace text on the console surface (spec 0067, AC-5): a command with its
/// Copy button, or several lines of text.
///
/// The text is `.callout.monospaced()` primary text, selectable, and wraps without a
/// horizontal scroll. VoiceOver reads the text as one element with the screen's label and
/// the text as its value, and the Copy button as its own element.
public struct ConsoleBlock: View {
    /// What the block shows.
    enum Kind: Equatable {
        case command(String, what: String, announcement: String)
        case text(String, copy: ConsoleCopy?)
    }

    let kind: Kind
    private let accessibilityLabel: String

    /// Creates a block for one command: "$ " in secondary text, then the command, with a
    /// Copy button that copies the command without the "$ ".
    ///
    /// - Parameters:
    ///   - command: The command.
    ///   - what: What the copy button names, in lower case.
    ///   - announcement: What VoiceOver says after the copy.
    ///   - accessibilityLabel: The VoiceOver label of the text.
    public init(
        command: String,
        what: String = "command",
        announcement: String = "Command copied.",
        accessibilityLabel: String = "Equivalent command"
    ) {
        kind = .command(command, what: what, announcement: announcement)
        self.accessibilityLabel = accessibilityLabel
    }

    /// Creates a block for several lines of text.
    ///
    /// - Parameters:
    ///   - text: The text.
    ///   - accessibilityLabel: The VoiceOver label of the text.
    ///   - copy: Adds a Copy button at the top right when not `nil`.
    public init(text: String, accessibilityLabel: String, copy: ConsoleCopy? = nil) {
        kind = .text(text, copy: copy)
        self.accessibilityLabel = accessibilityLabel
    }

    /// The text VoiceOver reads as the value: the command without its "$ ", or the text.
    var spokenText: String {
        switch kind {
        case .command(let command, _, _): command
        case .text(let text, _): text
        }
    }

    /// The block's content.
    public var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
            textView
                .font(.callout.monospaced())
                .foregroundStyle(.primary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(accessibilityLabel)
                .accessibilityValue(spokenText)
            copyButton
        }
        .consoleSurface()
        .accessibilityElement(children: .contain)
    }

    private var textView: Text {
        switch kind {
        case .command(let command, _, _):
            Text("\(Text("$ ").foregroundStyle(StacklyColor.textSecondary))\(command)")
        case .text(let text, _):
            Text(text)
        }
    }

    @ViewBuilder private var copyButton: some View {
        switch kind {
        case .command(let command, let what, let announcement):
            CopyButton(command, what: what, announcement: announcement)
        case .text(let text, let copy?):
            CopyButton(text, what: copy.what, announcement: copy.announcement)
        case .text:
            EmptyView()
        }
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { ConsoleGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { ConsoleGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { ConsoleGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
