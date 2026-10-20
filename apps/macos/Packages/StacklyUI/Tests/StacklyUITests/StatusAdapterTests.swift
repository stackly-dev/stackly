import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The DTO to UI status adapter: every operation state, the partial flag from the
/// fixtures, and the rule that `Adapters/` never falls back to `default:` (spec 0013
/// AC-16, AC-18).
struct StatusAdapterTests {
    /// Every operation state of the protocol and the UI status it maps to.
    nonisolated static let operationStates: [(OperationState, OperationStatus)] = [
        (.queued, .queued),
        (.running, .running),
        (.cancelRequested, .cancelRequested),
        (.succeeded, .succeeded),
        (.failed, .failed),
        (.cancelled, .cancelled),
        (.needsRecovery, .needsRecovery),
    ]

    @Test(arguments: operationStates)
    func everyOperationStateMapsToItsUIStatus(state: OperationState, status: OperationStatus) {
        #expect(StatusAdapter.operation(state) == status)
    }

    @Test func theMappingCoversEveryUIOperationStatus() {
        #expect(Set(Self.operationStates.map(\.1)) == Set(OperationStatus.allCases))
    }

    @Test func aSucceededOperationWithPartialItemsShowsAsPartial() throws {
        let done = try ContractFixture.operation(inEvent: "frame-event-operation-state-changed")
        #expect(done.result?.partial == [])
        #expect(StatusAdapter.isPartial(done) == false)
        #expect(StatusAdapter.appearance(of: done).label == "Done")

        guard
            case .operationAbandon(let abandoned) = try ContractFixture.reply(
                "frame-response-operation-abandon")
        else {
            Issue.record("the operation.abandon fixture holds another reply")
            return
        }
        let leftovers = abandoned.operation.result
        #expect(leftovers?.partial?.isEmpty == false)
        let partial = done.with(revision: 8, result: leftovers)
        #expect(StatusAdapter.isPartial(partial))
        #expect(
            StatusAdapter.appearance(of: partial) == .operation(state: .succeeded, partial: true))
        #expect(StatusAdapter.appearance(of: partial).label == "Partial")
    }

    @Test func aRunningOperationFromTheListFixtureShowsAsRunning() throws {
        let running = try #require(
            try ContractFixture.operationPage().items?.first { $0.state == .running })
        #expect(
            StatusAdapter.appearance(of: running) == .operation(state: .running, partial: false))
    }

    // MARK: - Instances

    /// Every lifecycle of the protocol and the UI status it maps to.
    nonisolated static let lifecycles: [(InstanceLifecycle, LifecycleStatus)] = [
        (.uninitialized, .uninitialized),
        (.initializing, .initializing),
        (.stopped, .stopped),
        (.starting, .starting),
        (.running, .running),
        (.stopping, .stopping),
        (.failed, .failed),
        (.recovering, .recovering),
        (.unknown, .unknown),
        (.retained, .retained),
    ]

    @Test(arguments: lifecycles)
    func everyLifecycleMapsToItsUIStatus(lifecycle: InstanceLifecycle, status: LifecycleStatus) {
        #expect(StatusAdapter.lifecycle(lifecycle) == status)
    }

    @Test func theLifecycleMappingCoversEveryUIStatus() {
        #expect(Set(Self.lifecycles.map(\.1)) == Set(LifecycleStatus.allCases))
    }

    nonisolated static func readiness(
        _ state: InstanceReadinessState, _ reason: String, _ detail: String? = nil
    )
        -> InstanceReadiness
    {
        InstanceReadiness(
            state: state, reason: reason, detail: detail,
            observedAt: Date(timeIntervalSince1970: 0))
    }

    @Test func readyAndUnknownReadinessMapDirectly() {
        #expect(StatusAdapter.readiness(Self.readiness(.ready, "probe_ok")) == .ready)
        #expect(StatusAdapter.readiness(Self.readiness(.unknown, "not_running")) == .unknown)
        // The reason and detail of a state other than not_ready change nothing.
        #expect(StatusAdapter.readiness(Self.readiness(.ready, "awaiting_probe", "x")) == .ready)
    }

    @Test func notReadyWithAwaitingProbeWaitsForTheFirstProbe() {
        let awaiting = Self.readiness(.notReady, "awaiting_probe", "ignored")
        #expect(StatusAdapter.readiness(awaiting) == .awaitingProbe)
    }

    @Test func notReadyForAnotherReasonKeepsTheDaemonsDetail() {
        let slow = Self.readiness(.notReady, "probe_failed", "connection refused")
        #expect(StatusAdapter.readiness(slow) == .notReady(detail: "connection refused"))
        let bare = Self.readiness(.notReady, "probe_failed")
        #expect(StatusAdapter.readiness(bare) == .notReady(detail: nil))
    }

    nonisolated static let healths: [(InstanceHealthState, HealthStatus)] = [
        (.unknown, .unknown),
        (.healthy, .healthy),
        (.degraded, .degraded),
        (.unhealthy, .unhealthy),
    ]

    @Test(arguments: healths)
    func everyHealthMapsToItsUIStatus(state: InstanceHealthState, status: HealthStatus) {
        let health = InstanceHealth(
            state: state, reason: "r", detail: nil, observedAt: Date(timeIntervalSince1970: 0))
        #expect(StatusAdapter.health(health) == status)
    }

    @Test func theHealthMappingCoversEveryUIStatus() {
        #expect(Set(Self.healths.map(\.1)) == Set(HealthStatus.allCases))
    }

    @Test func anInstanceFromTheFixtureShowsAsRunning() throws {
        let orders = try ContractFixture.instance()
        #expect(
            StatusAdapter.appearance(of: orders)
                == .instance(lifecycle: .running, readiness: .ready, health: .healthy))
        #expect(StatusAdapter.appearance(of: orders).label == "Running")
        let cache = try ContractFixture.instance("frame-response-instance-get-failed")
        #expect(StatusAdapter.appearance(of: cache).label == "Failed")
    }

    @Test func aRunningInstanceThatAwaitsItsFirstProbeShowsAsStarting() throws {
        let orders = try ContractFixture.instance()
        let waiting = orders.testing(readiness: Self.readiness(.notReady, "awaiting_probe"))
        let appearance = StatusAdapter.appearance(of: waiting)
        #expect(appearance.label == "Starting…")
        #expect(appearance.detail?.text == "Waiting for probe")
    }

    // MARK: - No fallback in Adapters/

    /// A `default` case in a switch, or `@unknown default`.
    static let fallback = #/(^|[^A-Za-z0-9_])default\s*:/#

    @Test func adaptersNeverFallBackToADefaultCase() throws {
        let folder = "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Adapters"
        let files = try SourceTree.files(under: folder).filter { $0.hasSuffix(".swift") }
        #expect(files.isEmpty == false, "no adapter found under \(folder)")
        for path in files {
            let text = try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
            let file = SourceFile(path: path, text: text)
            for (index, line) in file.codeLines.enumerated() where line.contains(Self.fallback) {
                Issue.record("\(path):\(index + 1) falls back to a default case: \(line)")
            }
        }
    }

    @Test func theFallbackRuleCatchesEveryFormOfDefault() {
        for example in ["default: .unknown", "    default:", "@unknown default: break"] {
            #expect(example.contains(Self.fallback), "missed: \(example)")
        }
        for example in ["case .queued: .queued", "let isDefault: Bool", "defaults: [String]"] {
            #expect(example.contains(Self.fallback) == false, "caught: \(example)")
        }
    }
}
