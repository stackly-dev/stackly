import StacklyClient
import SwiftUI

/// The Connection card of the Summary tab (spec 0021, *Tab Summary*): every connection
/// string of the provider, masked, with Show Password and Copy for a string with a secret and
/// Copy alone for one without, then the notes that matter before the string is used, then
/// one row per secret.
/// A string with a secret never holds its value here: Show and Copy go through
/// ``RevealableSecretText``, which asks `secret.reveal` only when pressed.
struct ConnectionCard: View {
    let summary: InstanceSummary
    let model: InstanceDetailModel
    let secrets: SecretsClient

    var body: some View {
        CardSection("Connection") {
            if let detail = model.detail {
                content(detail)
            } else if model.isDetailPending {
                DetailPending()
            } else {
                Text("—").foregroundStyle(StacklyColor.textSecondary)
            }
        }

    }

    @ViewBuilder
    private func content(_ detail: InstanceDetail) -> some View {
        if let text = DetailFormat.connectionUnavailableText(detail) {
            Text(text)
        } else if detail.connections.isEmpty {
            Text(DetailFormat.noConnectionText)
                .foregroundStyle(StacklyColor.textSecondary)
        } else {
            ForEach(detail.connections, id: \.name) { connection in
                ConnectionEntryRow(connection: connection, detail: detail, secrets: secrets)
            }
            if detail.connections.contains(where: \.portMayChange) {
                Text("The port can change the next time the instance starts.")
                    .font(.footnote)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
        }
        if Self.isNotAcceptingConnections(summary) {
            Label {
                Text("Not accepting connections yet.")
            } icon: {
                Image(systemName: Tone.caution.symbol)
            }
            .foregroundStyle(Tone.caution.text)
        }
        ForEach(detail.secrets, id: \.reference) { secret in
            LabeledContent("Secret") {
                VStack(alignment: .trailing, spacing: Spacing.s2) {
                    Text(secret.reference).font(.callout.monospaced())
                    if let note = DetailFormat.secretNote(secret) {
                        Text(note).font(.footnote)
                    }
                }
            }
            .labeledContentStyle(.stackly)
        }
    }

    /// Whether the process runs but readiness has not said `ready`: the string is not safe to
    /// use yet (BR-010).
    static func isNotAcceptingConnections(_ summary: InstanceSummary) -> Bool {
        summary.lifecycle == .running && summary.readiness.state != .ready
    }
}

/// One connection string of the Connection card, and of the inspector of the Instances page.
struct ConnectionEntryRow: View {
    let connection: ConnectionString
    let detail: InstanceDetail
    let secrets: SecretsClient

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            Text(connection.title)
                .font(.footnote)
                .foregroundStyle(StacklyColor.textSecondary)
            entry
        }

    }

    @ViewBuilder private var entry: some View {
        if let reason = DetailFormat.unavailableText(for: connection) {
            Text(reason)
        } else if let masked = connection.masked {
            let references = ConnectionComposer.references(in: connection.parts)
            if references.isEmpty {
                plain(masked)
            } else if let missing = DetailFormat.missingReference(
                in: connection, secrets: detail.secrets)
            {
                blocked(masked, missing: missing)
            } else {
                RevealableSecretText(
                    masked, references: references, client: secrets,

                    compose: ConnectionComposer.composer(for: connection.parts))
            }
        }
    }

    /// A string with no secret: selectable text and a Copy button on the plain pasteboard.
    private func plain(_ masked: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            Text(masked)
                .font(.callout.monospaced())
                .foregroundStyle(StacklyColor.textSecondary)
                .textSelection(.enabled)
            Spacer(minLength: Spacing.s8)
            CopyButton(
                masked, what: connection.title, style: .text)
        }
    }

    /// A string whose secret has no stored value: Show Password and Copy are off, with the
    /// reason beside them.
    private func blocked(_ masked: String, missing: String) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                Text(masked)
                    .font(.callout.monospaced())
                    .foregroundStyle(StacklyColor.textSecondary)

                Spacer(minLength: Spacing.s8)
                Button("Show") {}

                    .disabled(true)
                Button("Copy") {}

                    .disabled(true)
            }
            DisabledReasonText(DetailFormat.missingValueReason(missing))
        }
    }
}
