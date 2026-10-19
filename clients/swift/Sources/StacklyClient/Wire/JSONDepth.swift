/// Counts how deep JSON nests objects and arrays without a full parse, the
/// same scan Rust uses (`json::exceeds_depth`).
public enum JSONDepth {
    /// Whether `bytes` nest objects and arrays deeper than `maxDepth`.
    /// Brackets inside strings, escaped quotes included, do not count.
    ///
    /// - Parameters:
    ///   - bytes: The JSON payload.
    ///   - maxDepth: The deepest nesting accepted.
    public static func exceeds(_ bytes: some Sequence<UInt8>, _ maxDepth: UInt32) -> Bool {
        var depth: UInt32 = 0
        var inString = false
        var escaped = false
        for byte in bytes {
            if inString {
                if escaped {
                    escaped = false
                } else if byte == UInt8(ascii: "\\") {
                    escaped = true
                } else if byte == UInt8(ascii: "\"") {
                    inString = false
                }
                continue
            }
            switch byte {
            case UInt8(ascii: "\""):
                inString = true
            case UInt8(ascii: "{"), UInt8(ascii: "["):
                depth += 1
                if depth > maxDepth { return true }
            case UInt8(ascii: "}"), UInt8(ascii: "]"):
                depth = depth > 0 ? depth - 1 : 0
            default:
                break
            }
        }
        return false
    }
}
