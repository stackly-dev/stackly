#if DEBUG
    import SwiftUI

    /// The color scheme used by a fixture or snapshot.
    public struct FixtureAppearance: Sendable, Equatable {
        /// The color scheme.
        public var scheme: ColorScheme

        /// Creates an appearance.
        public init(scheme: ColorScheme = .light) { self.scheme = scheme }

        /// Light appearance.
        public static let light = FixtureAppearance()
        /// Dark appearance.
        public static let dark = FixtureAppearance(scheme: .dark)
    }

    extension View {
        /// Applies the fixture's color scheme.
        public func fixtureAppearance(_ appearance: FixtureAppearance) -> some View {
            environment(\.colorScheme, appearance.scheme)
        }
    }
#endif
