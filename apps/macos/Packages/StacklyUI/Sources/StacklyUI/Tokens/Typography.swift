import SwiftUI

// The symbol helper file: the only place in Stackly with a fixed font size. The source
// rule test (spec 0012 AC-2) allows it here and nowhere else.

extension View {
    /// Tabular digits for counts, ports, times, and sizes, so columns of numbers line up.
    /// The same as `monospacedDigit()`.
    public func stacklyNumeric() -> some View {
        monospacedDigit()
    }

    /// Code style for paths, endpoints, digests, and error codes: `.callout` monospaced,
    /// with text selection on so it can be copied.
    public func stacklyCode() -> some View {
        font(.callout.monospaced())
            .textSelection(.enabled)
    }
}

extension View {
    /// The font of the mark in a ``ProviderTile``: bold, `tileSize × 0.37` points, one
    /// line, shrinking down to half before it is cut. The only fixed font size besides
    /// the symbol helper below; a tile has a fixed size, so its mark does not scale.
    public func stacklyTileMark(tileSize: CGFloat) -> some View {
        font(.system(size: tileSize * 0.37, weight: .bold))
            .lineLimit(1)
            .minimumScaleFactor(0.5)
    }
}

extension Image {
    /// Sizes an SF Symbol at `size` points, scaled relative to `textStyle` with
    /// `@ScaledMetric`. Use it only for `Image(systemName:)` that needs its own size;
    /// other symbols take the text style around them.
    public func stacklySymbol(size: CGFloat, relativeTo textStyle: Font.TextStyle) -> some View {
        modifier(ScaledSymbolSize(size: size, textStyle: textStyle))
    }

    /// Sizes an SF Symbol from a named ``ScaledSize``, such as ``ScaledSize/bannerSymbol``.
    public func stacklySymbol(_ size: ScaledSize) -> some View {
        stacklySymbol(size: size.base, relativeTo: size.textStyle)
    }
}

private struct ScaledSymbolSize: ViewModifier {
    @ScaledMetric private var size: CGFloat

    init(size: CGFloat, textStyle: Font.TextStyle) {
        _size = ScaledMetric(wrappedValue: size, relativeTo: textStyle)
    }

    func body(content: Content) -> some View {
        content.font(.system(size: size))
    }
}
