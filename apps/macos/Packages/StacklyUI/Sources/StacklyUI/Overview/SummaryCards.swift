import SwiftUI

/// One card of the Overview: a dot and a label, the number, and an optional note (spec 0022,
/// *Kartu ringkasan*). The whole card is a button that opens the Instances page with the
/// filter of the card.
struct SummaryCard: View {
    /// What a card counts.
    enum Kind: String {
        case running, stopped, failed
    }

    let kind: Kind
    /// The number, or `nil` while the store has not loaded: a small spinner stands there,
    /// never a zero.
    let count: Int?
    /// "{n} not ready" on the Running card.
    var note: String?
    let open: @MainActor () -> Void

    @ScaledMetric(.statusDot) private var dotSize

    var body: some View {
        Button(action: open) {
            VStack(alignment: .leading, spacing: Spacing.s4) {
                HStack(spacing: Spacing.s6) {
                    Circle().fill(dot).frame(width: dotSize, height: dotSize)
                        .accessibilityHidden(true)
                    Text(label)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
                numberView
                if let note {
                    Text(note)
                        .font(.subheadline)
                        .foregroundStyle(Tone.caution.text)
                } else {
                    // Keeps the cards the same height.
                    Text(" ").font(.subheadline).accessibilityHidden(true)
                }
            }
            .padding(Spacing.cardInsets)
            .frame(maxWidth: .infinity, alignment: .leading)
            .cardSurface(radius: .large)
            .contentShape(RoundedRectangle(cornerRadius: Radius.large.value))
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Self.spokenLabel(label: label, count: count, note: note))
        .accessibilityHint(hint)
        .accessibilityIdentifier(AccessibilityID.Overview.card(kind.rawValue))
    }

    @ViewBuilder private var numberView: some View {
        if let count {
            Text("\(count)")
                .font(.largeTitle.weight(.semibold).monospacedDigit())
                .foregroundStyle(
                    kind == .failed && count > 0
                        ? AnyShapeStyle(Tone.critical.text) : AnyShapeStyle(.primary))
        } else {
            SectionProgress(size: CGSize(width: 40, height: 40))
                .frame(maxWidth: 40, alignment: .leading)
        }
    }

    private var label: String {
        switch kind {
        case .running: "Running"
        case .stopped: "Stopped"
        case .failed: "Failed"
        }
    }

    private var dot: Color {
        switch kind {
        case .running: Tone.positive.fill
        case .stopped: Tone.neutral.fill
        case .failed: Tone.critical.fill
        }
    }

    private var hint: String {
        switch kind {
        case .running: "Shows running instances."
        case .stopped: "Shows stopped instances."
        case .failed: "Shows failed instances."
        }
    }

    /// "Running, 4, 2 not ready", or just the label while the number has not loaded.
    static func spokenLabel(label: String, count: Int?, note: String?) -> String {
        guard let count else { return "\(label), loading" }
        return [label, "\(count)", note].compactMap { $0 }.joined(separator: ", ")
    }
}

/// The cards of the Overview in an adaptive grid. The Runtimes card joins when the
/// `RuntimeStore` of feature 18 is wired (milestone 4 of spec 0022).
struct SummaryCards: View {
    let counts: InstanceCounts?
    let show: @MainActor (InstanceFilter) -> Void

    var body: some View {
        LazyVGrid(
            columns: [GridItem(.adaptive(minimum: 130), spacing: Spacing.s12)],
            spacing: Spacing.s12
        ) {
            SummaryCard(
                kind: .running, count: counts?.ready,
                note: counts.flatMap { $0.notReady > 0 ? "\($0.notReady) not ready" : nil },
                open: { show(.running) })
            SummaryCard(kind: .stopped, count: counts?.stopped, open: { show(.stopped) })
            SummaryCard(kind: .failed, count: counts?.failed, open: { show(.failed) })
        }
    }
}
