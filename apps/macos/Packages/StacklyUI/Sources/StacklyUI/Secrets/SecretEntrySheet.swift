import StacklyClient
import SwiftUI

/// The sheet that fills the references an action needs (spec 0011, *GUI*): start, env up, or
/// restore stopped with `secret.needs_secret`.
///
/// Each reference gets a row with the reason it cannot be read, a ``SecretField``, and a Save
/// button that sends `secret.set` in mode `create`. Generate is offered only for a reference
/// whose `generateAllowed` is true. When a value exists already, the row offers Replace. When
/// a replace changes only the stored copy, the sheet asks first: the password inside the
/// database does not change. Once every row is saved, the main button repeats the action
/// that stopped.
///
/// The typed values live in this view's `@State`, are cleared once sent, and are cleared when
/// the sheet goes away.
public struct SecretEntrySheet: View {
    private let actionTitle: String
    private let copyFromOptions: [String]
    private let onContinue: @MainActor () -> Void
    private let onCancel: @MainActor () -> Void

    @State private var model: SecretEntryModel
    @State private var drafts: [String: SecretDraft]
    @State private var confirmation: ConfirmationRequest?

    /// Creates the sheet.
    ///
    /// - Parameters:
    ///   - needs: The references to fill, from ``SecretNeed/needs(in:)`` or a restore
    ///     preflight.
    ///   - client: The secret methods.
    ///   - actionTitle: The verb of the action to repeat once every row is saved, such as
    ///     “Start Instance”.
    ///   - copyFromOptions: The references Copy from offers, or empty to hide it.
    ///   - onContinue: Repeats the action that stopped.
    ///   - onCancel: Closes the sheet without repeating it.
    public init(
        needs: [SecretNeed],
        client: SecretsClient,
        actionTitle: String,
        copyFromOptions: [String] = [],
        onContinue: @escaping @MainActor () -> Void,
        onCancel: @escaping @MainActor () -> Void
    ) {
        self.actionTitle = actionTitle
        self.copyFromOptions = copyFromOptions
        self.onContinue = onContinue
        self.onCancel = onCancel
        _model = State(initialValue: SecretEntryModel(needs: needs, client: client))
        _drafts = State(
            initialValue: Dictionary(
                uniqueKeysWithValues: needs.map {
                    ($0.reference, SecretDraft(allowsGenerate: $0.generateAllowed))
                }))
    }

    /// The view's content.
    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.sectionGap) {
            VStack(alignment: .leading, spacing: Spacing.s4) {
                Text("Secrets needed")
                    .font(.title2.bold())
                    .accessibilityAddTraits(.isHeader)
                Text(
                    "Fill these before you continue. Values go to your Keychain and are never shown again unless you ask."
                )
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.s16) {
                    ForEach(model.rows) { row in
                        SecretEntryRow(
                            row: row,
                            draft: binding(for: row.id),
                            copyFromOptions: copyFromOptions.filter { $0 != row.id },
                            save: { send(row.id) { await model.save(reference: $0, draft: $1) } },
                            replace: {
                                send(row.id) { await model.replace(reference: $0, draft: $1) }
                            })
                    }
                }
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) {
                    clearDrafts()
                    onCancel()
                }
                .keyboardShortcut(.cancelAction)
                Button(actionTitle) {
                    clearDrafts()
                    onContinue()
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(!model.allSaved)
                .mutationGated()
            }
        }
        .padding(Spacing.pageHorizontal)
        .frame(minWidth: 460, idealWidth: 560, minHeight: 240)
        .stacklyConfirmation(item: $confirmation)
        .onChange(of: model.pendingConfirmation?.reference) {
            presentConfirmation()
        }
        .onDisappear { clearDrafts() }
    }

    private func binding(for reference: String) -> Binding<SecretDraft> {
        Binding(
            get: { drafts[reference] ?? SecretDraft() },
            set: { drafts[reference] = $0 })
    }

    /// Sends the row's draft, then forgets the typed value of a row that is saved.
    private func send(
        _ reference: String,
        _ action: @escaping @MainActor (String, SecretDraft) async -> Void
    ) {
        guard let draft = drafts[reference] else { return }
        Task {
            await action(reference, draft)
            if let row = model.rows.first(where: { $0.id == reference }),
                case .saved = row.state
            {
                drafts[reference]?.clear()
            }
        }
    }

    private func presentConfirmation() {
        guard let pending = model.pendingConfirmation else { return }
        let reference = pending.reference
        confirmation = ConfirmationRequest(
            title: "Replace the stored password?",
            message:
                "This changes only the copy in your Keychain. The password inside the database stays as it is, so the two will not match until you change it there too.",
            actionTitle: "Replace",
            role: .standard,
            perform: {
                guard let draft = drafts[reference] else { return }
                Task {
                    await model.confirmStoredOnly(reference: reference, draft: draft)
                    if let row = model.rows.first(where: { $0.id == reference }),
                        case .saved = row.state
                    {
                        drafts[reference]?.clear()
                    }
                }
            })
    }

    private func clearDrafts() {
        for key in drafts.keys { drafts[key]?.clear() }
    }
}

/// One row of the sheet: the reference, why it is needed, the field, and the way to save it.
struct SecretEntryRow: View {
    let row: SecretEntryModel.Row
    @Binding var draft: SecretDraft
    let copyFromOptions: [String]
    let save: @MainActor () -> Void
    let replace: @MainActor () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            Text(row.need.reference)
                .stacklyCode()
                .accessibilityLabel("Secret \(row.need.reference)")
            Text(row.need.reasonText)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
            if case .saved = row.state {
                savedNote
            } else {
                SecretField(
                    row.need.reference, draft: $draft, limits: row.need.limits,
                    allowsGenerate: row.need.generateAllowed, copyFromOptions: copyFromOptions)
                actions
            }
        }
        .padding(Spacing.cardInsets)
        .background(StacklyColor.surfaceCard, in: .rect(cornerRadius: Radius.medium.value))
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder private var savedNote: some View {
        if case .saved(let warnings) = row.state {
            Label("Saved", systemImage: "checkmark.circle.fill")
                .foregroundStyle(Tone.positive.text)
            if warnings.contains(.shortValueNotScrubbed) {
                Text("This value is shorter than 8 bytes, so log redaction can’t match it in text.")
                    .font(.callout)
                    .foregroundStyle(Tone.caution.text)
            }
        }
    }

    @ViewBuilder private var actions: some View {
        HStack(spacing: Spacing.s8) {
            switch row.state {
            case .exists, .needsConfirmation:
                Text("A value is stored already.")
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                Button("Replace") { replace() }
                    .disabled(!draft.canSubmit(limits: row.need.limits))
                    .mutationGated()
            case .saving:
                ProgressView().controlSize(.small)
                Text("Saving…").font(.callout)
            default:
                Button("Save") { save() }
                    .disabled(!draft.canSubmit(limits: row.need.limits))
                    .mutationGated()
            }
        }
        if case .failed(let message) = row.state {
            Text(message)
                .font(.callout)
                .foregroundStyle(Tone.critical.text)
        }
    }
}
