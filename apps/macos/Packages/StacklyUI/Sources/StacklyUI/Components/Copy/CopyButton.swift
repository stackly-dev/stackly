import SwiftUI

/// How a ``CopyButton`` looks.
public enum CopyButtonStyle {
    /// A small bordered button labeled "Copy".
    case text
    /// A borderless `doc.on.doc` symbol with a 24 × 24 pt click area.
    case icon
}

/// A button that copies plain text to the pasteboard, only when pressed (spec 0067, AC-4).
/// Pressing it writes the text, then for two seconds shows "Copied" with a `checkmark` (the icon style shows just the checkmark) without changing its width, then goes back. Every press writes again and starts the two seconds over.
/// The button never changes data, so it is never gated by ``MutationAvailability``. It is
/// for addresses, IDs, paths, digests, and commands; a secret always goes through
/// ``RevealableSecretText`` and ``ConcealedPasteboard``.
public struct CopyButton: View {
    @State private var confirmation = CopyConfirmation()
    @State private var task: Task<Void, Never>?

    private let text: @MainActor () -> String
    private let title: String
    private let what: String
    private let style: CopyButtonStyle

    /// Creates a copy button.
    /// - Parameters:
    ///   - text: What to copy.
    ///   - what: What it is, in lower case: "address", "instance ID".
    public init(
        _ text: String,
        what: String,
        style: CopyButtonStyle = .text
    ) {
        self.text = { text }
        self.title = "Copy"
        self.what = what
        self.style = style

    }

    /// A text button with its own title whose text is built only when pressed, for content
    /// too large to build on every render, such as the shown lines of a log (spec 0027).
    /// - Parameters:
    ///   - title: The title, such as "Copy Shown Lines".
    ///   - what: What it copies, in lower case: "shown lines".
    ///   - text: Builds what to copy.
    public init(
        title: String,
        what: String,
        text: @escaping @MainActor () -> String
    ) {
        self.text = text
        self.title = title
        self.what = what
        self.style = .text
    }

    /// The button's content.
    public var body: some View {
        CopyButtonFace(
            style: style,
            title: title,
            what: what,
            isConfirming: confirmation.isConfirming,
            action: press
        )
        .onDisappear {
            task?.cancel()
        }
    }

    private func press() {
        task?.cancel()
        let confirmation = confirmation
        let text = text()

        task = Task {
            await confirmation.copy(text)
        }
    }
}

/// The look of a copy button, with the confirming state given by the caller, so the
/// gallery can show "Copied" without a press.
struct CopyButtonFace: View {
    let style: CopyButtonStyle
    var title = "Copy"
    let what: String
    let isConfirming: Bool
    let action: () -> Void

    var body: some View {
        Group {
            switch style {
            case .text:
                Button(action: action) { textLabel }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
            case .icon:
                Button(action: action) { iconLabel }
                    .buttonStyle(.borderless)
            }
        }

        .help("Copy \(what)")

    }

    /// The title and "Copied" stacked, so the button keeps the width of the longer one.
    private var textLabel: some View {
        ZStack {
            Text(title)
                .opacity(isConfirming ? 0 : 1)
            Label {
                Text("Copied")
            } icon: {
                Image(systemName: "checkmark")
                    .foregroundStyle(Tone.positive.fill)
            }
            .opacity(isConfirming ? 1 : 0)
        }

    }

    private var iconLabel: some View {
        Image(systemName: isConfirming ? "checkmark" : "doc.on.doc")
            .foregroundStyle(
                isConfirming
                    ? AnyShapeStyle(Tone.positive.fill) : AnyShapeStyle(StacklyColor.textSecondary)
            )
            .frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)
            .contentShape(Rectangle())

    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { CopyGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { CopyGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { CopyGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
