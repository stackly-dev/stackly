#if DEBUG
    import Foundation

    /// The IDs of fixture items. They look like UUID v7 and are not real ones: the
    /// numbers 1 and up of a kind give the same ID on every run (spec 0068).
    public nonisolated enum FixtureID {
        /// The kind byte of an instance.
        public static let instance: UInt8 = 1
        /// The kind byte of an operation.
        public static let operation: UInt8 = 2
        /// The kind byte of a runtime.
        public static let runtime: UInt8 = 3
        /// The kind byte of a daemon.
        public static let daemon: UInt8 = 4

        /// The ID of item `number` of `kind`: bytes 0 to 5 zero, byte 6 `0x70`, byte 8
        /// `0x80`, byte 14 the kind, byte 15 the number.
        public static func uuid(kind: UInt8, number: UInt8) -> UUID {
            UUID(
                uuid: (
                    0, 0, 0, 0, 0, 0, 0x70, 0,
                    0x80, 0, 0, 0, 0, 0, kind, number
                ))
        }
    }
#endif
