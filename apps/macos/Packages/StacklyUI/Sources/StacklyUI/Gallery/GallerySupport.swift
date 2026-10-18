#if DEBUG
    import SwiftUI

    /// Layout values of the design gallery.
    enum GalleryLayout {
        /// The width large text is checked at: the minimum content width of the main
        /// window (spec 0013). Text styles do not scale with `dynamicTypeSize` on macOS 27,
        /// so a narrow width with the longest labels stands in for the large text check
        /// (see `apps/macos/design.md`, Accessibility).
        static let narrowWidth: CGFloat = 640
        /// The width of the light and dark previews.
        static let previewWidth: CGFloat = 960
        /// The height of every preview.
        static let previewHeight: CGFloat = 760
    }

    /// The three previews every gallery page has: light, dark, and the large text check,
    /// which is a narrow width on macOS 27.
    enum GalleryPreviewVariant {
        case light
        case dark
        case narrowWidth
    }

    extension View {
        /// Frames a gallery page for one of its three previews.
        @ViewBuilder
        func galleryPreview(_ variant: GalleryPreviewVariant) -> some View {
            switch variant {
            case .light:
                frame(width: GalleryLayout.previewWidth, height: GalleryLayout.previewHeight)
                    .preferredColorScheme(.light)
            case .dark:
                frame(width: GalleryLayout.previewWidth, height: GalleryLayout.previewHeight)
                    .preferredColorScheme(.dark)
            case .narrowWidth:
                frame(width: GalleryLayout.narrowWidth, height: GalleryLayout.previewHeight)
                    .preferredColorScheme(.light)
            }
        }
    }

    /// The scrolling frame of a gallery page: sections on ``StacklyColor/surfaceWindow``
    /// with the page inset.
    struct GalleryPageLayout<Content: View>: View {
        @ViewBuilder let content: Content

        var body: some View {
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                    content
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Spacing.pageHorizontal)
                .padding(.vertical, Spacing.s22)
            }
            .background(StacklyColor.surfaceWindow)
        }
    }

    /// A titled section of a gallery page: a headline, an optional note, and the samples.
    struct GallerySection<Content: View>: View {
        let title: String
        let note: String?
        @ViewBuilder let content: Content

        init(_ title: String, note: String? = nil, @ViewBuilder content: () -> Content) {
            self.title = title
            self.note = note
            self.content = content()
        }

        var body: some View {
            VStack(alignment: .leading, spacing: Spacing.s8) {
                Text(title)
                    .font(.headline)
                    .accessibilityAddTraits(.isHeader)
                if let note {
                    Text(note)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                content
            }
        }
    }
#endif
