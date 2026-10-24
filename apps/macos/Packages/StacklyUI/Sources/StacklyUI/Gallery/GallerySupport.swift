#if DEBUG
    import SwiftUI

    /// Layout values of the design gallery.
    enum GalleryLayout {
        /// The width large text is checked at: the minimum content width of the main window (spec 0013).
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

    extension EnvironmentValues {
        /// Whether ``GalleryPageLayout`` wraps a page in a scroll view; the snapshot suites
        /// turn it off so a page renders at its full height (spec 0068).
        @Entry var galleryScrollsPage = true
    }

    /// The scrolling frame of a gallery page: sections on ``StacklyColor/surfaceWindow``
    /// with the page inset. Without scrolling (``EnvironmentValues/galleryScrollsPage``)
    /// the page is the same column at its full height.
    struct GalleryPageLayout<Content: View>: View {
        @Environment(\.galleryScrollsPage) private var scrolls
        @ViewBuilder let content: Content

        var body: some View {
            Group {
                if scrolls {
                    ScrollView { column }
                } else {
                    column
                }
            }
            .background(StacklyColor.surfaceWindow)
        }

        private var column: some View {
            VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s22)
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
