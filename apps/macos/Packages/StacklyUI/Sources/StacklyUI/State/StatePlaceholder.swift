import SwiftUI

/// What a page or section shows instead of content: empty, failed, or unsupported. A
/// symbol, a title, a message, an optional error code, and up to two native buttons,
/// centered.
/// ``StateView`` shows it at page size. Use `.section` inside a card for a section that has its own state.
public struct StatePlaceholder: View {
    /// The size of a placeholder.
    public enum Size: Sendable {
        /// A whole page: `.title2` title, 44 pt symbol, 80 pt vertical padding.
        case page
        /// A section inside a card: `.title3` title, 34 pt symbol, 34 pt vertical padding.
        case section

        /// The title font.
        var titleFont: Font {
            switch self {
            case .page: .title2.weight(.semibold)
            case .section: .title3.weight(.semibold)
            }
        }

        /// The symbol size.
        var symbolSize: ScaledSize {
            switch self {
            case .page: .placeholderPageSymbol
            case .section: .placeholderSectionSymbol
            }
        }

        /// The vertical padding in points.
        var verticalPadding: CGFloat {
            switch self {
            case .page: 80
            case .section: 34
            }
        }

        /// The horizontal padding in points.
        var horizontalPadding: CGFloat {
            switch self {
            case .page: Spacing.pageHorizontal
            case .section: Spacing.s16
            }
        }
    }

    /// The widest a message runs before it wraps, in points (mockup).
    static let messageMaxWidth: CGFloat = 380

    private let content: PlaceholderContent
    private let size: Size

    /// A placeholder for empty content: its symbol, title, message, and actions.
    /// - Parameters:
    ///   - empty: The empty content, from ``EmptyContent/noData(symbol:title:message:primaryAction:secondaryAction:)``
    ///     or ``EmptyContent/noResults(query:message:clearFilters:)``.
    ///   - size: `.page` (default) or `.section`.
    public init(_ empty: EmptyContent, size: Size = .page) {
        self.init(content: PlaceholderContent(empty: empty), size: size)
    }

    /// A placeholder for a failed first load: the critical symbol, the title, the daemon's
    /// message and error code, Retry, and Open Diagnostics.
    /// - Parameters:
    ///   - failure: The failure content.
    ///   - size: `.page` (default) or `.section`.
    ///   - retry: Loads again. It stays enabled while mutations are blocked.
    ///   - openDiagnostics: Opens Diagnostics. It stays enabled while mutations are
    ///     blocked.
    public init(
        _ failure: FailureContent,
        size: Size = .page,
        retry: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void
    ) {
        self.init(
            content: PlaceholderContent(
                failure: failure,
                retry: retry,
                openDiagnostics: openDiagnostics
            ),
            size: size
        )
    }

    /// A placeholder for a capability that is not supported: the `nosign` symbol, the
    /// title, and the reason, with no action for the capability.
    /// - Parameters:
    ///   - unsupported: The unsupported content.
    ///   - size: `.page` (default) or `.section`.
    public init(_ unsupported: UnsupportedContent, size: Size = .page) {
        self.init(content: PlaceholderContent(unsupported: unsupported), size: size)
    }

    init(content: PlaceholderContent, size: Size) {
        self.content = content
        self.size = size
    }

    /// The view's content.
    public var body: some View {
        VStack(spacing: Spacing.s6) {
            Image(systemName: content.symbol)
                .stacklySymbol(size.symbolSize)
                .foregroundStyle(content.symbolColor)

            Text(content.title)
                .font(size.titleFont)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, Spacing.s4)
            Text(content.message)
                .foregroundStyle(StacklyColor.textSecondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: Self.messageMaxWidth)
            if let code = content.code {
                Text(code)
                    .stacklyCode()
                    .foregroundStyle(StacklyColor.textSecondary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if content.primaryAction != nil || content.secondaryAction != nil {
                HStack(spacing: Spacing.s8) {
                    if let secondary = content.secondaryAction {
                        StateActionButton(action: secondary, prominence: .standard)
                    }
                    if let primary = content.primaryAction {
                        StateActionButton(action: primary, prominence: .primary)
                    }
                }
                .padding(.top, Spacing.s8)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, size.verticalPadding)
        .padding(.horizontal, size.horizontalPadding)

    }
}

/// The parts of a placeholder, built from empty, failure, or unsupported content.
struct PlaceholderContent {
    /// How the symbol is colored.
    enum SymbolStyle: Equatable {
        /// ``StacklyColor/decorative``: empty and unsupported.
        case decorative
        /// The critical tone fill: failed.
        case critical
    }

    let symbol: String
    let symbolStyle: SymbolStyle
    let title: String
    let message: String
    let code: String?
    /// The main action, drawn `.borderedProminent` after the secondary one.
    let primaryAction: StateAction?
    /// The other action, drawn `.bordered`.
    let secondaryAction: StateAction?

    init(empty: EmptyContent) {
        symbol = empty.symbol
        symbolStyle = .decorative
        title = empty.title
        message = empty.message
        code = nil
        primaryAction = empty.primaryAction
        secondaryAction = empty.secondaryAction
    }

    init(
        failure: FailureContent,
        retry: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void
    ) {
        symbol = Tone.critical.symbol
        symbolStyle = .critical
        title = failure.title
        message = failure.message
        code = failure.code
        primaryAction = StateAction(title: "Retry", mutates: false, perform: retry)
        secondaryAction = StateAction(
            title: "Open Diagnostics",
            mutates: false,
            perform: openDiagnostics
        )
    }

    init(unsupported: UnsupportedContent) {
        symbol = "nosign"
        symbolStyle = .decorative
        title = unsupported.title
        message = unsupported.reason
        code = nil
        primaryAction = nil
        secondaryAction = nil
    }

    /// The symbol color.
    var symbolColor: Color {
        switch symbolStyle {
        case .decorative: StacklyColor.decorative
        case .critical: Tone.critical.fill
        }
    }
}

/// A one sentence empty row inside a grouped list, such as "Nothing running.", in
/// ``StacklyColor/textSecondary`` with the list row insets.
public struct InlineEmptyRow: View {
    private let text: String

    /// Creates the row.
    /// - Parameter text: The sentence, such as "Nothing running.".
    public init(_ text: String) {
        self.text = text
    }

    /// The view's content.
    public var body: some View {
        Text(text)
            .foregroundStyle(StacklyColor.textSecondary)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(Spacing.rowInsets)
    }
}
