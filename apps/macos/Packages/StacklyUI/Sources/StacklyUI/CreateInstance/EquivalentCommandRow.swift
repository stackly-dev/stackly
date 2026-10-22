import SwiftUI

/// The command that does the same as the draft (spec 0020, *Perintah setara*): the newest
/// one the daemon sent, with Copy. It shows after the first preflight reply and stays while
/// a preflight cannot run.
struct EquivalentCommandRow: View {
    let model: CreateInstanceModel

    var body: some View {
        if let command = model.lastPreflight?.command {
            VStack(alignment: .leading, spacing: Spacing.s6) {
                Text("Equivalent command")
                    .font(.headline)
                ConsoleBlock(command: command)
                    .accessibilityIdentifier(AccessibilityID.CreateInstance.command)
            }
        }
    }
}
