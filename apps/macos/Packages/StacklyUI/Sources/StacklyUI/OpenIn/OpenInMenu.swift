import StacklyClient
import SwiftUI

/// The "Open In" menu button (spec 0070, AC-5): the toolbar of the detail page and the
/// inspector of the Instances page show the same one. It shows nothing when the provider has
/// no action to show, and one disabled "Loading…" item while the detail loads.
struct OpenInMenu: View {
    /// The help of the button and the title of the submenu.
    static let title = "Open In"
    /// The SF Symbol of the button.
    static let symbol = "arrow.up.forward.app"
    /// The text of the item that stands in while the detail loads.
    static let loadingText = "Loading…"

    let content: OpenInContent

    var body: some View {
        switch content {
        case .hidden, .noSelection:
            EmptyView()
        case .loading:
            Menu {
                Button(Self.loadingText) {}
                    .disabled(true)
            } label: {
                Label(Self.title, systemImage: Self.symbol)
                    .labelStyle(.iconOnly)
            }
            .help(Self.title)
        case .sections(let sections):
            ActionMenu(sections, title: Self.title, symbol: Self.symbol)
        }
    }
}

extension OpenInContent {
    /// Whether the button of the toolbar and of the inspector shows at all: it does not when
    /// the provider has nothing to show (AC-5).
    var showsButton: Bool {
        switch self {
        case .hidden, .noSelection: false
        case .loading, .sections: true
        }
    }
}

extension View {
    /// Presents the Enter Secret… sheet that a card of Open In offers when the password was
    /// not available (spec 0070, AC-10). Saving the secret closes it and loads the detail
    /// again, since the reference is no longer `missing`.
    /// - Parameters:
    ///   - needs: The references to fill, or `nil` while the sheet is closed.
    ///   - secrets: The secret methods.
    ///   - reload: Loads the detail again.
    func openInSecretSheet(
        needs: Binding<[SecretNeed]?>, secrets: SecretsClient,
        reload: @escaping @MainActor () -> Void
    ) -> some View {
        sheet(
            isPresented: Binding(
                get: { needs.wrappedValue != nil },
                set: { if $0 == false { needs.wrappedValue = nil } })
        ) {
            SecretEntrySheet(
                needs: needs.wrappedValue ?? [], client: secrets, actionTitle: "Done",
                onContinue: {
                    needs.wrappedValue = nil
                    reload()
                },
                onCancel: { needs.wrappedValue = nil })
        }
    }
}
