#if DEBUG
    import SwiftUI

    // provisional: M2
    /// The Status page of the design gallery. Milestone 2 of spec 0012 fills it.
    struct StatusGalleryPage: View {
        var body: some View {
            GalleryPageLayout {
                GallerySection("Status", note: "Filled by milestone 2.") {
                    EmptyView()
                }
            }
        }
    }

    #Preview("Light") {
        StatusGalleryPage().galleryPreview(.light)
    }

    #Preview("Dark") {
        StatusGalleryPage().galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        StatusGalleryPage().galleryPreview(.narrowWidth)
    }
#endif
