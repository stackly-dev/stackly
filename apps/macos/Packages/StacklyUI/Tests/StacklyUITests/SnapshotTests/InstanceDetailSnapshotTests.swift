import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The detail page of an instance in every variant (spec 0021 AC-8, AC-11 to AC-16, spec 0068
/// AC-8): the Summary tab for each state of an instance, the states of the page itself, and
/// the page column next to an open inspector.
///
/// A page shows all four cards, so it renders on a canvas tall enough for the single column
/// of 640 pt. The inspector variant renders at 664 pt, the content column of a 900 pt
/// window, which leaves the 384 pt of the page column (spec 0021, *Aksesibilitas*).
extension SnapshotTests {
    struct InstanceDetailSnapshotTests {
        /// The page these snapshots show, named so the source rule finds its suite.
        private typealias Page = InstanceDetailPage

        private static let canvas = SnapshotSize.fitting(width: 1_044, narrowWidth: 640)
        private static let withInspector = SnapshotSize.fitting(width: 1_044, narrowWidth: 664)

        private static func id(_ number: UInt8) -> UUID {
            FixtureID.uuid(kind: FixtureID.instance, number: number)
        }

        @MainActor
        private func rig(
            instances: [InstanceSummary] = FixtureScenario.instanceCases,
            prepare: (PageRig) -> Void = { _ in }
        ) async -> PageRig {
            let rig = PageRig(instances: instances)
            prepare(rig)
            await rig.loadInstances()
            return rig
        }

        /// The Instances page on the detail page of instance `number`, with the inspector
        /// closed, on a canvas as tall as the single column needs.
        @MainActor
        private func detail(_ rig: PageRig, _ number: UInt8, height: CGFloat = 1_200)
            -> some View
        {
            rig.instancesPage(opening: Self.id(number), showsInspector: false)
                .frame(height: height)
        }

        // MARK: - Summary tab

        @Test(
            .evidence(
                covers: ["AC-014"],
                expected:
                    "The Autostart row of an instance that starts at login shows the toggle on and its note"
            ))
        func detailOfAnInstanceThatStartsAtLogin() async throws {
            var instances = FixtureScenario.instanceCases
            instances[0] = instances[0].startingAtLogin(.none)
            let rig = await rig(instances: instances)
            try await assertSnapshot(
                detail(rig, 1), named: "instance-detail-autostart", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012", "AC-012"],
                expected: "The detail of a running, ready instance renders in every variant"))
        func detailOfAReadyInstance() async throws {
            let rig = await rig()
            try await assertSnapshot(
                detail(rig, 1), named: "instance-detail-ready", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012", "AC-012", "REQ-012", "BR-010"],
                expected:
                    "The detail of a running instance that waits for its probe shows Process and Readiness apart and is never Ready"
            ))
        func detailOfAnInstanceWaitingForItsProbe() async throws {
            let rig = await rig()
            try await assertSnapshot(
                detail(rig, 2), named: "instance-detail-starting", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012", "REQ-012"],
                expected:
                    "The detail of a running instance whose probe fails renders in every variant"
            ))
        func detailOfAnInstanceThatIsNotReady() async throws {
            let rig = await rig()
            try await assertSnapshot(
                detail(rig, 10), named: "instance-detail-not-ready", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail of a stopped instance whose provider describes no connection string renders in every variant"
            ))
        func detailOfAStoppedInstanceWithoutAConnection() async throws {
            let rig = await rig()
            try await assertSnapshot(
                detail(rig, 4), named: "instance-detail-stopped", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["AC-027", "NFR-012"],
                expected:
                    "The detail of an instance still stopping without an operation shows the Still stopping banner with Stop Again and Force Stop… in every variant"
            ))
        func detailOfAnInstanceStillStopping() async throws {
            let rig = await rig()
            try await assertSnapshot(
                detail(rig, 3), named: "instance-detail-still-stopping", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Details card of a stopped instance whose last stop was forced shows Last exit Forced, and a crashed one Crashed, in every variant"
            ))
        func detailOfAnInstanceWhoseLastExitWasForcedOrCrashed() async throws {
            for (exit, name) in [
                (InstanceLastExit.forced, "instance-detail-last-exit-forced"),
                (.crashed, "instance-detail-last-exit-crashed"),
            ] {
                let instances = FixtureScenario.instanceCases.map {
                    $0.id == Self.id(4) ? $0.testing(lastExit: .some(exit)) : $0
                }
                let rig = await rig(instances: instances)
                try await assertSnapshot(detail(rig, 4), named: name, size: Self.canvas)
            }
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail of a failed instance with its output and Retry renders in every variant"
            ))
        func detailOfAFailedInstance() async throws {
            let rig = await rig()
            try await assertSnapshot(
                detail(rig, 5), named: "instance-detail-failed", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail of an instance whose start failed on a port that is taken renders in every variant"
            ))
        func detailOfAnInstanceWhoseStartFailedOnATakenPort() async throws {
            let rig = await rig(
                instances: FixtureScenario.instanceCases + [FixtureStartConflict.instance])
            try await assertSnapshot(
                detail(rig, FixtureStartConflict.number), named: "instance-detail-port-conflict",
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012", "AC-068", "AC-010", "VAL-020"],
                expected:
                    "The failure banner of each state F1 to F7 (crash loop, port with Use Port or Change Port, after a port change, data in use, other, refused start, restart) renders in every variant without a cut label or code line"
            ))
        func detailOfEachFailureBanner() async throws {
            for fixture in FixtureFailureBanner.allCases {
                let rig = await rig(
                    instances: FixtureScenario.instanceCases + [fixture.instance],
                    prepare: { $0.script.instanceOperations = fixture.operations })
                try await assertSnapshot(
                    detail(rig, fixture.number),
                    named: "instance-detail-failure-\(fixture.snapshotName)", size: Self.canvas)
            }
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail of an instance that needs a secret turns Show Password and Copy off with the reason"
            ))
        func detailOfAnInstanceThatNeedsASecret() async throws {
            let rig = await rig()
            try await assertSnapshot(
                detail(rig, 9), named: "instance-detail-needs-secret", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail of an instance whose data another process holds renders in every variant"
            ))
        func detailOfAnInstanceWhoseDataIsInUse() async throws {
            let rig = await rig()
            try await assertSnapshot(
                detail(rig, 7), named: "instance-detail-data-in-use", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail of an instance whose process is not confirmed renders in every variant"
            ))
        func detailOfAnInstanceWhoseProcessIsNotConfirmed() async throws {
            let rig = await rig()
            try await assertSnapshot(
                detail(rig, 8), named: "instance-detail-unconfirmed", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail of an instance with a NATS style string that has no port yet renders in every variant"
            ))
        func detailOfAnInstanceWhoseConnectionHasNoPortYet() async throws {
            let rig = await rig(instances: [Self.natsWithoutPort])
            try await assertSnapshot(
                detail(rig, 20), named: "instance-detail-no-port", size: Self.canvas)
        }

        /// A stopped NATS instance whose port the daemon assigns at start.
        private static let natsWithoutPort: InstanceSummary = {
            let base = InstanceSummary.fixture(
                number: 20, name: "events", provider: "official/nats", runtimeVersion: "2.11.9",
                port: nil)
            return base.replacingEndpoints([
                EndpointStatus(
                    name: "client", transport: .tcp, host: "127.0.0.1", port: nil,
                    portMode: .auto, bound: false, socketPath: nil)
            ])
        }()

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail of an instance whose provider package cannot be read says so in the Connection card"
            ))
        func detailOfAnInstanceWhosePackageCannotBeRead() async throws {
            let rig = await rig {
                let summary = FixtureScenario.instanceCases[3]
                $0.script.detailOverrides[summary.id] = FixtureDetail.detail(
                    of: summary, packageReadable: false)
            }
            try await assertSnapshot(
                detail(rig, 4), named: "instance-detail-package-unreadable", size: Self.canvas)
        }

        // MARK: - States of the page

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail page whose detail failed to load keeps its rows and shows Retry"
            ))
        func detailThatFailedToLoad() async throws {
            let rig = await rig {
                $0.script.detailFailure = .rejected(PageRig.failure)
            }
            try await assertSnapshot(
                detail(rig, 1), named: "instance-detail-load-failed", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail page whose detail has not arrived shows the rows of the summary at once"
            ))
        func detailThatHasNotArrived() async throws {
            let rig = await rig {
                $0.script.detailHangs = true
            }
            try await assertSnapshot(
                detail(rig, 1), named: "instance-detail-loading", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The detail page of an instance nobody knows says Instance Not Found"))
        func detailOfAnInstanceThatDoesNotExist() async throws {
            let rig = await rig()
            let unknown = FixtureID.uuid(kind: FixtureID.instance, number: 99)
            try await assertSnapshot(
                rig.instancesPage(opening: unknown, showsInspector: false).frame(height: 748),
                named: "instance-detail-not-found", size: Self.canvas)
        }

        // MARK: - Opened from a request

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Instances page opened on an instance by a request of the navigator shows its detail"
            ))
        func detailOpenedByTheNavigator() async throws {
            let rig = await rig()
            try await assertSnapshot(
                OpenedByRequest(page: rig.instancesPage(showsInspector: false), id: Self.id(1))
                    .frame(height: 1_200),
                named: "instance-detail-from-request", size: Self.canvas)
        }

        // MARK: - Next to the inspector

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail page in the 384 pt column beside the open inspector renders without cut text"
            ))
        func detailBesideTheInspector() async throws {
            let rig = await rig()
            try await assertSnapshot(
                rig.instancesPage(
                    opening: Self.id(1), selection: Self.id(1).uuidString.lowercased(),
                    showsInspector: true
                )
                .frame(height: 1_500),
                named: "instance-detail-with-inspector", size: Self.withInspector)
        }
    }
}

/// A page in a window of its own with a navigator that is asked to open an instance when it
/// appears: every variant renders a window of its own, so each gets a new navigator and a new
/// request, which the page consumes.
private struct OpenedByRequest<Page: View>: View {
    @State private var navigator = ShellNavigator(section: .instances)
    let page: Page
    let id: UUID

    var body: some View {
        page
            .environment(\.shellNavigator, navigator)
            .onAppear { navigator.openInstance(id) }
    }
}

extension InstanceSummary {
    /// A copy with other endpoints; every other field stays.
    fileprivate func replacingEndpoints(_ endpoints: [EndpointStatus]?) -> InstanceSummary {
        InstanceSummary(
            id: id, name: name, displayName: displayName, revision: revision, software: software,
            provider: provider, runtimeId: runtimeId, runtimeVersion: runtimeVersion,
            lifecycle: lifecycle, desired: desired, readiness: readiness, health: health,
            statusSeq: statusSeq, process: process, lastFailure: lastFailure, restart: restart,
            restartTotal: 0,
            blocker: blocker, endpoints: endpoints, leftoverProcesses: leftoverProcesses,
            logsUnavailable: logsUnavailable, ownership: ownership, initialized: initialized,
            capabilities: nil, lastExit: nil, autostart: autostart)
    }

    /// A copy that starts at login, with `session` as its lease; every other field stays.
    fileprivate func startingAtLogin(_ session: AutostartSession) -> InstanceSummary {
        InstanceSummary(
            id: id, name: name, displayName: displayName, revision: revision, software: software,
            provider: provider, runtimeId: runtimeId, runtimeVersion: runtimeVersion,
            lifecycle: lifecycle, desired: desired, readiness: readiness, health: health,
            statusSeq: statusSeq, process: process, lastFailure: lastFailure, restart: restart,
            restartTotal: restartTotal, blocker: blocker, endpoints: endpoints,
            leftoverProcesses: leftoverProcesses, logsUnavailable: logsUnavailable,
            ownership: ownership, initialized: initialized, capabilities: capabilities,
            lastExit: lastExit, autostart: InstanceAutostart(policy: .atLogin, session: session))
    }
}

extension FixtureFailureBanner {
    /// The case name in the kebab case of snapshot names, such as `port-in-use`.
    var snapshotName: String {
        rawValue.reduce(into: "") { name, character in
            if character.isUppercase {
                name += "-" + character.lowercased()
            } else {
                name.append(character)
            }
        }
    }
}
