import StacklyClient
import SwiftUI

/// The New Instance sheet (spec 0020, *Sheet New Instance*): the category and the runtime,
/// the name, a port per endpoint, the settings the daemon's schema declares, "Start after
/// creating", the preflight, and the equivalent command. Create goes on only when the
/// preflight of this very draft is ready; the daemon decides everything it shows.
/// The sheet is built on ``StacklySheet``, so Return runs Create, Escape runs Cancel, and the
/// footer says why Create is off. While the create operation runs the form is read only and
/// the sheet shows its stages; Cancel then asks before it stops the operation.
/// The typed text of a secret lives in this view's `@State` and is cleared as soon as Create
/// is clicked, and when the sheet goes away. The model only learns the source of each
/// secret.
public struct NewInstanceSheet: View {
    /// The side margin the grouped `Form` adds inside its container, in points.
    static let groupedFormInset: CGFloat = 20

    @Environment(\.mutationAvailability) private var gate
    @Environment(\.dismiss) private var dismiss
    @Environment(\.shellNavigator) private var navigator

    @State private var drafts: [String: SecretDraft] = [:]

    private let model: CreateInstanceModel

    /// Creates the sheet over `model`.
    /// - Parameter model: The model the owner of the sheet keeps. Call
    ///   ``CreateInstanceModel/begin(_:)`` before presenting.
    public init(model: CreateInstanceModel) {
        self.init(model: model, initialDrafts: [:])
    }

    /// Creates the sheet with drafts already filled, for previews and snapshots of a secret
    /// that reads Copy from.
    init(model: CreateInstanceModel, initialDrafts: [String: SecretDraft]) {
        self.model = model
        _drafts = State(initialValue: initialDrafts)
    }

    /// The sheet's content.
    public var body: some View {
        StacklySheet(
            title: "New Instance",
            subtitle: model.duplicateSubtitle
                ?? "Its own data directory, port and secret. Nothing is shared with other instances.",
            tile: model.tile,
            primary: model.createAction(secrets: secretInputs, didSend: clearDrafts),
            cancel: model.cancelAction(gate: gate, close: { dismiss() }),
            minWidth: 560
        ) {
            content
        }

        .onDisappear { clearDrafts() }
        // A new form starts every secret on its own choice, so the old typed text goes.
        .onChange(of: model.schema.map(ObjectIdentifier.init)) { clearDrafts() }
    }

    // MARK: - Content

    @ViewBuilder private var content: some View {
        if model.hasNoInstalledRuntimes {
            StatePlaceholder(
                .noData(
                    symbol: "shippingbox", title: "No Runtimes",
                    message: "Install a runtime from the catalog to create instances.",
                    primaryAction: StateAction(title: "Browse Catalog", mutates: false) {
                        dismiss()
                        navigator?.show(.catalog)
                    }),
                size: .section)
        } else {
            VStack(alignment: .leading, spacing: Spacing.s16) {
                if let banner = model.banner {
                    StateBanner(
                        tone: banner.tone, title: banner.title, message: banner.message,
                        code: banner.code, base: .window
                    )

                }
                formArea
                switch model.phase {
                case .loadingForm:
                    DelayedProgress()
                        .frame(maxWidth: .infinity, minHeight: 60)

                case .formFailed(let failure):
                    StatePlaceholder(
                        failure, size: .section,
                        retry: { model.retryLoad() },
                        openDiagnostics: {
                            dismiss()
                            navigator?.show(.diagnostics)
                        })
                case .choosingRuntime:
                    Text("No installed runtimes in this category.")
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                case .editing, .submitting, .creating, .finished:
                    if model.isReadOnly {
                        CreateProgressSection(model: model)
                    }
                    PreflightSection(model: model)
                    EquivalentCommandRow(model: model)
                }
            }
        }
    }

    /// The controls: the chooser always, the rest once the form is read.
    private var formArea: some View {
        Form {
            if model.duplicateSource == nil || model.form != nil {
                Section {
                    if model.duplicateSource == nil {
                        runtimeChooser
                    } else if let label = model.duplicateRuntimeLabel {
                        LabeledContent("Runtime") {
                            Text(label)
                        }

                    }
                    if model.form != nil {
                        nameField
                    }
                }
            }
            if let schema = model.schema {
                if !model.endpoints.isEmpty {
                    Section("Ports") {
                        ForEach(model.endpoints) { endpoint in
                            EndpointPortRow(model: model, endpoint: endpoint)
                        }
                    }
                }
                SchemaFormSections(schema: schema, drafts: $drafts)
                Section {
                    Toggle(isOn: startAfterCreating) {
                        VStack(alignment: .leading, spacing: Spacing.s2) {
                            Text("Start after creating")
                            Text("Initialise the data directory, start, and wait for readiness.")
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                        }
                    }

                }
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
        .scrollDisabled(true)
        // The grouped form insets its cards by its own margin; the sheet's cards sit on the
        // sheet's margin, so the form gives that margin back to line the two up.
        .padding(.horizontal, -Self.groupedFormInset)
        .disabled(model.isReadOnly)
    }

    /// The category filter and the runtime picker of a create.
    @ViewBuilder private var runtimeChooser: some View {
        Picker("Category", selection: category) {
            ForEach(CreateCategory.allCases, id: \.self) { Text($0.title) }
        }
        .pickerStyle(.segmented)

        Picker("Runtime", selection: runtime) {
            ForEach(model.visibleRuntimes, id: \.id) { runtime in
                Text(CreateInstanceModel.label(of: runtime)).tag(UUID?.some(runtime.id))
            }
            if model.runtimeID == nil {
                Text("Choose a runtime").tag(UUID?.none)
            }
        }

    }

    private var nameField: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            TextField("Name", text: name)

                .autocorrectionDisabled()

            if let error = model.nameError {
                Label(error, systemImage: Tone.critical.symbol)
                    .font(.callout)
                    .foregroundStyle(Tone.critical.text)
            }
        }
    }

    // MARK: - Bindings and drafts

    private var category: Binding<CreateCategory> {
        Binding(get: { model.category }, set: { model.selectCategory($0) })
    }

    private var runtime: Binding<UUID?> {
        Binding(get: { model.runtimeID }, set: { model.selectRuntime($0) })
    }

    private var name: Binding<String> {
        Binding(get: { model.name }, set: { model.setName($0) })
    }

    private var startAfterCreating: Binding<Bool> {
        Binding(
            get: { model.startAfterCreating }, set: { model.setStartAfterCreating($0) })
    }

    /// The secret inputs of the drafts, built at the click.
    private func secretInputs() -> [SecretFieldInput] {
        guard let schema = model.schema else { return [] }
        return schema.rows.compactMap { row in
            guard case .secret(let limits) = row.control else { return nil }
            let draft = drafts[row.id] ?? SecretDraft()
            return draft.input(limits: limits).map { SecretFieldInput(field: row.id, input: $0) }
        }
    }

    private func clearDrafts() {
        for key in drafts.keys { drafts[key]?.clear() }
        drafts = [:]
    }
}
