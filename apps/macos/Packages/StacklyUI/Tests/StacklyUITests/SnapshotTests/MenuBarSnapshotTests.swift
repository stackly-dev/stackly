import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The menu bar popover in every scenario of spec 0032 (*Fixture*, AC-26), light and dark:
/// the Services section for a normal, a failed, an offline, a long, and an empty list, the
/// Stop All preview with an instance already stopping and ones left alone, and the result
/// with two instances that did not stop.
extension SnapshotTests {
    struct MenuBarSnapshotTests {
        private static let size = SnapshotSize.fitting(
            width: MenuBarPopover.width, narrowWidth: MenuBarPopover.width)
        private static let variants: [SnapshotVariant] = [.light, .dark]

        /// Actions that do nothing: a snapshot never opens a window.
        private static let actions = MenuBarActions(
            openStackly: {}, openInstance: { _ in }, showAllInstances: {}, prepareSettings: {},
            closeWindow: {}, isMainWindowOpen: { true }, quit: {})

        @MainActor
        private static func runtime(_ id: FixtureScenario.ID) async throws -> ShellRuntime {
            try await settledRuntime(FixtureScenario(id))
        }

        @MainActor
        private static func snapshot(
            _ runtime: ShellRuntime, named name: String,
            sourceLocation: SourceLocation = #_sourceLocation
        ) async throws {
            defer { runtime.willTerminate() }
            try await assertSnapshot(
                MenuBarPopover(
                    model: runtime.menuBar, actions: actions, clearsFinishedStopAll: false
                )
                .environment(runtime.model),
                named: name, size: size, variants: variants, sourceLocation: sourceLocation)
        }

        @Test(
            .evidence(
                covers: ["REQ-027", "NFR-012"],
                expected:
                    "The popover of a connected daemon lists the services with their status and endpoints, in light and dark"
            ))
        @MainActor
        func normal() async throws {
            let runtime = try await Self.runtime(.menubarNormal)
            #expect(runtime.menuBar.iconVariant == .normal)
            try await Self.snapshot(runtime, named: "menubar-normal")
        }

        @Test(
            .evidence(
                covers: ["REQ-027", "NFR-012"],
                expected: "The popover puts a failed instance first and counts it in the header"
            ))
        @MainActor
        func failed() async throws {
            let runtime = try await Self.runtime(.menubarFailed)
            #expect(runtime.menuBar.iconVariant == .failed)
            #expect(runtime.menuBar.failedCount == 1)
            try await Self.snapshot(runtime, named: "menubar-failed")
        }

        @Test(
            .evidence(
                covers: ["REQ-027", "NFR-012"],
                expected:
                    "The popover of an unreachable daemon shows the last known state with every button off"
            ))
        @MainActor
        func offlineLastKnown() async throws {
            let runtime = try await Self.runtime(.menubarOfflineLastKnown)
            #expect(runtime.menuBar.iconVariant == .offline)
            #expect(runtime.menuBar.rows.allSatisfy { $0.button.isEnabled == false })
            try await Self.snapshot(runtime, named: "menubar-offline-last-known")
        }

        @Test(
            .evidence(
                covers: ["REQ-027", "NFR-012"],
                expected: "The popover of twelve instances shows eight and the Show all row"
            ))
        @MainActor
        func many() async throws {
            let runtime = try await Self.runtime(.menubarMany)
            #expect(runtime.menuBar.rows.count == 8)
            #expect(runtime.menuBar.hiddenCount == 4)
            try await Self.snapshot(runtime, named: "menubar-many")
        }

        @Test(
            .evidence(
                covers: ["REQ-027", "NFR-012"],
                expected: "The popover with no instance says there is none yet"
            ))
        @MainActor
        func empty() async throws {
            let runtime = try await Self.runtime(.menubarEmpty)
            #expect(runtime.menuBar.rows.isEmpty)
            try await Self.snapshot(runtime, named: "menubar-empty")
        }

        @Test(
            .evidence(
                covers: ["REQ-027", "NFR-012"],
                expected:
                    "The Stop All preview lists what stops with its demand, what is already stopping, and what is left alone"
            ))
        @MainActor
        func stopPreview() async throws {
            let runtime = try await Self.runtime(.menubarStopPreview)
            runtime.menuBar.beginStopAll()
            try await waitUntil {
                if case .preview = runtime.menuBar.phase { true } else { false }
            }
            try await Self.snapshot(runtime, named: "menubar-stop-preview")
        }

        @Test(
            .evidence(
                covers: ["REQ-027", "NFR-012"],
                expected: "The Stop All result names the two instances that did not stop"
            ))
        @MainActor
        func stopPartial() async throws {
            let runtime = try await Self.runtime(.menubarStopPartial)
            runtime.menuBar.beginStopAll()
            try await waitUntil {
                if case .preview = runtime.menuBar.phase { true } else { false }
            }
            runtime.menuBar.confirmStopAll()
            try await waitUntil {
                if case .result = runtime.menuBar.phase { true } else { false }
            }
            try await Self.snapshot(runtime, named: "menubar-stop-partial")
        }
    }
}
