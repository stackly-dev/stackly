import Foundation
import StacklyClient
import Testing

/// The wire time format of spec 0006: RFC 3339 in UTC with exactly three
/// fractional digits.
struct StacklyJSONTests {
    @Test func encodesUtcWithExactlyThreeFractionalDigits() throws {
        let date = try #require(StacklyJSON.date(from: "2026-10-18T03:04:05.123Z"))
        #expect(StacklyJSON.string(from: date) == "2026-10-18T03:04:05.123Z")
        let whole = try #require(StacklyJSON.date(from: "2026-10-18T03:04:05Z"))
        #expect(StacklyJSON.string(from: whole) == "2026-10-18T03:04:05.000Z")
    }

    @Test func readsAnOffsetAsTheSameInstantInUtc() throws {
        let offset = try #require(StacklyJSON.date(from: "2026-10-18T10:04:05+07:00"))
        let utc = try #require(StacklyJSON.date(from: "2026-10-18T03:04:05.000Z"))
        #expect(offset == utc)
    }

    @Test func rejectsATimeWithoutAnOffset() {
        #expect(StacklyJSON.date(from: "2026-10-18T03:04:05") == nil)
        #expect(throws: DecodingError.self) {
            try StacklyJSON.decoder().decode(Date.self, from: Data("\"2026-10-18\"".utf8))
        }
    }

    @Test(arguments: [0, 1, 9, 10, 99, 100, 123, 499, 500, 998, 999])
    func everyMillisecondSurvivesARoundTrip(_ millisecond: Int) throws {
        let text = "2026-10-18T03:04:05.\(String(format: "%03d", millisecond))Z"
        let date = try #require(StacklyJSON.date(from: text))
        #expect(StacklyJSON.string(from: date) == text)
        let encoded = try StacklyJSON.encoder().encode([date])
        let decoded = try StacklyJSON.decoder().decode([Date].self, from: encoded)
        #expect(decoded == [date])
    }

    @Test func encodesTimesBeforeTheEpoch() {
        let date = Date(timeIntervalSince1970: -0.001)
        #expect(StacklyJSON.string(from: date) == "1969-12-31T23:59:59.999Z")
    }

    @Test func contractInfoMatchesTheVersionFixture() throws {
        let fixture = try StacklyJSON.decoder().decode(
            ProtocolVersion.self, from: Fixture.data("protocol-version.json"))
        #expect(ContractInfo.protocolVersion == fixture)
        #expect(ContractInfo.digest.count == 64)
        #expect(ContractInfo.digest.allSatisfy { "0123456789abcdef".contains($0) })
    }
}
