import AppKit
import SwiftUI

/// The symbol and system color of each top level section of the app.
nonisolated public enum SectionStyle: CaseIterable, Hashable, Sendable {
    /// Overview.
    case overview
    /// Instances.
    case instances
    /// Environments.
    case environments
    /// Catalog.
    case catalog
    /// Runtimes.
    case runtimes
    /// Operations.
    case operations
    /// Diagnostics.
    case diagnostics

    /// The SF Symbol of the section.
    public var symbol: String {
        switch self {
        case .overview: "square.grid.2x2"
        case .instances: "server.rack"
        case .environments: "square.3.layers.3d"
        case .catalog: "book"
        case .runtimes: "shippingbox"
        case .operations: "arrow.triangle.2.circlepath"
        case .diagnostics: "waveform.path.ecg"
        }
    }

    /// The color of the section symbol, a system color.
    public var color: Color {
        Color(nsColor: systemColor)
    }

    /// The system color behind ``color``.
    var systemColor: NSColor {
        switch self {
        case .overview: .systemBlue
        case .instances: .systemGreen
        case .environments: .systemPurple
        case .catalog: .systemOrange
        case .runtimes: .systemTeal
        case .operations: .systemIndigo
        case .diagnostics: .systemPink
        }
    }
}

/// The icon of a section: its symbol in its color, taking the font around it.
///
/// It is decorative and hidden from VoiceOver, because the section label next to it
/// carries the meaning. The app shell (spec 0013) places it in the sidebar.
public struct SectionIcon: View {
    private let section: SectionStyle

    /// Creates a section icon.
    ///
    /// - Parameter section: The section to draw.
    public init(_ section: SectionStyle) {
        self.section = section
    }

    /// The icon's content.
    public var body: some View {
        Image(systemName: section.symbol)
            .foregroundStyle(section.color)
            .accessibilityHidden(true)
    }
}
