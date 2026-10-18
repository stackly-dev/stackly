import SwiftUI

/// The content of the Settings scene until feature 32 fills it: one General tab that says
/// it comes in a later build.
public struct SettingsPlaceholder: View {
    // provisional: F32 (feature 32 fills Settings)

    /// The size of the Settings window.
    static let size = CGSize(width: 480, height: 200)

    /// Creates the placeholder.
    public init() {}

    /// The placeholder's content.
    public var body: some View {
        TabView {
            Tab("General", systemImage: "gearshape") {
                Text(UnbuiltPage.message)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .padding(Spacing.s22)
            }
        }
        .frame(width: Self.size.width, height: Self.size.height)
    }
}
