import StacklyUI
import SwiftUI

@main
struct StacklyApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
        }

        #if DEBUG
            // The design gallery of spec 0012, listed in the Window menu. Debug builds only.
            Window("Design Gallery", id: "design-gallery") {
                DesignGallery()
            }
            .defaultSize(width: 1100, height: 780)
        #endif
    }
}
