/// A frame header announced a length the codec refuses.
public enum FrameError: Error, Hashable, Sendable {
    /// The header announced a length of 0: `ipc.frame_empty`.
    case empty
    /// The header announced, or the payload has, more bytes than the limit:
    /// `ipc.frame_too_large`.
    case tooLarge(length: UInt64, max: UInt32)
}

/// Splits a byte stream into frame payloads: a 4 byte big endian length, then
/// that many bytes (spec 0006).
///
/// It refuses a header that announces 0 or more than the limit as soon as the
/// header is complete, before any payload byte is kept for it, and keeps
/// refusing after that. Frames before the refused header can still be taken.
/// The caller takes every complete frame before it pushes more, so the buffer
/// never holds more than one frame plus one read chunk.
public struct FrameDecoder: Sendable {
    /// The length of a frame header, in bytes.
    public static let headerLength = 4

    /// The length limit.
    public private(set) var maxLength: UInt32
    /// Bytes received and not yet handed out, from `start` on.
    private var buffer: [UInt8] = []
    /// Offset in `buffer` of the first byte not yet handed out.
    private var start = 0
    /// Offset in `buffer` of the next header to check. Every header before it
    /// passed.
    private var checked = 0
    /// The first refused header, after which nothing more is accepted.
    private var failure: FrameError?
    /// The most bytes the buffer held at once, for the memory bound test.
    private(set) var peakBufferedCount = 0

    /// A decoder that refuses frames longer than `maxLength` bytes.
    ///
    /// - Parameter maxLength: The largest payload accepted.
    public init(maxLength: UInt32) {
        self.maxLength = maxLength
    }

    /// Changes the limit, after the handshake gave the daemon's.
    ///
    /// - Parameter maxLength: The new largest payload accepted.
    public mutating func setMaxLength(_ maxLength: UInt32) {
        self.maxLength = maxLength
    }

    /// Bytes received and not yet handed out. Above 0 between frames means a
    /// frame has started.
    public var bufferedCount: Int {
        buffer.count - start
    }

    /// Adds one chunk of the stream.
    ///
    /// - Parameter bytes: The bytes read.
    /// - Throws: ``FrameError`` as soon as a complete header announces a length
    ///   of 0 or above the limit, and again on every later call.
    public mutating func push(_ bytes: some Collection<UInt8>) throws(FrameError) {
        if let failure { throw failure }
        if start > 0 {
            buffer.removeSubrange(0..<start)
            checked -= start
            start = 0
        }
        buffer.append(contentsOf: bytes)
        peakBufferedCount = max(peakBufferedCount, buffer.count)
        while checked + Self.headerLength <= buffer.count {
            let length = Self.readLength(buffer, at: checked)
            if let error = Self.check(UInt64(length), max: maxLength) {
                failure = error
                throw error
            }
            checked += Self.headerLength + Int(length)
        }
    }

    /// Takes the next complete frame payload, or `nil` when no frame is
    /// complete yet.
    public mutating func nextFrame() -> [UInt8]? {
        guard start < checked, buffer.count - start >= Self.headerLength else { return nil }
        let end = start + Self.headerLength + Int(Self.readLength(buffer, at: start))
        guard end <= buffer.count else { return nil }
        let payload = Array(buffer[(start + Self.headerLength)..<end])
        start = end
        return payload
    }

    /// The big endian length at `offset`.
    static func readLength(_ bytes: [UInt8], at offset: Int) -> UInt32 {
        bytes[offset..<(offset + headerLength)].reduce(0) { ($0 << 8) | UInt32($1) }
    }

    /// The error for a length of 0 or above `max`, or `nil` when it is fine.
    static func check(_ length: UInt64, max: UInt32) -> FrameError? {
        if length == 0 { return .empty }
        if length > UInt64(max) { return .tooLarge(length: length, max: max) }
        return nil
    }
}
