import SwiftUI

struct CreateProgressSection: View {
    let model: CreateInstanceModel

    var body: some View {
        CardSection("Creating") {
            if let progress = model.progress {
                StagedProgress(
                    progress, layout: .stages)
            } else {
                HStack(spacing: Spacing.s8) {
                    ProgressView()
                        .controlSize(.small)

                    Text("Sending…")
                }

            }
        }

    }
}
