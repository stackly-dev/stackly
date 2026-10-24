import SwiftUI

extension View {
    /// It only composes tokens of spec 0012, so there is no new contrast pair.
    public func consoleSurface() -> some View {
        modifier(ConsoleSurfaceModifier())
    }
}

private struct ConsoleSurfaceModifier: ViewModifier {
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
                    lineWidth: 1 / displayScale
                )
            }
    }
}

/// A block of monospace text on the console surface (spec 0067, AC-5): a command with its
/// Copy button, or several lines of text.
/// The text is `.callout.monospaced()` primary text, selectable, and wraps without a horizontal scroll.
public struct ConsoleBlock: View {
    /// What the block shows.
    enum Kind: Equatable {
        case command(String, what: String)
        case text(String, copy: ConsoleCopy?)
    }

    let kind: Kind

    /// Creates a block for one command: "$ " in secondary text, then the command, with a
    /// Copy button that copies the command without the "$ ".
    /// - Parameters:
    ///   - command: The command.
    ///   - what: What the copy button names, in lower case.
    public init(
        command: String,
        what: String = "command"
    ) {
        kind = .command(command, what: what)

    }

    /// Creates a block for several lines of text.
    /// - Parameters:
    ///   - text: The text.
    ///   - copy: Adds a Copy button at the top right when not `nil`.
    public init(text: String, copy: ConsoleCopy? = nil) {
        kind = .text(text, copy: copy)

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

            copyButton
        }
        .consoleSurface()

    }

    private var textView: Text {
        switch kind {
        case .command(let command, _):
            Text("\(Text("$ ").foregroundStyle(StacklyColor.textSecondary))\(command)")
        case .text(let text, _):
            Text(text)
        }
    }

    @ViewBuilder private var copyButton: some View {
        switch kind {
        case .command(let command, let what):
            CopyButton(command, what: what)
        case .text(let text, let copy?):
            CopyButton(text, what: copy.what)
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
