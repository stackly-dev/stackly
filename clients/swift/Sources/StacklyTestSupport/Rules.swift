/// The pattern checks the Rust rules in `stackly-evidence` also apply, so a
/// wrong argument fails the test on a laptop instead of only in a run.
enum Rules {
    /// The prefixes a PRD ID may carry.
    private static let prdPrefixes: Set<Substring> = ["AC", "REQ", "NFR", "VAL", "BR"]

    /// True when `id` matches `^(AC|REQ|NFR|VAL|BR)-[0-9]{3}$`, for example
    /// `AC-011` or `NFR-005`.
    static func isPRDID(_ id: String) -> Bool {
        guard let dash = id.firstIndex(of: "-") else {
            return false
        }
        let number = id.utf8[id.utf8.index(after: dash)...]
        return prdPrefixes.contains(id[..<dash]) && number.count == 3
            && number.allSatisfy(isASCIIDigit)
    }

    /// True when `version` is exact, matching
    /// `^[0-9]+(\.[0-9]+)+([+-][0-9A-Za-z.]+)?$`: `17.4` and `8.4.6+build.1`
    /// pass, selectors such as `17`, `@17`, or `^17` do not.
    static func isExactVersion(_ version: String) -> Bool {
        let bytes = Array(version.utf8)
        let split = bytes.firstIndex { $0 == UInt8(ascii: "+") || $0 == UInt8(ascii: "-") }
        let core = bytes[..<(split ?? bytes.endIndex)]
        let parts = core.split(separator: UInt8(ascii: "."), omittingEmptySubsequences: false)
        let coreIsExact =
            parts.count >= 2 && parts.allSatisfy { !$0.isEmpty && $0.allSatisfy(isASCIIDigit) }
        guard let split else {
            return coreIsExact
        }
        let suffix = bytes[(split + 1)...]
        return coreIsExact && !suffix.isEmpty
            && suffix.allSatisfy { isASCIIAlphanumeric($0) || $0 == UInt8(ascii: ".") }
    }

    /// True when `digest` is a SHA-256 written as 64 lowercase hex characters
    /// with no prefix, the same form as `sha256` in the runtime inventory.
    static func isSHA256Hex(_ digest: String) -> Bool {
        digest.utf8.count == 64
            && digest.utf8.allSatisfy { isASCIIDigit($0) || (0x61...0x66).contains($0) }
    }

    /// Every rule `covers` and `expected` break: at least one ID, every ID a
    /// PRD ID, no ID twice, and a non empty expected sentence.
    static func argumentViolations(covers: [String], expected: String) -> [String] {
        var violations: [String] = []
        if covers.isEmpty {
            violations.append("covers must list at least one PRD ID")
        }
        for (index, id) in covers.enumerated() {
            if !isPRDID(id) {
                violations.append(
                    "covers entry `\(id)` is not a PRD ID (AC, REQ, NFR, VAL, or BR plus three digits)"
                )
            }
            if covers[..<index].contains(id) {
                violations.append("covers lists `\(id)` twice")
            }
        }
        if isBlank(expected) {
            violations.append("expected must not be empty")
        }
        return violations
    }

    /// True when `text` holds nothing but whitespace.
    static func isBlank(_ text: String) -> Bool {
        text.allSatisfy(\.isWhitespace)
    }

    private static func isASCIIDigit(_ byte: UInt8) -> Bool {
        (0x30...0x39).contains(byte)
    }

    private static func isASCIIAlphanumeric(_ byte: UInt8) -> Bool {
        isASCIIDigit(byte) || (0x41...0x5A).contains(byte) || (0x61...0x7A).contains(byte)
    }
}
