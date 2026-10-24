#if DEBUG
    import StacklyClient
    import SwiftUI

    /// The Secrets page of the design gallery (spec 0011, step 11): the secret entry sheet and
    /// the revealable text on the real daemon, so the flows can be tried by hand with a fixture
    /// credential and a temporary `STACKLY_ROOT`.
    /// It is a debugging aid, not a product screen. Without a client it says so.
    struct SecretsGalleryPage: View {
        /// The reference the page works on. `local:debug/...` is free for `secret.set`; the
        /// first segments `instance` and `stackly` are not.
        static let defaultReference = "local:debug/gallery/password"
        private static let maskedURI = "postgres://admin:****@127.0.0.1:5432/app"

        let client: SecretsClient?

        @State private var reference = SecretsGalleryPage.defaultReference
        @State private var showsSheet = false
        @State private var status = "Not checked yet."
        @State private var lastAction = ""
        @State private var confirmation: ConfirmationRequest?
        @State private var sheetGeneration = 0

        var body: some View {
            if let client {
                content(client)
            } else {
                Text("No daemon link. Run the app with a daemon to use this page.")
                    .foregroundStyle(StacklyColor.textSecondary)
                    .padding(Spacing.pageHorizontal)
            }
        }

        private func content(_ client: SecretsClient) -> some View {
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                    Text(
                        "Fill a reference through the entry sheet, then show, copy, and hide it. Use a value that starts with the fixture prefix, so a leak scan can find it."
                    )
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)

                    VStack(alignment: .leading, spacing: Spacing.s8) {
                        TextField("Reference", text: $reference)
                            .textFieldStyle(.roundedBorder)
                            .stacklyCode()
                        HStack(spacing: Spacing.s8) {
                            Button("Open Entry Sheet") {
                                sheetGeneration += 1
                                showsSheet = true
                            }
                            .mutationGated()
                            Button("Check Status") { Task { await checkStatus(client) } }
                            Button("Delete Reference…", role: .destructive) {
                                confirmation = ConfirmationRequest(
                                    title: "Delete “\(reference)”?",
                                    message:
                                        "Removes the reference and the value in the Keychain, so the sheet can be tried again.",
                                    actionTitle: "Delete", role: .destructive,
                                    perform: { Task { await delete(client) } })
                            }
                            .mutationGated()
                        }
                        Text(status)
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                        if !lastAction.isEmpty {
                            Text(lastAction)
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                        }
                    }
                    .padding(Spacing.cardInsets)
                    .cardSurface()

                    VStack(alignment: .leading, spacing: Spacing.s8) {
                        Text("Connection string").font(.headline)
                        RevealableSecretText(
                            Self.maskedURI, references: [reference], client: client,
                            compose: { values in
                                let value = values[reference]?.value ?? ""
                                return "postgres://admin:\(value)@127.0.0.1:5432/app"
                            }
                        )
                        .id(reference)
                    }
                    .padding(Spacing.cardInsets)
                    .cardSurface()
                }
                .padding(Spacing.pageHorizontal)
            }
            .sheet(isPresented: $showsSheet) {
                SecretEntrySheet(
                    needs: [
                        SecretNeed(
                            reference: reference, reason: .missing, generateAllowed: true,
                            limits: SecretLimits(minLength: 8, maxLength: 64, printableOnly: true))
                    ],
                    client: client, actionTitle: "Done",
                    onContinue: {
                        showsSheet = false
                        lastAction = "The entry sheet finished: every row is saved."
                    },
                    onCancel: {
                        showsSheet = false
                        lastAction = "The entry sheet was cancelled."
                    }
                )
                .id(sheetGeneration)
            }
            .stacklyConfirmation(item: $confirmation)
        }

        private func checkStatus(_ client: SecretsClient) async {
            do throws(ClientError) {
                let reply = try await client.secretStatus(reference: reference)
                status =
                    "Status: \(reply.summary.status.rawValue), revision \(reply.summary.revision). Store: \(reply.store.state.rawValue) (\(reply.store.target?.rawValue ?? "none"))."
            } catch {
                status = "Status failed: \(RevealModel.message(for: error))"
            }
        }

        private func delete(_ client: SecretsClient) async {
            do throws(ClientError) {
                try await client.deleteSecret(reference: reference)
                status = "Deleted."
            } catch {
                status = "Delete failed: \(RevealModel.message(for: error))"
            }
        }
    }

    #Preview("Secrets, no link") {
        SecretsGalleryPage(client: nil).galleryPreview(.light)
    }
#endif
