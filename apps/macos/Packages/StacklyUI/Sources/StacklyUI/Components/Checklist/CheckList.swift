import SwiftUI

/// The result of one check: the five statuses of the *Status checklist* table of spec 0067,
/// each with a tone, a symbol, and a word VoiceOver reads, so no status is only a color.
///
/// A screen maps its own result (a preflight check, a doctor result) to a status.
nonisolated public enum CheckStatus: Equatable, Sendable {
    /// The check passed.
    case passed
    /// The check passed with a warning.
    case warning
    /// The check failed.
    case failed
    /// The check is running.
    case pending
    /// A status this build does not know.
    case unrecognized

    /// The tone of the status.
    public var tone: Tone {
        switch self {
        case .passed: .positive
        case .warning: .caution
        case .failed: .critical
        case .pending: .neutral
        case .unrecognized: .caution
        }
    }

    /// The SF Symbol of the status, or `nil` for `pending`, which shows a spinner after a
    /// short delay.
    public var symbol: String? {
        switch self {
        case .passed: "checkmark.circle.fill"
        case .warning: "exclamationmark.triangle.fill"
        case .failed: "xmark.circle.fill"
        case .pending: nil
        case .unrecognized: "questionmark.diamond.fill"
        }
    }

    /// The word VoiceOver reads for the status.
    public var word: String {
        switch self {
        case .passed: "Passed"
        case .warning: "Warning"
        case .failed: "Failed"
        case .pending: "Checking"
        case .unrecognized: "Unrecognized status"
        }
    }
}

/// One row of a ``CheckList``.
public struct CheckItem: Identifiable {
    /// A stable key: the `<id>` of the identifier `checklist.row.<id>`.
    public var id: String
    /// The status.
    public var status: CheckStatus
    /// What was checked.
    public var title: String
    /// What the check found, shown in full, or `nil`.
    public var detail: String?
    /// An action for the row, such as choosing another folder, or `nil`.
    public var action: StacklyAction?

    /// Creates a row.
    ///
    /// - Parameters:
    ///   - id: A stable key.
    ///   - status: The status.
    ///   - title: What was checked.
    ///   - detail: What the check found, or `nil`.
    ///   - action: An action for the row, or `nil`.
    public init(
        id: String, status: CheckStatus, title: String, detail: String? = nil,
        action: StacklyAction? = nil
    ) {
        self.id = id
        self.status = status
        self.title = title
        self.detail = detail
        self.action = action
    }

    /// What VoiceOver reads for the row: "{title}, {status}: {detail}", without the detail
    /// when there is none.
    public var accessibilityLabel: String {
        guard let detail, !detail.isEmpty else { return "\(title), \(status.word)" }
        return "\(title), \(status.word): \(detail)"
    }
}

/// How a ``CheckList`` lays out its rows.
public enum CheckListLayout {
    /// A grid with columns at least 200 pt wide and 16 pt apart: two columns in a 560 pt
    /// sheet and one in a narrow width.
    case adaptive
    /// One column.
    case single
}

/// The rows of checks with a symbol, a title, and the detail in full (spec 0067, AC-13).
///
/// A screen wraps it in a ``CardSection``, for example titled "Preflight". The symbol
/// shape differs per status, so Differentiate Without Color changes nothing. A `pending`
/// row shows a small spinner only after 300 ms, so a quick check does not flash. VoiceOver
/// reads each row as one element and its action as its own.
public struct CheckList: View {
    private let items: [CheckItem]
    private let layout: CheckListLayout

    /// Creates a checklist.
    ///
    /// - Parameters:
    ///   - items: The rows.
    ///   - layout: How the rows are laid out; adaptive by default.
    public init(_ items: [CheckItem], layout: CheckListLayout = .adaptive) {
        self.items = items
        self.layout = layout
    }

    /// The checklist's content.
    public var body: some View {
        switch layout {
        case .adaptive:
            LazyVGrid(
                columns: [
                    GridItem(
                        .adaptive(minimum: Self.columnMinWidth), spacing: Spacing.s16,
                        alignment: .topLeading)
                ],
                alignment: .leading,
                spacing: Spacing.s8
            ) {
                rows
            }
        case .single:
            VStack(alignment: .leading, spacing: Spacing.s8) {
                rows
            }
        }
    }

    /// The narrowest a column of the adaptive layout gets: 200 pt.
    static let columnMinWidth: CGFloat = 200

    private var rows: some View {
        ForEach(items) { item in
            CheckRow(item: item)
        }
    }
}

/// One row: the symbol, the title, the detail, and the action.
private struct CheckRow: View {
    let item: CheckItem

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                CheckSymbol(status: item.status)
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(item.title)
                        .font(.callout)
                        .foregroundStyle(.primary)
                    if let detail = item.detail {
                        Text(detail)
                            .font(.subheadline)
                            .foregroundStyle(StacklyColor.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(item.accessibilityLabel)
            .accessibilityIdentifier(AccessibilityID.Checklist.row(item.id))
            if let action = item.action {
                ActionButton(action)
                    .controlSize(.small)
                    .padding(.leading, symbolWidth + Spacing.s8)
            }
        }
        .animation(StacklyMotion.crossfadeAnimation, value: item.status)
    }

    @ScaledMetric(relativeTo: .callout) private var symbolWidth: CGFloat = CheckSymbol.width
}

/// The symbol of a status, or a delayed spinner for `pending`, in a box of fixed width so
/// titles line up.
private struct CheckSymbol: View {
    /// The width of the symbol box at the default text size.
    static let width: CGFloat = 18

    @ScaledMetric(relativeTo: .callout) private var width: CGFloat = Self.width
    @State private var visibility = DelayedVisibility()

    let status: CheckStatus

    var body: some View {
        ZStack {
            if let symbol = status.symbol {
                Image(systemName: symbol)
                    .stacklySymbol(size: 15, relativeTo: .callout)
                    .foregroundStyle(status.tone.fill)
                    .transition(StacklyMotion.crossfade)
            } else if visibility.isVisible {
                ProgressView()
                    .controlSize(.small)
                    .transition(StacklyMotion.crossfade)
            }
        }
        .frame(width: width)
        .accessibilityHidden(true)
        .task(id: status) {
            if status == .pending {
                await visibility.run()
            }
        }
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { CheckListGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { CheckListGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { CheckListGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
