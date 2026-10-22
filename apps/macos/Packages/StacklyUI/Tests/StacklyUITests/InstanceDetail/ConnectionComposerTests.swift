import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// `ConnectionComposer` against the fixture the Rust `compose` reads too (spec 0021 AC-3):
/// both give the same string for every case.
struct ConnectionComposerTests {
    @Test func everyFixtureCaseGivesTheWrittenStringOrTheWrittenError() throws {
        let cases = try DetailFixture.composeCases()
        #expect(cases.count >= 15)
        for item in cases {
            if let expected = item.expected {
                let text = try ConnectionComposer.compose(parts: item.parts, values: item.values)
                #expect(text == expected, "\(item.name)")
            } else if let reference = item.missingValue {
                #expect(
                    throws: ConnectionComposer.Failure.missingValue(reference: reference),
                    "\(item.name)"
                ) {
                    try ConnectionComposer.compose(parts: item.parts, values: item.values)
                }
            } else {
                Issue.record("\(item.name) has neither an expected string nor a missing value")
            }
        }
    }

    @Test func aPartOfAnUnknownKindOrWithoutItsFieldIsInvalid() {
        let unknown = ConnectionPart(kind: .unrecognized, text: "x", secret: nil)
        let hollowText = ConnectionPart(kind: .text, text: nil, secret: nil)
        let hollowSecret = ConnectionPart(kind: .secret, text: nil, secret: nil)
        let both = ConnectionPart(
            kind: .text, text: "x", secret: ConnectionSecretPart(field: "f", reference: "r"))
        for part in [unknown, hollowText, hollowSecret, both] {
            #expect(throws: ConnectionComposer.Failure.invalidPart) {
                try ConnectionComposer.compose(parts: [part], values: [:])
            }
        }
    }

    @Test func referencesAreListedOnceInOrder() throws {
        let item = try #require(
            try DetailFixture.composeCases().first { $0.name == "two secrets in one entry" })
        let references = ConnectionComposer.references(in: item.parts + item.parts)
        #expect(references.count == 2)
        #expect(Set(references) == Set(item.values.keys))
    }

    @Test func theComposerForARevealModelReadsTheRevealedValues() throws {
        let item = try #require(
            try DetailFixture.composeCases().first { $0.name == "a space" })
        let reference = try #require(item.values.keys.first)
        let compose = ConnectionComposer.composer(for: item.parts)

        let text = try compose([reference: RevealedSecret(reference: reference, value: "pass word")]
        )
        #expect(text == item.expected)
        #expect(throws: ConnectionComposer.Failure.self) { try compose([:]) }
    }
}
