import SwiftUI

/// The input for one secret field: Generate (when the field allows it), Enter, or Copy
/// from another reference (spec 0011, *GUI*).
///
/// Enter uses a native `SecureField`, with a button to show the typed text for a moment. The
/// field takes the field's ``SecretLimits`` and tells the person what is wrong; the sheet or
/// form that owns the draft keeps its main button off until
/// ``SecretDraft/canSubmit(limits:)`` is true. The daemon checks the same limits again.
///
/// The typed value lives in the `draft` binding, which the owning view keeps in `@State`. The
/// field adds no state of its own beyond whether the typed text is shown, and it hides the
/// text again when the view goes away.
public struct SecretField: View {
    private let title: String
    private let limits: SecretLimits
    private let allowsGenerate: Bool
    private let copyFromOptions: [String]
    private let copyFromLabels: [String: String]
    @Binding private var draft: SecretDraft

    @State private var showsTypedText = false
    @State private var hasEdited = false

    /// Creates a secret field.
    ///
    /// - Parameters:
    ///   - title: The field's name, such as “Admin password”. VoiceOver reads it.
    ///   - draft: What the person chose and typed. Keep it in `@State`.
    ///   - limits: The field's `minLength`, `maxLength`, and `printableOnly`.
    ///   - allowsGenerate: Whether Generate is offered. A field that cannot take a generated
    ///     value passes `false`.
    ///   - copyFromOptions: The references Copy from offers; Copy from is hidden when empty.
    ///   - copyFromLabels: The words to show for a reference that has some, such as “Admin
    ///     password of “pg17-dev””. A reference without a label shows as its own text.
    public init(
        _ title: String,
        draft: Binding<SecretDraft>,
        limits: SecretLimits = .base,
        allowsGenerate: Bool = true,
        copyFromOptions: [String] = [],
        copyFromLabels: [String: String] = [:]
    ) {
        self.title = title
        self._draft = draft
        self.limits = limits
        self.allowsGenerate = allowsGenerate
        self.copyFromOptions = copyFromOptions
        self.copyFromLabels = copyFromLabels
    }

    /// The choices on offer, in order.
    var sources: [SecretDraftSource] {
        SecretDraftSource.allCases.filter { source in
            switch source {
            case .generate: allowsGenerate
            case .enter: true
            case .copyFrom: !copyFromOptions.isEmpty
            }
        }
    }

    /// The view's content.
    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            if sources.count > 1 {
                Picker(title, selection: $draft.source) {
                    ForEach(sources, id: \.self) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .accessibilityLabel("\(title), source")
            }
            switch draft.source {
            case .generate:
                Text("Stackly generates a strong password and keeps it in your Keychain.")
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            case .enter:
                enterRow
                hint
            case .copyFrom:
                Picker("Copy from", selection: $draft.copyFromReference) {
                    Text("Choose a secret").tag(String?.none)
                    ForEach(copyFromOptions, id: \.self) {
                        Text(copyFromLabels[$0] ?? $0).tag(String?.some($0))
                    }
                }
                .accessibilityLabel("\(title), copy from")
            }
        }
        .onChange(of: draft.source) {
            showsTypedText = false
        }
        .onDisappear {
            showsTypedText = false
        }
    }

    private var enterRow: some View {
        HStack(spacing: Spacing.s8) {
            Group {
                if showsTypedText {
                    TextField(title, text: $draft.text)
                        .accessibilityLabel("\(title), shown")
                } else {
                    SecureField(title, text: $draft.text)
                        .accessibilityLabel(title)
                }
            }
            .textFieldStyle(.roundedBorder)
            .autocorrectionDisabled()
            .onChange(of: draft.text) { hasEdited = true }
            Button(showsTypedText ? "Hide" : "Show") {
                showsTypedText.toggle()
            }
            .accessibilityLabel(showsTypedText ? "Hide \(title)" : "Show \(title)")
            .frame(minHeight: HitTarget.minimum)
        }
    }

    @ViewBuilder private var hint: some View {
        if hasEdited, let violation = limits.violation(for: draft.text) {
            Text(violation.message)
                .font(.callout)
                .foregroundStyle(Tone.critical.text)
                .accessibilityLabel("\(title): \(violation.message)")
        } else if let lengthHint = limits.lengthHint {
            Text(lengthHint)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
        }
    }
}
