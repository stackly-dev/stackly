import AppKit
import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The detail panel of a hosted Catalog page across a new snapshot (spec 0017 AC-10): a
/// restarted daemon reloads the catalog, which drops the detail cache, and the page asks
/// again for the provider that stays selected.
@Suite(.timeLimit(.minutes(1)))
@MainActor
struct CatalogPageDetailTests {
    let script: CatalogScript

    init() throws {
        script = try CatalogScript.fixture()
    }

    // covers: AC-10 (step 99, the catalog comes back after stacklyd restarts)
    @Test func aNewSnapshotLoadsTheSelectedDetailAgain() async throws {
        var server = UUID()
        let store = script.store(serverInstance: { server })
        await store.store.resnapshot()
        let window = host(store)
        defer { window.close() }
        try await waitUntil("the first detail call") { script.getCalls.count == 1 }

        server = UUID()
        await store.store.resnapshot()

        try await waitUntil("a second detail call after the new snapshot") {
            script.getCalls.count == 2
        }
        #expect(script.getCalls == ["official/nats", "official/nats"])
        #expect(try phase(store) == .loaded(try CatalogFixture.detail("nats")))
    }

    // covers: AC-10 (a failed detail does not need Retry once the daemon is back)
    @Test func aNewSnapshotReplacesAFailedDetail() async throws {
        let nats = try #require(script.details["official/nats"])
        script.details["official/nats"] = nil
        let store = script.store()
        await store.store.resnapshot()
        let window = host(store)
        defer { window.close() }
        try await waitUntil("the failed detail") {
            if case .failed = try phase(store) { return true }
            return false
        }

        script.details["official/nats"] = nats
        await store.store.resnapshot()

        try await waitUntil("the detail after the new snapshot") {
            try phase(store) == .loaded(nats)
        }
    }

    // MARK: - Helpers

    /// Hosts the page in a window with NATS selected, so its tasks run.
    private func host(_ store: CatalogStore) -> SnapshotWindow {
        let size = CGSize(width: 1_044, height: 748)
        let page = CatalogPage(
            catalog: store, waiting: nil, hasConnection: true, reconnect: {},
            openDiagnostics: {}, selection: "official/nats"
        )
        .actionHost()
        let hosting = NSHostingView(rootView: page)
        let window = SnapshotWindow(size: size)
        window.contentView = hosting
        hosting.frame = CGRect(origin: .zero, size: size)
        hosting.layoutSubtreeIfNeeded()
        return window
    }

    /// What the panel shows for NATS at the revision the store holds.
    private func phase(_ store: CatalogStore) throws -> ProviderDetailModel.Phase {
        let nats = try #require(store.providers?.first { $0.provider == "official/nats" })
        return store.detail.phase(for: nats.provider, revision: nats.revision)
    }

    /// Polls `done` for up to 3 counted seconds while the hosted page runs its tasks.
    private func waitUntil(
        _ what: String, _ done: () throws -> Bool
    ) async throws {
        var timer = ResponsiveTimer(maxCharge: .milliseconds(40))
        while try done() == false {
            timer.lap()
            guard timer.counted < .seconds(3) else {
                Issue.record("timed out waiting for \(what)")
                return
            }
            try await Task.sleep(for: .milliseconds(20))
        }
    }
}
