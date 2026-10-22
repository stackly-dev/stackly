import SwiftUI

/// The progress of the create operation (spec 0020, *Create*): the six stages of
/// `create_instance` with the bar, and each stage said once to VoiceOver as it starts.
struct CreateProgressSection: View {
    let model: CreateInstanceModel

    var body: some View {
        CardSection("Creating") {
            if let progress = model.progress {
                StagedProgress(
                    progress, layout: .stages, accessibilityLabel: "Creating the instance",
                    announcesStages: true)
            } else {
                HStack(spacing: Spacing.s8) {
                    ProgressView()
                        .controlSize(.small)
                        .accessibilityHidden(true)
                    Text("Sending…")
                }
                .accessibilityElement(children: .combine)
            }
        }
        .accessibilityIdentifier(AccessibilityID.CreateInstance.progress)
    }
}
