import AppKit
import SwiftUI

/// One of the five fixed meanings a status, badge, or banner can carry.
///
/// A tone is never shown by color alone: it always comes with a label, a symbol, or both.
/// Tone text sits only on a plain surface from the contrast table or on a tint of the
/// same tone, never on another tone's tint.
nonisolated public enum Tone: CaseIterable, Sendable {
    /// Working as expected: running and ready, verified, tested.
    case positive
    /// In transition or needing attention without having failed: starting, recovering,
    /// unknown, drift, partial, stale, experimental.
    case caution
    /// Failed or blocking: failed, error, conflict.
    case critical
    /// Idle or plain information: stopped, retained, done, cancelled, not tested.
    case neutral
    /// Being worked on at the user's request: a running operation, install progress.
    case informative

    /// The fill for dots, icons, and the tint base. It is a system color, so macOS
    /// supplies its Increase Contrast variant. A fill is never text.
    public var fill: Color {
        Color(nsColor: systemFill)
    }

    /// The system color behind ``fill``.
    var systemFill: NSColor {
        switch self {
        case .positive: .systemGreen
        case .caution: .systemOrange
        case .critical: .systemRed
        case .neutral: .systemGray
        case .informative: .systemBlue
        }
    }

    /// The text color of the tone: AA on every plain surface and on its own tint, and
    /// 7:1 on plain surfaces under Increase Contrast.
    public var text: ColorToken {
        switch self {
        case .positive:
            ColorToken(
                light: 0x18712D,
                dark: 0x4ADE74,
                highContrastLight: 0x135A24,
                highContrastDark: 0x4ADE74
            )
        case .caution:
            ColorToken(
                light: 0x965000,
                dark: 0xFFB340,
                highContrastLight: 0x774000,
                highContrastDark: 0xFFB340
            )
        case .critical:
            ColorToken(
                light: 0xB8160C,
                dark: 0xFF867F,
                highContrastLight: 0x9D120A,
                highContrastDark: 0xFFAAA5
            )
        case .neutral:
            StacklyColor.textSecondary
        case .informative:
            ColorToken(
                light: 0x0759BA,
                dark: 0x62AEFF,
                highContrastLight: 0x064C9E,
                highContrastDark: 0x8EC5FF
            )
        }
    }

    /// The SF Symbol of the tone, shown on every banner and in front of a badge label
    /// while Differentiate Without Color is on.
    public var symbol: String {
        switch self {
        case .positive: "checkmark.circle.fill"
        case .caution: "exclamationmark.triangle.fill"
        case .critical: "xmark.circle.fill"
        case .neutral: "info.circle.fill"
        case .informative: "ellipsis.circle.fill"
        }
    }
}
