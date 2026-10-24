import SwiftUI

/// A banner on a tone tint: the tone symbol, a title, an optional message and code block,
/// an optional list of items, and up to three native buttons.
/// Use it for an error, warning, or note about one item (a failed instance, a port conflict, config drift), next to that item, while the rest of the page stays as it is. ``StateView`` also uses it for stale and partial notes.
public struct StateBanner: View {
    /// How many items show before "and N more" with Show All.
    static let visibleItemLimit = 5
    /// The most actions a banner shows.
    static let actionLimit = 3

    private let symbolSize = ScaledSize.bannerSymbol.base
    @State private var showsAllItems = false

    private let tone: Tone
    private let title: String
    private let message: String?
    private let code: String?
    private let items: [PartialItem]
    private let actions: [StateAction]
    private let base: SurfaceToken

    /// Creates a banner.
    /// An action that mutates is disabled while     ``MutationAvailability`` is `.blocked`.   - base: The surface the tint is mixed onto: `.card` (default) or `.window`.
    public init(
        tone: Tone,
        title: String,
        message: String? = nil,
        code: String? = nil,
        items: [PartialItem] = [],
        actions: [StateAction] = [],
        base: SurfaceToken = .card
    ) {
        assert(actions.count <= Self.actionLimit, "a banner shows at most three actions")
        self.tone = tone
        self.title = title
        self.message = message
        self.code = code
        self.items = items
        self.actions = Array(actions.prefix(Self.actionLimit))
        self.base = base
    }

    /// The view's content.
    public var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
            Image(systemName: tone.symbol)
                .stacklySymbol(.bannerSymbol)
                .foregroundStyle(tone.fill)
                .frame(width: symbolSize)

            VStack(alignment: .leading, spacing: Spacing.s6) {
                Text(title)
                    .font(.body.weight(.semibold))
                    .fixedSize(horizontal: false, vertical: true)
                if let message {
                    Text(message)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let code {
                    CodeBlock(code: code)
                }
                if items.isEmpty == false {
                    itemList
                }
                if actions.isEmpty == false {
                    HStack(spacing: Spacing.s8) {
                        ForEach(actions, id: \.title) { action in
                            StateActionButton(action: action, prominence: .standard)
                        }
                    }
                    .padding(.top, Spacing.s2)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(
            EdgeInsets(
                top: Spacing.s12, leading: Spacing.s14, bottom: Spacing.s12, trailing: Spacing.s14)
        )
        .toneSurface(tone, on: base, strength: .banner)

    }

    private var itemList: some View {
        let split = Self.split(items, showsAll: showsAllItems)
        return VStack(alignment: .leading, spacing: Spacing.s4) {
            ForEach(split.shown) { item in
                BannerItemRow(item: item)
            }
            if split.hiddenCount > 0 {
                HStack(spacing: Spacing.s8) {
                    Text("and \(split.hiddenCount) more")
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                    Button {
                        showsAllItems = true
                    } label: {
                        Text("Show All")
                            .font(.callout.weight(.medium))
                            .frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.link)

                }
            }
        }
    }

    /// The items to show and how many stay hidden behind Show All: the first five until
    /// Show All is pressed, then all of them.
    static func split(
        _ items: [PartialItem],
        showsAll: Bool
    ) -> (shown: [PartialItem], hiddenCount: Int) {
        guard showsAll == false, items.count > visibleItemLimit else {
            return (items, 0)
        }
        return (Array(items.prefix(visibleItemLimit)), items.count - visibleItemLimit)
    }
}

/// One item of a banner: the name in semibold, the reason, and an optional action.
private struct BannerItemRow: View {
    let item: PartialItem

    var body: some View {
        let name = Text(item.name).fontWeight(.semibold)
        let reason = Text(item.reason).foregroundStyle(StacklyColor.textSecondary)
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            Text("\(name) · \(reason)")
                .font(.callout)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let action = item.action {
                StateActionButton(action: action, prominence: .standard)
                    .controlSize(.small)
            }
        }
    }
}

/// A code block: `.stacklyCode()` text on ``StacklyColor/surfaceConsole`` with
/// ``Radius/small`` corners.
private struct CodeBlock: View {
    let code: String

    var body: some View {
        Text(code)
            .stacklyCode()
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(
                EdgeInsets(
                    top: Spacing.s8, leading: Spacing.s12, bottom: Spacing.s8, trailing: Spacing.s12
                )
            )
            .background(
                StacklyColor.surfaceConsole,
                in: RoundedRectangle(cornerRadius: Radius.small.value)
            )
    }
}

/// How prominent a ``StateActionButton`` is.
enum StateActionProminence {
    /// `.borderedProminent`: the main action of a placeholder.
    case primary
    /// `.bordered`: every other action.
    case standard
}

/// A native button for a ``StateAction``: gated by ``MutationAvailability`` when the
/// action mutates, and disabled with a small spinner while it is busy. It is never the
/// default button.
struct StateActionButton: View {
    @Environment(\.mutationAvailability) private var availability

    let action: StateAction
    let prominence: StateActionProminence

    var body: some View {
        HStack(spacing: Spacing.s6) {
            button
                .disabled(action.isBusy)
                .disabled(reason: action.disabledReason(in: availability))
            if action.isBusy {
                ProgressView()
                    .controlSize(.small)
                    .transition(StacklyMotion.crossfade)
            }
        }
    }

    @ViewBuilder private var button: some View {
        let perform = action.perform
        switch prominence {
        case .primary:
            Button(action.title) { perform() }
                .buttonStyle(.borderedProminent)
        case .standard:
            Button(action.title) { perform() }
                .buttonStyle(.bordered)
        }
    }
}
