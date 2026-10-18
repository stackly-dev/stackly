#if DEBUG
    import SwiftUI

    /// One confirmation of the gallery, with copy from the mockup.
    nonisolated struct ConfirmationSample: Identifiable, Sendable {
        /// The label of the button that opens the dialog.
        let trigger: String
        /// The dialog title.
        let title: String
        /// The dialog message.
        let message: String
        /// The action button label.
        let actionTitle: String
        /// The role of the action.
        let role: ConfirmationRequest.Role

        var id: String { title }

        /// Every sample, destructive first.
        static let all = [
            ConfirmationSample(
                trigger: "Remove pg17-dev…",
                title: "Remove “pg17-dev”?",
                message:
                    "The instance is unregistered and its process stopped. The data directory (1.24 GB) stays on disk.",
                actionTitle: "Remove",
                role: .destructive
            ),
            ConfirmationSample(
                trigger: "Stop mysql-erp…",
                title: "Stop “mysql-erp”?",
                message:
                    "It’s leased by erp-development. Stopping it leaves the environment partially ready. Data is kept.",
                actionTitle: "Stop",
                role: .destructive
            ),
            ConfirmationSample(
                trigger: "Overwrite pg17-dev…",
                title: "Overwrite “pg17-dev”?",
                message:
                    "Replaces the current data with bk-0412. A safety snapshot is taken first and the instance restarts.",
                actionTitle: "Overwrite",
                role: .destructive
            ),
            ConfirmationSample(
                trigger: "Remove PostgreSQL 16.10…",
                title: "Remove PostgreSQL 16.10?",
                message:
                    "Deletes the runtime binaries. No instance uses it. Instance data is never touched.",
                actionTitle: "Remove",
                role: .destructive
            ),
            ConfirmationSample(
                trigger: "Bring Down erp-development…",
                title: "Bring down “erp-development”?",
                message:
                    "Releases the lease and stops only instances nothing else needs. Shared and manually started instances keep running. Data is kept.",
                actionTitle: "Bring Down",
                role: .standard
            ),
        ]

        /// The request for this sample.
        @MainActor
        func request(perform: @escaping @MainActor () -> Void) -> ConfirmationRequest {
            ConfirmationRequest(
                title: title,
                message: message,
                actionTitle: actionTitle,
                role: role,
                perform: perform
            )
        }
    }

    /// The Confirmation page of the design gallery: destructive and standard dialogs with
    /// copy from the mockup, the result of the last one, and the keyboard behavior to
    /// check in the real app.
    struct ConfirmationGalleryPage: View {
        @State private var request: ConfirmationRequest?
        @State private var pendingTitle: String?
        @State private var outcome = "No dialog opened yet."

        var body: some View {
            GalleryPageLayout {
                GallerySection(
                    "Destructive",
                    note:
                        "Red action button, caution badge, and Cancel as the default button: Return and Escape both cancel."
                ) {
                    samples(of: .destructive)
                }
                GallerySection(
                    "Standard",
                    note: "The action is the default button: Return runs it, Escape cancels."
                ) {
                    samples(of: .standard)
                }
                GallerySection(
                    "Last Result",
                    note: "Shows whether the last dialog ran its action or was cancelled."
                ) {
                    Text(outcome)
                        .font(.body)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(Spacing.cardInsets)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .cardSurface()
                        .accessibilityLabel("Last result: \(outcome)")
                }
                GallerySection(
                    "Keyboard",
                    note:
                        "What to check with the real keyboard. Tab reaches the other button only while Keyboard navigation is on in System Settings › Keyboard; then Space presses it."
                ) {
                    keyboardTable
                }
            }
            .stacklyConfirmation(item: $request)
            .onChange(of: request == nil) { _, isClosed in
                if isClosed, let pendingTitle {
                    outcome = "Cancelled “\(pendingTitle)”. Nothing ran."
                    self.pendingTitle = nil
                }
            }
        }

        /// The buttons that open the samples of one role.
        private func samples(of role: ConfirmationRequest.Role) -> some View {
            VStack(alignment: .leading, spacing: Spacing.s8) {
                ForEach(ConfirmationSample.all.filter { $0.role == role }) { sample in
                    Button(sample.trigger) {
                        open(sample)
                    }
                    .buttonStyle(.bordered)
                }
            }
        }

        /// The expected keys per role.
        private var keyboardTable: some View {
            Grid(alignment: .leading, horizontalSpacing: Spacing.s16, verticalSpacing: Spacing.s8) {
                GridRow {
                    Text("Role")
                    Text("Return")
                    Text("Escape")
                }
                .font(.headline)
                GridRow {
                    Text("Destructive")
                    Text("Cancel; nothing runs")
                    Text("Cancel; nothing runs")
                }
                GridRow {
                    Text("Standard")
                    Text("Runs the action")
                    Text("Cancel; nothing runs")
                }
            }
            .font(.body)
            .padding(Spacing.cardInsets)
            .frame(maxWidth: .infinity, alignment: .leading)
            .cardSurface()
        }

        /// Shows the dialog of `sample`, replacing any request still open.
        private func open(_ sample: ConfirmationSample) {
            pendingTitle = sample.title
            request = sample.request {
                outcome = "Ran “\(sample.actionTitle)” from “\(sample.title)”."
                pendingTitle = nil
            }
        }
    }

    #Preview("Light") {
        ConfirmationGalleryPage().galleryPreview(.light)
    }

    #Preview("Dark") {
        ConfirmationGalleryPage().galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        ConfirmationGalleryPage().galleryPreview(.narrowWidth)
    }
#endif
