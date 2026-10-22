import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The Runtimes page without a view (spec 0018 AC-17): its state in the fixed order, the
/// rows, the notes, the inspector's detail model, and the width rule of the table.
@Suite(.timeLimit(.minutes(1)))
struct RuntimesPageTests {
    private static func state(
        rows: [RuntimeRowModel]? = [], installing: [InstallingRuntime] = [],
        error: ErrorBody? = nil, unsupported: Bool = false, waiting: FailureContent? = nil,
        loadedAt: Date? = Date(timeIntervalSince1970: 1), partial: PartialNote? = nil
    ) -> ViewState<[RuntimeRowModel]> {
        RuntimesPage.state(
            rows: rows, installing: installing, error: error, unsupported: unsupported,
            waiting: waiting, loadedAt: loadedAt, isRefreshing: false, partial: partial,
            browseCatalog: {})
    }

    private static let failure = ErrorBody(
        code: "internal.unexpected", category: .internal, message: "the registry could not be read",
        retryable: false, resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
        rule: nil, items: nil)

    private static let rows = RuntimeRowModel.rows(of: FixtureRuntime.summaries, catalog: nil)

    // MARK: - State

    @Test func beforeTheFirstSnapshotThePageIsLoading() {
        #expect(Self.state(rows: nil, loadedAt: nil).shape == .loading)
    }

    @Test func aPageWithoutRuntimesIsEmptyWithBrowseCatalog() {
        guard case .empty(let content, _) = Self.state() else {
            Issue.record("expected the empty state")
            return
        }
        #expect(content.title == "No Runtimes")
        #expect(content.message == "Install a runtime from the catalog to create instances.")
        #expect(content.primaryAction?.title == "Browse Catalog")
        #expect(content.primaryAction?.mutates == false)
    }

    @Test func aPageWithoutRuntimesButWithAnInstallRunningIsNotEmpty() {
        let state = Self.state(installing: [FixtureRuntime.installing])
        #expect(state.shape == .loaded)
    }

    @Test func aPageWithRuntimesIsLoaded() {
        #expect(Self.state(rows: Self.rows).shape == .loaded)
    }

    @Test func aFirstLoadThatFailedShowsTheDaemonsMessageAndCode() {
        guard case .failed(let content) = Self.state(rows: nil, error: Self.failure, loadedAt: nil)
        else {
            Issue.record("expected the failed state")
            return
        }
        #expect(content.title == "Couldn’t Load Runtimes")
        #expect(content.code == "internal.unexpected")
    }

    @Test func aDaemonThatCannotBeReachedWinsOverTheStoreFailureWhileThereIsNoData() {
        let waiting = FailureContent(
            title: "Waiting for stacklyd", message: "m", code: "ipc.endpoint_missing")
        guard
            case .failed(let content) = Self.state(
                rows: nil, error: Self.failure, waiting: waiting, loadedAt: nil)
        else {
            Issue.record("expected the failed state")
            return
        }
        #expect(content.title == "Waiting for stacklyd")
    }

    @Test func aDaemonWithoutTheMethodIsUnsupported() {
        guard case .unsupported(let content) = Self.state(unsupported: true) else {
            Issue.record("expected the unsupported state")
            return
        }
        #expect(content.title == "Runtimes Unavailable")
    }

    @Test func aFailedRefreshKeepsTheRowsAndAddsAStaleNote() {
        guard case .loaded(let rows, let notes) = Self.state(rows: Self.rows, error: Self.failure)
        else {
            Issue.record("expected the loaded state")
            return
        }
        #expect(rows.count == 4)
        #expect(notes.stale != nil)
    }

    // MARK: - Partial

    @Test func runtimesThatCouldNotBeRefreshedGetOneItemEachWithRetry() {
        let first = UUID()
        let second = UUID()
        let failures = [
            first: ItemFailure(error: Self.failure, revision: 3),
            second: ItemFailure(error: Self.failure, revision: 4),
        ]
        let note = RuntimesPage.partial(
            failures: failures,
            names: [first: "PostgreSQL 17.6", second: "Traefik 3.7.15"], retry: { _ in })
        #expect(note?.summary == "Couldn’t refresh 2 runtimes.")
        #expect(note?.items.count == 2)
        #expect(note?.items.first?.name == "PostgreSQL 17.6")
        #expect(note?.items.first?.action?.title == "Retry")
        let one = RuntimesPage.partial(
            failures: [first: ItemFailure(error: Self.failure, revision: 3)], names: [:],
            retry: { _ in })
        #expect(one?.summary == "Couldn’t refresh 1 runtime.")
        #expect(RuntimesPage.partial(failures: [:], names: [:], retry: { _ in }) == nil)
    }

    // MARK: - Table

    @Test func sourceAndDigestWaitForATableOf560Points() {
        #expect(RuntimesPage.showsSourceAndDigest(559) == false)
        #expect(RuntimesPage.showsSourceAndDigest(560))
        #expect(RuntimesPage.showsSourceAndDigest(384) == false)
    }

    @Test func aRowTakesTheTileOfTheProviderThatRunsTheSameSoftware() {
        let catalog = [
            ProviderSummary.fixture(),
            ProviderSummary.fixture(
                provider: "official/nats", displayName: "NATS", mark: "Na", tile: .green),
        ]
        let rows = RuntimeRowModel.rows(of: FixtureRuntime.summaries, catalog: catalog)
        #expect(rows[0].tile == TileSpec(mark: "Pg", tile: .blue, name: "PostgreSQL"))
        #expect(rows[2].tile == TileSpec(mark: "Na", tile: .green, name: "NATS Server"))
        #expect(
            rows[3].tile == TileSpec(mark: nil, tile: nil, name: "Traefik"),
            "no provider of that software: a gray tile with the default mark")
    }

    @Test func anInstallTakesTheTileOfItsProviderById() {
        let catalog = [ProviderSummary.fixture()]
        let tile = RuntimeRowModel.tile(
            software: "postgresql", name: "PostgreSQL", provider: "official/postgresql",
            catalog: catalog)
        #expect(tile == TileSpec(mark: "Pg", tile: .blue, name: "PostgreSQL"))
        #expect(
            RuntimeRowModel.tile(
                software: "postgresql", name: "PostgreSQL", provider: "official/postgresql",
                catalog: nil) == TileSpec(mark: nil, tile: nil, name: "PostgreSQL"))
    }

    // MARK: - Detail

    @Test func theDetailLoadsOnceAndIsKeptForTheRevision() async {
        let calls = Counter()
        let model = RuntimeDetailModel(call: { call throws(ClientError) in
            calls.count += 1
            guard case .runtimeGet(let params) = call,
                params.runtimeId == FixtureRuntime.summaries[0].id
            else { throw .rejected(ClientErrorCatalog.protocolViolation()) }
            return .runtimeGet(FixtureRuntime.detail)
        })
        let id = FixtureRuntime.summaries[0].id
        #expect(model.phase(for: id, revision: 1) == .loading)
        await model.load(id, revision: 1)
        await model.load(id, revision: 1)
        #expect(calls.count == 1)
        #expect(model.phase(for: id, revision: 1) == .loaded(FixtureRuntime.detail))
        #expect(model.phase(for: id, revision: 2) == .loading, "a new revision is read again")
        model.invalidate()
        #expect(model.phase(for: id, revision: 1) == .loading)
    }

    @Test func aDetailThatFailedShowsItsFailureUntilRetry() async {
        let failing = Counter()
        let model = RuntimeDetailModel(call: { call throws(ClientError) in
            failing.count += 1
            if failing.count == 1 { throw .rejected(Self.failure) }
            return .runtimeGet(FixtureRuntime.detail)
        })
        let id = FixtureRuntime.summaries[0].id
        await model.load(id, revision: 1)
        guard case .failed(let content) = model.phase(for: id, revision: 1) else {
            Issue.record("expected a failure")
            return
        }
        #expect(content.title == "Couldn’t Load Runtime")
        await model.load(id, revision: 1)
        #expect(failing.count == 1, "a failure is kept until Retry")
        await model.retry(id, revision: 1)
        #expect(model.phase(for: id, revision: 1) == .loaded(FixtureRuntime.detail))
    }

    // MARK: - Feedback

    @Test func theWindowSaysHowAnInstallEnded() {
        let id = UUID()
        let installed = FinishedInstall(
            operationId: id, name: "PostgreSQL", version: "18.1", outcome: .succeeded)
        #expect(installed.feedback?.message == "PostgreSQL 18.1 installed and verified.")
        #expect(installed.feedback?.tone == .positive)
        let cancelled = FinishedInstall(
            operationId: id, name: "PostgreSQL", version: "18.1", outcome: .cancelled)
        #expect(
            cancelled.feedback?.message
                == "Install of PostgreSQL 18.1 was cancelled. Nothing was kept.")
        #expect(cancelled.feedback?.tone == .neutral)
        let failed = FinishedInstall(
            operationId: id, name: "PostgreSQL", version: "18.1", outcome: .failed)
        #expect(failed.feedback == nil, "a failure shows in the Catalog detail panel")
    }

    // MARK: - Cancel

    @Test func cancelInstallIsDisabledDuringCommitAndSendsTheCancel() async {
        let sent = SentCalls()
        let store = RuntimeStore(
            call: { _ throws(ClientError) in throw .rejected(Self.failure) },
            send: { call, _ in
                sent.calls.append(call)
                return .notSent(ClientErrorCatalog.notConnected())
            })
        let context = InstanceActionContext(report: { _ in })
        let commit = store.cancelAction(
            for: FixtureRuntime.installing, stage: "commit", context: context)
        #expect(commit.disabledReason == "Can’t be cancelled during Commit")
        #expect(commit.id == "cancelInstall")
        #expect(commit.title == "Cancel Install")
        #expect(commit.mutates)
        #expect(commit.confirmation == nil, "cancelling an operation asks no confirmation")
        let extract = store.cancelAction(
            for: FixtureRuntime.installing, stage: "extract", context: context)
        #expect(extract.disabledReason == nil)
        await store.cancel(FixtureRuntime.installing, context: context)
        guard case .operationCancel(let params)? = sent.calls.first else {
            Issue.record("expected operation.cancel")
            return
        }
        #expect(params.operationId == FixtureRuntime.installing.operationId)
    }
}

/// A counter a `@Sendable` closure can bump from the main actor.
private final class Counter {
    var count = 0
}

/// The calls a scripted daemon was sent.
private final class SentCalls {
    var calls: [Call] = []
}
