import Foundation
import StacklyClient
import Testing

/// The `operation` area of protocol 1.3 from the shared fixtures (spec 0007
/// AC-18, AC-20): every reply and event decodes, foreign values of the open
/// enums read as `.unrecognized`, and the list cursor is sent back as is.
struct OperationContractTests {
    static func reply(_ file: String) throws -> Reply {
        guard case .response(let response) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return response.reply
    }

    static func event(_ file: String) throws -> Event {
        guard case .event(let event) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return event
    }

    @Test func anOperationThatNeedsRecoveryCarriesItsHintCauseAndActions() throws {
        guard case .operationGet(let detail) = try Self.reply("frame-response-operation-get.json")
        else {
            Issue.record("expected an operation.get reply")
            return
        }
        let operation = detail.operation
        #expect(operation.state == .needsRecovery)
        #expect(operation.failure == nil)
        let recovery = try #require(operation.recovery)
        #expect(recovery.hint == .abandon)
        #expect(recovery.cause == .compensationFailed)
        #expect(recovery.actions == [.abandon])
        #expect(recovery.error?.code == "operation.compensation_failed")
        #expect(recovery.error?.operationId == operation.id)
        #expect(detail.steps?.map(\.state) == [.done, .failed])
        #expect(detail.created?.first?.state == .compensationFailed)
    }

    @Test func aListPageCarriesItsNextCursorForTheNextRequest() throws {
        guard case .operationList(let page) = try Self.reply("frame-response-operation-list.json")
        else {
            Issue.record("expected an operation.list reply")
            return
        }
        let items = try #require(page.items)
        #expect(items.count == 2)
        #expect(items[0].progress == .counted(CountedProgress(done: 1_048_576, total: 8_388_608)))
        #expect(items[1].progress == .indeterminate)
        #expect(items[1].waitingOn != nil)
        #expect(items[1].trigger.origin == .background)
        #expect(items[1].trigger.client == nil)

        let request = try Fixture.ipc(Request.self, "request-operation-list.json")
        guard case .operationList(let params) = request.call else {
            Issue.record("expected an operation.list request")
            return
        }
        #expect(params.before == page.next)
        #expect(params.activeOnly == true)
        #expect(params.limit == 20)
    }

    @Test func listParamsWithoutFieldsDecode() throws {
        let request = try Fixture.ipc(Request.self, "request-operation-list-without-fields.json")
        guard case .operationList(let params) = request.call else {
            Issue.record("expected an operation.list request")
            return
        }
        #expect(params.activeOnly == nil)
        #expect(params.limit == nil)
    }

    @Test func cancelResumeAbandonAndStatsDecode() throws {
        guard
            case .operationCancel(let cancel) = try Self.reply(
                "frame-response-operation-cancel.json"),
            case .operationResume(let resumed) = try Self.reply(
                "frame-response-operation-resume.json"),
            case .operationAbandon(let abandon) = try Self.reply(
                "frame-response-operation-abandon.json"),
            case .operationGetStats(let stats) = try Self.reply(
                "frame-response-operation-get-stats.json")
        else {
            Issue.record("expected the four operation replies")
            return
        }
        #expect(cancel.disposition == .requested)
        #expect(cancel.operation.state == .cancelRequested)
        #expect(cancel.operation.cancelRequestedAt != nil)
        #expect(resumed.state == .queued)
        #expect(resumed.pointOfNoReturnAt != nil)
        #expect(abandon.operation.failure?.reason == "abandoned")
        #expect(abandon.operation.failure?.error.code == "operation.abandoned")
        #expect(abandon.operation.result?.partial?.contains { $0.outcome == .leftOver } == true)
        #expect(abandon.leftovers?.first?.state == .created)
        #expect(stats.maxRunning == 8)
        #expect(stats.maxQueued == 64)
        #expect(stats.rejectedQueueFull == 0)
    }

    @Test func bothOperationEventsAreAboutTheOperationAtItsRevision() throws {
        let changed = try Self.event("frame-event-operation-state-changed.json")
        guard case .operationStateChanged(let summary) = changed.kind else {
            Issue.record("expected operation.state_changed")
            return
        }
        #expect(summary.state == .succeeded)
        #expect(
            changed.resource
                == ResourceRef(kind: .operation, id: summary.id.uuidString.lowercased()))
        #expect(changed.revision == summary.revision)

        let progress = try Self.event("frame-event-operation-progress.json")
        guard case .operationProgress(let running) = progress.kind else {
            Issue.record("expected operation.progress")
            return
        }
        #expect(running.state == .running)
        #expect(progress.revision == running.revision)
    }

    @Test func foreignValuesOfTheOperationEnumsDecodeAsUnrecognized() throws {
        guard
            case .operationGet(let detail) = try Self.reply(
                "frame-response-operation-get-unknown-values.json"),
            case .operationCancel(let cancel) = try Self.reply(
                "frame-response-operation-cancel-unknown-disposition.json")
        else {
            Issue.record("expected the foreign value replies")
            return
        }
        let operation = detail.operation
        #expect(operation.kind == .unrecognized)
        #expect(operation.trigger.origin == .unrecognized)
        #expect(operation.trigger.client == .unrecognized)
        #expect(operation.recovery?.hint == .unrecognized)
        #expect(operation.recovery?.cause == .unrecognized)
        #expect(operation.recovery?.actions == [.unrecognized, .abandon])
        #expect(operation.result?.partial?.first?.outcome == .unrecognized)
        #expect(detail.steps?.last?.state == .unrecognized)
        #expect(detail.created?.first?.state == .unrecognized)
        #expect(cancel.disposition == .unrecognized)
    }

    @Test func theOperationStateIsClosed() {
        #expect(throws: DecodingError.self) {
            try StacklyJSON.decoder().decode(OperationState.self, from: Data("\"paused\"".utf8))
        }
    }

    @Test func anAcceptedOperationSaysWhetherItReplayed() throws {
        let accepted = try Fixture.ipc(OperationAccepted.self, "dto-operation-accepted.json")
        #expect(accepted.replayed == false)
        #expect(accepted.operation.state == .queued)
        #expect(accepted.operation.revision == 1)
    }

    @Test func aReusedIdempotencyKeyNamesTheFirstOperation() throws {
        guard
            case .error(let frame) = try Fixture.ipc(
                ServerFrame.self, "frame-error-idempotency-key-reused.json")
        else {
            Issue.record("expected an error frame")
            return
        }
        #expect(frame.error.code == "operation.idempotency_key_reused")
        #expect(frame.error.category == .invalidInput)
        #expect(frame.error.detail("operationId") == "0192f3a4-0000-7000-8000-000000000001")
    }
}
