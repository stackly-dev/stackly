import Foundation
import Testing

@testable import StacklyClient

/// The staged decode of daemon frames (spec 0013 AC-4).
struct ServerFrameDecoderTests {
    static let violation = ClientError.outcomeUnknown(ClientErrorCatalog.protocolViolation())

    static func decode(_ text: String, maxDepth: UInt32 = 32) throws(ClientError) -> DecodedFrame {
        try ServerFrameDecoder.decode(Array(text.utf8), maxDepth: maxDepth)
    }

    @Test func fixtureFramesDecode() throws {
        for file in [
            "frame-response-handshake.json", "frame-error-full.json", "frame-event-gap.json",
            "frame-event-shutting-down.json",
        ] {
            let bytes = Array(try Fixture.data("ipc/\(file)"))
            _ = try ServerFrameDecoder.decode(bytes, maxDepth: 32)
        }
    }

    @Test func anUnknownEventKindIsSkippedWithItsSequence() throws {
        let bytes = Array(try Fixture.data("ipc/frame-event-unknown-kind.json"))
        let frame = try ServerFrameDecoder.decode(bytes, maxDepth: 32)
        guard case .skipped(_, let sequence, let kind) = frame else {
            Issue.record("expected skipped, got \(frame)")
            return
        }
        #expect(sequence > 0)
        #expect(ContractNames.eventKinds.contains(kind) == false)
    }

    @Test func thirtyThreeLevelsAreAProtocolViolation() {
        let deep = String(repeating: "[", count: 33) + String(repeating: "]", count: 33)
        #expect(throws: Self.violation) {
            try Self.decode(#"{"type":"response","content":\#(deep)}"#)
        }
    }

    @Test func aForeignFrameTypeIsAProtocolViolation() throws {
        let bytes = Array(try Fixture.data("ipc/frame-unknown-type.json"))
        #expect(throws: Self.violation) { try ServerFrameDecoder.decode(bytes, maxDepth: 32) }
    }

    @Test func aKnownKindThatDoesNotDecodeIsAProtocolViolation() {
        let text =
            #"{"type":"event","content":{"subscriptionId":1,"sequence":2,"at":"2026-10-18T03:04:05.123Z","kind":{"type":"events.gap","content":{"reason":7}}}}"#
        #expect(throws: Self.violation) { try Self.decode(text) }
    }

    @Test func aReplyWithAnUnknownTagIsAProtocolViolation() {
        let text =
            #"{"type":"response","content":{"requestId":"a","reply":{"type":"daemon.future"}}}"#
        #expect(throws: Self.violation) { try Self.decode(text) }
    }
}
