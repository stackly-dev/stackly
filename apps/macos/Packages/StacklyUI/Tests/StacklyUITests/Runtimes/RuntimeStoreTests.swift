import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The runtime store (spec 0018 AC-17, AC-18): snapshots, the `runtime.state_changed`
/// refetch, the joins the Catalog reads, and the Get action with its key and feedback.
@Suite(.timeLimit(.minutes(1)))
struct RuntimeStoreTests {
    /// What the daemon answers and what it was sent.
    final class Script {
        var page = FixtureRuntime.page
        var failure: ClientError?
        var details: [UUID: RuntimeDetail] = [:]
        var reads: [String] = []
        var sent: [(call: Call, key: String)] = []
        var outcomes: [MutationOutcome] = []
        var streams: [UUID: AsyncStream<OperationSnapshot>.Continuation] = [:]
        var feedback: [Feedback] = []
        var clock = Date(timeIntervalSince1970: 1_000)
        var holdNextSend = false
        var gate: CheckedContinuation<Void, Never>?

        func open() {
            gate?.resume()
            gate = nil
        }

        func read(_ call: Call) throws(ClientError) -> Reply {
            reads.append(call.methodName)
            if let failure { throw failure }
            switch call {
            case .runtimeList:
                return .runtimeList(page)
            case .runtimeGet(let params):
                guard let detail = details[params.runtimeId] else {
                    throw .rejected(Self.notFound)
                }
                return .runtimeGet(detail)
            default:
                throw .rejected(ClientErrorCatalog.protocolViolation())
            }
        }

        func send(_ call: Call, _ key: String) async -> MutationOutcome {
            sent.append((call, key))
            if holdNextSend {
                holdNextSend = false
                await withCheckedContinuation { gate = $0 }
            }
            let outcome: MutationOutcome =
                outcomes.isEmpty
                ? .notSent(ClientErrorCatalog.notConnected()) : outcomes.removeFirst()
            if case .outcomeUnknown(let body, key: "") = outcome {
                return .outcomeUnknown(body, key: key)
            }
            return outcome
        }

        func updates(_ id: UUID) -> AsyncStream<OperationSnapshot> {
            let (stream, continuation) = AsyncStream.makeStream(of: OperationSnapshot.self)
            streams[id] = continuation
            return stream
        }

        static let notFound = ErrorBody(
            code: "resource.not_found", category: .notFound, message: "no such runtime",
            retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
            operationId: nil, rule: nil, items: nil)
    }

    let script = Script()
    let context: InstanceActionContext
    let reported = Reported()

    final class Reported {
        var feedback: [Feedback] = []
    }

    init() {
        let reported = reported
        context = InstanceActionContext(report: { reported.feedback.append($0) })
    }

    func makeStore() -> RuntimeStore {
        let script = script
        return RuntimeStore(
            call: { call throws(ClientError) in try script.read(call) },
            send: { call, key in await script.send(call, key) },
            updates: { script.updates($0) },
            now: { script.clock }, sleep: { _ in })
    }

    func loaded() async -> RuntimeStore {
        let store = makeStore()
        await store.store.resnapshot()
        return store
    }

    static let postgres = FixtureRuntime.summaries[0]
    static let installing = FixtureRuntime.installing

    // MARK: - Snapshot

    @Test func theSubtitleWaitsForTheFirstSnapshotThenCountsTheRuntimes() async {
        let store = makeStore()
        #expect(store.subtitle == nil)
        #expect(store.loadedCount == nil)
        await store.store.resnapshot()
        #expect(store.subtitle == "4 installed")
        #expect(store.loadedCount == 4)
    }

    @Test func theFirstPageNamesTheInstallsThatHaveNotEnded() async {
        let store = await loaded()
        #expect(store.installing == [Self.installing])
        #expect(store.installingRuntime(sha256: Self.installing.sha256) == Self.installing)
        #expect(store.installingRuntime(sha256: String(repeating: "0", count: 64)) == nil)
    }

    @Test func theRuntimesComeByNameThenNewestVersionFirst() async {
        let newer = RuntimeSummary.fixture(number: 20, software: "postgresql", version: "18.1")
        script.page = RuntimeListPage(
            items: [FixtureRuntime.summaries[1], Self.postgres, newer], next: nil, installing: [])
        let store = await loaded()
        #expect(store.runtimes.map(\.displayName) == ["MySQL", "PostgreSQL", "PostgreSQL"])
        #expect(store.runtimes.map(\.version) == ["8.4.6", "18.1", "17.6"])
    }

    @Test func aFailedSnapshotKeepsTheErrorAndAnOlderDaemonMarksItUnsupported() async {
        script.failure = .rejected(
            ErrorBody(
                code: "request.unknown_method", category: .unsupported, message: "unknown method",
                retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
                operationId: nil, rule: nil, items: nil))
        let store = await loaded()
        #expect(store.store.unsupported)
        #expect(store.subtitle == nil)
    }

    // MARK: - Joins

    @Test func installedCountJoinsOnTheSoftwareAndCountsOnlyInstalledRuntimes() async {
        var broken = RuntimeSummary.fixture(number: 21, software: "postgresql", version: "16.1")
        broken = broken.with(state: .failed)
        script.page = RuntimeListPage(
            items: [Self.postgres, broken], next: nil, installing: [])
        let store = await loaded()
        #expect(store.installedCount(software: "postgresql") == 1)
        #expect(store.installedCount(software: "nats") == 0)
        #expect(store.installedCount(software: nil) == 0)
    }

    @Test func anArtifactIsInstalledWhenARuntimeOfItsDigestIsInstalled() async {
        let store = await loaded()
        let digest = Self.postgres.artifactSha256 ?? ""
        #expect(store.installedRuntime(sha256: digest)?.id == Self.postgres.id)
        #expect(store.installedRuntime(sha256: String(repeating: "f", count: 64)) == nil)
    }

    // MARK: - Events

    @Test func aStateChangedEventReadsTheRuntimeAgain() async throws {
        let store = await loaded()
        let changed = Self.postgres.with(state: .drifted, revision: 2)
        script.details[Self.postgres.id] = RuntimeDetail(
            summary: changed, path: "/p", artifact: nil, executables: nil, compatibility: nil)
        let event = Event(
            subscriptionId: 1, sequence: 9, at: FixtureClock.now,
            resource: ResourceRef(kind: .runtime, id: Self.postgres.id.uuidString.lowercased()),
            revision: 2, kind: .runtimeStateChanged)
        store.store.receive(event)
        for _ in 0..<200 where store.store.items[Self.postgres.id]?.state != .drifted {
            try await Task.sleep(for: .milliseconds(5))
        }
        #expect(store.store.items[Self.postgres.id]?.state == .drifted)
        #expect(script.reads.contains("runtime.get"))
    }

    @Test func anEventWithoutARuntimeOrARevisionChangesNothing() async {
        let store = await loaded()
        let bare = Event(
            subscriptionId: 1, sequence: 9, at: FixtureClock.now, resource: nil, revision: nil,
            kind: .runtimeStateChanged)
        #expect(
            RuntimeStore.reduce(bare).isIgnore, "an event that names no runtime cannot be fetched")
        let other = Event(
            subscriptionId: 1, sequence: 10, at: FixtureClock.now,
            resource: ResourceRef(kind: .runtime, id: "not-a-uuid"), revision: 3,
            kind: .runtimeStateChanged)
        #expect(RuntimeStore.reduce(other).isIgnore)
        #expect(store.store.count == 4)
    }

    @Test func aRuntimeThatWasRemovedLeavesTheStore() async {
        var removed = Self.postgres
        removed = removed.with(state: .absent)
        script.page = RuntimeListPage(
            items: [removed, FixtureRuntime.summaries[1]], next: nil, installing: [])
        let store = await loaded()
        #expect(store.store.count == 1)
        #expect(store.subtitle == "1 installed")
    }

    // MARK: - Get

    private func started(_ operation: OperationSummary) -> MutationOutcome {
        .completed(
            .runtimeInstall(
                InstallRuntimeReply(
                    resolved: FixtureRuntime.resolved,
                    outcome: .started(OperationAccepted(operation: operation, replayed: false)))))
    }

    private func get(_ store: RuntimeStore) async {
        await store.get(
            provider: "official/postgresql", displayName: "PostgreSQL", version: "18.1",
            sha256: Self.installing.sha256, context: context)
    }

    @Test func getSendsTheInstallWithTheDigestThePersonSaw() async throws {
        let store = await loaded()
        script.outcomes = [started(.fixture(number: 1, kind: .installRuntime, state: .running))]
        await get(store)
        let sent = try #require(script.sent.first)
        guard case .runtimeInstall(let params) = sent.call else {
            Issue.record("Get did not send runtime.install")
            return
        }
        #expect(params.provider == "official/postgresql")
        #expect(params.selector == "18.1")
        #expect(params.expectedSha256 == Self.installing.sha256)
        #expect(UUID(uuidString: sent.key) != nil, "the key is a UUID")
        #expect(sent.key == sent.key.lowercased())
        #expect(
            reported.feedback.map(\.message)
                == ["Installing PostgreSQL 18.1. Staged, verified, then committed."])
        #expect(reported.feedback.first?.tone == .informative)
    }

    @Test func aSecondClickWhileTheFirstHasNoAnswerSendsNothing() async throws {
        let store = await loaded()
        script.holdNextSend = true
        script.outcomes = [started(.fixture(number: 1, kind: .installRuntime, state: .running))]
        let first = Task { await get(store) }
        for _ in 0..<200 where store.isStarting(sha256: Self.installing.sha256) == false {
            try await Task.sleep(for: .milliseconds(5))
        }
        #expect(store.isStarting(sha256: Self.installing.sha256))
        await get(store)
        #expect(script.sent.count == 1, "the second click sent nothing")
        script.open()
        await first.value
        #expect(store.isStarting(sha256: Self.installing.sha256) == false)
        #expect(script.sent.count == 1)
    }

    @Test func aRejectedInstallIsKeptForTheDetailPanelAndMakesNoCard() async {
        let store = await loaded()
        let body = ErrorBody(
            code: "runtime.selector_changed", category: .conflict,
            message: "the catalog changed since you looked", retryable: false, resource: nil,
            fields: nil, hint: "Reload the catalog and try again.", details: nil,
            operationId: nil, rule: nil, items: nil)
        script.outcomes = [.rejected(body)]
        await get(store)
        #expect(reported.feedback.isEmpty)
        let failure = store.failure(for: "official/postgresql")
        #expect(failure?.code == "runtime.selector_changed")
        #expect(failure?.message == "The catalog changed since you looked.")
        #expect(failure?.hint == "Reload the catalog and try again.")
        store.dismissFailure(for: "official/postgresql")
        #expect(store.failure(for: "official/postgresql") == nil)
    }

    @Test func aNewClickClearsTheFailureOfTheProvider() async {
        let store = await loaded()
        script.outcomes = [
            .rejected(
                ErrorBody(
                    code: "runtime.selector_changed", category: .conflict, message: "changed",
                    retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
                    operationId: nil, rule: nil, items: nil)),
            started(.fixture(number: 1, kind: .installRuntime, state: .running)),
        ]
        await get(store)
        #expect(store.failure(for: "official/postgresql") != nil)
        await get(store)
        #expect(store.failure(for: "official/postgresql") == nil)
    }

    @Test func aMutationThatWasNotSentShowsTheCautionCard() async {
        let store = await loaded()
        script.outcomes = [.notSent(ClientErrorCatalog.notConnected())]
        await get(store)
        #expect(reported.feedback.map(\.message) == [MutationFeedback.notSent])
        #expect(reported.feedback.first?.tone == .caution)
    }

    @Test func anUnknownOutcomeKeepsTheKeyForFiveMinutes() async {
        let store = await loaded()
        let unknown = MutationOutcome.outcomeUnknown(ClientErrorCatalog.notConnected(), key: "")
        script.outcomes = [unknown, unknown, unknown]
        await get(store)
        await get(store)
        script.clock = script.clock.addingTimeInterval(RuntimeStore.keyLifetime + 1)
        await get(store)
        let keys = script.sent.map(\.key)
        #expect(keys.count == 3)
        #expect(keys[0] == keys[1], "the retry reuses the key so it cannot start a second install")
        #expect(keys[2] != keys[1], "an old key is not reused")
        #expect(reported.feedback.first?.tone == .caution)
    }

    @Test func anInstallThatIsAlreadyThereIsNotedAndTheRuntimeIsKept() async {
        let store = await loaded()
        let extra = RuntimeSummary.fixture(number: 22, software: "postgresql", version: "18.1")
        script.outcomes = [
            .completed(
                .runtimeInstall(
                    InstallRuntimeReply(
                        resolved: FixtureRuntime.resolved, outcome: .alreadyInstalled(extra))))
        ]
        await get(store)
        #expect(reported.feedback.map(\.message) == ["PostgreSQL 18.1 is already installed."])
        #expect(store.store.items[extra.id] != nil)
    }

    @Test func theGetActionIsBusyWhileTheClickHasNoAnswerAndCarriesTheGateReason() async {
        let store = await loaded()
        let action = store.getAction(
            provider: "official/postgresql", displayName: "PostgreSQL", version: "18.1",
            sha256: Self.installing.sha256,
            disabledReason: "Needs macOS 28.0 or later.", context: context)
        #expect(action.id == "runtimeGet")
        #expect(action.title == "Get")
        #expect(action.mutates)
        #expect(action.disabledReason == "Needs macOS 28.0 or later.")
        #expect(action.isBusy == false)
    }

    // MARK: - Recorded installs

    private func finish(_ state: OperationState, failure: OperationFailureInfo? = nil) async
        -> (FinishedInstall?, RuntimeStore)
    {
        let store = await loaded()
        let operation = OperationSummary.fixture(
            number: 1, kind: .installRuntime, state: state, failure: failure)
        store.recordStarted(
            operationId: operation.id, provider: "official/postgresql", name: "PostgreSQL",
            version: "18.1")
        for _ in 0..<200 where script.streams[operation.id] == nil {
            try? await Task.sleep(for: .milliseconds(5))
        }
        script.page = RuntimeListPage(items: FixtureRuntime.summaries, next: nil, installing: [])
        script.streams[operation.id]?.yield(operation)
        script.streams[operation.id]?.finish()
        var finished = store.finishedInstalls.makeAsyncIterator()
        return (await finished.next(), store)
    }

    @Test func anInstallThatSucceededIsReportedWithTheNameOfTheRunningInstall() async {
        let (finished, store) = await finish(.succeeded)
        #expect(finished?.outcome == .succeeded)
        #expect(finished?.name == "PostgreSQL")
        #expect(finished?.version == "18.1")
        #expect(store.installing.isEmpty, "the install left the list once it ended")
    }

    @Test func anInstallThatWasCancelledIsReported() async {
        let (finished, _) = await finish(.cancelled)
        #expect(finished?.outcome == .cancelled)
    }

    @Test func anInstallThatFailedIsKeptForTheDetailPanel() async {
        let failure = OperationFailureInfo(
            reason: "step_failed", retryable: true,
            error: ErrorBody(
                code: "runtime.download_offline", category: .unavailable,
                message: "no connection", retryable: true, resource: nil, fields: nil,
                hint: "Connect to the internet and try again.", details: nil, operationId: nil,
                rule: nil, items: nil))
        let (finished, store) = await finish(.failed, failure: failure)
        #expect(finished?.outcome == .failed)
        #expect(store.failure(for: "official/postgresql")?.code == "runtime.download_offline")
    }
}

extension StoreEffect {
    /// Whether the effect is `.ignore`.
    fileprivate var isIgnore: Bool {
        if case .ignore = self { true } else { false }
    }
}

extension RuntimeSummary {
    /// A runtime of the fixture host that is installed and managed.
    fileprivate static func fixture(number: UInt8, software: String, version: String)
        -> RuntimeSummary
    {
        RuntimeSummary(
            id: FixtureID.uuid(kind: FixtureID.runtime, number: number), revision: 1,
            software: software, displayName: software == "postgresql" ? "PostgreSQL" : software,
            version: version, line: String(version.prefix(2)),
            target: HostTarget(os: .macos, arch: .aarch64), installMode: .managed,
            state: .installed, artifactSha256: FixtureRuntime.digest(number),
            installedAt: FixtureClock.daysAgo(1), lastVerifiedAt: FixtureClock.minutesAgo(1),
            support: .experimental, supportReason: .runtimeTested, usedBy: [],
            dependentRuntimes: [])
    }

    /// A copy in another state and revision.
    fileprivate func with(state: RuntimeState, revision: UInt64? = nil) -> RuntimeSummary {
        RuntimeSummary(
            id: id, revision: revision ?? self.revision, software: software,
            displayName: displayName, version: version, line: line, target: target,
            installMode: installMode, state: state, artifactSha256: artifactSha256,
            installedAt: installedAt, lastVerifiedAt: lastVerifiedAt, support: support,
            supportReason: supportReason, usedBy: usedBy, dependentRuntimes: dependentRuntimes)
    }
}
