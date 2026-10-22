import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Runtimes page in every variant (spec 0068 AC-8, spec 0018 AC-17): the runtime that
/// is chosen, the Installing section, and every state of the page, plus the three states of
/// the Get column of the Catalog.
///
/// The states with the inspector open render at the width of the page column and the
/// inspector together: 664 pt is the content column of a 900 pt window, which leaves the
/// 384 pt of the table (spec 0018, *Halaman Runtimes*).
extension SnapshotTests {
    struct RuntimesSnapshotTests {
        /// The page these snapshots show, named so the source rule finds its suite.
        private typealias Page = RuntimesPage

        private static func id(_ number: UInt8) -> String {
            FixtureID.uuid(kind: FixtureID.runtime, number: number).uuidString.lowercased()
        }

        /// The page area with the inspector open, narrow at the 900 pt window.
        private static let withInspector = SnapshotSize.fitting(width: 1_044, narrowWidth: 664)

        @MainActor
        private func rig(
            page: RuntimeListPage = FixtureRuntime.page, load: Bool = true,
            prepare: (RuntimeRig) -> Void = { _ in }
        ) async -> RuntimeRig {
            let rig = RuntimeRig(page: page)
            prepare(rig)
            if load { await rig.load() }
            return rig
        }

        /// A view of the page area's height, so `fitting` knows how tall it is.
        @MainActor
        private func framed<V: View>(_ view: V) -> some View {
            view.frame(height: 748)
        }

        private static let unknownMethod = ClientError.rejected(PageRig.unknownMethod)

        // MARK: - Chosen runtime and Installing

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Runtimes page with a runtime chosen renders in every variant"
            ))
        func runtimesWithARuntimeChosen() async throws {
            let rig = await rig()
            try await assertSnapshot(
                framed(rig.runtimesPage(selection: Self.id(1))), named: "runtimes-chosen",
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Runtimes page with nothing chosen renders in every variant"
            ))
        func runtimesWithNothingChosen() async throws {
            let rig = await rig()
            try await assertSnapshot(
                framed(rig.runtimesPage()), named: "runtimes-no-selection",
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Runtimes page with an install running and the inspector closed renders in every variant"
            ))
        func runtimesWithAnInstallRunning() async throws {
            let rig = await rig()
            try await assertSnapshot(
                rig.runtimesPage(showsInspector: false), named: "runtimes-installing")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Runtimes page whose runtime is not installed renders in every variant"
            ))
        func runtimesWithARuntimeThatIsNotInstalled() async throws {
            let drifted = FixtureRuntime.summaries[1].withState(.drifted)
            let missing = FixtureRuntime.summaries[2].withState(.missing)
            let page = RuntimeListPage(
                items: [FixtureRuntime.summaries[0], drifted, missing], next: nil, installing: [])
            let rig = await rig(page: page)
            try await assertSnapshot(
                rig.runtimesPage(showsInspector: false), named: "runtimes-states")
        }

        // MARK: - States of the page

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Runtimes page without runtimes renders in every variant"
            ))
        func runtimesWithoutRuntimes() async throws {
            let rig = await rig(page: RuntimeListPage(items: [], next: nil, installing: []))
            try await assertSnapshot(
                rig.runtimesPage(showsInspector: false), named: "runtimes-empty")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Runtimes page before the first snapshot renders in every variant"
            ))
        func runtimesBeforeTheFirstSnapshot() async throws {
            let rig = await rig(load: false)
            try await assertSnapshot(
                rig.runtimesPage(showsInspector: false), named: "runtimes-loading",
                contrastInvariant: true,
                contrastInvariantReason:
                    "only a system spinner, which Increase Contrast does not change")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Runtimes page that failed to load renders in every variant"
            ))
        func runtimesThatFailedToLoad() async throws {
            let rig = await rig(load: false) { $0.script.listFailure = .rejected(PageRig.failure) }
            await rig.load()
            try await assertSnapshot(
                rig.runtimesPage(showsInspector: false), named: "runtimes-failed")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Runtimes page of a daemon that cannot list runtimes renders in every variant"
            ))
        func runtimesOfADaemonWithoutTheMethod() async throws {
            let rig = await rig(load: false) { $0.script.listFailure = Self.unknownMethod }
            await rig.load()
            try await assertSnapshot(
                rig.runtimesPage(showsInspector: false), named: "runtimes-daemon-unsupported")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Runtimes page after a failed refresh renders in every variant"
            ))
        func runtimesAfterAFailedRefresh() async throws {
            let rig = await rig()
            rig.script.listFailure = .rejected(PageRig.failure)
            await rig.runtimes.store.refresh()
            try await assertSnapshot(
                rig.runtimesPage(showsInspector: false), named: "runtimes-stale")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Runtimes page with a runtime that could not be refreshed renders in every variant"
            ))
        func runtimesWithARuntimeThatCouldNotBeRefreshed() async throws {
            let rig = await rig()
            // A state event for a runtime the store does not hold asks the daemon for it,
            // and the daemon fails.
            rig.script.getFailure = .rejected(PageRig.failure)
            let stranger = FixtureID.uuid(kind: FixtureID.runtime, number: 40)
            rig.runtimes.store.receive(
                Event(
                    subscriptionId: 1, sequence: 1, at: FixtureClock.now,
                    resource: ResourceRef(kind: .runtime, id: stranger.uuidString.lowercased()),
                    revision: 9, kind: .runtimeStateChanged))
            try await waitUntil { rig.runtimes.store.itemFailures.isEmpty == false }
            try await assertSnapshot(
                rig.runtimesPage(showsInspector: false), named: "runtimes-partial")
        }

        // MARK: - Get column of the Catalog

        /// The digest of the NATS artifact for this Mac in `protocols/fixtures/catalog/`.
        private static let natsDigest =
            "ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd"

        private static let installingNats = InstallingRuntime(
            operationId: FixtureID.uuid(kind: FixtureID.operation, number: 1),
            runtimeId: FixtureID.uuid(kind: FixtureID.runtime, number: 9),
            provider: "official/nats", software: "nats", displayName: "NATS Server",
            version: "2.15.1", target: HostTarget(os: .macos, arch: .aarch64),
            sha256: natsDigest)

        private static let installedNats = RuntimeSummary(
            id: FixtureID.uuid(kind: FixtureID.runtime, number: 9), revision: 1,
            software: "nats", displayName: "NATS Server", version: "2.15.1", line: "2.15",
            target: HostTarget(os: .macos, arch: .aarch64), installMode: .managed,
            state: .installed, artifactSha256: natsDigest, installedAt: FixtureClock.daysAgo(1),
            lastVerifiedAt: FixtureClock.minutesAgo(1), support: .experimental,
            supportReason: .runtimeTested, usedBy: [], dependentRuntimes: [])

        /// The Catalog page with NATS chosen, over runtimes and operations that say what the
        /// Get column shows.
        @MainActor
        private func catalogPage(
            runtimes page: RuntimeListPage, operations: [OperationSummary] = [],
            failing: Bool = false
        ) async throws -> some View {
            let script = try CatalogScript.fixture()
            let catalog = script.store()
            await catalog.store.resnapshot()
            for provider in catalog.providers ?? [] {
                await catalog.detail.load(provider.provider, revision: provider.revision)
            }
            let runtimes = RuntimeStore(
                call: { call throws(ClientError) in
                    guard case .runtimeList = call else {
                        throw .rejected(ClientErrorCatalog.protocolViolation())
                    }
                    return .runtimeList(page)
                },
                send: { _, _ in
                    .rejected(
                        ErrorBody(
                            code: "runtime.selector_changed", category: .conflict,
                            message: "the catalog changed since you looked", retryable: false,
                            resource: nil, fields: nil,
                            hint: "Reload the catalog and try again.", details: nil,
                            operationId: nil, rule: nil, items: nil))
                },
                now: { FixtureClock.now }, sleep: { _ in })
            await runtimes.store.resnapshot()
            if failing {
                await runtimes.get(
                    provider: "official/nats", displayName: "NATS Server", version: "2.15.1",
                    sha256: Self.natsDigest, context: InstanceActionContext(report: { _ in }))
            }
            let center = OperationCenter(
                fetchPage: { _ throws(ClientError) in
                    StacklyUI.Page(items: operations, next: nil)
                },
                fetchOne: { id throws(ClientError) in operations.first { $0.id == id } },
                now: { FixtureClock.now }, sleep: { _ in })
            await center.store.resnapshot()
            return CatalogPage(
                catalog: catalog, runtimes: runtimes, operations: center, waiting: nil,
                hasConnection: true, reconnect: {}, openDiagnostics: {},
                selection: "official/nats"
            )
            .actionHost()
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The version matrix of a provider with nothing installed shows Get in every variant"
            ))
        func theGetColumnWithTheGetButton() async throws {
            let page = RuntimeListPage(items: [], next: nil, installing: [])
            try await assertSnapshot(
                catalogPage(runtimes: page), named: "runtime-get-button")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The version matrix of a provider whose install is running shows the progress in every variant"
            ))
        func theGetColumnWithAnInstallRunning() async throws {
            let page = RuntimeListPage(items: [], next: nil, installing: [Self.installingNats])
            let operation = OperationSummary.fixture(
                number: 1, kind: .installRuntime, state: .running, stage: "verify_digest")
            try await assertSnapshot(
                catalogPage(runtimes: page, operations: [operation]),
                named: "runtime-get-installing")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The version matrix of a provider that is installed shows Installed in every variant"
            ))
        func theGetColumnWithTheRuntimeInstalled() async throws {
            let page = RuntimeListPage(items: [Self.installedNats], next: nil, installing: [])
            try await assertSnapshot(
                catalogPage(runtimes: page), named: "runtime-get-installed")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail panel of a provider whose install was refused shows why in every variant"
            ))
        func theDetailPanelAfterARefusedInstall() async throws {
            let page = RuntimeListPage(items: [], next: nil, installing: [])
            try await assertSnapshot(
                catalogPage(runtimes: page, failing: true), named: "runtime-get-failed")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Runtimes page whose chosen runtime could not load its detail renders in every variant"
            ))
        func runtimesWhoseDetailFailedToLoad() async throws {
            let rig = await rig()
            rig.script.getFailure = .rejected(PageRig.failure)
            await rig.runtimes.detail.load(
                FixtureRuntime.summaries[0].id, revision: FixtureRuntime.summaries[0].revision)
            try await assertSnapshot(
                framed(rig.runtimesPage(selection: Self.id(1))), named: "runtimes-detail-failed",
                size: Self.withInspector)
        }
    }
}

extension RuntimeSummary {
    /// A copy in another state.
    fileprivate func withState(_ state: RuntimeState) -> RuntimeSummary {
        RuntimeSummary(
            id: id, revision: revision, software: software, displayName: displayName,
            version: version, line: line, target: target, installMode: installMode,
            state: state, artifactSha256: artifactSha256, installedAt: installedAt,
            lastVerifiedAt: lastVerifiedAt, support: support, supportReason: supportReason,
            usedBy: usedBy, dependentRuntimes: dependentRuntimes)
    }
}
