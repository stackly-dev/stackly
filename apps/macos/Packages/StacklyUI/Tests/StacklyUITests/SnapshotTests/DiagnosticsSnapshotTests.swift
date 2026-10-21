import SwiftUI
import Testing

@testable import StacklyUI

/// The Diagnostics page, fed from a settled fixture runtime (spec 0068, AC-8).
extension SnapshotTests {
    struct DiagnosticsSnapshotTests {
        private func check(_ id: FixtureScenario.ID, named name: String) async throws {
            let runtime = try await settledRuntime(FixtureScenario(id))
            defer { runtime.willTerminate() }
            let model = runtime.model
            try await assertSnapshot(
                DiagnosticsPage(
                    diagnostics: model.diagnostics.pinningFixtureContract(),
                    isReconnecting: model.presentation.isReconnecting,
                    reconnect: {},
                    refresh: {}),
                named: name)
        }

        @Test(
            .evidence(
                covers: ["REQ-024", "NFR-012"],
                expected: "Diagnostics of a connected daemon renders in every variant"
            ))
        func diagnosticsWhenConnected() async throws {
            try await check(.connectedMixed, named: "diagnostics-connected-mixed")
        }

        @Test(
            .evidence(
                covers: ["REQ-024", "NFR-012"],
                expected: "Diagnostics after the daemon went offline renders in every variant"
            ))
        func diagnosticsWhenOffline() async throws {
            try await check(.daemonOffline, named: "diagnostics-daemon-offline")
        }

        @Test(
            .evidence(
                covers: ["REQ-024", "NFR-012"],
                expected: "Diagnostics of an untrusted socket renders in every variant"
            ))
        func diagnosticsWhenUntrusted() async throws {
            try await check(.daemonUntrusted, named: "diagnostics-daemon-untrusted")
        }
    }
}
