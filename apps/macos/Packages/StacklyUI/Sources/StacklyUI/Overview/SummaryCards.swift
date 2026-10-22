import SwiftUI

/// One card of the Overview: a dot and a label, the number, and an optional note (spec 0022,
/// *Kartu ringkasan*). The whole card is a button that opens the Instances page with the
/// filter of the card.
struct SummaryCard: View {
    /// What a card counts.
    enum Kind: String {
        case running, stopped, failed, runtimes
    }

    let kind: Kind
    /// The number, or `nil` while the store has not loaded: a small spinner stands there,
    /// never a zero.
    let count: Int?
    /// "{n} not ready" on the Running card, "Couldn’t load" on a Runtimes card without data.
    var note: String?
    /// Whether the store behind the card failed without data: a dash stands for the number.
    var loadFailed = false
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
        if loadFailed {
            Text("–")
                .font(.largeTitle.weight(.semibold).monospacedDigit())
                .foregroundStyle(StacklyColor.textSecondary)
        } else if let count {
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
        case .runtimes: "Runtimes"
        }
    }

    private var dot: Color {
        switch kind {
        case .running: Tone.positive.fill
        case .stopped: Tone.neutral.fill
        case .failed: Tone.critical.fill
        case .runtimes: SectionStyle.runtimes.color
        }
    }

    private var hint: String {
        switch kind {
        case .running: "Shows running instances."
        case .stopped: "Shows stopped instances."
        case .failed: "Shows failed instances."
        case .runtimes: "Shows Runtimes."
        }
    }

    /// "Running, 4, 2 not ready", or just the label while the number has not loaded.
    static func spokenLabel(label: String, count: Int?, note: String?) -> String {
        guard let count else {
            return note.map { "\(label), \($0)" } ?? "\(label), loading"
        }
        return [label, "\(count)", note].compactMap { $0 }.joined(separator: ", ")
    }
}

/// What the Runtimes card shows (spec 0022, *Kartu ringkasan*).
enum RuntimesCardState: Equatable {
    /// The first snapshot has not arrived.
    case loading
    /// The number of runtimes, the same as the subtitle of the Runtimes page.
    case loaded(Int)
    /// The store failed and has no data.
    case failed

    /// The state of the card for the store, or `nil` when the card is not drawn: there is no
    /// store, or the daemon does not know `runtime.list`.
    static func of(_ runtimes: RuntimeStore?) -> RuntimesCardState? {
        guard let runtimes, runtimes.store.unsupported == false else { return nil }
        if let count = runtimes.loadedCount { return .loaded(count) }
        return runtimes.store.error == nil ? .loading : .failed
    }

    /// The words under the number.
    var note: String? {
        self == .failed ? "Couldn’t load" : nil
    }

    /// The number, or `nil` while it is not known.
    var count: Int? {
        if case .loaded(let count) = self { count } else { nil }
    }
}

/// The cards of the Overview in an adaptive grid. The Runtimes card joins when the
/// `RuntimeStore` is wired and the daemon lists runtimes (milestone 4 of spec 0022).
struct SummaryCards: View {
    let counts: InstanceCounts?
    var runtimes: RuntimesCardState?
    let show: @MainActor (InstanceFilter) -> Void
    var showRuntimes: @MainActor () -> Void = {}

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
            if let runtimes {
                SummaryCard(
                    kind: .runtimes, count: runtimes.count, note: runtimes.note,
                    loadFailed: runtimes == .failed, open: showRuntimes)
            }
        }
    }
}
