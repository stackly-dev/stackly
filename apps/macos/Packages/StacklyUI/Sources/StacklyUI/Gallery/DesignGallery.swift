#if DEBUG
    import SwiftUI

    /// The design gallery of spec 0012: every token, status, state, and confirmation of
    /// `StacklyUI` on one page each, with sample data from the mockup.
    ///
    /// The Debug app opens it in the "Design Gallery" window, so the system accessibility
    /// settings (Increase Contrast, Reduce Transparency, Differentiate Without Color,
    /// Reduce Motion) can be checked in the real app. It is not part of Release builds.
    public struct DesignGallery: View {
        @State private var selection: GalleryTopic? = .tokens
        @State private var isNarrow = false

        /// Creates the gallery.
        public init() {}

        /// The gallery's content.
        public var body: some View {
            NavigationSplitView {
                List(GalleryTopic.allCases, selection: $selection) { topic in
                    Label(topic.title, systemImage: topic.symbol)
                }
                .navigationSplitViewColumnWidth(min: 170, ideal: 190)
            } detail: {
                let topic = selection ?? .tokens
                topic.page
                    .frame(maxWidth: isNarrow ? GalleryLayout.narrowWidth : .infinity)
                    .frame(maxWidth: .infinity)
                    .navigationTitle(topic.title)
            }
            .toolbar {
                Toggle(
                    "Narrow Width",
                    systemImage: "arrow.left.and.right.square",
                    isOn: $isNarrow
                )
                .help(
                    "Show the page at \(Int(GalleryLayout.narrowWidth)) pt, the minimum content width"
                )
            }
        }
    }

    /// The pages of the design gallery, in sidebar order.
    enum GalleryTopic: CaseIterable, Identifiable {
        case tokens
        case status
        case states
        case confirmation

        var id: Self { self }

        /// The page title.
        var title: String {
            switch self {
            case .tokens: "Tokens"
            case .status: "Status"
            case .states: "States"
            case .confirmation: "Confirmation"
            }
        }

        /// The sidebar symbol.
        var symbol: String {
            switch self {
            case .tokens: "swatchpalette"
            case .status: "circle.badge.checkmark"
            case .states: "rectangle.stack"
            case .confirmation: "exclamationmark.bubble"
            }
        }

        /// The page view.
        @ViewBuilder var page: some View {
            switch self {
            case .tokens: TokensGalleryPage()
            case .status: StatusGalleryPage()
            case .states: StatesGalleryPage()
            case .confirmation: ConfirmationGalleryPage()
            }
        }
    }

    #Preview("Light") {
        DesignGallery().galleryPreview(.light)
    }

    #Preview("Dark") {
        DesignGallery().galleryPreview(.dark)
    }
#endif
