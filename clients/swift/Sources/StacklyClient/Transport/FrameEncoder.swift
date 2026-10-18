/// Writes frames: a 4 byte big endian length, then the payload (spec 0006).
public enum FrameEncoder {
    /// `payload` prefixed with its length.
    ///
    /// - Parameters:
    ///   - payload: The JSON payload.
    ///   - maxLength: The largest payload allowed: 1 048 576 bytes before the
    ///     handshake, `limits.maxFrameBytes` after it.
    /// - Throws: ``FrameError/empty`` for an empty payload and
    ///   ``FrameError/tooLarge(length:max:)`` above the limit, before anything
    ///   is written.
    public static func encode(
        _ payload: some Collection<UInt8>,
        maxLength: UInt32
    ) throws(FrameError) -> [UInt8] {
        let length = UInt64(payload.count)
        if let error = FrameDecoder.check(length, max: maxLength) { throw error }
        let header = UInt32(length)
        var frame: [UInt8] = [
            UInt8(truncatingIfNeeded: header >> 24),
            UInt8(truncatingIfNeeded: header >> 16),
            UInt8(truncatingIfNeeded: header >> 8),
            UInt8(truncatingIfNeeded: header),
        ]
        frame.reserveCapacity(FrameDecoder.headerLength + payload.count)
        frame.append(contentsOf: payload)
        return frame
    }
}
