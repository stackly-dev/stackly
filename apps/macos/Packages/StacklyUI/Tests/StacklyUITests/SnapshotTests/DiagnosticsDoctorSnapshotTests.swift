import StacklyClient
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// The Doctor and Diagnostic Bundle sections, the finding sheets, and the bundle sheet in each
/// state of spec 0034 (AC-21), from ``FixtureDoctor`` (spec 0068).
extension SnapshotTests {
    struct DiagnosticsDoctorSnapshotTests {
        private static let area = SnapshotSize.fitting(width: 1044, narrowWidth: 640)
        private static let sheet = SnapshotSize.fitting(width: 640, narrowWidth: 560)

        /// The lower part of the Diagnostics page: the Doctor and the Diagnostic Bundle.
        private struct DoctorArea: View {
            let doctor: DoctorModel
            var isConnected = true
            var focus: DiagnosticsFocus?

            var body: some View {
                VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                    DoctorSection(
                        doctor: doctor, isConnected: isConnected, focus: focus,
                        routes: .thisBuild, open: { _, _ in }, showAll: {})
                    DiagnosticBundleSection(isConnected: isConnected, preview: {})
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Spacing.pageHorizontal)
                .padding(.vertical, Spacing.s22)
            }
        }

        /// A save panel the snapshots never open.
        private struct NoPanel: BundleDestinationChoosing {
            func choose(suggestedName: String) async -> BundleDestination? { nil }
        }

        private func area(
            _ state: FixtureDoctorModels.Doctor, named name: String, isConnected: Bool = true,
            focus: DiagnosticsFocus? = nil
        ) async throws {
            let doctor = await FixtureDoctorModels.doctor(state)
            try await assertSnapshot(
                DoctorArea(doctor: doctor, isConnected: isConnected, focus: focus),
                named: name, size: Self.area)
        }

        // MARK: - Doctor section

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected: "Before the first run the Doctor says so and offers Run Doctor"
            ))
        func neverRun() async throws {
            try await area(.neverRun, named: "doctor-never-run")
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected: "The first run shows the delayed spinner with what the doctor checks"
            ))
        func runningWithoutAReport() async throws {
            try await area(.running, named: "doctor-running")
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected:
                    "A mixed report shows the port conflict with Use Port, the drift with Review, and every other check passed"
            ))
        func mixedReport() async throws {
            try await area(.report(FixtureDoctor.mixed), named: "doctor-mixed")
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected: "A report where every check passed shows No issues"
            ))
        func allChecksPassed() async throws {
            try await area(.report(FixtureDoctor.allOK), named: "doctor-all-ok")
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected:
                    "In recovery mode the registry Error shows and the checks that need it read as unavailable warnings"
            ))
        func recoveryMode() async throws {
            try await area(.report(FixtureDoctor.recovery), named: "doctor-recovery")
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected: "A focus on an instance shows its findings only, with Show All"
            ))
        func instanceFocus() async throws {
            try await area(
                .report(FixtureDoctor.mixed), named: "doctor-instance-focus",
                focus: .instance(FixtureDoctor.mysqlErp.id))
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected:
                    "Without stacklyd the last report stays, not dimmed, with stacklyd isn’t connected, and Preview Bundle is off"
            ))
        func disconnectedWithTheLastReport() async throws {
            try await area(
                .report(FixtureDoctor.mixed), named: "doctor-disconnected", isConnected: false)
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected: "An older stacklyd shows Doctor Unavailable with what to do"
            ))
        func unsupported() async throws {
            try await area(.unsupported, named: "doctor-unsupported")
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected: "A first run that failed shows the error with Retry"
            ))
        func failedWithoutAReport() async throws {
            try await area(.failed, named: "doctor-failed")
        }

        // MARK: - Finding sheets

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected:
                    "The pid_reused sheet shows both processes and every identity check, with Show Instance"
            ))
        func pidReusedSheet() async throws {
            try await assertSnapshot(
                DoctorFindingSheet(
                    finding: FixtureDoctor.pidReused, routes: .thisBuild, open: { _ in }),
                named: "finding-pid-reused", size: Self.sheet)
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected:
                    "The start_failed sheet shows the last output and the provider's remedy with copyable commands and paths"
            ))
        func startFailedSheet() async throws {
            try await assertSnapshot(
                DoctorFindingSheet(
                    finding: FixtureDoctor.startFailed, routes: .thisBuild, open: { _ in }),
                named: "finding-start-failed", size: Self.sheet)
        }

        // MARK: - Bundle sheet

        private func bundle(_ state: FixtureDoctorModels.Bundle, named name: String)
            async throws
        {
            let model = await FixtureDoctorModels.bundle(state)
            try await assertSnapshot(
                BundleSheet(model: model, chooser: NoPanel(), exported: { _ in }),
                named: name, size: Self.sheet)
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected: "The bundle sheet shows the spinner while stacklyd builds the preview"
            ))
        func bundleSheetLoading() async throws {
            try await bundle(.loading, named: "bundle-sheet-loading")
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected:
                    "The bundle sheet lists every entry with its path, description, and size, the redaction note, and Export…"
            ))
        func bundleSheetList() async throws {
            try await bundle(.previewed, named: "bundle-sheet-list")
        }

        @Test(
            .evidence(
                covers: ["REQ-019", "AC-019"],
                expected: "A preview that failed shows the error in the sheet with Try Again"
            ))
        func bundleSheetError() async throws {
            try await bundle(.failed, named: "bundle-sheet-error")
        }
    }
}
