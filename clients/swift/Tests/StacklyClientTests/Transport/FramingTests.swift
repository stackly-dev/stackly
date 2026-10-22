import Foundation
import Testing

@testable import StacklyClient

/// Swift framing against the shared vectors in `protocols/fixtures/framing/`
/// and the memory bound (spec 0013 AC-3, AC-20).
struct FramingTests {
    struct Vector: Decodable, Sendable, CustomTestStringConvertible {
        let name: String
        let hex: String
        let chunks: [Int]?
        let max: UInt32
        let frames: [String]
        let error: String?

        var testDescription: String { name }
    }

    static func vectors() throws -> [Vector] {
        struct Document: Decodable {
            let vectors: [Vector]
        }
        return try JSONDecoder()
            .decode(Document.self, from: Fixture.data("framing/vectors.json")).vectors
    }

    /// Feeds `bytes` in `chunks`, taking every frame after each push like the
    /// connection does, and returns the frames and the first error.
    static func run(
        _ bytes: [UInt8],
        chunks: [Int],
        max: UInt32
    ) -> (frames: [[UInt8]], error: FrameError?) {
        var decoder = FrameDecoder(maxLength: max)
        var frames: [[UInt8]] = []
        var offset = 0
        for size in chunks {
            let end = min(bytes.count, offset + size)
            do {
                try decoder.push(bytes[offset..<end])
            } catch {
                while let frame = decoder.nextFrame() { frames.append(frame) }
                return (frames, error)
            }
            while let frame = decoder.nextFrame() { frames.append(frame) }
            offset = end
        }
        return (frames, nil)
    }

    @Test(arguments: try FramingTests.vectors())
    func vectorDecodesInItsChunksAndAtEverySplit(_ vector: Vector) throws {
        let bytes = try Hex.decode(vector.hex)
        let expected = try vector.frames.map(Hex.decode)
        var plans = [vector.chunks ?? [bytes.count], Array(repeating: 1, count: bytes.count)]
        for split in 0...bytes.count {
            plans.append([split, bytes.count - split])
        }
        for plan in plans {
            let result = Self.run(bytes, chunks: plan, max: vector.max)
            #expect(result.frames == expected, "\(vector.name) in \(plan)")
            #expect(
                result.error.map(ClientErrorCatalog.code(of:)) == vector.error, "\(vector.name)")
        }
    }

    @Test func aFrameOfExactlyOneMebibyteFitsAndTheBufferStaysBounded() throws {
        let max = ContractLimits.controlFrameMax
        let payload = [UInt8](repeating: UInt8(ascii: "a"), count: Int(max))
        let stream =
            try FrameEncoder.encode(payload, maxLength: max)
            + FrameEncoder.encode(
                Array("{}".utf8), maxLength: max)
        var decoder = FrameDecoder(maxLength: max)
        var frames: [[UInt8]] = []
        var offset = 0
        while offset < stream.count {
            let end = min(stream.count, offset + ContractLimits.readChunk)
            try decoder.push(stream[offset..<end])
            while let frame = decoder.nextFrame() { frames.append(frame) }
            offset = end
        }
        #expect(frames.count == 2)
        #expect(frames.first?.count == Int(max))
        #expect(decoder.peakBufferedCount <= Int(max) + 4 + ContractLimits.readChunk)
    }

    @Test func oneByteAboveTheLimitFailsBeforeThePayloadIsKept() {
        let max = ContractLimits.controlFrameMax
        var decoder = FrameDecoder(maxLength: max)
        let header: [UInt8] = [0x00, 0x10, 0x00, 0x01]
        #expect(throws: FrameError.tooLarge(length: UInt64(max) + 1, max: max)) {
            try decoder.push(header)
        }
        #expect(decoder.peakBufferedCount == 4)
        #expect(throws: FrameError.self) { try decoder.push([0x7b]) }
    }

    @Test func aLowerLimitRefusesAHeaderAlreadyCheckedButNotHandedOut() throws {
        var decoder = FrameDecoder(maxLength: 1_024)
        // One frame of 2 bytes, then the header of one of 256 bytes.
        try decoder.push([0x00, 0x00, 0x00, 0x02, 0x7b, 0x7d, 0x00, 0x00, 0x01, 0x00])
        #expect(decoder.nextFrame() == Array("{}".utf8))
        #expect(decoder.refusal == nil)
        decoder.setMaxLength(64)
        #expect(decoder.refusal == .tooLarge(length: 256, max: 64))
        #expect(decoder.nextFrame() == nil)
        #expect(throws: FrameError.tooLarge(length: 256, max: 64)) { try decoder.push([0x7b]) }
    }

    @Test func aHigherLimitAcceptsAHeaderOnlyTheOldLimitRefused() throws {
        var decoder = FrameDecoder(maxLength: 2)
        let bytes: [UInt8] = [0x00, 0x00, 0x00, 0x02, 0x7b, 0x7d, 0x00, 0x00, 0x00, 0x03]
        #expect(throws: FrameError.tooLarge(length: 3, max: 2)) { try decoder.push(bytes) }
        #expect(decoder.nextFrame() == Array("{}".utf8))
        decoder.setMaxLength(16)
        #expect(decoder.refusal == nil)
        try decoder.push(Array("[1]".utf8))
        #expect(decoder.nextFrame() == Array("[1]".utf8))
    }

    @Test func encoderRefusesEmptyAndOversizedPayloadsBeforeWriting() throws {
        #expect(throws: FrameError.empty) { try FrameEncoder.encode([UInt8](), maxLength: 16) }
        #expect(throws: FrameError.tooLarge(length: 5, max: 4)) {
            try FrameEncoder.encode(Array("12345".utf8), maxLength: 4)
        }
        #expect(
            try FrameEncoder.encode(Array("{}".utf8), maxLength: 16) == [0, 0, 0, 2, 0x7b, 0x7d])
    }

    @Test func depthScannerCountsOnlyBracketsOutsideStrings() {
        let deep32 = String(repeating: "[", count: 32) + String(repeating: "]", count: 32)
        let deep33 = String(repeating: "[", count: 33) + String(repeating: "]", count: 33)
        #expect(JSONDepth.exceeds(Array(deep32.utf8), 32) == false)
        #expect(JSONDepth.exceeds(Array(deep33.utf8), 32))
        let quoted = #"{"a":"[[[[\"[[[[","b":[{}]}"#
        // Depth 3 ({, [, {}); the brackets and the escaped quote in the string
        // do not count.
        #expect(JSONDepth.exceeds(Array(quoted.utf8), 3) == false)
        #expect(JSONDepth.exceeds(Array(quoted.utf8), 2))
    }
}

/// Hex text to bytes, for the shared vectors.
enum Hex {
    struct InvalidHex: Error {}

    static func decode(_ text: String) throws -> [UInt8] {
        let digits = Array(text.utf8)
        guard digits.count.isMultiple(of: 2) else { throw InvalidHex() }
        return try stride(from: 0, to: digits.count, by: 2).map { index in
            guard
                let byte = UInt8(
                    String(decoding: digits[index...index + 1], as: UTF8.self), radix: 16)
            else { throw InvalidHex() }
            return byte
        }
    }
}
