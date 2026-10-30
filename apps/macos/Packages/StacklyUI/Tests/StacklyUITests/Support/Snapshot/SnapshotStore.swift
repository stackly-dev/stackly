import Foundation

/// Where snapshot references and failure artifacts live, and how an image is checked
/// against them (spec 0068, AC-6). The settings are values so a test can point it at a
/// temporary folder.
struct SnapshotStore: Sendable {
    /// The most a reference PNG may weigh: 1 MB.
    static let maximumReferenceBytes = 1_048_576

    /// The folder of the reference PNGs: `__Snapshots__/<suite file name>/` next to the suite.
    let referenceDirectory: URL
    /// The folder of the `actual` and `diff` PNGs of a failed check.
    let failureDirectory: URL
    /// Whether a changed reference is written again (`STACKLY_SNAPSHOT_RECORD=1`).
    let record: Bool
    /// Whether this is an evidence run, which never writes a reference.
    let evidence: Bool

    /// What a check found.
    enum Outcome: Equatable {
        /// The image matches.
        case match
        /// The check failed; the text is the message of the issue.
        case failure(String)
    }

    /// The store for the suite file at `suitePath`, with the settings of the environment.
    static func forSuite(
        at suitePath: String, environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> SnapshotStore {
        let file = URL(filePath: suitePath)
        let stem = file.deletingPathExtension().lastPathComponent
        let evidence = environment["STACKLY_EVIDENCE_DIR"].map { $0.isEmpty == false } ?? false
        return SnapshotStore(
            referenceDirectory: file.deletingLastPathComponent()
                .appending(path: "__Snapshots__").appending(path: stem),
            failureDirectory: SourceTree.repositoryRoot
                .appending(path: "target/snapshot-failures").appending(path: stem),
            record: environment["STACKLY_SNAPSHOT_RECORD"] == "1" && evidence == false,
            evidence: evidence)
    }

    /// Whether `name` is lowercase ASCII letters, digits, and `-` only.
    static func isValid(name: String) -> Bool {
        name.isEmpty == false
            && name.utf8.allSatisfy {
                ($0 >= 97 && $0 <= 122) || ($0 >= 48 && $0 <= 57) || $0 == 45
            }
    }

    /// The reference file of `name` and `suffix`.
    func referenceURL(name: String, suffix: String) -> URL {
        referenceDirectory.appending(path: "\(name).\(suffix).png")
    }

    /// The artifact file of `name`, `suffix`, and `kind` (`actual` or `diff`).
    func failureURL(name: String, suffix: String, kind: String) -> URL {
        failureDirectory.appending(path: "\(name).\(suffix).\(kind).png")
    }

    /// Deletes the artifacts of an earlier failed run of `name`.
    func clearArtifacts(name: String) {
        let manager = FileManager.default
        let entries = (try? manager.contentsOfDirectory(atPath: failureDirectory.path())) ?? []
        for entry in entries
        where entry.hasPrefix("\(name).")
            && (entry.hasSuffix(".actual.png") || entry.hasSuffix(".diff.png"))
        {
            try? manager.removeItem(at: failureDirectory.appending(path: entry))
        }
    }

    /// Writes `image` as the artifact `actual` of `name` and `suffix`; returns its path.
    @discardableResult
    func writeActual(_ image: SnapshotImage, name: String, suffix: String) -> String {
        let url = failureURL(name: name, suffix: suffix, kind: "actual")
        try? FileManager.default.createDirectory(
            at: failureDirectory, withIntermediateDirectories: true)
        try? image.pngData().write(to: url)
        return url.path(percentEncoded: false)
    }

    /// Checks `image` against the reference of `name` and `suffix`.
    func check(_ image: SnapshotImage, name: String, suffix: String) throws -> Outcome {
        let url = referenceURL(name: name, suffix: suffix)
        let path = url.path(percentEncoded: false)
        let png = try image.pngData()
        guard FileManager.default.fileExists(atPath: path) else {
            if evidence { return .failure("missing reference \(path); evidence runs never record") }
            return try write(png, to: url, then: "recorded \(path), review and commit")
        }
        let referenceBytes =
            (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? Int) ?? 0
        if referenceBytes > Self.maximumReferenceBytes {
            return .failure("reference \(path) is \(referenceBytes) bytes, above the 1 MB limit")
        }
        let reference = try SnapshotImage.read(url)
        guard reference.width == image.width, reference.height == image.height else {
            let actual = writeActual(image, name: name, suffix: suffix)
            return .failure(
                "size differs for \(path): reference \(reference.width) × \(reference.height) px, "
                    + "actual \(image.width) × \(image.height) px; actual at \(actual)")
        }
        let difference = image.difference(from: reference)
        guard difference.pixelCount > 0 else { return .match }
        if record {
            return try write(png, to: url, then: "re-recorded \(path)")
        }
        let actual = writeActual(image, name: name, suffix: suffix)
        let diffURL = failureURL(name: name, suffix: suffix, kind: "diff")
        try? Self.diffImage(reference: reference, difference: difference).pngData().write(
            to: diffURL)
        return .failure(
            "\(difference.pixelCount) px differ from \(path), largest channel difference "
                + "\(difference.largest) of 255; actual at \(actual), diff at "
                + "\(diffURL.path(percentEncoded: false))")
    }

    private func write(_ png: Data, to url: URL, then message: String) throws -> Outcome {
        let path = url.path(percentEncoded: false)
        guard png.count <= Self.maximumReferenceBytes else {
            return .failure("the PNG for \(path) is \(png.count) bytes, above the 1 MB limit")
        }
        try FileManager.default.createDirectory(
            at: referenceDirectory, withIntermediateDirectories: true)
        try png.write(to: url)
        return .failure(message)
    }

    /// The reference dimmed to a third of its brightness with every differing pixel red.
    static func diffImage(
        reference: SnapshotImage, difference: SnapshotImage.Difference
    ) -> SnapshotImage {
        var pixels = reference.pixels
        for pixel in 0..<(reference.width * reference.height) {
            let base = pixel * 4
            if difference.mask[pixel] {
                pixels[base] = 255
                pixels[base + 1] = 0
                pixels[base + 2] = 0
                pixels[base + 3] = 255
            } else {
                pixels[base] = pixels[base] / 3
                pixels[base + 1] = pixels[base + 1] / 3
                pixels[base + 2] = pixels[base + 2] / 3
                pixels[base + 3] = 255
            }
        }
        return SnapshotImage(width: reference.width, height: reference.height, pixels: pixels)
    }
}
