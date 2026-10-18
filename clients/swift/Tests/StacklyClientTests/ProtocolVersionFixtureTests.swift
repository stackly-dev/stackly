import Foundation
import StacklyClient
import Testing

/// Checks the generated `ProtocolVersion` against the fixture the Rust tests
/// also read, comparing decoded values rather than raw JSON text.
struct ProtocolVersionFixtureTests {
    @Test func fixtureDecodesToCurrentVersion() throws {
        let decoded = try StacklyJSON.decoder().decode(
            ProtocolVersion.self, from: Fixture.data("protocol-version.json"))

        #expect(decoded == ProtocolVersion(major: 1, minor: 0))
    }

    @Test func encodedValueDecodesLikeTheFixture() throws {
        let fixture = try StacklyJSON.decoder().decode(
            ProtocolVersion.self, from: Fixture.data("protocol-version.json"))

        let encoded = try StacklyJSON.encoder().encode(ProtocolVersion(major: 1, minor: 0))
        let roundTripped = try StacklyJSON.decoder().decode(ProtocolVersion.self, from: encoded)

        #expect(roundTripped == fixture)
    }
}
