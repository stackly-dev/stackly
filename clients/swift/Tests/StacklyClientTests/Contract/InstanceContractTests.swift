import Foundation
import StacklyClient
import Testing

/// The `instance` area of protocol 1.8 from the shared fixtures (spec 0010
/// AC-16): the snapshot, the three instance events, `daemon.reconciling`,
/// and the create params decode; foreign values of the open enums read as
/// `.unrecognized`; and a summary from an older daemon decodes without
/// `ownership` and `initialized`. The `statusSeq` recipe itself belongs to
/// the `InstanceStore` of spec 0013 step 16, not to this package.
struct InstanceContractTests {
    static func reply(_ file: String) throws -> Reply {
        guard case .response(let response) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return response.reply
    }

    static func summary(_ file: String) throws -> InstanceSummary {
        guard case .instanceGet(let summary) = try reply(file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return summary
    }

    static func event(_ file: String) throws -> Event {
        guard case .event(let event) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return event
    }

    static func error(_ file: String) throws -> ErrorBody {
        guard case .error(let frame) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return frame.error
    }

    @Test func aRunningSnapshotCarriesTheStatusFieldsOfTheSpecTable() throws {
        let running = try Self.summary("frame-response-instance-get.json")
        #expect(running.lifecycle == .running)
        #expect(running.desired == .running)
        #expect(running.statusSeq == 4)
        #expect(running.readiness.state == .ready)
        #expect(running.health.state == .healthy)
        #expect(running.ownership == .manual)
        #expect(running.initialized == true)
        let process = try #require(running.process)
        #expect(process.pid == 4242)
        #expect(process.adopted == false)
        #expect(process.adoptable == true)
        let endpoints = try #require(running.endpoints)
        #expect(endpoints.map(\.portMode) == [.fixed, .auto, .fixed])
        #expect(endpoints.map(\.port) == [5432, 49211, nil])
        #expect(endpoints[2].transport == .unix)
        #expect(endpoints[2].socketPath != nil)
    }

    @Test func aFailedSnapshotCarriesTheFailureTheRestartStatusAndTheLeftovers() throws {
        let failed = try Self.summary("frame-response-instance-get-failed.json")
        #expect(failed.lifecycle == .failed)
        let failure = try #require(failed.lastFailure)
        #expect(failure.code == "crash_loop")
        #expect(failure.exitCode == 1)
        #expect(failure.excerpt?.count == 2)
        #expect(failed.restart.exhausted)
        #expect(failed.leftoverProcesses == [4243, 4244])
        #expect(failed.displayName == nil)
    }

    @Test func theThreeBlockersCarryTheirEvidenceAndNotCheckedIsCamelCase() throws {
        let unproven = try Self.summary("frame-response-instance-get-identity-unproven.json")
        guard case .identityUnproven(let blocker) = try #require(unproven.blocker) else {
            Issue.record("expected an identity unproven blocker")
            return
        }
        #expect(blocker.evidence.checks?.count == 7)
        #expect(blocker.evidence.lock == .held)

        let inUse = try Self.summary("frame-response-instance-get-data-in-use.json")
        guard case .dataInUse(let held) = try #require(inUse.blocker) else {
            Issue.record("expected a data in use blocker")
            return
        }
        #expect(held.evidence.stopped)
        let checks = try #require(held.evidence.checks)
        #expect(checks[2...].allSatisfy { $0.result == .notChecked })
        let text = String(
            decoding: try Fixture.data("ipc/frame-response-instance-get-data-in-use.json"),
            as: UTF8.self)
        #expect(text.contains("\"notChecked\""))

        let needs = try Self.summary("frame-response-instance-get-needs-secret.json")
        guard case .needsSecret(let secret) = try #require(needs.blocker) else {
            Issue.record("expected a needs secret blocker")
            return
        }
        #expect(secret.references?.count == 1)
    }

    @Test func aSummaryFromAnOlderDaemonDecodesWithoutOwnershipAndInitialized() throws {
        let older = try Self.summary("frame-response-instance-get-without-ownership.json")
        #expect(older.ownership == nil)
        #expect(older.initialized == nil)
        let fresh = try Self.summary("frame-response-instance-get-uninitialized.json")
        #expect(fresh.initialized == false)
        #expect(fresh.lifecycle == .uninitialized)
    }

    @Test func foreignValuesOfTheInstanceEnumsReadAsUnrecognized() throws {
        let found = try Self.summary("frame-response-instance-get-unknown-values.json")
        #expect(found.ownership == .unrecognized)
        let endpoint = try #require(found.endpoints?.first)
        #expect(endpoint.transport == .unrecognized)
        #expect(endpoint.portMode == .unrecognized)
        guard case .identityUnproven(let blocker) = try #require(found.blocker) else {
            Issue.record("expected an identity unproven blocker")
            return
        }
        let check = try #require(blocker.evidence.checks?.first)
        #expect(check.name == .unrecognized)
        #expect(check.result == .unrecognized)
        #expect(blocker.evidence.lock == .unrecognized)

        guard
            case .daemonReconciling(let reconciling) = try Self.event(
                "frame-event-daemon-reconciling-unknown-phase.json"
            ).kind
        else {
            Issue.record("expected daemon.reconciling")
            return
        }
        #expect(reconciling.phase == .unrecognized)
    }

    @Test func theInstanceEventsDecodeWithTheirResourceAndRevision() throws {
        let changed = try Self.event("frame-event-instance-state-changed.json")
        guard case .instanceStateChanged(let state) = changed.kind else {
            Issue.record("expected instance.state_changed")
            return
        }
        #expect(
            changed.resource
                == ResourceRef(kind: .instance, id: state.instance.id.uuidString.lowercased()))
        #expect(changed.revision == state.instance.revision)

        let health = try Self.event("frame-event-instance-health-changed.json")
        guard case .instanceHealthChanged(let status) = health.kind else {
            Issue.record("expected instance.health_changed")
            return
        }
        #expect(status.statusSeq == 5)
        #expect(status.health.state == .degraded)
        #expect(status.health.detail == "probe took 2.4 seconds")

        let lower = try Self.event("frame-event-instance-health-changed-lower-revision.json")
        guard case .instanceHealthChanged(let newest) = lower.kind else {
            Issue.record("expected instance.health_changed")
            return
        }
        #expect(newest.statusSeq > status.statusSeq)
        #expect(try #require(lower.revision) < #require(health.revision))
        if case .identityUnproven = try #require(newest.blocker) {
        } else {
            Issue.record("expected an identity unproven blocker")
        }

        let removed = try Self.event("frame-event-instance-removed.json")
        guard case .instanceRemoved(let gone) = removed.kind else {
            Issue.record("expected instance.removed")
            return
        }
        #expect(removed.resource?.id == gone.instanceId.uuidString.lowercased())
    }

    @Test func daemonReconcilingIsADaemonEventWithTheCounts() throws {
        for file in [
            "frame-event-daemon-reconciling-started.json",
            "frame-event-daemon-reconciling-finished.json",
        ] {
            let event = try Self.event(file)
            #expect(event.resource == nil)
            #expect(event.revision == nil)
            guard case .daemonReconciling = event.kind else {
                Issue.record("\(file): expected daemon.reconciling")
                return
            }
        }
        guard
            case .daemonReconciling(let finished) = try Self.event(
                "frame-event-daemon-reconciling-finished.json"
            ).kind
        else { return }
        #expect(finished.phase == .finished)
        #expect(finished.instances == 6)
        #expect(finished.adopted == 2)
        #expect(finished.unknown == 1)
        #expect(finished.restarting == 1)
    }

    @Test func theListAndActionRepliesDecode() throws {
        guard case .instanceList(let page) = try Self.reply("frame-response-instance-list.json")
        else {
            Issue.record("expected an instance.list reply")
            return
        }
        #expect(page.items?.count == 2)
        #expect(page.next?.hasPrefix("orders-db_") == true)

        guard
            case .instanceStart(let already) = try Self.reply(
                "frame-response-instance-start-already-running.json")
        else {
            Issue.record("expected an instance.start reply")
            return
        }
        #expect(already.alreadyInState)
        #expect(already.accepted == nil)

        guard case .instanceStop(let stop) = try Self.reply("frame-response-instance-stop.json")
        else {
            Issue.record("expected an instance.stop reply")
            return
        }
        #expect(stop.accepted?.operation.kind == .stopInstance)
    }

    @Test func theCreateRequestCarriesPortRequestsSettingsAndSecretInputs() throws {
        let request = try Fixture.ipc(Request.self, "request-instance-create.json")
        guard case .instanceCreate(let params) = request.call else {
            Issue.record("expected instance.create")
            return
        }
        #expect(request.idempotencyKey == "create-orders-db-0001")
        #expect(params.provider.revision == "2026.10.1")
        let endpoints = try #require(params.endpoints)
        #expect(endpoints.map(\.port) == [.fixed(5432), .auto])
        #expect(params.settings.members["maxConnections"] == .number(50))
        #expect(params.settings.members["logStatements"] == .string("ddl"))
        let secrets = try #require(params.secrets)
        #expect(secrets.map(\.field) == ["adminPassword", "replicaPassword"])

        let minimal = try Fixture.ipc(Request.self, "request-instance-create-without-fields.json")
        guard case .instanceCreate(let bare) = minimal.call else {
            Issue.record("expected instance.create")
            return
        }
        #expect(bare.provider.revision == nil)
        #expect(bare.endpoints == nil)
        #expect(bare.secrets == nil)
    }

    @Test func theCreateRequestWithSettingsRoundTripsThroughTheWireCoders() throws {
        let request = try Fixture.ipc(Request.self, "request-instance-create.json")
        let encoded = try StacklyJSON.encoder().encode(request)
        let again = try StacklyJSON.decoder().decode(Request.self, from: encoded)
        #expect(again == request)
        guard case .instanceCreate(let params) = again.call else {
            Issue.record("expected instance.create")
            return
        }
        #expect(params.settings.members["maxConnections"] == .number(50))
        let text = String(decoding: encoded, as: UTF8.self)
        #expect(text.contains("\"maxConnections\":50"))
    }

    @Test func settingsRefuseAWholeNumberBeyondTheSafeRangeAndKeepTheBoundary() throws {
        let decoder = StacklyJSON.decoder()
        let boundary = try decoder.decode(
            JSONObject.self,
            from: Data(#"{"a":9007199254740991,"b":-9007199254740991,"c":1.5}"#.utf8))
        #expect(boundary.members["a"] == .number(9_007_199_254_740_991))
        #expect(boundary.members["c"] == .number(1.5))
        for text in [#"{"a":9007199254740993}"#, #"{"a":[{"b":-9007199254740993}]}"#] {
            #expect(throws: DecodingError.self) {
                try decoder.decode(JSONObject.self, from: Data(text.utf8))
            }
        }
    }

    @Test func aSettingsDocumentKeepsNestedValues() throws {
        let data = try Fixture.data("ipc/dto-settings-document.json")
        let settings = try StacklyJSON.decoder().decode(JSONObject.self, from: data)
        #expect(
            settings.members["nested"]
                == .object(["list": .array([.number(1), .number(2), .number(3)])]))
        #expect(throws: DecodingError.self) {
            try StacklyJSON.decoder().decode(JSONObject.self, from: Data("[1]".utf8))
        }
    }

    @Test func theSupervisorErrorsCarryTheirItemsAndPortFields() throws {
        let unproven = try Self.error("frame-error-supervisor-identity-unproven.json")
        #expect(unproven.code == "supervisor.identity_unproven")
        #expect(unproven.items?.count == 2)
        #expect(unproven.items?[1].fields?.first { $0.key == "name" }?.value == "holds_lock")

        let unavailable = try Self.error("frame-error-supervisor-endpoint-unavailable.json")
        #expect(unavailable.retryable)
        #expect(unavailable.category == .unavailable)
        #expect(unavailable.fields?.count == 1)

        let override = try Self.error("frame-error-supervisor-stop-requires-override.json")
        #expect(override.items?.first?.kind == "environment")
    }
}
