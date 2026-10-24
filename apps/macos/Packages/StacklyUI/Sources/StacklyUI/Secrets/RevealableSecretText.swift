import AppKit
import StacklyClient
import SwiftUI

/// Text with a secret inside it, shown masked, with Show and Copy buttons (spec 0011,
/// *Reveal dan copy*). The Summary tab uses it for a connection string.
/// The masked text is what the daemon already redacted, for example a URI with `****` for the
/// password. Show asks the daemon for the value of every reference and replaces the masked
/// text with the text `compose` builds, until Hide, 30 seconds, the view going away, or the
/// app becoming inactive. Copy asks again with purpose `copy`, builds the full text with
/// `compose`, and writes it with ``ConcealedPasteboard``. Both are all or nothing: one reveal
/// that fails cancels the action and nothing is shown or copied.
/// Hide, the 30 second limit, or the app turning inactive while Show still waits for the
/// daemon cancels that Show with a message ("Hidden because Stackly went to the background.
/// Press Show to try again."); a Copy that waits finishes anyway, because it shows nothing.
/// Show, Hide, and Copy are native buttons, reachable with Tab and activated with Space.
public struct RevealableSecretText: View {

    @State private var model: RevealModel
    private let maskedText: String

    /// Creates the view.
    /// - Parameters:
    ///   - maskedText: The text to show while hidden, already redacted by the daemon.
    ///   - references: One or more different references whose values `compose` needs.
    ///   - client: The secret methods.
    ///   - compose: Builds the full text from the revealed values, keyed by reference. It may     throw; the action then shows and copies nothing.
    public init(
        _ maskedText: String,
        references: [String],
        client: SecretsClient,

        compose: @escaping RevealModel.Compose
    ) {
        self.init(
            maskedText,

            model: RevealModel(
                references: references,
                reveal: { reference, purpose throws(ClientError) in
                    try await client.revealSecret(reference: reference, purpose: purpose)
                },
                compose: compose))
    }

    init(
        _ maskedText: String,

        model: RevealModel
    ) {
        self.maskedText = maskedText

        _model = State(initialValue: model)
    }

    /// The view's content.
    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                text
                Spacer(minLength: Spacing.s8)
                Button(model.isShown ? "Hide" : "Show") {
                    if model.isShown {
                        model.hide()
                    } else {
                        Task { await model.show() }
                    }
                }

                .disabled(model.phase == .working)
                Button("Copy") {
                    Task { await model.copy() }
                }

                .disabled(model.phase == .working)
            }
            note
        }
        .onDisappear { model.viewDisappeared() }
        .onReceive(
            NotificationCenter.default.publisher(for: NSApplication.didResignActiveNotification)
        ) { _ in
            model.appBecameInactive()
        }

    }

    @ViewBuilder private var text: some View {
        if let shown = model.shownText {
            Text(shown)
                .stacklyCode()

        } else {
            Text(maskedText)
                .font(.callout.monospaced())
                .foregroundStyle(StacklyColor.textSecondary)

        }
    }

    @ViewBuilder private var note: some View {
        switch model.phase {
        case .working:
            Text("Asking stacklyd…")
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
        case .failed(let message):
            Text(message)
                .font(.callout)
                .foregroundStyle(Tone.critical.text)
        case .copied:
            Text("Copied. The clipboard clears in 90 seconds.")
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
        case .idle:
            EmptyView()
        }
    }
}
