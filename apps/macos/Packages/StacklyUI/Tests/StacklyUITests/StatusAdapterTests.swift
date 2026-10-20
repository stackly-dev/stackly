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
