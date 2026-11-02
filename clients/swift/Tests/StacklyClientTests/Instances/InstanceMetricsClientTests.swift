import Foundation
import StacklyClient
import Testing

/// `getMetrics` of `StacklyClient` over a fake `send`, and the shared metrics fixtures as the
/// client reads them (spec 0028 AC-11, AC-13): the params carry the cursor, a reply of another
/// instance or kind is a protocol violation, and a value or reason this build does not know is
/// `.unrecognized`, except in the three typeshare enums that cannot hold one.
struct InstanceMetricsClientTests {
    private typealias Fake = InstanceDetailClientTests.Fake

    private static let instance = UUID(uuidString: "0192f3a5-0000-7000-8000-000000000001") ?? UUID()

    private static func metrics(_ file: String) throws -> MetricsReply {
        guard
            case .instanceGetMetrics(let reply) = try CreateContractTests.reply(
                "frame-response-instance-get-metrics-\(file).json")
        else { throw FixtureShapeError.unexpected(file) }
        return reply
    }

    @Test func getMetricsSendsTheCursorAndReadsTheReply() async throws {
        let live = try Self.metrics("reset")
        let fake = Fake(.success(.instanceGetMetrics(live)))
        let reply = try await fake.client.getMetrics(
            instanceId: Self.instance, historyId: "4242-1760950802.000000", afterSeq: 2)

        #expect(
            fake.made == [
                .instanceGetMetrics(
                    GetMetricsParams(
                        instanceId: Self.instance, historyId: "4242-1760950802.000000",
                        afterSeq: 2))
            ])
        #expect(reply == live)
    }

    @Test func theFirstCallCarriesNoCursor() async throws {
        let fake = Fake(.success(.instanceGetMetrics(try Self.metrics("live"))))
        _ = try await fake.client.getMetrics(instanceId: Self.instance)

        #expect(
            fake.made == [
                .instanceGetMetrics(
                    GetMetricsParams(instanceId: Self.instance, historyId: nil, afterSeq: nil))
            ])
    }

    @Test func getMetricsRefusesAReplyOfAnotherInstance() async throws {
        let fake = Fake(.success(.instanceGetMetrics(try Self.metrics("live"))))
        do {
            _ = try await fake.client.getMetrics(instanceId: UUID())
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .rejected(ClientErrorCatalog.protocolViolation()))
        }
    }

    @Test func getMetricsRefusesAReplyOfAnotherKind() async throws {
        let fake = Fake(
            .success(try CreateContractTests.reply("frame-response-instance-get-data-usage.json")))
        do {
            _ = try await fake.client.getMetrics(instanceId: Self.instance)
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .rejected(ClientErrorCatalog.protocolViolation()))
        }
    }

    @Test func getMetricsPassesTheDaemonsRefusalOn() async throws {
        let body = ErrorBody(
            code: "instance.not_found", category: .notFound, message: "no such instance",
            retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
            operationId: nil, rule: nil, items: nil)
        let fake = Fake(.failure(.rejected(body)))
        do {
            _ = try await fake.client.getMetrics(instanceId: Self.instance)
            Issue.record("expected the refusal")
        } catch {
            #expect(error == .rejected(body))
        }
    }

    @Test func aLiveReplyHoldsSamplesProcessesAndTheRestartTotal() throws {
        let live = try Self.metrics("live")

        #expect(live.state == .live)
        #expect(live.lifecycle == .running)
        #expect(live.intervalSeconds == 5)
        #expect(live.capacity == 60)
        #expect(live.historyId != nil)
        #expect(live.process != nil)
        #expect(live.samples.isEmpty == false)
        #expect(live.latestProcesses?.contains { $0.isMain } == true)
    }

    @Test func aNotRunningReplyHoldsNothingToDraw() throws {
        let reply = try Self.metrics("not-running")

        #expect(reply.state == .notRunning)
        #expect(reply.historyReset)
        #expect(reply.samples.isEmpty)
        #expect(reply.historyId == nil)
        #expect(reply.process == nil)
    }

    @Test func aReplyWithNoNewSamplesKeepsTheCursor() throws {
        let reply = try Self.metrics("no-new-samples")

        #expect(reply.samples.isEmpty)
        #expect(reply.historyReset == false)
        #expect(reply.historyId != nil)
    }

    @Test func aValueTheBuildDoesNotKnowReadsAsUnrecognized() throws {
        let reply = try Self.metrics("unknown-values")

        #expect(reply.state == .unrecognized)
        #expect(reply.samples.first?.main.scope == .unrecognized)
        #expect(
            reply.samples.first?.main.cpu
                == .unavailable(UnavailableContent(reason: .unrecognized)))
        #expect(reply.dataUsage == .unavailable(UnavailableContent(reason: .unrecognized)))
    }

    /// The typeshare enums `NumberMetric`, `CountMetric`, and `DataUsageMetric` fail to decode
    /// a `type` they do not know instead of reading `.unrecognized` (a limit of the generator);
    /// the screen reads such a reply as a failed load, never as zero.
    @Test func aMetricTypeTheBuildDoesNotKnowFailsToDecode() throws {
        #expect(throws: DecodingError.self) {
            _ = try Self.metrics("unknown-types")
        }
    }
}
