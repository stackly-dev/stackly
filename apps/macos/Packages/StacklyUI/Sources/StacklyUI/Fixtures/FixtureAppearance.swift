#if DEBUG
    import SwiftUI

    /// One appearance a fixture or snapshot renders in: the color scheme and the two
    /// accessibility settings the render can pin (spec 0068, AC-5).
    public struct FixtureAppearance: Sendable, Equatable {
        /// The color scheme.
        public var scheme: ColorScheme
        /// Whether Increase Contrast is on.
        public var increasedContrast: Bool
        /// Whether Reduce Transparency is on.
        public var reduceTransparency: Bool

        /// Creates an appearance.
        public init(
            scheme: ColorScheme = .light,
            increasedContrast: Bool = false,
            reduceTransparency: Bool = false
        ) {
            self.scheme = scheme
            self.increasedContrast = increasedContrast
            self.reduceTransparency = reduceTransparency
        }

        /// Light with the system defaults.
        public static let light = FixtureAppearance()
        /// Dark with the system defaults.
        public static let dark = FixtureAppearance(scheme: .dark)
        /// Light with Increase Contrast.
        public static let highContrastLight = FixtureAppearance(increasedContrast: true)
        /// Dark with Increase Contrast.
        public static let highContrastDark = FixtureAppearance(
            scheme: .dark, increasedContrast: true)
        /// Light with Reduce Transparency.
        public static let reducedTransparency = FixtureAppearance(reduceTransparency: true)
    }

    extension View {
        /// Pins the color scheme, Increase Contrast, and Reduce Transparency of
        /// `appearance`, and turns Differentiate Without Color and Reduce Motion off, so the
        /// result never depends on the settings of the Mac that renders it.
        ///
        /// This is the only place in the package that sets the underscored environment
        /// values, which Apple does not document; the guard test in `StacklyUITests` fails
        /// when one stops working in a new SDK (spec 0068, AC-5, AC-7).
        public func fixtureAppearance(_ appearance: FixtureAppearance) -> some View {
            self
                .environment(\.colorScheme, appearance.scheme)
                .environment(
                    \._colorSchemeContrast, appearance.increasedContrast ? .increased : .standard
                )
                .environment(\._accessibilityReduceTransparency, appearance.reduceTransparency)
                .environment(\._accessibilityDifferentiateWithoutColor, false)
                .environment(\._accessibilityReduceMotion, false)
        }
    }
#endif
