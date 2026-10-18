import SwiftUI

extension View {
    /// Sets the page's title and subtitle in the window toolbar.
    ///
    /// Every page of the main window calls it once and adds its own toolbar actions with
    /// `.toolbar`. An action that changes data through the daemon also takes
    /// `.mutationGated()`.
    ///
    /// - Parameters:
    ///   - title: The page title, such as "Instances".
    ///   - subtitle: The subtitle, such as "3 instances", or `nil` for none.
    public func pageChrome(title: String, subtitle: String? = nil) -> some View {
        modifier(PageChrome(title: title, subtitle: subtitle))
    }
}

private struct PageChrome: ViewModifier {
    let title: String
    let subtitle: String?

    func body(content: Content) -> some View {
        if let subtitle {
            content
                .navigationTitle(title)
                .navigationSubtitle(subtitle)
        } else {
            content
                .navigationTitle(title)
        }
    }
}
