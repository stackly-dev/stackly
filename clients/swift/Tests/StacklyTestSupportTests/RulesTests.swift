import Testing

@testable import StacklyTestSupport

/// The same pattern checks the Rust rules in `stackly-evidence` apply.
struct RulesTests {
    @Test func prdIDsNeedAKnownPrefixAndThreeDigits() {
        #expect(Rules.isPRDID("AC-011"))
        #expect(Rules.isPRDID("REQ-051"))
        #expect(Rules.isPRDID("BR-001"))
        #expect(!Rules.isPRDID("AC-11"))
        #expect(!Rules.isPRDID("AC-0111"))
        #expect(!Rules.isPRDID("OQ-011"))
        #expect(!Rules.isPRDID("ac-011"))
        #expect(!Rules.isPRDID("AC011"))
        #expect(!Rules.isPRDID("AC-01a"))
    }

    @Test func exactVersionsRejectSelectors() {
        #expect(Rules.isExactVersion("17.4"))
        #expect(Rules.isExactVersion("2.15.0"))
        #expect(Rules.isExactVersion("25.8.1.5101"))
        #expect(Rules.isExactVersion("8.4.6+build.1"))
        #expect(Rules.isExactVersion("9.1.2-rc1"))
        #expect(!Rules.isExactVersion("17"))
        #expect(!Rules.isExactVersion("@17"))
        #expect(!Rules.isExactVersion("^17.0"))
        #expect(!Rules.isExactVersion("17."))
        #expect(!Rules.isExactVersion("17.4-"))
        #expect(!Rules.isExactVersion("17.4+a_b"))
    }

    @Test func digestsAre64LowercaseHexCharacters() {
        #expect(Rules.isSHA256Hex(String(repeating: "a", count: 64)))
        #expect(!Rules.isSHA256Hex(String(repeating: "A", count: 64)))
        #expect(!Rules.isSHA256Hex("sha256:" + String(repeating: "a", count: 57)))
        #expect(!Rules.isSHA256Hex(String(repeating: "a", count: 63)))
    }

    @Test func argumentsReportEmptyUnknownAndDuplicateIDs() {
        let violations = Rules.argumentViolations(
            covers: ["AC-011", "OQ-001", "AC-011"], expected: "a rules test")
        #expect(violations.count == 2)
        #expect(Rules.argumentViolations(covers: [], expected: "a rules test").count == 1)
        #expect(Rules.argumentViolations(covers: ["AC-011"], expected: "  ").count == 1)
        #expect(Rules.argumentViolations(covers: ["AC-011"], expected: "a rules test").isEmpty)
    }
}
