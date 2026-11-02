import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// Restart Daemon in every variant (spec 0030 AC-16, spec 0068): the Daemon section of
/// Settings for each connection, the confirmation sheet with a blocking operation, the window
/// banner in each state of a restart, and the status of a row the restart stopped.
extension SnapshotTests {
    struct RestartSnapshotTests {
        private typealias F = FixtureRestart

        private static var isPrimed = false

        /// Renders a sheet once and keeps nothing, for the reason the New Instance snapshots
        /// give: the first sheet a process draws looks different from every later one.
        @MainActor
        private static func prime() async throws {
            guard !isPrimed else { return }
            isPrimed = true
            let model = restartModel()
            await model.begin()
            _ = try await render(
                RestartConfirmationSheet(restart: model), variant: .light,
                size: .fitting(width: 520, narrowWidth: 520))
        }

        @MainActor
        private static func restartModel(
            _ preview: RestartPreview = F.preview()
        ) -> DaemonRestartModel {
            DaemonRestartModel(
                preview: { () throws(ClientError) in preview },
                restart: { _, _, _ in .notSent(ClientErrorCatalog.notConnected()) },
                now: { FixtureClock.now })
        }

        // MARK: - The sheet

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Restart Daemon sheet lists what keeps running, what stops, and the operations in flight in every variant"
            ))
        @MainActor
        func theSheetWithABlockingOperation() async throws {
            try await Self.prime()
            let blocking = [
                BlockingOperation(operationId: UUID(), kind: .createInstance, resumable: false),
                BlockingOperation(operationId: UUID(), kind: .installRuntime, resumable: true),
            ]
            let model = Self.restartModel(F.preview(blocking: blocking))
            await model.begin()
            try await assertSnapshot(
                RestartConfirmationSheet(restart: model), named: "restart-sheet-blocking",
                size: .fitting(width: 520, narrowWidth: 520))
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Restart Daemon sheet shows the new plan after the daemon changed it, and the daemon's refusal"
            ))
        @MainActor
        func theSheetAfterThePlanChangedAndARefusal() async throws {
            try await Self.prime()
            let model = DaemonRestartModel(
                preview: { () throws(ClientError) in F.preview() },
                restart: { _, _, _ in
                    .rejected(
                        ErrorBody(
                            code: "daemon.restart_busy", category: .conflict,
                            message: "an operation that cannot resume is still running",
                            retryable: false, resource: nil, fields: nil, hint: nil,
                            details: nil, operationId: nil, rule: nil, items: nil))
                },
                now: { FixtureClock.now })
            await model.begin()
            await model.confirm()
            try await assertSnapshot(
                RestartConfirmationSheet(restart: model), named: "restart-sheet-busy",
                size: .fitting(width: 520, narrowWidth: 520))
        }

        // MARK: - The banner

        private struct Banners: View {
            let rows: [(String, RestartBannerContent)]

            var body: some View {
                VStack(alignment: .leading, spacing: Spacing.s14) {
                    ForEach(rows, id: \.0) { row in
                        VStack(alignment: .leading, spacing: Spacing.s4) {
                            Text(row.0).font(.caption).foregroundStyle(StacklyColor.textSecondary)
                            RestartBanner(row.1, dismiss: {})
                        }
                    }
                }
                .frame(width: 640)
                .padding(Spacing.pageHorizontal)
                .background(StacklyColor.surfaceWindow)
            }
        }

        private static func content(
            _ plan: RestartPlanView?, awaiting: Bool = false
        ) -> RestartBannerContent {
            guard
                let content = RestartBannerContent.make(
                    plan: plan, isAwaitingPlan: awaiting, dismissedPlanID: nil,
                    now: FixtureClock.now)
            else { fatalError("the fixture plan always has a banner") }
            return content
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The restart banner renders while waiting, while the daemon is away, while the instances come back, and when the restart failed, was cancelled, or expired"
            ))
        @MainActor
        func theBannerInEveryState() async throws {
            let failed = [
                F.instance(1, "pg17-dev", outcome: .adopted),
                F.instance(
                    3, "valkey-cache", decision: .stopRestore, reason: .handoverUnproven,
                    stopped: true, outcome: .failed, failureCode: "endpoint_in_use"),
            ]
            let coming = [
                F.instance(1, "pg17-dev", outcome: .adopted),
                F.instance(
                    3, "valkey-cache", decision: .stopRestore, reason: .handoverUnproven,
                    stopped: true),
            ]
            let rows: [(String, RestartBannerContent)] = [
                ("waiting", Self.content(nil, awaiting: true)),
                ("away", Self.content(F.plan(.handedOver, instances: F.mixedInstances))),
                ("restoring", Self.content(F.plan(.restoring, instances: coming))),
                ("failed", Self.content(F.plan(.finished, instances: failed))),
                (
                    "aborted",
                    Self.content(
                        F.plan(
                            .aborted,
                            instances: F.mixedInstances.dropLast()
                                + [
                                    F.instance(
                                        2, "nats-bus", decision: .stopRestore,
                                        reason: .handoverUnproven, stopped: true),
                                    F.instance(
                                        3, "valkey-cache", decision: .stopRestore,
                                        reason: .handoverUnproven),
                                ], closeReason: .stopTimeout))
                ),
                (
                    "expired",
                    Self.content(
                        F.plan(.expired, instances: F.mixedInstances, closeReason: .bootChanged))
                ),
            ]
            try await assertSnapshot(
                Banners(rows: rows), named: "restart-banner-states",
                size: .fitting(width: 700, narrowWidth: 640))
        }

        // MARK: - The Settings row

        private struct Sections: View {
            let models: [(String, DaemonModel)]

            var body: some View {
                VStack(alignment: .leading, spacing: Spacing.s14) {
                    ForEach(models, id: \.0) { entry in
                        Form { DaemonSettingsSection(model: entry.1) }
                            .formStyle(.grouped)
                            .scrollDisabled(true)
                            .frame(height: 110)
                    }
                }
                .frame(width: SettingsRoot.size.width)
                .background(StacklyColor.surfaceWindow)
            }
        }

        @MainActor
        private static func settingsModel(
            connected: Bool, launch: LaunchContextKind = .launchd
        ) -> DaemonModel {
            let link = DaemonModelTests.FakeLink()
            let model = DaemonModel(
                link: link.handle, signals: nil, endpoint: nil,
                launchedAt: DaemonModelTests.launch)
            if connected {
                var daemon = DaemonModelTests.daemon(.ready)
                daemon.info = DaemonRestartModelTests.info(restart: nil, context: launch)
                model.receive(.connected(daemon))
            } else {
                model.receive(DaemonModelTests.closed())
            }
            return model
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Daemon section of Settings renders connected, disconnected, and outside launchd"
            ))
        @MainActor
        func theDaemonSectionForEachConnection() async throws {
            try await assertSnapshot(
                Sections(models: [
                    ("connected", Self.settingsModel(connected: true)),
                    ("disconnected", Self.settingsModel(connected: false)),
                    ("manual", Self.settingsModel(connected: true, launch: .manual)),
                ]),
                named: "settings-daemon-section",
                size: .fitting(width: SettingsRoot.size.width, narrowWidth: SettingsRoot.size.width)
            )
        }

        // MARK: - The row

        private struct Marks: View {
            var body: some View {
                let stopped = InstanceSummary.fixture(
                    number: 3, name: "valkey-cache", provider: "official/valkey",
                    lifecycle: .stopped, desired: .running)
                VStack(alignment: .leading, spacing: Spacing.s8) {
                    StatusLabel(
                        InstanceRowModel(stopped, provider: nil, refreshFailed: false).appearance)
                    StatusLabel(
                        InstanceRowModel(
                            stopped, provider: nil, refreshFailed: false, stoppedForRestart: true
                        ).appearance)
                }
                .padding(Spacing.pageHorizontal)
                .frame(width: 320, alignment: .leading)
                .background(StacklyColor.surfaceWindow)
            }
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The status of an instance the restart stopped says why, in every variant"
            ))
        @MainActor
        func theStatusOfAStoppedForRestartRow() async throws {
            try await assertSnapshot(
                Marks(), named: "restart-row-mark", size: .fitting(width: 320, narrowWidth: 320))
        }
    }
}
