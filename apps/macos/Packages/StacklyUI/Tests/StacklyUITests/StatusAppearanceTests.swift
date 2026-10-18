import AppKit
import Testing

@testable import StacklyUI

/// The instance and operation status tables of spec 0012 (AC-4).
struct StatusAppearanceTests {
    /// One row of the instance status table, with every input that maps to it.
    nonisolated struct InstanceRow: CustomTestStringConvertible, Sendable {
        let lifecycle: LifecycleStatus
        let inputs: [(ReadinessStatus, HealthStatus)]
        let label: String
        let tone: Tone
        let symbol: String
        let detail: StatusDetail?

        var testDescription: String {
            guard lifecycle == .running, let (readiness, health) = inputs.first else {
                return "\(lifecycle)"
            }
            return "running, \(readiness), \(health)"
        }
    }

    /// Every readiness and health pair, for rows that read neither.
    nonisolated static let anyReadinessAndHealth: [(ReadinessStatus, HealthStatus)] =
        [ReadinessStatus.ready, .awaitingProbe, .notReady(detail: "Crash loop · paused"), .unknown]
        .flatMap { readiness in HealthStatus.allCases.map { (readiness, $0) } }

    /// Every health, for rows that do not read it.
    nonisolated static func anyHealth(_ readiness: ReadinessStatus) -> [(
        ReadinessStatus, HealthStatus
    )] {
        HealthStatus.allCases.map { (readiness, $0) }
    }

    nonisolated static let instanceRows = [
        InstanceRow(
            lifecycle: .uninitialized,
            inputs: anyReadinessAndHealth,
            label: "Not Initialized",
            tone: .neutral,
            symbol: "circle.dashed",
            detail: nil
        ),
        InstanceRow(
            lifecycle: .initializing,
            inputs: anyReadinessAndHealth,
            label: "Initializing…",
            tone: .caution,
            symbol: "hourglass.circle.fill",
            detail: nil
        ),
        InstanceRow(
            lifecycle: .stopped,
            inputs: anyReadinessAndHealth,
            label: "Stopped",
            tone: .neutral,
            symbol: "stop.circle.fill",
            detail: nil
        ),
        InstanceRow(
            lifecycle: .starting,
            inputs: anyReadinessAndHealth,
            label: "Starting…",
            tone: .caution,
            symbol: "hourglass.circle.fill",
            detail: nil
        ),
        InstanceRow(
            lifecycle: .running,
            inputs: anyHealth(.awaitingProbe),
            label: "Starting…",
            tone: .caution,
            symbol: "hourglass.circle.fill",
            detail: StatusDetail(text: "Waiting for probe", tone: .caution)
        ),
        InstanceRow(
            lifecycle: .running,
            inputs: anyHealth(.notReady(detail: "Crash loop · paused")),
            label: "Not Ready",
            tone: .caution,
            symbol: "exclamationmark.circle.fill",
            detail: StatusDetail(text: "Crash loop · paused", tone: .caution)
        ),
        InstanceRow(
            lifecycle: .running,
            inputs: anyHealth(.notReady(detail: nil)) + anyHealth(.notReady(detail: ""))
                + anyHealth(.notReady(detail: " \n")),
            label: "Not Ready",
            tone: .caution,
            symbol: "exclamationmark.circle.fill",
            detail: StatusDetail(text: "Readiness check failed", tone: .caution)
        ),
        InstanceRow(
            lifecycle: .running,
            inputs: anyHealth(.unknown),
            label: "Running",
            tone: .caution,
            symbol: "questionmark.circle.fill",
            detail: StatusDetail(text: "Readiness unknown", tone: .caution)
        ),
        InstanceRow(
            lifecycle: .running,
            inputs: [(.ready, .healthy), (.ready, .unknown)],
            label: "Running",
            tone: .positive,
            symbol: "checkmark.circle.fill",
            detail: nil
        ),
        InstanceRow(
            lifecycle: .running,
            inputs: [(.ready, .degraded)],
            label: "Running",
            tone: .positive,
            symbol: "checkmark.circle.fill",
            detail: StatusDetail(text: "Degraded", tone: .caution)
        ),
        InstanceRow(
            lifecycle: .running,
            inputs: [(.ready, .unhealthy)],
            label: "Running",
            tone: .positive,
            symbol: "checkmark.circle.fill",
            detail: StatusDetail(text: "Unhealthy", tone: .critical)
        ),
        InstanceRow(
            lifecycle: .recovering,
            inputs: anyReadinessAndHealth,
            label: "Recovering…",
            tone: .caution,
            symbol: "arrow.clockwise.circle.fill",
            detail: nil
        ),
        InstanceRow(
            lifecycle: .stopping,
            inputs: anyReadinessAndHealth,
            label: "Stopping…",
            tone: .caution,
            symbol: "hourglass.circle.fill",
            detail: nil
        ),
        InstanceRow(
            lifecycle: .failed,
            inputs: anyReadinessAndHealth,
            label: "Failed",
            tone: .critical,
            symbol: "xmark.circle.fill",
            detail: nil
        ),
        InstanceRow(
            lifecycle: .unknown,
            inputs: anyReadinessAndHealth,
            label: "Unknown",
            tone: .caution,
            symbol: "questionmark.circle.fill",
            detail: StatusDetail(text: "Process identity not confirmed", tone: .caution)
        ),
        InstanceRow(
            lifecycle: .retained,
            inputs: anyReadinessAndHealth,
            label: "Retained",
            tone: .neutral,
            symbol: "archivebox.circle.fill",
            detail: nil
        ),
    ]

    /// One row of the operation status table.
    nonisolated struct OperationRow: CustomTestStringConvertible, Sendable {
        let state: OperationStatus
        let partial: Bool
        let label: String
        let tone: Tone

        var testDescription: String { partial ? "\(state), partial" : "\(state)" }
    }

    nonisolated static let operationRows = [
        OperationRow(state: .queued, partial: false, label: "Queued", tone: .neutral),
        OperationRow(state: .running, partial: false, label: "Running", tone: .informative),
        OperationRow(
            state: .cancelRequested,
            partial: false,
            label: "Cancelling…",
            tone: .caution
        ),
        OperationRow(state: .succeeded, partial: false, label: "Done", tone: .neutral),
        OperationRow(state: .succeeded, partial: true, label: "Partial", tone: .caution),
        OperationRow(state: .failed, partial: false, label: "Failed", tone: .critical),
        OperationRow(state: .cancelled, partial: false, label: "Cancelled", tone: .neutral),
        OperationRow(
            state: .needsRecovery,
            partial: false,
            label: "Needs Recovery",
            tone: .critical
        ),
    ]

    @Test(arguments: instanceRows)
    func instanceStatusMatchesItsTableRow(row: InstanceRow) {
        #expect(row.inputs.isEmpty == false)
        for (readiness, health) in row.inputs {
            let appearance = StatusAppearance.instance(
                lifecycle: row.lifecycle,
                readiness: readiness,
                health: health
            )
            #expect(
                appearance
                    == StatusAppearance(
                        label: row.label,
                        tone: row.tone,
                        symbol: row.symbol,
                        detail: row.detail
                    ),
                "readiness \(readiness), health \(health)"
            )
        }
    }

    @Test func instanceTableCoversEveryLifecycleState() {
        let covered = Set(Self.instanceRows.map(\.lifecycle))
        #expect(covered == Set(LifecycleStatus.allCases))
    }

    @Test func instanceTableCoversEveryRunningReadinessAndHealth() {
        let covered = Self.instanceRows.filter { $0.lifecycle == .running }.flatMap(\.inputs)
        for (readiness, health) in Self.anyReadinessAndHealth {
            #expect(
                covered.contains { $0.0 == readiness && $0.1 == health },
                "running, \(readiness), \(health) has no row"
            )
        }
    }

    @Test func notReadyDetailFromTheDaemonIsShownAsItCame() {
        let detail = "Probe failed: connection refused on 127.0.0.1:5433 after 3 attempts"
        let appearance = StatusAppearance.instance(
            lifecycle: .running,
            readiness: .notReady(detail: detail),
            health: .unhealthy
        )
        #expect(appearance.detail == StatusDetail(text: detail, tone: .caution))
    }

    @Test(arguments: operationRows)
    func operationStatusMatchesItsTableRow(row: OperationRow) {
        let appearance = StatusAppearance.operation(state: row.state, partial: row.partial)
        #expect(appearance.label == row.label)
        #expect(appearance.tone == row.tone)
        #expect(appearance.symbol == row.tone.symbol)
        #expect(appearance.detail == nil)
    }

    @Test(arguments: OperationStatus.allCases.filter { $0 != .succeeded })
    func partialIsIgnoredOutsideSucceeded(state: OperationStatus) {
        #expect(
            StatusAppearance.operation(state: state, partial: true)
                == StatusAppearance.operation(state: state, partial: false)
        )
    }

    @Test func operationTableCoversEveryState() {
        #expect(Set(Self.operationRows.map(\.state)) == Set(OperationStatus.allCases))
    }

    @Test func transitionalLabelsUseTheEllipsisCharacter() {
        let labels = Self.instanceRows.map(\.label) + Self.operationRows.map(\.label)
        for label in labels {
            #expect(label.contains("...") == false, "\(label) uses three periods")
        }
        #expect(labels.filter { $0.hasSuffix("…") }.count == 6)
    }

    @Test func everyStatusSymbolExistsInSFSymbols() {
        var symbols = Set(Self.instanceRows.map(\.symbol))
        symbols.formUnion(Self.instanceRows.compactMap(\.detail).map(\.tone.symbol))
        symbols.formUnion(Self.operationRows.map(\.tone.symbol))
        for symbol in symbols.sorted() {
            #expect(
                NSImage(systemSymbolName: symbol, accessibilityDescription: nil) != nil,
                "\(symbol) is not an SF Symbol on this macOS"
            )
        }
    }
}
