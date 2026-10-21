import SwiftUI

/// The four accessibility settings a snapshot pins, set through the underscored
/// environment setters that spec 0068 allows in tests (until `FixtureAppearance` takes
/// over in the next step).
struct ProbeAppearance {
    var scheme: ColorScheme = .light
    var increasedContrast = false
    var reduceTransparency = false
}

extension View {
    /// Pins the color scheme, contrast, and transparency of `appearance`, and turns
    /// Differentiate Without Color and Reduce Motion off.
    func probeAppearance(_ appearance: ProbeAppearance) -> some View {
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
