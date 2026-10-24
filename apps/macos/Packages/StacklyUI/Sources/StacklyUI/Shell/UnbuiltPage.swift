import SwiftUI

/// The neutral placeholder of a page whose feature is not built yet: the section symbol,
/// the section title, and "Coming in a later build.", with no action and no subtitle.
/// The feature that builds the page replaces it.
public struct UnbuiltPage: View {
    /// The message of every unbuilt page.
    static let message = "Coming in a later build."

    private let section: SidebarSection

    /// Creates the placeholder.
    /// - Parameter section: The section whose page is not built yet.
    public init(section: SidebarSection) {
        self.section = section
    }

    /// The placeholder's content.
    public var body: some View {
        StatePlaceholder(
            EmptyContent.noData(
                symbol: section.style.symbol,
                title: section.title,
                message: Self.message
            ),
            size: .page
        )
    }
}
