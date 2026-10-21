import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// A rendered snapshot: 8 bit RGBA pixels in sRGB, rows from the top.
nonisolated struct SnapshotImage: Equatable, Sendable {
    /// The width in pixels.
    let width: Int
    /// The height in pixels.
    let height: Int
    /// `width * height * 4` bytes, premultiplied alpha.
    let pixels: [UInt8]

    /// The result of comparing two images of one size.
    struct Difference: Equatable, Sendable {
        /// How many pixels differ by more than the tolerance in any channel.
        let pixelCount: Int
        /// The largest channel difference found anywhere, from 0 to 255.
        let largest: Int
        /// The differing pixels, one flag per pixel, row by row.
        let mask: [Bool]
    }

    /// Redraws the cached display of `view` into an RGBA 8 bit sRGB context.
    ///
    /// - Throws: ``SnapshotError/captureFailed(_:)`` when AppKit gives no bitmap.
    @MainActor
    static func capture(_ view: NSView) throws -> SnapshotImage {
        let bounds = view.bounds
        guard let representation = view.bitmapImageRepForCachingDisplay(in: bounds) else {
            throw SnapshotError.captureFailed("no bitmap for the view")
        }
        view.cacheDisplay(in: bounds, to: representation)
        guard let cgImage = representation.cgImage else {
            throw SnapshotError.captureFailed("no image in the bitmap")
        }
        return try draw(cgImage)
    }

    /// Draws `cgImage` into a fresh RGBA 8 bit sRGB context and copies the pixels.
    static func draw(_ cgImage: CGImage) throws -> SnapshotImage {
        let width = cgImage.width
        let height = cgImage.height
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
            let context = CGContext(
                data: nil,
                width: width,
                height: height,
                bitsPerComponent: 8,
                bytesPerRow: width * 4,
                space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            )
        else {
            throw SnapshotError.captureFailed("no sRGB context")
        }
        context.draw(cgImage, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let data = context.data else {
            throw SnapshotError.captureFailed("no pixel data")
        }
        let bytes = UnsafeBufferPointer(
            start: data.assumingMemoryBound(to: UInt8.self), count: width * height * 4)
        return SnapshotImage(width: width, height: height, pixels: Array(bytes))
    }

    /// Reads a PNG file, redrawn into the same RGBA 8 bit sRGB layout.
    static func read(_ url: URL) throws -> SnapshotImage {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
            let cgImage = CGImageSourceCreateImageAtIndex(source, 0, nil)
        else {
            throw SnapshotError.captureFailed("cannot read \(url.path(percentEncoded: false))")
        }
        return try draw(cgImage)
    }

    /// The image as PNG data.
    func pngData() throws -> Data {
        guard let space = CGColorSpace(name: CGColorSpace.sRGB) else {
            throw SnapshotError.captureFailed("no sRGB space")
        }
        var copy = pixels
        let image: CGImage? = copy.withUnsafeMutableBytes { buffer in
            guard
                let context = CGContext(
                    data: buffer.baseAddress,
                    width: width,
                    height: height,
                    bitsPerComponent: 8,
                    bytesPerRow: width * 4,
                    space: space,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                )
            else { return nil }
            return context.makeImage()
        }
        let output = NSMutableData()
        guard let image,
            let destination = CGImageDestinationCreateWithData(
                output, UTType.png.identifier as CFString, 1, nil)
        else {
            throw SnapshotError.captureFailed("cannot encode a PNG")
        }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else {
            throw SnapshotError.captureFailed("cannot encode a PNG")
        }
        return output as Data
    }

    /// Compares with `other`, which must have the same size.
    ///
    /// A pixel differs when any RGBA channel differs by more than `tolerance`.
    func difference(from other: SnapshotImage, tolerance: Int = 2) -> Difference {
        precondition(width == other.width && height == other.height, "sizes must match")
        var count = 0
        var largest = 0
        var mask = [Bool](repeating: false, count: width * height)
        for pixel in 0..<(width * height) {
            var differs = false
            for channel in 0..<4 {
                let index = pixel * 4 + channel
                let delta = abs(Int(pixels[index]) - Int(other.pixels[index]))
                largest = max(largest, delta)
                if delta > tolerance { differs = true }
            }
            if differs {
                count += 1
                mask[pixel] = true
            }
        }
        return Difference(pixelCount: count, largest: largest, mask: mask)
    }
}

/// Why a snapshot could not be taken or compared.
nonisolated enum SnapshotError: Error, CustomStringConvertible {
    /// AppKit gave no usable bitmap.
    case captureFailed(String)
    /// The view did not settle within the limit; the last capture is kept as an artifact.
    case didNotSettle(last: SnapshotImage)

    var description: String {
        switch self {
        case .captureFailed(let reason): "snapshot capture failed: \(reason)"
        case .didNotSettle: "snapshot did not settle"
        }
    }
}
