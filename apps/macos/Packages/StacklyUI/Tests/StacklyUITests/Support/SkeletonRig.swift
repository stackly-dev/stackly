import Darwin
import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// What the `skeleton_seed --json` example prints (spec 0014, *Seed*).
struct SeedReport: Decodable {
    /// The provider the seed pinned.
    struct Provider: Decodable {
        let name: String
        let revision: String
    }

    let root: String
    let runtimeId: UUID
    let provider: Provider
}

/// A problem the rig reports with a message the test output shows.
struct RigFailure: Error, CustomStringConvertible {
    let description: String
}

/// The walking skeleton's real parts for one test: a private root seeded by the
/// `skeleton_seed` example, a real `stacklyd` on that root, and a second client
/// connection (not interactive) that makes the changes the GUI model must follow.
///
/// Binaries come from `STACKLY_DAEMON_BIN` and `STACKLY_SEED_BIN`, with
/// `target/debug/stacklyd` and `target/debug/examples/skeleton_seed` as defaults. A
/// missing binary ends the test as `BLOCKED` evidence. ``shutDown()`` stops every
/// instance the rig made and the daemon, so no fake server outlives the test.
final class SkeletonRig {
    /// The real daemon, on the rig's private root.
    let daemon: LiveDaemon
    /// What the seed wrote into the root.
    let seed: SeedReport
    /// The test's own connection, which makes the changes. ``restartDaemon()`` replaces it.
    private(set) var connection: DaemonConnection
    /// The operations this connection started, in order.
    private(set) var startedOperations: [UUID] = []
    /// The instances this connection created.
    private(set) var createdInstances: [UUID] = []

    private init(daemon: LiveDaemon, seed: SeedReport, connection: DaemonConnection) {
        self.daemon = daemon
        self.seed = seed
        self.connection = connection
    }

    /// Seeds a fresh root, starts the daemon on it, and opens the second connection.
    ///
    /// - Parameter recordsEvidence: Whether the test carries the evidence trait.
    static func start(recordsEvidence: Bool) async throws -> SkeletonRig {
        let seedBinary = Self.seedBinaryURL
        let seedPath = seedBinary.path(percentEncoded: false)
        guard FileManager.default.isExecutableFile(atPath: seedPath) else {
            try LiveDaemon.unavailable(
                "no skeleton_seed binary at \(seedPath); build it with cargo build -p stacklyd --example skeleton_seed",
                owner: "feature 14", recordsEvidence: recordsEvidence)
        }
        let fakeServer = seedBinary.deletingLastPathComponent().deletingLastPathComponent()
            .appending(path: "stackly-fake-server").path(percentEncoded: false)
        guard FileManager.default.isExecutableFile(atPath: fakeServer) else {
            try LiveDaemon.unavailable(
                "no stackly-fake-server binary at \(fakeServer); build it with cargo build -p stackly-fake-server",
                owner: "feature 14", recordsEvidence: recordsEvidence)
        }
        let daemon = try LiveDaemon(recordsEvidence: recordsEvidence)
        let outcome = try await ProcessRunner.run(
            seedBinary, arguments: ["--root", daemon.root, "--json"], timeout: .seconds(60))
        guard outcome.status == 0, outcome.timedOut == false else {
            throw RigFailure(
                description:
                    "skeleton_seed ended with status \(outcome.status) (timed out: \(outcome.timedOut)): \(outcome.output)"
            )
        }
        let seed = try JSONDecoder().decode(SeedReport.self, from: Data(outcome.output.utf8))
        try await daemon.start()
        let client = ClientInfo(
            kind: .cli, version: "0.1.0", pid: UInt32(ProcessInfo.processInfo.processIdentifier),
            interactive: false)
        let (connection, _) = try await DaemonConnection.open(
            endpoint: daemon.endpoint, policy: .sameUser, client: client)
        return SkeletonRig(daemon: daemon, seed: seed, connection: connection)
    }

    /// Runs `body` on a fresh rig and always shuts the rig down afterwards.
    static func run<Result>(
        recordsEvidence: Bool, _ body: (SkeletonRig) async throws -> Result
    ) async throws -> Result {
        let rig = try await start(recordsEvidence: recordsEvidence)
        do {
            let result = try await body(rig)
            await rig.shutDown()
            return result
        } catch {
            await rig.shutDown()
            throw error
        }
    }

    // MARK: - Changes

    /// Creates an instance of the seeded fake provider and waits until the operation
    /// succeeds.
    ///
    /// - Parameters:
    ///   - name: The instance name.
    ///   - settings: Provider settings; the secret fields are the daemon's to generate.
    /// - Returns: The instance ID.
    func create(_ name: String, settings: [String: JSONValue] = [:]) async throws -> UUID {
        let params = CreateInstanceParams(
            name: name, displayName: nil,
            provider: ProviderSelector(name: seed.provider.name, revision: seed.provider.revision),
            runtimeId: seed.runtimeId,
            endpoints: [EndpointRequest(name: "http", port: .auto)],
            settings: JSONObject(settings), secrets: nil)
        let reply = try await send(.instanceCreate(params), key: Self.newKey())
        guard case .instanceCreate(let action) = reply, let accepted = action.accepted else {
            throw RigFailure(description: "instance.create gave no operation for \(name)")
        }
        startedOperations.append(accepted.operation.id)
        try await requireSucceeded(accepted.operation.id, "create \(name)")
        let page = try await listInstances()
        guard let made = page.first(where: { $0.name == name }) else {
            throw RigFailure(description: "instance.list does not show \(name) after its create")
        }
        createdInstances.append(made.id)
        return made.id
    }

    /// Starts an instance and returns the operation without waiting for it.
    @discardableResult
    func beginStart(_ id: UUID) async throws -> UUID {
        let reply = try await send(
            .instanceStart(StartInstanceParams(instanceId: id, expectedRevision: nil)),
            key: Self.newKey())
        return try accept(reply, "start")
    }

    /// Starts an instance and waits until the operation succeeds.
    func start(_ id: UUID) async throws {
        try await requireSucceeded(try await beginStart(id), "start \(id)")
    }

    /// Stops an instance and waits until the operation succeeds.
    func stop(_ id: UUID) async throws {
        let reply = try await send(
            .instanceStop(
                StopInstanceParams(instanceId: id, expectedRevision: nil, overrideLeases: false)),
            key: Self.newKey())
        try await requireSucceeded(try accept(reply, "stop"), "stop \(id)")
    }

    private func accept(_ reply: Reply, _ what: String) throws -> UUID {
        switch reply {
        case .instanceStart(let action), .instanceStop(let action):
            guard let accepted = action.accepted else {
                throw RigFailure(description: "instance.\(what) started no operation")
            }
            startedOperations.append(accepted.operation.id)
            return accepted.operation.id
        default:
            throw RigFailure(description: "instance.\(what) gave the wrong reply")
        }
    }

    /// Sends `call`, again with the same key while the daemon answers `daemon.starting`
    /// (its supervisor is still reconciling), for at most 20 seconds.
    private func send(_ call: Call, key: String? = nil) async throws -> Reply {
        let deadline = ContinuousClock.now + .seconds(20)
        while true {
            do {
                return try await connection.call(call, idempotencyKey: key)
            } catch ClientError.rejected(let body)
                where body.code == "daemon.starting" && ContinuousClock.now < deadline
            {
                try await Task.sleep(for: .milliseconds(100))
            }
        }
    }

    /// Starts an instance whose start must be refused for a secret it cannot read, and
    /// returns the error code, whether the daemon refuses the call or the operation
    /// ends failed.
    func startRefusedCode(_ id: UUID) async throws -> String {
        let reply: Reply
        do {
            reply = try await send(
                .instanceStart(StartInstanceParams(instanceId: id, expectedRevision: nil)),
                key: Self.newKey())
        } catch ClientError.rejected(let body) {
            return body.code
        }
        let operation = try accept(reply, "start")
        let deadline = ContinuousClock.now + .seconds(60)
        while true {
            let answer = try await send(.operationGet(GetOperationParams(operationId: operation)))
            guard case .operationGet(let detail) = answer else {
                throw RigFailure(description: "operation.get gave the wrong reply")
            }
            switch detail.operation.state {
            case .failed, .cancelled, .needsRecovery:
                return detail.operation.failure?.error.code ?? "no failure code"
            case .succeeded:
                throw RigFailure(description: "start \(id) succeeded but was to be refused")
            case .queued, .running, .cancelRequested:
                break
            }
            guard ContinuousClock.now < deadline else {
                throw RigFailure(description: "start \(id) did not finish within 60 seconds")
            }
            try await Task.sleep(for: .milliseconds(50))
        }
    }

    // MARK: - Keychain and restart

    /// The file keychain of the rig's root (spec 0011).
    var keychainFile: String {
        "\(daemon.root)/data/keychain/stackly.keychain-db"
    }

    /// Locks the file keychain with `security lock-keychain`, the way a locked login
    /// session would leave it, so secrets cannot be read until the daemon opens it again.
    func lockKeychain() async throws {
        let outcome = try await ProcessRunner.run(
            URL(filePath: "/usr/bin/security"), arguments: ["lock-keychain", keychainFile],
            timeout: .seconds(30))
        guard outcome.status == 0, outcome.timedOut == false else {
            throw RigFailure(
                description:
                    "security lock-keychain ended with status \(outcome.status): \(outcome.output)"
            )
        }
    }

    /// Stops the daemon the usual way and starts it again on the same root, which opens
    /// the file keychain again; the test's connection is replaced with a new one.
    func restartDaemon() async throws {
        await connection.close()
        await daemon.stop()
        try await daemon.start()
        let client = ClientInfo(
            kind: .cli, version: "0.1.0", pid: UInt32(ProcessInfo.processInfo.processIdentifier),
            interactive: false)
        let (fresh, _) = try await DaemonConnection.open(
            endpoint: daemon.endpoint, policy: .sameUser, client: client)
        connection = fresh
    }

    // MARK: - Reads

    /// One instance through `instance.get`.
    func instance(_ id: UUID) async throws -> InstanceSummary {
        let reply = try await send(.instanceGet(GetInstanceParams(instanceId: id)))
        guard case .instanceGet(let summary) = reply else {
            throw RigFailure(description: "instance.get gave the wrong reply")
        }
        return summary
    }

    /// Every instance through `instance.list`.
    func listInstances() async throws -> [InstanceSummary] {
        let reply = try await send(
            .instanceList(ListInstancesParams(limit: 200, before: nil)))
        guard case .instanceList(let page) = reply else {
            throw RigFailure(description: "instance.list gave the wrong reply")
        }
        return page.items ?? []
    }

    /// The IDs of every operation `operation.list` shows.
    func operationIDs() async throws -> Set<UUID> {
        let reply = try await send(
            .operationList(
                ListOperationsParams(activeOnly: nil, resource: nil, limit: 200, before: nil)))
        guard case .operationList(let page) = reply else {
            throw RigFailure(description: "operation.list gave the wrong reply")
        }
        return Set((page.items ?? []).map(\.id))
    }

    /// Waits up to `seconds` until `check` holds for the instance.
    func waitForInstance(
        _ id: UUID, _ what: String, seconds: Double = 30,
        until check: (InstanceSummary) -> Bool
    ) async throws -> InstanceSummary {
        let deadline = ContinuousClock.now + .milliseconds(Int64(seconds * 1000))
        while true {
            let current = try await instance(id)
            if check(current) { return current }
            guard ContinuousClock.now < deadline else {
                throw RigFailure(
                    description:
                        "timed out after \(seconds) s waiting for \(what); lifecycle \(current.lifecycle), readiness \(current.readiness.state)"
                )
            }
            try await Task.sleep(for: .milliseconds(50))
        }
    }

    private func requireSucceeded(_ id: UUID, _ what: String) async throws {
        let deadline = ContinuousClock.now + .seconds(60)
        while true {
            let reply = try await send(.operationGet(GetOperationParams(operationId: id)))
            guard case .operationGet(let detail) = reply else {
                throw RigFailure(description: "operation.get gave the wrong reply")
            }
            switch detail.operation.state {
            case .succeeded:
                return
            case .failed, .cancelled, .needsRecovery:
                throw RigFailure(
                    description:
                        "\(what) ended \(detail.operation.state) (\(detail.operation.failure?.error.code ?? "no failure code"))"
                )
            case .queued, .running, .cancelRequested:
                break
            }
            guard ContinuousClock.now < deadline else {
                throw RigFailure(description: "\(what) did not finish within 60 seconds")
            }
            try await Task.sleep(for: .milliseconds(50))
        }
    }

    /// `GET /health` on the first endpoint of `instance`, as the status code.
    static func healthStatus(of instance: InstanceSummary) async throws -> Int {
        guard let endpoint = (instance.endpoints ?? []).first, let port = endpoint.port else {
            throw RigFailure(description: "\(instance.name) has no endpoint port")
        }
        let host = endpoint.host ?? "127.0.0.1"
        let url = try #require(URL(string: "http://\(host):\(port)/health"))
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 5
        let (_, response) = try await URLSession(configuration: configuration).data(from: url)
        return (response as? HTTPURLResponse)?.statusCode ?? 0
    }

    // MARK: - Teardown

    /// Stops every instance the rig made, kills any process still left, closes the
    /// connection, and stops the daemon.
    func shutDown() async {
        for id in createdInstances {
            guard let current = try? await instance(id), current.lifecycle != .stopped else {
                continue
            }
            try? await stop(id)
            if let after = try? await instance(id), after.lifecycle != .stopped,
                let pid = after.process?.pid
            {
                Darwin.kill(pid_t(pid), SIGKILL)
            }
        }
        await connection.close()
        await daemon.stop()
    }

    // MARK: - Locations

    private static var seedBinaryURL: URL {
        if let override = ProcessInfo.processInfo.environment["STACKLY_SEED_BIN"],
            override.hasPrefix("/")
        {
            return URL(filePath: override)
        }
        return SourceTree.url(of: "target/debug/examples/skeleton_seed")
    }

    private static func newKey() -> String {
        UUID().uuidString.lowercased()
    }
}

/// Follows one instance in an ``InstanceStore`` and keeps each distinct pair of
/// lifecycle and readiness it passes through.
///
/// It records from the store's `didApply` hook, in the same turn as each change the store
/// applies. A sampler on a timer lost states whenever a busy main actor ran the store's
/// changes and the test's next check back to back: the last state before ``finish()``
/// above all.
final class InstanceTrail {
    /// One observed state.
    struct Step: Equatable, CustomStringConvertible {
        let lifecycle: InstanceLifecycle
        let readiness: InstanceReadinessState

        var description: String { "\(lifecycle)/\(readiness)" }
    }

    /// The steps seen, in order, without repeats.
    private(set) var steps: [Step] = []
    private var name = ""
    private var store: InstanceStore?
    private var earlierObserver: ((StoreChange<InstanceSummary>) -> Void)?

    /// Starts following the instance named `name`: records its state now and after every
    /// change `store` applies. The store's earlier observer still gets every change.
    func follow(_ name: String, in store: InstanceStore) {
        self.name = name
        self.store = store
        let earlier = store.store.didApply
        earlierObserver = earlier
        store.store.didApply = { [weak self] change in
            earlier?(change)
            self?.record()
        }
        record()
    }

    /// Stops following and gives the store back its earlier observer.
    func finish() {
        guard let store else { return }
        store.store.didApply = earlierObserver
        self.store = nil
        earlierObserver = nil
    }

    /// The position of the first step that matches, or `nil`.
    func firstIndex(_ lifecycle: InstanceLifecycle, _ readiness: InstanceReadinessState) -> Int? {
        steps.firstIndex(of: Step(lifecycle: lifecycle, readiness: readiness))
    }

    private func record() {
        guard let item = store?.instances.first(where: { $0.name == name }) else { return }
        let step = Step(lifecycle: item.lifecycle, readiness: item.readiness.state)
        if steps.last != step { steps.append(step) }
    }
}
