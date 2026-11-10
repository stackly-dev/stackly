import StacklyClient
import SwiftUI

/// What the buttons of a failure banner do (spec 0033, AC-10, AC-13, and AC-16), as
/// ``StacklyAction``s so the mutation gate and the busy state work as everywhere else.
@MainActor
struct FailureBannerActions {
    /// The instance.
    let summary: InstanceSummary
    /// The main action of the instance: Retry, Start, or a busy one.
    let main: StacklyAction
    /// Opens the Ports sheet, or `nil` where the window has none (then no port button shows).
    let edit: InstanceEditAction?
    /// The navigator of the window, or `nil` in a preview or a snapshot of a page alone.
    let navigator: ShellNavigator?
    /// Shows the Logs tab of the open detail page, or `nil` where there is none.
    let showLogs: (@MainActor () -> Void)?

    /// The action behind one button of the content, or `nil` when this place cannot offer it.
    /// - Parameter action: The button of the content.
    func stacklyAction(for action: FailureBannerContent.Action) -> StacklyAction? {
        switch action {
        case .retry:
            return retry
        case .usePort(let fix):
            return edit?.portAction(for: source, fix: fix)
        case .changePort:
            return edit?.portAction(for: source, fix: nil)
        case .showLogs:
            guard let showLogs else { return nil }
            return StacklyAction(
                id: "failure.showLogs", title: action.title, mutates: false, perform: showLogs)
        case .diagnose:
            return StacklyAction(
                id: "failure.diagnose", title: action.title, mutates: false,
                perform: { [navigator] in
                    // provisional: F34 (ShellNavigator.openDiagnostics(focus: .instance(id)) of spec 0034)
                    navigator?.show(.diagnostics)
                })
        }
    }

    /// Retry: the main action of spec 0022 under the banner's title. It sends one new
    /// `instance.start` with a new key, keeps the reason it is off, and shows "Starting…"
    /// while it runs (AC-13).
    var retry: StacklyAction {
        var retry = main
        retry.id = "failure.retry"
        retry.symbol = nil
        if retry.isBusy == false {
            retry.title = FailureBannerContent.Action.retry.title
        }
        return retry
    }

    /// Opens the failed operation in Operations (AC-16).
    /// - Parameter id: The operation.
    func showOperation(_ id: UUID) {
        // provisional: F35 (ShellNavigator.openOperation(_:) of spec 0035)
        navigator?.show(.operations)
    }

    private var source: InstanceEditSource {
        InstanceEditSource(id: summary.id, name: summary.name)
    }
}

/// The failure banner of an instance (spec 0033, *View*): the tone symbol, the title, the
/// sentences, the code block whose text can be selected, and the buttons, on the tone tint
/// of ``StateBanner``. The Summary tab shows all of it with Show in Operations beside the
/// code; the inspector shows the title, the excerpt, and Use Port.
struct InstanceFailureBanner: View {
    /// Where the banner stands.
    enum Mode {
        /// The Summary tab of the detail page, on the window surface.
        case summary
        /// The inspector of the Instances page, on the card surface.
        case inspector
    }

    private let symbolSize = ScaledSize.bannerSymbol.base

    let content: FailureBannerContent
    let mode: Mode
    let actions: FailureBannerActions

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
            Image(systemName: content.tone.symbol)
                .stacklySymbol(.bannerSymbol)
                .foregroundStyle(content.tone.fill)
                .frame(width: symbolSize)

            VStack(alignment: .leading, spacing: Spacing.s6) {
                Text(content.title)
                    .font(.body.weight(.semibold))
                    .fixedSize(horizontal: false, vertical: true)
                if mode == .summary {
                    Text(Self.sentences(of: content))
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let code = mode == .summary ? content.summaryCode : content.inspectorCode {
                    codeRow(code)
                }
                let buttons = self.buttons
                if buttons.isEmpty == false {
                    buttonRows(buttons)
                        .padding(.top, Spacing.s2)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(
            EdgeInsets(
                top: Spacing.s12, leading: Spacing.s14, bottom: Spacing.s12, trailing: Spacing.s14)
        )
        .toneSurface(content.tone, on: mode == .summary ? .window : .card, strength: .banner)
    }

    /// The sentence of the content, the hint of the daemon, and the note after a port change.
    static func sentences(of content: FailureBannerContent) -> String {
        [
            content.message, content.hint,
            content.portsChanged ? FailureBannerContent.portsChangedNote : nil,
        ]
        .compactMap { $0 }
        .joined(separator: " ")
    }

    /// The buttons this mode shows, in the order of the content; the first is the main one.
    var buttons: [StacklyAction] {
        let wanted = mode == .summary ? content.actions : content.inspectorActions
        return wanted.compactMap(actions.stacklyAction(for:))
    }

    // MARK: - Pieces

    private func codeRow(_ code: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            Text(code)
                .stacklyCode()
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(
                    EdgeInsets(
                        top: Spacing.s8, leading: Spacing.s12, bottom: Spacing.s8,
                        trailing: Spacing.s12)
                )
                .background(
                    StacklyColor.surfaceConsole,
                    in: RoundedRectangle(cornerRadius: Radius.small.value))
            if mode == .summary, let id = content.operationId {
                Button("Show in Operations") { actions.showOperation(id) }
                    .buttonStyle(.link)
                    .fixedSize()
            }
        }
    }

    /// One row of buttons when they fit, else two rows; a label is never cut.
    private func buttonRows(_ buttons: [StacklyAction]) -> some View {
        ViewThatFits(in: .horizontal) {
            row(buttons, startingAt: 0)
            VStack(alignment: .leading, spacing: Spacing.s8) {
                row(Array(buttons.prefix(2)), startingAt: 0)
                row(Array(buttons.dropFirst(2)), startingAt: 2)
            }
        }
    }

    private func row(_ buttons: [StacklyAction], startingAt offset: Int) -> some View {
        HStack(spacing: Spacing.s8) {
            ForEach(Array(buttons.enumerated()), id: \.element.id) { index, action in
                // The first button of the Summary tab is the main one.
                if mode == .summary, offset + index == 0 {
                    ActionButton(action, prominence: .primary, showsReason: false)
                        .fixedSize()
                } else {
                    ActionButton(action, prominence: .secondary, showsReason: false)
                        .fixedSize()
                }
            }
        }
    }
}
