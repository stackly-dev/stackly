import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The Welcome and Storage steps of the onboarding in the fixture runtime (spec 0016, AC-4,
/// AC-6, AC-10). The Background Agent, Catalog Trust, and Command Line steps join with their
/// milestones, and milestone 4 adds the window-level suite.
extension SnapshotTests {
    struct OnboardingSnapshotTests {
        /// A fixed icon, so the reference does not depend on the app that hosts the test.
        private static var icon: NSImage {
            let configuration = NSImage.SymbolConfiguration(pointSize: 80, weight: .regular)
            return NSImage(systemSymbolName: "shippingbox.fill", accessibilityDescription: nil)?
                .withSymbolConfiguration(configuration) ?? NSImage()
        }

        private func check(_ step: OnboardingStep, named name: String) async throws {
            // The scenario shows the onboarding, so it names no presence to settle on: wait
            // until the daemon answered, which is when the Storage step has its folders.
            let runtime = ShellBootstrap.fixture(FixtureScenario(.onboarding))
            defer { runtime.willTerminate() }
            let model = runtime.model
            try await waitUntil { model.lastDaemon?.info != nil }
            runtime.onboarding.go(to: step)
            try await assertSnapshot(
                OnboardingFlow(onboarding: runtime.onboarding, appIcon: Self.icon)
                    .environment(\.fixtureNow, FixtureClock.now)
                    .environment(\.loadingDelay, .seconds(3600))
                    .environment(model)
                    .environment(runtime.commandLineTool),
                named: name, size: .fitting(width: 1280, narrowWidth: 900))
        }

        @Test(
            .evidence(
                covers: ["REQ-024", "NFR-012"],
                expected: "The Welcome step renders in every variant"
            ))
        func welcomeStep() async throws {
            try await check(.welcome, named: "onboarding-welcome")
        }

        @Test(
            .evidence(
                covers: ["REQ-014", "NFR-012"],
                expected: "The Background Agent step of a fresh install renders in every variant"
            ))
        func agentStep() async throws {
            try await check(.backgroundAgent, named: "onboarding-agent")
        }

        @Test(
            .evidence(
                covers: ["REQ-032", "NFR-012"],
                expected: "The Command Line step of a fresh install renders in every variant"
            ))
        func commandLineStep() async throws {
            try await check(.commandLine, named: "onboarding-command-line")
        }

        @Test(
            .evidence(
                covers: ["REQ-024", "NFR-012"],
                expected: "The Storage step renders in every variant"
            ))
        func storageStep() async throws {
            try await check(.storage, named: "onboarding-storage")
        }
    }
}
