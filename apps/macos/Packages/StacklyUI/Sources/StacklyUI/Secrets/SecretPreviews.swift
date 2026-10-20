#if DEBUG
    import StacklyClient
    import SwiftUI

    /// A client for previews: it answers every call with a fixed result and never talks to a
    /// daemon. The values are examples, not credentials.
    private func previewClient() -> SecretsClient {
        SecretsClient { call, _ throws(ClientError) in
            switch call {
            case .secretReveal(let params):
                return .secretReveal(
                    RevealSecretReply(reference: params.reference, value: "not-a-real-secret"))
            case .secretSet(let params):
                return .secretSet(
                    SetSecretReply(
                        summary: SecretRefSummary(
                            reference: params.reference, ownerKind: .user, ownerId: nil,
                            purpose: "user", status: .present, statusObservedAt: nil,
                            valueOrigin: .entered, valueUpdatedAt: nil, usedBy: [], revision: 1),
                        warnings: [], effects: []))
            default:
                throw .notSent(ClientErrorCatalog.notConnected())
            }
        }
    }

    private struct SecretFieldPreview: View {
        @State private var draft = SecretDraft(allowsGenerate: false)

        var body: some View {
            SecretField(
                "Admin password", draft: $draft,
                limits: SecretLimits(minLength: 12, maxLength: 64, printableOnly: true),
                allowsGenerate: true,
                copyFromOptions: ["local:erp/postgres/replica"]
            )
            .padding(Spacing.pageHorizontal)
        }
    }

    #Preview("Secret field") {
        SecretFieldPreview().frame(width: 460)
    }

    #Preview("Secret field, dark") {
        SecretFieldPreview().frame(width: 460).preferredColorScheme(.dark)
    }

    #Preview("Secret entry sheet") {
        SecretEntrySheet(
            needs: [
                SecretNeed(
                    reference: "local:erp/postgres/admin", reason: .missing, generateAllowed: true,
                    limits: SecretLimits(minLength: 12)),
                SecretNeed(
                    reference: "local:erp/postgres/replica", reason: .storeLocked,
                    generateAllowed: false),
            ],
            client: previewClient(), actionTitle: "Start Instance",
            copyFromOptions: ["local:erp/postgres/other"], onContinue: {}, onCancel: {})
    }

    #Preview("Revealable secret text") {
        RevealableSecretText(
            "postgres://admin:****@127.0.0.1:5432/app",
            references: ["local:erp/postgres/admin"], client: previewClient(),
            compose: { values in
                "postgres://admin:\(values["local:erp/postgres/admin"]?.value ?? "")@127.0.0.1:5432/app"
            }
        )
        .padding(Spacing.pageHorizontal)
        .frame(width: 520)
    }
#endif
