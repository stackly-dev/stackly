import Foundation
import ServiceManagement
import Synchronization

/// The live ``BackgroundAgentControlling``: `SMAppService.agent(plistName:)` for the bundle
/// the app runs from (spec 0015, *GUI*). It is the only file that imports
/// `ServiceManagement`.
/// Each call checks the bundle first, like the Rust adapter: the bundle identifier, the
/// LaunchAgent plist, and `stacklyd` must be there. The OS call runs off the main actor
/// with a 10 second limit; a late answer is dropped and the OS call finishes on its own.
nonisolated final class SMAppServiceAgent: BackgroundAgentControlling {
    /// The longest a call waits for the OS.
    static let callLimit: Duration = .seconds(10)

    private static let errorDomain = "SMAppServiceErrorDomain"

    private let bundleURL: URL
    private let bundleIdentifier: String?
    private let limit: Duration

    /// Creates the agent for the running app.
    init() {
        self.bundleURL = CanonicalPath.resolve(Bundle.main.bundleURL)
        self.bundleIdentifier = Bundle.main.bundleIdentifier
        self.limit = Self.callLimit
    }

    /// Creates the agent for another bundle, with its own limit, for tests.
    init(bundleURL: URL, bundleIdentifier: String?, limit: Duration = SMAppServiceAgent.callLimit) {
        self.bundleURL = CanonicalPath.resolve(bundleURL)
        self.bundleIdentifier = bundleIdentifier
        self.limit = limit
    }

    // MARK: - BackgroundAgentControlling

    @concurrent func status() async throws(AgentActionError) -> AgentProbe {
        try await call(.status) { _ in }
    }

    @concurrent func register() async throws(AgentActionError) -> AgentProbe {
        try await call(.register) { service in
            try service.register()
        }
    }

    @concurrent func unregister() async throws(AgentActionError) -> AgentProbe {
        try await call(.unregister) { service in
            try service.unregister()
        }
    }

    @concurrent func relaunch() async throws(AgentActionError) -> AgentProbe {
        try await call(.relaunch) { service in
            do {
                try service.unregister()
            } catch let error as NSError where Self.kind(of: error) == .notFound {
                // Nothing was registered, so there is nothing to undo.
            }
            try service.register()
        }
    }

    func openApprovalSettings() {
        SMAppService.openSystemSettingsLoginItems()
    }

    // MARK: - The bundle

    /// Why the agent cannot work for the bundle at `bundleURL`, or `nil` when it can: the
    /// bundle must have the app's identifier, the LaunchAgent plist, and `stacklyd`.
    static func locate(bundleURL: URL, bundleIdentifier: String?) -> AgentProbe? {
        guard bundleIdentifier == AgentIdentifiers.appBundleID else {
            return .notBundled(executable: bundleURL.path)
        }
        let fileManager = FileManager.default
        let plist = bundleURL.appending(path: AgentIdentifiers.launchAgentsDir)
            .appending(path: AgentIdentifiers.daemonPlistFile)
        let daemon = bundleURL.appending(path: AgentIdentifiers.daemonBundleProgram)
        guard fileManager.fileExists(atPath: plist.path),
            fileManager.isExecutableFile(atPath: daemon.path)
        else {
            return .helperMissing(bundle: bundleURL.path)
        }
        return nil
    }

    /// Runs `change` on a background queue with the limit, then reads the status again.
    /// `change` throws an `NSError` when the OS refuses.
    private func call(
        _ op: AgentOp,
        _ change: @escaping @Sendable (SMAppService) throws -> Void
    ) async throws(AgentActionError) -> AgentProbe {
        if let problem = Self.locate(bundleURL: bundleURL, bundleIdentifier: bundleIdentifier) {
            return problem
        }
        let bundlePath = bundleURL.path
        let limit = self.limit
        let result: Result<AgentProbe, AgentActionError> = await withCheckedContinuation {
            continuation in
            let once = Once(continuation)
            DispatchQueue.global(qos: .userInitiated).async {
                let service = SMAppService.agent(plistName: AgentIdentifiers.daemonPlistFile)
                do {
                    try change(service)
                    once.finish(
                        .success(
                            .registration(Self.registration(service.status), bundle: bundlePath)))
                } catch {
                    once.finish(.failure(Self.map(error, op: op)))
                }
            }
            DispatchQueue.global().asyncAfter(deadline: .now() + limit.timeInterval) {
                once.finish(.failure(.timedOut(op: op)))
            }
        }
        return try result.get()
    }

    // MARK: - Mapping

    /// The registration for the OS status.
    static func registration(_ status: SMAppService.Status) -> AgentRegistration {
        switch status {
        case .enabled: .enabled
        case .requiresApproval: .requiresApproval
        case .notRegistered: .notRegistered
        case .notFound: .notFound
        @unknown default: .unrecognized(status.rawValue)
        }
    }

    /// The kind of an OS error, by the constants of `SMErrors.h`.
    static func kind(of error: NSError) -> AgentOsErrorKind {
        guard error.domain == errorDomain else { return .other }
        switch error.code {
        case kSMErrorAlreadyRegistered: return .alreadyRegistered
        case kSMErrorLaunchDeniedByUser: return .launchDenied
        case kSMErrorJobMustBeEnabled: return .mustBeEnabled
        case kSMErrorInvalidSignature: return .invalidSignature
        case kSMErrorJobNotFound, kSMErrorJobPlistNotFound: return .notFound
        default: return .other
        }
    }

    private static func map(_ error: any Error, op: AgentOp) -> AgentActionError {
        let ns = error as NSError
        return .os(op: op, kind: kind(of: ns), domain: ns.domain, code: ns.code)
    }

    /// Resumes a continuation at most once, whichever of the answer and the limit comes
    /// first.
    private final class Once: Sendable {
        private let continuation:
            Mutex<CheckedContinuation<Result<AgentProbe, AgentActionError>, Never>?>

        init(_ continuation: CheckedContinuation<Result<AgentProbe, AgentActionError>, Never>) {
            self.continuation = Mutex(continuation)
        }

        func finish(_ result: Result<AgentProbe, AgentActionError>) {
            let taken = continuation.withLock {
                value -> CheckedContinuation<Result<AgentProbe, AgentActionError>, Never>? in
                defer { value = nil }
                return value
            }
            taken?.resume(returning: result)
        }
    }
}

extension Duration {
    /// The duration in seconds, for `DispatchTime` arithmetic.
    nonisolated fileprivate var timeInterval: TimeInterval {
        let parts = components
        return TimeInterval(parts.seconds) + TimeInterval(parts.attoseconds) / 1e18
    }
}
