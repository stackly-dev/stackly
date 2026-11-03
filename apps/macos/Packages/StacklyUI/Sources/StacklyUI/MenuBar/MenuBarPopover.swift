import StacklyClient
import SwiftUI

/// What the popover can ask of the windows of the app. The app builds it over the activation
/// policy controller, the window opener, and the destination queue; a test or a preview
/// builds one that records.
public struct MenuBarActions {
    /// Open Stackly: the main window comes to the front.
    public var openStackly: @MainActor () -> Void
    /// A click on a service name: the main window shows the detail of that instance.
    public var openInstance: @MainActor (UUID) -> Void
    /// "Show all {n} in Stackly…": the Instances page.
    public var showAllInstances: @MainActor () -> Void
    /// Runs before Settings… opens its window.
    public var prepareSettings: @MainActor () -> Void
    /// Close Window: closes the main window.
    public var closeWindow: @MainActor () -> Void
    /// Whether the main window is open, for Close Window.
    public var isMainWindowOpen: @MainActor () -> Bool
    /// Quit Stackly: closes the app, nothing else.
    public var quit: @MainActor () -> Void

    /// Creates the actions.
    public init(
        openStackly: @escaping @MainActor () -> Void,
        openInstance: @escaping @MainActor (UUID) -> Void,
        showAllInstances: @escaping @MainActor () -> Void,
        prepareSettings: @escaping @MainActor () -> Void,
        closeWindow: @escaping @MainActor () -> Void,
        isMainWindowOpen: @escaping @MainActor () -> Bool,
        quit: @escaping @MainActor () -> Void
    ) {
        self.openStackly = openStackly
        self.openInstance = openInstance
        self.showAllInstances = showAllInstances
        self.prepareSettings = prepareSettings
        self.closeWindow = closeWindow
        self.isMainWindowOpen = isMainWindowOpen
        self.quit = quit
    }
}

/// The content of the menu bar popover (spec 0032, *Popover menu bar*): the header, the
/// Services, the operations that run, the Stop All preview or result, and the actions. The
/// surface is the system popover material; the popover shows no shortcut of its own.
public struct MenuBarPopover: View {
    /// The width of the popover.
    static let width: CGFloat = 340

    private let model: MenuBarModel
    private let actions: MenuBarActions
    private let clearsFinishedStopAll: Bool
    @Environment(\.openSettings) private var openSettings
    @State private var mainWindowIsOpen = false

    /// Creates the popover.
    /// - Parameters:
    ///   - model: The menu bar model.
    ///   - actions: What the popover asks of the windows.
    public init(model: MenuBarModel, actions: MenuBarActions) {
        self.init(model: model, actions: actions, clearsFinishedStopAll: true)
    }

    /// Creates the popover. A snapshot passes `false` to draw a finished Stop All, which the
    /// popover otherwise drops when it opens again.
    init(model: MenuBarModel, actions: MenuBarActions, clearsFinishedStopAll: Bool) {
        self.model = model
        self.actions = actions
        self.clearsFinishedStopAll = clearsFinishedStopAll
    }

    /// The popover.
    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            Divider()
            services
            operations
            stopAll
            Divider()
            commands
        }
        .frame(width: Self.width)
        .onAppear {
            mainWindowIsOpen = actions.isMainWindowOpen()
            if clearsFinishedStopAll { model.clearFinishedStopAll() }
        }
    }

    // MARK: - Header

    private var header: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            Text(MenuBarCopy.title).font(.headline)
            HStack(spacing: Spacing.s8) {
                Text(model.statusText)
                Text("·")
                Text(MenuBarCopy.running(model.runningCount))
                Text("·")
                Text(MenuBarCopy.failed(model.failedCount))
            }
            .font(.callout)
            .foregroundStyle(StacklyColor.textSecondary)
        }
        .padding(Spacing.cardInsets)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    // MARK: - Services

    private var services: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Services").font(.subheadline.weight(.semibold))
                if model.isConnected == false {
                    Text(MenuBarCopy.lastKnown)
                        .font(.caption)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
            }
            .padding(
                EdgeInsets(
                    top: Spacing.s8, leading: Spacing.s16, bottom: Spacing.s4,
                    trailing: Spacing.s16))
            let rows = model.rows
            if rows.isEmpty {
                Text(MenuBarCopy.noInstances)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .padding(
                        EdgeInsets(
                            top: Spacing.s4, leading: Spacing.s16, bottom: Spacing.s12,
                            trailing: Spacing.s16))
            }
            ForEach(rows) { row in
                serviceRow(row)
            }
            if model.hiddenCount > 0 {
                Button(action: actions.showAllInstances) {
                    Text(MenuBarCopy.showAll(model.totalCount))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .foregroundStyle(Tone.informative.text)
                .padding(
                    EdgeInsets(
                        top: Spacing.s6, leading: Spacing.s16, bottom: Spacing.s8,
                        trailing: Spacing.s16))
            }
        }
    }

    private func serviceRow(_ row: MenuBarRow) -> some View {
        HStack(spacing: Spacing.s8) {
            Button {
                actions.openInstance(row.id)
            } label: {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(row.name)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    HStack(spacing: Spacing.s6) {
                        StatusLabel(row.appearance, context: .row)
                            .font(.caption)
                        Text(row.endpoint)
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            Button {
                model.perform(row)
            } label: {
                Image(systemName: row.button.kind == .start ? "play.fill" : "stop.fill")
                    .frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .disabled(row.button.isEnabled == false)
            .help(row.button.tooltip)
        }
        .padding(
            EdgeInsets(
                top: Spacing.s4, leading: Spacing.s16, bottom: Spacing.s4, trailing: Spacing.s8))
    }

    // MARK: - Operations

    @ViewBuilder private var operations: some View {
        let rows = model.operationRows
        if rows.isEmpty == false {
            Divider()
            VStack(alignment: .leading, spacing: Spacing.s8) {
                ForEach(rows) { operation in
                    VStack(alignment: .leading, spacing: Spacing.s4) {
                        HStack {
                            Text(operation.title).lineLimit(1)
                            Spacer(minLength: Spacing.s8)
                            if let percent = operation.percent {
                                Text("\(percent)%")
                                    .monospacedDigit()
                                    .foregroundStyle(StacklyColor.textSecondary)
                            }
                        }
                        .font(.callout)
                        if let percent = operation.percent {
                            ProgressView(value: Double(percent), total: 100)
                        } else {
                            ProgressView().controlSize(.small)
                            Text(MenuBarCopy.working)
                                .font(.caption)
                                .foregroundStyle(StacklyColor.textSecondary)
                        }
                    }
                }
            }
            .padding(Spacing.cardInsets)
        }
    }

    // MARK: - Stop All

    @ViewBuilder private var stopAll: some View {
        switch model.phase {
        case .idle:
            EmptyView()
        case .loadingPreview:
            panel {
                ProgressView().controlSize(.small)
            }
        case .preview(let preview, let changed):
            panel { previewContent(preview, changed: changed) }
        case .stopping(let preview):
            panel {
                Text(StopAllCopy.title).font(.headline)
                HStack(spacing: Spacing.s8) {
                    ProgressView().controlSize(.small)
                    Text("Stopping \(preview.items.count)…").font(.callout)
                }
            }
        case .result(let preview, let result):
            panel { resultContent(preview, result) }
        case .failed(let message):
            panel {
                Text(message).font(.callout)
                Button("Done") { model.dismissStopAll() }
            }
        }
    }

    private func panel<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            Divider()
            VStack(alignment: .leading, spacing: Spacing.s8) {
                content()
            }
            .padding(Spacing.cardInsets)
        }
    }

    @ViewBuilder private func previewContent(_ preview: StopAllPreview, changed: Bool)
        -> some View
    {
        Text(StopAllCopy.title).font(.headline)
        if changed {
            Text(StopAllCopy.changed).font(.callout).foregroundStyle(Tone.caution.text)
        }
        if preview.items.isEmpty {
            Text(StopAllCopy.nothingRunning).font(.callout)
        }
        ForEach(preview.items, id: \.instanceId) { item in
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text(item.name)
                let demand = StopAllCopy.demand(item.demand)
                if demand.isEmpty == false {
                    Text(demand).font(.caption).foregroundStyle(StacklyColor.textSecondary)
                }
            }
        }
        ForEach(preview.alreadyStopping, id: \.instanceId) { item in
            Text(StopAllCopy.alreadyStopping(item.name))
                .font(.callout).foregroundStyle(StacklyColor.textSecondary)
        }
        ForEach(preview.skipped, id: \.instanceId) { skip in
            Text(StopAllCopy.skipped(skip.name, code: skip.code))
                .font(.callout).foregroundStyle(StacklyColor.textSecondary)
        }
        if preview.items.isEmpty == false {
            Text(StopAllCopy.footer(preview))
                .font(.callout).foregroundStyle(StacklyColor.textSecondary)
        }
        HStack {
            Spacer()
            Button(StopAllCopy.confirm) { model.confirmStopAll() }
                .disabled(preview.items.isEmpty || model.isConnected == false)
            Button(StopAllCopy.cancel) { model.dismissStopAll() }
                // Cancel is the default button; the prominent style says so without a key.
                .buttonStyle(.borderedProminent)
        }
    }

    @ViewBuilder private func resultContent(_ preview: StopAllPreview, _ result: StopAllResult)
        -> some View
    {
        let outcome = StopAllCopy.outcome(of: result, preview: preview)
        Text(outcome.text).font(.callout)
        HStack {
            if outcome.opensStackly {
                Button(MenuBarCopy.openStackly) { actions.openStackly() }
            }
            Spacer()
            Button("Done") { model.dismissStopAll() }
        }
    }

    // MARK: - Actions

    private var commands: some View {
        VStack(alignment: .leading, spacing: 0) {
            item(MenuBarCopy.openStackly, action: actions.openStackly)
            item(
                MenuBarCopy.stopAllServices,
                disabled: model.canBeginStopAll == false,
                reason: model.stopAllDisabledReason, action: model.beginStopAll)
            item(MenuBarCopy.settings) {
                actions.prepareSettings()
                openSettings()
            }
            item(
                MenuBarCopy.closeWindow, disabled: mainWindowIsOpen == false,
                action: actions.closeWindow)
            Divider().padding(.vertical, Spacing.s4)
            item(MenuBarCopy.quit, action: actions.quit)
        }
        .padding(.vertical, Spacing.s4)
    }

    private func item(
        _ title: String, disabled: Bool = false, reason: String? = nil,
        action: @escaping @MainActor () -> Void
    ) -> some View {
        Button(action: action) {
            Text(title)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Spacing.s16)
                .padding(.vertical, Spacing.s6)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(disabled)
        .help(reason ?? "")
    }
}
