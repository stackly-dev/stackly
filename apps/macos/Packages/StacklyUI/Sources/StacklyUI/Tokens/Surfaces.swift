import AppKit
import SwiftUI

/// How strongly a tone tints its surface.
nonisolated public enum ToneStrength: Sendable, CaseIterable {
    /// A 16% tint, for badges. Its default shape is a capsule.
    case badge
    /// A 12% tint, for banners. Its default shape is a rounded rectangle with
    /// ``Radius/medium`` corners.
    case banner

    /// The share of the tone fill in the mix.
    var fraction: Double {
        switch self {
        case .badge: 0.16
        case .banner: 0.12
        }
    }
}

/// The opaque background of a tone surface: the base token with the tone fill mixed in.
nonisolated struct ToneSurfaceStyle: ShapeStyle {
    let tone: Tone
    let base: SurfaceToken
    let strength: ToneStrength

    func resolve(in environment: EnvironmentValues) -> Color.Resolved {
        let baseColor = base.color.components(
            scheme: environment.colorScheme,
            contrast: environment.colorSchemeContrast
        )
        let fill = SRGBColor(tone.fill.resolve(in: environment))
        return baseColor.mixed(with: fill, fraction: strength.fraction).resolved
    }
}

extension View {
    /// Draws an opaque tone tint behind the view: the `base` surface with the tone fill
    /// mixed in at 16% (`.badge`, in a capsule) or 12% (`.banner`, in a rounded
    /// rectangle with ``Radius/medium`` corners). While Increase Contrast is on, it adds
    /// a 1 pt border in the tone fill.
    ///
    /// A tone surface never sits on glass or on a system background, and only text of
    /// the same tone, `.primary`, or ``StacklyColor/textSecondary`` goes on it.
    public func toneSurface(
        _ tone: Tone,
        on base: SurfaceToken = .card,
        strength: ToneStrength
    ) -> some View {
        modifier(DefaultToneSurfaceModifier(tone: tone, base: base, strength: strength))
    }

    /// Draws an opaque tone tint behind the view in `shape`, with the same colors and
    /// Increase Contrast border as ``toneSurface(_:on:strength:)``.
    public func toneSurface<S: InsettableShape>(
        _ tone: Tone,
        on base: SurfaceToken = .card,
        strength: ToneStrength,
        in shape: S
    ) -> some View {
        modifier(ToneSurfaceModifier(tone: tone, base: base, strength: strength, shape: shape))
    }

    /// Draws a custom card behind the view: ``StacklyColor/surfaceCard`` with `radius`
    /// corners and a hairline `separatorColor` border that becomes 1 pt while Increase
    /// Contrast is on. It adds no padding; use ``Spacing/cardInsets`` for the content.
    public func cardSurface(radius: Radius = .medium) -> some View {
        modifier(CardSurfaceModifier(radius: radius))
    }
}

private struct DefaultToneSurfaceModifier: ViewModifier {
    let tone: Tone
    let base: SurfaceToken
    let strength: ToneStrength

    func body(content: Content) -> some View {
        switch strength {
        case .badge:
            content.toneSurface(tone, on: base, strength: strength, in: Capsule())
        case .banner:
            content.toneSurface(
                tone,
                on: base,
                strength: strength,
                in: RoundedRectangle(cornerRadius: Radius.medium.value)
            )
        }
    }
}

private struct ToneSurfaceModifier<S: InsettableShape>: ViewModifier {
    @Environment(\.colorSchemeContrast) private var contrast

    let tone: Tone
    let base: SurfaceToken
    let strength: ToneStrength
    let shape: S

    func body(content: Content) -> some View {
        content
            .background(shape.fill(ToneSurfaceStyle(tone: tone, base: base, strength: strength)))
            .overlay {
                if contrast == .increased {
                    shape.strokeBorder(tone.fill, lineWidth: 1)
                }
            }
    }
}

private struct CardSurfaceModifier: ViewModifier {
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.displayScale) private var displayScale

    let radius: Radius

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius.value)
        content
            .background(shape.fill(StacklyColor.surfaceCard))
            .overlay {
                shape.strokeBorder(
                    Color(nsColor: .separatorColor),
                    lineWidth: contrast == .increased ? 1 : 1 / displayScale
                )
            }
    }
}
