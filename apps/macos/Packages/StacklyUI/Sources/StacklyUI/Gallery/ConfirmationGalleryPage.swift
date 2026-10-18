#if DEBUG
    import SwiftUI

    // provisional: M4
    /// The Confirmation page of the design gallery. Milestone 4 of spec 0012 fills it.
    struct ConfirmationGalleryPage: View {
        var body: some View {
            GalleryPageLayout {
                GallerySection("Confirmation", note: "Filled by milestone 4.") {
                    EmptyView()
                }
            }
        }
    }

    #Preview("Light") {
        ConfirmationGalleryPage().galleryPreview(.light)
    }

    #Preview("Dark") {
        ConfirmationGalleryPage().galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        ConfirmationGalleryPage().galleryPreview(.narrowWidth)
    }
#endif
