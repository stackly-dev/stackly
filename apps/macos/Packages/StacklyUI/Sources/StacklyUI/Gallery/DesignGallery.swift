#if DEBUG
    import StacklyClient
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
        private let secrets: SecretsClient?

        /// Creates the gallery.
        ///
        /// - Parameter secrets: The secret methods over the app's real daemon link, for the
        ///   Secrets page (spec 0011, step 11). Without it that page says there is no link.
        public init(secrets: SecretsClient? = nil) {
            self.secrets = secrets
        }

        /// The gallery's content.
        public var body: some View {
            NavigationSplitView {
                List(GalleryTopic.allCases, selection: $selection) { topic in
                    Label(topic.title, systemImage: topic.symbol)
                }
                .navigationSplitViewColumnWidth(min: 170, ideal: 190)
            } detail: {
                let topic = selection ?? .tokens
                topic.page(secrets: secrets)
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
        case secrets
        case components

        var id: Self { self }

        /// The page title.
        var title: String {
            switch self {
            case .tokens: "Tokens"
            case .status: "Status"
            case .states: "States"
            case .confirmation: "Confirmation"
            case .secrets: "Secrets"
            case .components: "Components"
            }
        }

        /// The sidebar symbol.
        var symbol: String {
            switch self {
            case .tokens: "swatchpalette"
            case .status: "circle.badge.checkmark"
            case .states: "rectangle.stack"
            case .confirmation: "exclamationmark.bubble"
            case .secrets: "key"
            case .components: "square.grid.2x2"
            }
        }

        /// The page view.
        @ViewBuilder func page(secrets: SecretsClient?) -> some View {
            switch self {
            case .tokens: TokensGalleryPage()
            case .status: StatusGalleryPage()
            case .states: StatesGalleryPage()
            case .confirmation: ConfirmationGalleryPage()
            case .secrets: SecretsGalleryPage(client: secrets)
            case .components: ComponentsGalleryPage()
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
