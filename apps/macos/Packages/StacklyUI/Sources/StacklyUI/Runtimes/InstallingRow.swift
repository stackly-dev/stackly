import StacklyClient
import SwiftUI

/// How the progress of an install is worked out (spec 0018, *Halaman Runtimes*): the five
/// stages of `install_runtime` and the percent of ``StagedProgressValue``.
nonisolated enum RuntimeInstallProgress {
    /// The five stages: Download, Verify digest, Extract, Validate binary, Commit.
    static var stages: [StageDefinition] {
        OperationStages.definitions(for: .installRuntime) ?? []
    }

    /// The value of an install. Before the operation reaches the store (a click just
    /// accepted, or an install another client started) the install reads as queued.
    ///
    /// - Parameter operation: The `install_runtime` operation, or `nil` when the operation
    ///   center does not hold it yet.
    static func value(operation: OperationSnapshot?) -> StagedProgressValue {
        guard let operation else {
            return StagedProgressValue(
                stages: stages.map { ProgressStage(key: $0.key, title: $0.title, state: .pending) },
                runningIndex: nil, fraction: nil, percent: nil, currentTitle: "Queued",
                outcome: .waiting)
        }
        return .from(operation: operation, stages: stages)
    }
}

/// One install that has not ended, in the Installing section of the Runtimes page (spec
/// 0018, *Halaman Runtimes*): the provider tile, `Install <name> <version>`, and the
/// compact staged progress with the stage and the percent.
///
/// A context menu holds Cancel Install, and VoiceOver gets the same action. The text wraps,
/// so nothing is cut in the 384 pt page column next to the inspector.
struct InstallingRow: View {
    @Environment(\.actionHost) private var host
    @Environment(\.mutationAvailability) private var gate

    let install: InstallingRuntime
    let tile: TileSpec
    let progress: StagedProgressValue
    let cancel: StacklyAction

    var body: some View {
        HStack(alignment: .top, spacing: Spacing.s12) {
            ProviderTile(tile, size: ProviderTile.rowSize)
            VStack(alignment: .leading, spacing: Spacing.s4) {
                Text(RuntimeCopy.installTitle(install))
                    .font(.body.weight(.semibold))
                    .fixedSize(horizontal: false, vertical: true)
                StagedProgress(
                    progress, layout: .compact,
                    accessibilityLabel: RuntimeCopy.installTitle(install), announcesStages: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .contextMenu { ActionMenuItems([[cancel]]) }
        .accessibilityAction(named: cancel.title) {
            cancel.run(host: host, gate: gate)
        }
        .accessibilityIdentifier(AccessibilityID.Runtimes.installingRow(install.operationId))
    }
}
