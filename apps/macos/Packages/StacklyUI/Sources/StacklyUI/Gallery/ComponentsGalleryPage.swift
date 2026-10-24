#if DEBUG
    import SwiftUI

    /// The Components page of the design gallery: every state of every shared component
    /// (spec 0067, *State di galeri*), and the list feature 68 renders as snapshots.
    /// Each milestone owns the sections of its own file, so the page itself stays the same.
    struct ComponentsGalleryPage: View {
        var body: some View {
            GalleryPageLayout {
                BaseComponentsSections()
                SheetComponentsSections()
                InspectorComponentsSections()
                LogComponentsSections()
            }
        }
    }

    #Preview("Light") {
        ComponentsGalleryPage().galleryPreview(.light)
    }

    #Preview("Dark") {
        ComponentsGalleryPage().galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        ComponentsGalleryPage().galleryPreview(.narrowWidth)
    }
#endif
