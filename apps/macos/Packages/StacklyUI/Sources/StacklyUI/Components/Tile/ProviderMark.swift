import AppKit
import StacklyClient

/// The mark of a provider tile when the catalog gives none (spec 0017).
nonisolated public enum ProviderMark {
    /// The first two ASCII letters or digits of `name`, the first in upper case and the
    /// second in lower case ("PostgreSQL" gives "Po", "9front" gives "9f"), or "?" when
    /// `name` is `nil` or has none.
    public static func defaultMark(for name: String?) -> String {
        let characters = (name ?? "").filter { $0.isASCII && ($0.isLetter || $0.isNumber) }
        guard let first = characters.first else { return "?" }
        let second = characters.dropFirst().first
        return first.uppercased() + (second?.lowercased() ?? "")
    }
}

/// The fill of a tile by ``TileColor``: a system color, so macOS supplies the Increase
/// Contrast variant (spec 0067, *Warna tile*).
nonisolated enum TileFill {
    /// The system color of `color`; gray for `nil`, `.gray`, and `.unrecognized`.
    static func color(for color: TileColor?) -> NSColor {
        switch color {
        case .blue: .systemBlue
        case .cyan: .systemCyan
        case .teal: .systemTeal
        case .green: .systemGreen
        case .yellow: .systemYellow
        case .orange: .systemOrange
        case .red: .systemRed
        case .brown: .systemBrown
        case .indigo: .systemIndigo
        case .gray, .unrecognized, nil: .systemGray
        }
    }
}
