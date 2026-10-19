import Foundation
import StacklyClient
import Testing

/// What protocol 1.4 changed, read from the shared fixtures (spec 0006 AC-7,
/// AC-8, AC-10): an error lists at most 32 fields and counts the rest in
/// `details.omitted`, a request or frame must be a JSON object, and the
/// latest time the daemon can send still reads and writes back.
struct BoundedErrorContractTests {
    struct CatalogCode: Decodable, Sendable {
        let code: String
        let details: [String]
    }

    struct Catalog: Decodable, Sendable {
        let codes: [CatalogCode]
    }

    @Test func aCappedInvalidParamsErrorKeepsThirtyTwoFieldsAndCountsTheRest() throws {
        guard
            case .error(let frame) = try Fixture.ipc(
                ServerFrame.self, "frame-error-invalid-params-capped.json")
        else {
            Issue.record("expected an error frame")
            return
        }
        let fields = try #require(frame.error.fields)
        #expect(fields.count == 32)
        #expect(fields.first?.path == "kinds")
        #expect(fields.first?.code == "too_many")
        #expect(fields.last?.path == "kinds[30]")
        let details = try #require(frame.error.details)
        #expect(details.first { $0.key == "omitted" }?.value == "1")
        #expect(details.first { $0.key == "method" }?.value == "events.subscribe")
    }

    @Test func everyCodeThatCarriesFieldsListsTheOmittedDetail() throws {
        let catalog = try JSONDecoder().decode(
            Catalog.self, from: Fixture.repositoryFile("protocols/error-codes.json"))
        for code in ["request.invalid_params", "request.malformed"] {
            let entry = try #require(catalog.codes.first { $0.code == code }, "\(code)")
            #expect(entry.details.contains("omitted"), "\(code)")
        }
    }

    @Test func positionalArraysDoNotDecodeAsRequestsOrFrames() throws {
        let decoder = StacklyJSON.decoder()
        for file in ["reject-positional-request.json", "reject-positional-params.json"] {
            #expect(throws: (any Error).self, "\(file)") {
                try decoder.decode(Request.self, from: Fixture.data("ipc/\(file)"))
            }
        }
        for file in ["frame-positional.json", "frame-error-positional-content.json"] {
            #expect(throws: (any Error).self, "\(file)") {
                try decoder.decode(ServerFrame.self, from: Fixture.data("ipc/\(file)"))
            }
        }
    }

    @Test func theLatestTimeTheDaemonSendsReadsAndWritesBackUnchanged() throws {
        guard
            case .event(let event) = try Fixture.ipc(
                ServerFrame.self, "frame-event-at-latest-time.json")
        else {
            Issue.record("expected an event frame")
            return
        }
        #expect(StacklyJSON.string(from: event.at) == "9999-12-30T22:00:00.999Z")
    }
}
