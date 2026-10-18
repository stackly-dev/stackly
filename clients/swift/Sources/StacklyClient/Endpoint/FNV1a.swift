/// The FNV-1a 64 bit hash Rust uses for the runtime root of an alternative
/// root (spec 0004, `RootSet::under_override`).
public enum FNV1a {
    /// The FNV-1a 64 bit offset basis.
    static let offsetBasis: UInt64 = 0xcbf2_9ce4_8422_2325
    /// The FNV-1a 64 bit prime.
    static let prime: UInt64 = 0x0000_0100_0000_01b3

    /// The FNV-1a 64 bit hash of `bytes`.
    ///
    /// - Parameter bytes: The raw bytes, never text that went through a
    ///   `String` or `URL`.
    public static func hash64(_ bytes: some Sequence<UInt8>) -> UInt64 {
        bytes.reduce(offsetBasis) { hash, byte in
            (hash ^ UInt64(byte)) &* prime
        }
    }

    /// The lower 32 bits of ``hash64(_:)`` as 8 lowercase hex characters, the
    /// `<fnv8>` of `stackly-<fnv8>`.
    ///
    /// - Parameter bytes: The raw bytes of the canonical root path.
    public static func short8(_ bytes: some Sequence<UInt8>) -> String {
        hex(hash64(bytes) & 0xffff_ffff, digits: 8)
    }

    /// `value` as `digits` lowercase hex characters, zero padded.
    static func hex(_ value: UInt64, digits: Int) -> String {
        let text = String(value, radix: 16)
        return String(repeating: "0", count: max(0, digits - text.count)) + text
    }
}
