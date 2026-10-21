import StacklyClient
import SwiftUI

/// The capabilities of a provider (spec 0017, *Urutan capability*): the three baseline
/// rows every provider has, then the eight capabilities in the daemon's order. A
/// capability the provider lacks reads "{label} · not supported".
///
/// The mark in front of a row is decoration; the words carry the meaning, and VoiceOver
/// reads "{label}, supported" or "{label}, not supported".
struct CapabilityList: View {
    /// One row of the list.
    struct Row: Identifiable, Equatable {
        let label: String
        let supported: Bool

        var id: String { label }

        /// What the row writes.
        var text: String {
            supported ? label : "\(label) · not supported"
        }

        /// What VoiceOver reads.
        var spokenText: String {
            "\(label), \(supported ? "supported" : "not supported")"
        }
    }

    let capabilities: [CapabilityStatus]

    /// The rows in order: the baseline, then the capabilities.
    static func rows(for capabilities: [CapabilityStatus]) -> [Row] {
        CatalogCopy.baselineCapabilities.map { Row(label: $0, supported: true) }
            + capabilities.map {
                Row(label: CatalogCopy.capabilityLabel($0.capability), supported: $0.supported)
            }
    }

    var body: some View {
        ForEach(Self.rows(for: capabilities)) { row in
            HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                Image(systemName: row.supported ? "checkmark" : "minus")
                    .foregroundStyle(StacklyColor.decorative)
                    .frame(width: 14)
                    .accessibilityHidden(true)
                Text(row.text)
                    .font(.body)
                    .fixedSize(horizontal: false, vertical: true)
                    .foregroundStyle(
                        row.supported
                            ? AnyShapeStyle(.primary)
                            : AnyShapeStyle(StacklyColor.textSecondary))
                Spacer(minLength: 0)
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(row.spokenText)
        }
    }
}
