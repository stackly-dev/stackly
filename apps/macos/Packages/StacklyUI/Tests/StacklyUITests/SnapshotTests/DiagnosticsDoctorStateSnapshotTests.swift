import StacklyClient
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// A Doctor run that fails while a report is shown (spec 0034, AC-17, *State seksi Doctor*):
/// the section keeps that report and the meta of its own run, so it renders exactly like the
/// report alone, in every variant; the error placeholder is only for a failure without one.
extension SnapshotTests {
    struct DiagnosticsDoctorStateSnapshotTests {
        private static let area = SnapshotSize.fitting(width: 1044, narrowWidth: 640)

        /// The Doctor section as the Diagnostics page lays it out.
        private struct Area: View {
            let doctor: DoctorModel

            var body: some View {
                DoctorSection(
                    doctor: doctor, isConnected: true, focus: nil, routes: .thisBuild,
                    open: { _, _ in }, showAll: {}
                )
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Spacing.pageHorizontal)
                .padding(.vertical, Spacing.s22)
            }
        }

        /// The replies a scripted daemon has left.
        private final class Replies {
            var left: [Result<Reply, ClientError>]

            init(_ replies: [Result<Reply, ClientError>]) { left = replies }

            func next() throws(ClientError) -> Reply {
                guard left.isEmpty == false else {
                    throw .notSent(ClientErrorCatalog.notConnected())
                }
                return try left.removeFirst().get()
            }
        }

        /// A model whose daemon answers `replies` in turn, one per run.
        private func model(_ replies: [Result<Reply, ClientError>]) async -> DoctorModel {
            let script = Replies(replies)
            let model = DoctorModel(
                read: { _ throws(ClientError) in try script.next() },
                now: { FixtureClock.now })
            for _ in replies { await model.request(.user) }
            return model
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected:
                    "A Run Doctor that fails while a report is shown keeps that report and its Last run meta, and renders exactly like the report alone in light, dark, and narrow"
            ))
        func aFailedRunKeepsTheShownReportAndItsMeta() async throws {
            if let reason = SnapshotPreflight.blockedReason(.current()) {
                try Evidence.blocked(reason: reason, owner: "user")
            }
            let failedAfter = await model([
                .success(.doctorRun(FixtureDoctor.mixed)),
                .failure(.outcomeUnknown(FixtureDoctor.runFailed)),
            ])
            #expect(failedAfter.failure?.code == FixtureDoctor.runFailed.code)
            #expect(failedAfter.report == FixtureDoctor.mixed)
            #expect(failedAfter.lastRunAt == FixtureDoctor.mixed.finishedAt)
            #expect(failedAfter.running == false)
            let reportOnly = await model([.success(.doctorRun(FixtureDoctor.mixed))])
            #expect(reportOnly.failure == nil)

            for variant in SnapshotVariant.standard {
                let failed = try await render(
                    Area(doctor: failedAfter), variant: variant, size: Self.area)
                let shown = try await render(
                    Area(doctor: reportOnly), variant: variant, size: Self.area)
                #expect(
                    failed == shown,
                    "\(variant.suffix): the failed run changes nothing on screen")
            }
            let firstFailure = await model([.failure(.outcomeUnknown(FixtureDoctor.runFailed))])
            #expect(firstFailure.report == nil)
            let placeholder = try await render(
                Area(doctor: firstFailure), variant: .light, size: Self.area)
            let shown = try await render(
                Area(doctor: reportOnly), variant: .light, size: Self.area)
            #expect(placeholder != shown, "without a report the failure shows instead")
        }
    }
}
