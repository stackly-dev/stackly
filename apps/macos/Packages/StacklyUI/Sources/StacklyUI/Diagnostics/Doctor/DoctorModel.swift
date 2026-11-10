import Foundation
import StacklyClient

/// The doctor of the Diagnostics page (spec 0034, *GUI*, AC-17, AC-20): the last report of
/// `doctor.run`, whether a run is going, and why the last one failed.
///
/// A run always checks everything (no `instanceId`); a focus only filters on the client. At
/// most one call runs. A trigger that comes during a run is ignored, except Run Doctor, which
/// starts one more run after that one. The report stays while a new run goes and while the
/// daemon is away. A daemon that does not know the method makes the model unsupported until a
/// new daemon run. The model belongs to ``DaemonModel``, so the report and the badge survive a
/// change of page.
@Observable
public final class DoctorModel {
    /// What asks for a run.
    public enum Trigger: Equatable, Sendable {
        /// Run Doctor.
        case user
        /// The shell caught up with the daemon run `serverInstanceId`; only a new run counts.
        case caughtUp(serverInstanceId: UUID?)
        /// Diagnostics appeared; only without a report or with one older than 5 minutes.
        case appeared
        /// Something opened Diagnostics on purpose, such as a failure banner.
        case opened
    }

    /// How old a report may be before Diagnostics runs the doctor again when it appears.
    static let staleAfter: TimeInterval = 5 * 60

    /// The last report.
    public private(set) var report: DoctorReport?
    /// Whether a run is going.
    public private(set) var running = false
    /// The error of the last run, or `nil` after a success.
    public private(set) var failure: ErrorBody?
    /// Whether the daemon does not know `doctor.run`.
    public private(set) var unsupported = false

    @ObservationIgnored private var queued = false
    @ObservationIgnored private var lastServerInstanceId: UUID??
    @ObservationIgnored private let read: @MainActor (Call) async throws(ClientError) -> Reply
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private weak var capacity: CapacityModel?

    /// Creates a model that has not run.
    /// - Parameters:
    ///   - read: Sends one call to the daemon.
    ///   - capacity: Takes the `CapacityReport` of the `disk_space` check, or `nil`.
    ///   - now: The clock.
    public init(
        read: @escaping @MainActor (Call) async throws(ClientError) -> Reply,
        capacity: CapacityModel? = nil,
        now: @escaping () -> Date = { Date() }
    ) {
        self.read = read
        self.capacity = capacity
        self.now = now
    }

    /// When the last report finished.
    public var lastRunAt: Date? { report?.finishedAt }

    /// The Error findings of the last report, for the sidebar badge.
    public var errorCount: Int { Int(clamping: report?.errors ?? 0) }

    /// Starts a run if `trigger` calls for one.
    public func request(_ trigger: Trigger) async {
        switch trigger {
        case .caughtUp(let id):
            if lastServerInstanceId == .some(id) { return }
            lastServerInstanceId = .some(id)
            unsupported = false
        case .appeared:
            if let report, now().timeIntervalSince(report.finishedAt) <= Self.staleAfter {
                return
            }
        case .user, .opened:
            break
        }
        if unsupported { return }
        if running {
            if trigger == .user { queued = true }
            return
        }
        await run()
    }

    private func run() async {
        running = true
        defer { running = false }
        repeat {
            queued = false
            await runOnce()
        } while queued && !unsupported
    }

    private func runOnce() async {
        do {
            let reply = try await read(.doctorRun(DoctorParams(instanceId: nil, checks: nil)))
            guard case .doctorRun(let fresh) = reply else {
                failure = ClientErrorCatalog.protocolViolation()
                return
            }
            report = fresh
            failure = nil
            if let capacity = fresh.checks?.first(where: { $0.id == .diskSpace })?.capacity {
                self.capacity?.accept(capacity)
            }
        } catch {
            switch error {
            case .rejected(let body) where body.code == "request.unknown_method":
                unsupported = true
                failure = nil
            case .rejected(let body), .notSent(let body), .outcomeUnknown(let body):
                failure = body
            }
        }
    }
}
