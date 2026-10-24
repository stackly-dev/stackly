import Foundation
import Observation
import StacklyClient

/// What the Needs Attention rules read about the disk: the last report and how the last
/// call ended.
nonisolated public struct CapacityState: Equatable, Sendable {
    /// The last successful report, kept while a later call fails.
    public var report: CapacityReport?
    /// The error of the last call, or `nil` when it succeeded.
    public var failure: ErrorBody?
    /// Whether the daemon does not know `storage.get_capacity`.
    public var unsupported: Bool
    /// Whether a call ended at least once, in success or failure.
    public var attempted: Bool

    /// Creates a state; nothing is known by default.
    public init(
        report: CapacityReport? = nil, failure: ErrorBody? = nil, unsupported: Bool = false,
        attempted: Bool = false
    ) {
        self.report = report
        self.failure = failure
        self.unsupported = unsupported
        self.attempted = attempted
    }
}

/// The space left on the disk that holds Stackly data, read through `storage.get_capacity`
/// (spec 0022, *Kapasitas disk*). The daemon owns the level; this model never applies a
/// threshold.
/// The Overview asks for a reading when it appears, every 60 seconds while it shows and the
/// daemon is connected, and when a snapshot round completes. At most one call runs, and a
/// reply older than the report held is ignored. The model belongs to ``DaemonModel``, so it
/// survives a change of page.
@Observable
public final class CapacityModel {
    /// The time between two readings while the Overview shows.
    static var pollInterval: Duration { .seconds(60) }

    /// The last successful report.
    public private(set) var report: CapacityReport?
    /// The error of the last call, or `nil` after a success.
    public private(set) var failure: ErrorBody?
    /// Whether the daemon does not know the method.
    public private(set) var unsupported = false
    /// Whether a call ended at least once.
    public private(set) var attempted = false

    @ObservationIgnored private var isReading = false
    @ObservationIgnored private let read: @MainActor (Call) async throws(ClientError) -> Reply
    @ObservationIgnored private let sleep: @MainActor (Duration) async throws -> Void

    /// Creates a model that has not read anything.
    /// - Parameters:
    ///   - read: Sends one call to the daemon.
    ///   - sleep: Waits; tests inject one that does not.
    public init(
        read: @escaping @MainActor (Call) async throws(ClientError) -> Reply,
        sleep: @escaping @MainActor (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.read = read
        self.sleep = sleep
    }

    /// What the rules read.
    public var state: CapacityState {
        CapacityState(
            report: report, failure: failure, unsupported: unsupported, attempted: attempted)
    }

    /// Reads the disk now. A call that is already running makes this one return at once.
    public func refresh() async {
        guard isReading == false else { return }
        isReading = true
        defer { isReading = false }
        do {
            let reply = try await read(.storageGetCapacity)
            guard case .storageGetCapacity(let fresh) = reply else {
                failure = ClientErrorCatalog.protocolViolation()
                attempted = true
                return
            }
            if let held = report, fresh.checkedAt < held.checkedAt {
                attempted = true
                return
            }
            report = fresh
            failure = nil
            unsupported = false
        } catch {
            switch error {
            case .rejected(let body) where body.code == "request.unknown_method":
                unsupported = true
                failure = nil
            case .rejected(let body), .notSent(let body), .outcomeUnknown(let body):
                failure = body
            }
        }
        attempted = true
    }

    /// Reads the disk now and again every 60 seconds, for as long as the task runs and the
    /// daemon is connected. The page runs it with `.task(id:)` on the connected state, so it
    /// starts again when the daemon comes back.
    /// - Parameter isConnected: Whether the presence is connected.
    public func poll(isConnected: @MainActor () -> Bool) async {
        while Task.isCancelled == false, isConnected() {
            await refresh()
            do {
                try await sleep(Self.pollInterval)
            } catch {
                return
            }
        }
    }
}
