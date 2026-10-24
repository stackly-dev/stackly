import SwiftUI

/// The message shown when the inspector has no selection.
public struct InspectorPlaceholder: View {
    let title: String
    let message: String?

    /// Creates a placeholder.
    /// - Parameters:
    ///   - title: The title, "No Selection" by default.
    ///   - message: What to do, for example "Select an instance to see its details."
    public init(title: String = "No Selection", message: String? = nil) {
        self.title = title
        self.message = message
    }

    /// The placeholder's content.
    public var body: some View {
        VStack(spacing: Spacing.s6) {
            Text(title)
                .font(.title3.weight(.semibold))
                .foregroundStyle(.primary)
            if let message {
                Text(message)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
        }
        .multilineTextAlignment(.center)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, maxHeight: .infinity)

    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { InspectorPlaceholderGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { InspectorPlaceholderGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { InspectorPlaceholderGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
