import StacklyClient
import SwiftUI

/// What the Catalog's version matrix needs to put the Get column next to a provider's
/// versions (spec 0018, *Kolom aksi Catalog*).
struct RuntimeGetSource {
    /// The provider ID.
    let provider: String
    /// The name shown for the provider.
    let displayName: String
    /// This Mac, from the catalog info.
    let host: HostTarget
    /// The runtimes and the Get action.
    let runtimes: RuntimeStore
    /// The operation center, for the percent of a running install, or `nil`.
    let operations: OperationCenter?
}

/// What the action column shows for one version.
nonisolated enum RuntimeGetCell: Equatable {
    /// The version has no artifact for this Mac to get.
    case none
    /// A runtime built from the artifact is installed.
    case installed
    /// The artifact is installing, with the progress of the operation.
    case installing(StagedProgressValue)
    /// The Get button, disabled with the reason of the install gate when it refuses.
    case get(artifact: CatalogArtifact, sha256: String, disabledReason: String?)

    /// The artifact of `version` built for `host` that can be downloaded, with its digest.
    static func hostArtifact(of version: CatalogVersion, host: HostTarget) -> (
        artifact: CatalogArtifact, sha256: String
    )? {
        for artifact in version.artifacts ?? []
        where artifact.os == host.os && artifact.arch == host.arch
            && artifact.status == .available
        {
            if let sha256 = artifact.sha256 { return (artifact, sha256) }
        }
        return nil
    }

    /// The reason the install gate refuses `version`, or `nil` when it accepts it.
    static func disabledReason(of version: CatalogVersion) -> String? {
        guard version.install.installable == false else { return nil }
        return CatalogCopy.reasonText(version.install.reason ?? .untested)
    }

    /// The help of the Get button: the reason it cannot run, the install gate's or the
    /// mutation gate's, and otherwise what it does. ``ActionButton`` writes the reason as the
    /// help of a disabled button, but a help set outside it wins, so this one carries it too.
    static func help(for availability: ActionAvailability, displayName: String, version: String)
        -> String
    {
        if case .disabled(let reason, _) = availability { return reason }
        return "Install \(displayName) \(version)"
    }

    /// Decides the cell. The installed runtime wins over a running install, which wins over
    /// the button; the join key is the digest of the artifact (spec 0018).
    /// - Parameters:
    ///   - version: The version of the row.
    ///   - host: This Mac.
    ///   - installed: The installed runtime of the artifact's digest, or `nil`.
    ///   - installing: The running install of the artifact's digest, or `nil`.
    ///   - operation: The operation of that install, or `nil` when the center lacks it.
    static func resolve(
        version: CatalogVersion, host: HostTarget, installed: (String) -> RuntimeSummary?,
        installing: (String) -> InstallingRuntime?,
        operation: (UUID) -> OperationSnapshot?
    ) -> RuntimeGetCell {
        guard let (artifact, sha256) = hostArtifact(of: version, host: host) else { return .none }
        if installed(sha256) != nil { return .installed }
        if let install = installing(sha256) {
            return .installing(
                RuntimeInstallProgress.value(operation: operation(install.operationId)))
        }
        return .get(
            artifact: artifact, sha256: sha256, disabledReason: disabledReason(of: version))
    }
}

/// The action column of the version matrix (spec 0018, *Kolom aksi Catalog*): Get, the
/// progress of the install, or Installed, for the artifact built for this Mac.
/// Get is `runtime.install` with the digest the person saw. The progress is a small bar of
/// 76 pt with the percent; it is a screen view, the one exception to the single builder of
/// spec 0067, because `StagedProgress` has no size that small.
struct RuntimeGetAction: View {
    /// The width of the small progress bar.
    static let progressWidth: CGFloat = 76

    @Environment(\.shellNavigator) private var navigator
    @Environment(\.feedbackCenter) private var feedbackCenter
    @Environment(\.mutationAvailability) private var gate

    let source: RuntimeGetSource
    let version: CatalogVersion

    private var cell: RuntimeGetCell {
        RuntimeGetCell.resolve(
            version: version, host: source.host,
            installed: { source.runtimes.installedRuntime(sha256: $0) },
            installing: { source.runtimes.installingRuntime(sha256: $0) },
            operation: { source.operations?.store.items[$0] })
    }

    var body: some View {
        switch cell {
        case .none:
            EmptyView()
        case .installed:
            Label("Installed", systemImage: "checkmark.circle.fill")
                .labelStyle(.titleAndIcon)
                .font(.callout.weight(.semibold))
                .foregroundStyle(Tone.positive.text)

        case .installing(let progress):
            miniProgress(progress)
        case .get(_, let sha256, let reason):
            let action = source.runtimes.getAction(
                provider: source.provider, displayName: source.displayName,
                version: version.version, sha256: sha256, disabledReason: reason,
                context: InstanceActionContext.make(
                    feedbackCenter: feedbackCenter, navigator: navigator))
            ActionButton(action, showsReason: false)
                .controlSize(.small)
                .help(
                    RuntimeGetCell.help(
                        for: action.availability(gate: gate), displayName: source.displayName,
                        version: version.version))

        }
    }

    private func miniProgress(_ progress: StagedProgressValue) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            Text(progress.percent.map { "\($0)%" } ?? progress.currentTitle)
                .font(.callout)
                .monospacedDigit()
                .foregroundStyle(StacklyColor.textSecondary)
            if let fraction = progress.fraction {
                ProgressView(value: fraction)
                    .progressViewStyle(.linear)
                    .frame(width: Self.progressWidth)
            } else {
                ProgressView()
                    .progressViewStyle(.linear)
                    .frame(width: Self.progressWidth)
            }
        }

    }
}

#if DEBUG
    /// The three states of the column side by side: an installed runtime, a running install,
    /// and an artifact to get.
    private struct RuntimeGetPreview: View {
        @State private var rig = RuntimeRig(
            page: RuntimeListPage(
                items: [FixtureRuntime.summaries[0]], next: nil,
                installing: [FixtureRuntime.installing]))
        @State private var isLoaded = false

        private static let host = HostTarget(os: .macos, arch: .aarch64)

        private func version(_ name: String, digest: String?) -> CatalogVersion {
            CatalogVersion(
                version: name, line: name, support: nil,
                install: InstallVerdict(installable: true, reason: nil),
                artifacts: [
                    CatalogArtifact(
                        id: name, os: .macos, arch: .aarch64, status: .available, sha256: digest,
                        minOs: "13.0", downloadSize: 1, installedSize: nil, license: nil,
                        licenseUrl: nil, source: nil, blockedReason: nil, dependencies: nil)
                ])
        }

        var body: some View {
            Group {
                if isLoaded {
                    let source = RuntimeGetSource(
                        provider: "official/postgresql", displayName: "PostgreSQL", host: Self.host,
                        runtimes: rig.runtimes, operations: rig.operations)
                    VStack(alignment: .leading, spacing: Spacing.s16) {
                        RuntimeGetAction(
                            source: source,
                            version: version("17.6", digest: FixtureRuntime.digest(1)))
                        RuntimeGetAction(
                            source: source,
                            version: version("18.1", digest: FixtureRuntime.digest(2)))
                        RuntimeGetAction(
                            source: source,
                            version: version("18.2", digest: FixtureRuntime.digest(3)))
                    }
                    .padding(Spacing.pageHorizontal)
                    .actionHost()
                } else {
                    Color.clear
                }
            }
            .task {
                await rig.load()
                isLoaded = true
            }
        }
    }

    #Preview("Light") {
        RuntimeGetPreview()
            .frame(width: 240, height: 160)
            .preferredColorScheme(.light)
    }

    #Preview("Dark") {
        RuntimeGetPreview()
            .frame(width: 240, height: 160)
            .preferredColorScheme(.dark)
    }
#endif
