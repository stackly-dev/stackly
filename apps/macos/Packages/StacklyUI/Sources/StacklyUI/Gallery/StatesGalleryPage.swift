#if DEBUG
    import SwiftUI

    // provisional: M3
    /// The States page of the design gallery. Milestone 3 of spec 0012 fills it.
    struct StatesGalleryPage: View {
        var body: some View {
            GalleryPageLayout {
                GallerySection("States", note: "Filled by milestone 3.") {
                    EmptyView()
                }
            }
        }
    }

    #Preview("Light") {
        StatesGalleryPage().galleryPreview(.light)
    }

    #Preview("Dark") {
        StatesGalleryPage().galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        StatesGalleryPage().galleryPreview(.narrowWidth)
    }
#endif
