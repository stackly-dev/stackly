import SwiftUI

/// The toolbar button that shows and hides the inspector (spec 0067, AC-18).
/// ``SwiftUI/View/stacklyInspector(isPresented:help:content:)`` puts it in the toolbar; spec 0069 binds the View menu command to the same binding.
public struct InspectorToggle: View {
    @Binding private var isPresented: Bool
    private let help: String?

    /// Creates the toggle.
    public init(isPresented: Binding<Bool>, help: String? = nil) {
        _isPresented = isPresented
        self.help = help
    }

    /// The label for the state of the inspector.
    static func label(isPresented: Bool) -> String {
        isPresented ? "Hide Inspector" : "Show Inspector"
    }

    /// The symbol of the toggle.
    static let symbol = "sidebar.trailing"

    /// The tooltip for the state of the inspector.
    static func helpText(isPresented: Bool, help: String?) -> String {
        help ?? label(isPresented: isPresented)
    }

    /// The toggle's content.
    public var body: some View {
        Button {
            isPresented.toggle()
        } label: {
            Label(Self.label(isPresented: isPresented), systemImage: Self.symbol)
                .labelStyle(.iconOnly)
        }
        .help(Self.helpText(isPresented: isPresented, help: help))

    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { InspectorLiveGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { InspectorLiveGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { InspectorLiveGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
