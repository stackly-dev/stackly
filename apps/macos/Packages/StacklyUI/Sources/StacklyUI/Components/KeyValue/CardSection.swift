import SwiftUI

/// A titled card of rows (spec 0067, AC-3): an optional `.headline` title with an
/// accessory on its right, then a ``SwiftUI/View/cardSurface(radius:)`` card holding every
/// subview of the content as one row, then an optional footer.
///
/// Each subview is one row, set apart by a divider inset 14 pt from the left, with 11 pt
/// above and below and 14 pt at the sides, and at least ``ScaledSize/rowMinHeight`` high.
/// A `ForEach` makes one row per element; a row made of several views is wrapped in an
/// `HStack` or `VStack` by the screen. Put ``SwiftUI/View/labeledContentStyle(_:)`` with
/// ``StacklyLabeledContentStyle`` on the section when every row is read only. A button in
/// a row, such as ``CopyButton``, stands next to the `LabeledContent`, not inside it.
public struct CardSection<Accessory: View, Footer: View, Content: View>: View {
    @ScaledMetric(.rowMinHeight) private var rowMinHeight

    private let title: String?
    private let accessory: Accessory
    private let footer: Footer
    private let content: Content

    /// Creates a section with an accessory and a footer.
    ///
    /// - Parameters:
    ///   - title: The title, or `nil` for a card without one.
    ///   - accessory: The view on the right of the title, for example a button.
    ///   - footer: The view under the card; a `String` footer uses the other initializer.
    ///   - content: The rows, one per subview.
    public init(
        _ title: String? = nil,
        @ViewBuilder accessory: () -> Accessory,
        @ViewBuilder footer: () -> Footer,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.accessory = accessory()
        self.footer = footer()
        self.content = content()
    }

    /// The section's content.
    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            if title != nil || Accessory.self != EmptyView.self {
                header
            }
            card
            if Footer.self != EmptyView.self {
                footer
                    .font(.footnote)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
            if let title {
                Text(title)
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .accessibilityAddTraits(.isHeader)
            }
            Spacer(minLength: Spacing.s12)
            accessory
        }
    }

    private var card: some View {
        Group(subviews: content) { rows in
            VStack(spacing: 0) {
                ForEach(rows) { row in
                    row
                        .padding(Spacing.rowInsets)
                        .frame(maxWidth: .infinity, minHeight: rowMinHeight, alignment: .leading)
                    if row.id != rows.last?.id {
                        Divider()
                            .padding(.leading, Spacing.s14)
                    }
                }
            }
        }
        .cardSurface()
    }
}

extension CardSection where Accessory == EmptyView, Footer == EmptyView {
    /// Creates a section with a title and rows.
    ///
    /// - Parameters:
    ///   - title: The title, or `nil` for a card without one.
    ///   - content: The rows, one per subview.
    public init(_ title: String? = nil, @ViewBuilder content: () -> Content) {
        self.init(title, accessory: { EmptyView() }, footer: { EmptyView() }, content: content)
    }
}

extension CardSection where Accessory == EmptyView, Footer == Text {
    /// Creates a section with a title, rows, and a text footer.
    ///
    /// - Parameters:
    ///   - title: The title, or `nil` for a card without one.
    ///   - footer: The footnote under the card.
    ///   - content: The rows, one per subview.
    public init(
        _ title: String? = nil,
        footer: String,
        @ViewBuilder content: () -> Content
    ) {
        self.init(title, accessory: { EmptyView() }, footer: { Text(footer) }, content: content)
    }
}

extension CardSection where Footer == EmptyView {
    /// Creates a section with a title, an accessory on its right, and rows.
    ///
    /// - Parameters:
    ///   - title: The title, or `nil` for a card without one.
    ///   - accessory: The view on the right of the title, for example a button.
    ///   - content: The rows, one per subview.
    public init(
        _ title: String? = nil,
        @ViewBuilder accessory: () -> Accessory,
        @ViewBuilder content: () -> Content
    ) {
        self.init(title, accessory: accessory, footer: { EmptyView() }, content: content)
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { KeyValueGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { KeyValueGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { KeyValueGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
