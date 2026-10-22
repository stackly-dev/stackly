import AppKit
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// The snapshot helper itself (spec 0068, AC-4 to AC-6): the comparison, the recording
/// rules, the artifacts, the size limits, and the render environment.
extension SnapshotTests {
    struct SnapshotHelperTests {
        // MARK: - Fixtures for the pure checks

        /// A solid image of `value` in the red, green, and blue channels.
        static func solid(_ value: UInt8, width: Int = 8, height: Int = 6) -> SnapshotImage {
            SnapshotImage(
                width: width, height: height,
                pixels: (0..<(width * height)).flatMap { _ in [value, value, value, 255] })
        }

        /// `image` with one pixel's red channel changed by `delta`.
        static func changing(_ image: SnapshotImage, by delta: Int) -> SnapshotImage {
            var pixels = image.pixels
            pixels[0] = UInt8(Int(pixels[0]) + delta)
            return SnapshotImage(width: image.width, height: image.height, pixels: pixels)
        }

        static func temporaryStore(
            record: Bool = false, evidence: Bool = false
        ) throws -> (SnapshotStore, URL) {
            let root = URL(filePath: NSTemporaryDirectory())
                .appending(path: "stackly-snapshot-\(UUID().uuidString.prefix(8))")
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            let store = SnapshotStore(
                referenceDirectory: root.appending(path: "references"),
                failureDirectory: root.appending(path: "failures"),
                record: record, evidence: evidence)
            return (store, root)
        }

        // MARK: - Comparison

        @Test func aChannelDifferenceOfTwoPassesAndThreeFails() {
            let base = Self.solid(100)
            #expect(Self.changing(base, by: 2).difference(from: base).pixelCount == 0)
            let off = Self.changing(base, by: 3).difference(from: base)
            #expect(off.pixelCount == 1)
            #expect(off.largest == 3)
        }

        @Test func aMissingReferenceIsWrittenAndTheTestFails() throws {
            let (store, root) = try Self.temporaryStore()
            defer { try? FileManager.default.removeItem(at: root) }
            let outcome = try store.check(Self.solid(10), name: "page", suffix: "light")
            let path = store.referenceURL(name: "page", suffix: "light").path(percentEncoded: false)
            #expect(outcome == .failure("recorded \(path), review and commit"))
            #expect(FileManager.default.fileExists(atPath: path))
            #expect(try store.check(Self.solid(10), name: "page", suffix: "light") == .match)
        }

        @Test func anEvidenceRunNeverWritesAReference() throws {
            let (store, root) = try Self.temporaryStore(evidence: true)
            defer { try? FileManager.default.removeItem(at: root) }
            let outcome = try store.check(Self.solid(10), name: "page", suffix: "light")
            guard case .failure(let message) = outcome else {
                Issue.record("a missing reference must fail in an evidence run")
                return
            }
            #expect(message.hasPrefix("missing reference"))
            let path = store.referenceURL(name: "page", suffix: "light").path(percentEncoded: false)
            #expect(FileManager.default.fileExists(atPath: path) == false)
        }

        @Test func aChangedPixelWritesActualAndDiffAndReportsTheCount() throws {
            let (store, root) = try Self.temporaryStore()
            defer { try? FileManager.default.removeItem(at: root) }
            _ = try store.check(Self.solid(100), name: "page", suffix: "light")
            let outcome = try store.check(
                Self.changing(Self.solid(100), by: 9), name: "page", suffix: "light")
            guard case .failure(let message) = outcome else {
                Issue.record("a changed pixel must fail")
                return
            }
            #expect(message.contains("1 px differ"))
            #expect(message.contains("largest channel difference 9 of 255"))
            let actual = store.failureURL(name: "page", suffix: "light", kind: "actual")
            let diff = store.failureURL(name: "page", suffix: "light", kind: "diff")
            #expect(message.contains(actual.path(percentEncoded: false)))
            let picture = try SnapshotImage.read(diff)
            #expect(picture.pixels[0] == 255 && picture.pixels[1] == 0 && picture.pixels[2] == 0)
            #expect(picture.pixels[4] == 33)  // 100 / 3: the dimmed reference
        }

        @Test func aSizeChangeFailsWithBothSizes() throws {
            let (store, root) = try Self.temporaryStore()
            defer { try? FileManager.default.removeItem(at: root) }
            _ = try store.check(Self.solid(100), name: "page", suffix: "light")
            let outcome = try store.check(
                Self.solid(100, width: 9), name: "page", suffix: "light")
            guard case .failure(let message) = outcome else {
                Issue.record("a size change must fail")
                return
            }
            #expect(message.contains("reference 8 × 6 px, actual 9 × 6 px"))
        }

        @Test func recordModeRewritesOnlyWhatChangedAndStillFails() throws {
            let (store, root) = try Self.temporaryStore(record: true)
            defer { try? FileManager.default.removeItem(at: root) }
            _ = try store.check(Self.solid(100), name: "page", suffix: "light")
            #expect(try store.check(Self.solid(100), name: "page", suffix: "light") == .match)
            let outcome = try store.check(Self.solid(150), name: "page", suffix: "light")
            let path = store.referenceURL(name: "page", suffix: "light").path(percentEncoded: false)
            #expect(outcome == .failure("re-recorded \(path)"))
            #expect(try store.check(Self.solid(150), name: "page", suffix: "light") == .match)
        }

        @Test func aReferenceAboveOneMegabyteIsNeverWrittenNorAccepted() throws {
            // A noisy image does not compress: 600 × 600 px of random bytes is above 1 MB.
            var generator = SystemRandomNumberGenerator()
            let noise = (0..<(600 * 600 * 4)).map { _ in
                UInt8.random(in: 0...255, using: &generator)
            }
            let image = SnapshotImage(width: 600, height: 600, pixels: noise)
            let (store, root) = try Self.temporaryStore(record: true)
            defer { try? FileManager.default.removeItem(at: root) }
            guard case .failure(let message) = try store.check(image, name: "big", suffix: "light")
            else {
                Issue.record("an oversize PNG must fail")
                return
            }
            #expect(message.contains("above the 1 MB limit"))
            let path = store.referenceURL(name: "big", suffix: "light").path(percentEncoded: false)
            #expect(FileManager.default.fileExists(atPath: path) == false)
            // An oversize reference already on disk fails the comparison too.
            try FileManager.default.createDirectory(
                at: store.referenceDirectory, withIntermediateDirectories: true)
            try image.pngData().write(to: store.referenceURL(name: "big", suffix: "light"))
            guard case .failure(let again) = try store.check(image, name: "big", suffix: "light")
            else {
                Issue.record("an oversize reference must fail")
                return
            }
            #expect(again.contains("above the 1 MB limit"))
        }

        @Test func reduceTransparencyMustEqualLightOrNameSystemMaterial() throws {
            let (store, root) = try Self.temporaryStore()
            defer { try? FileManager.default.removeItem(at: root) }
            let light = Self.solid(100)
            #expect(store.checkIdentical(light, to: light, name: "page", suffix: "rt") == .match)
            guard
                case .failure(let message) = store.checkIdentical(
                    Self.changing(light, by: 40), to: light, name: "page", suffix: "rt")
            else {
                Issue.record("a different rt render must fail")
                return
            }
            #expect(message.contains("systemMaterial: true"))
            #expect(
                FileManager.default.fileExists(
                    atPath: store.failureURL(name: "page", suffix: "rt", kind: "diff").path()))
        }

        @Test func startingACheckDeletesTheArtifactsOfAnEarlierFailure() throws {
            let (store, root) = try Self.temporaryStore()
            defer { try? FileManager.default.removeItem(at: root) }
            _ = try store.check(Self.solid(100), name: "page", suffix: "light")
            _ = try store.check(
                Self.changing(Self.solid(100), by: 9), name: "page", suffix: "light")
            let actual = store.failureURL(name: "page", suffix: "light", kind: "actual")
            let other = store.writeActual(Self.solid(1), name: "other", suffix: "light")
            #expect(FileManager.default.fileExists(atPath: actual.path()))
            store.clearArtifacts(name: "page")
            #expect(FileManager.default.fileExists(atPath: actual.path()) == false)
            #expect(FileManager.default.fileExists(atPath: other))
        }

        @Test func namesAreLowercaseAsciiLettersDigitsAndDashes() {
            #expect(SnapshotStore.isValid(name: "tokens-page-2"))
            for bad in ["", "Tokens", "a_b", "a b", "a.b", "é"] {
                #expect(SnapshotStore.isValid(name: bad) == false, "accepted “\(bad)”")
            }
        }

        @Test func theStoreFindsItsFoldersFromTheSuiteFileAndTheEnvironment() {
            let plain = SnapshotStore.forSuite(
                at: "/r/Tests/SnapshotTests/GallerySnapshotTests.swift", environment: [:])
            #expect(
                plain.referenceDirectory.path()
                    == "/r/Tests/SnapshotTests/__Snapshots__/GallerySnapshotTests")
            #expect(
                plain.failureDirectory.path().hasSuffix(
                    "target/snapshot-failures/GallerySnapshotTests"))
            #expect(plain.record == false && plain.evidence == false)
            let record = SnapshotStore.forSuite(
                at: "/r/a.swift", environment: ["STACKLY_SNAPSHOT_RECORD": "1"])
            #expect(record.record)
            let evidence = SnapshotStore.forSuite(
                at: "/r/a.swift",
                environment: ["STACKLY_SNAPSHOT_RECORD": "1", "STACKLY_EVIDENCE_DIR": "/e"])
            #expect(evidence.evidence && evidence.record == false)
        }

        // MARK: - Preflight

        @Test func theSystemFactsDecideWhetherASnapshotIsBlocked() {
            var facts = SnapshotPreflight.Facts(
                hasGUISession: true, increaseContrast: false, reduceTransparency: false,
                differentiateWithoutColor: false, reduceMotion: false)
            #expect(SnapshotPreflight.blockedReason(facts) == nil)
            facts.reduceMotion = true
            #expect(
                SnapshotPreflight.blockedReason(facts)
                    == "system accessibility setting on: Reduce Motion")
            facts.increaseContrast = true
            #expect(
                SnapshotPreflight.blockedReason(facts)
                    == "system accessibility setting on: Increase Contrast")
            facts.hasGUISession = false
            #expect(SnapshotPreflight.blockedReason(facts) == "no GUI login session")
        }

        // MARK: - Rendering

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected:
                    "A fitting height of zero or above 4000 pt fails the render with a message"
            ))
        func aFittingHeightOfZeroOrAbove4000Fails() async throws {
            for height: CGFloat in [0, 4500] {
                do {
                    _ = try await SnapshotRender.render(
                        Color.clear.frame(width: 100, height: height), width: 100, height: .fitting,
                        scheme: .light)
                    Issue.record("a fitting height of \(height) must fail")
                } catch let error as SnapshotRenderError {
                    #expect("\(error)".contains("outside 1 to 4000 pt"))
                }
            }
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected: "A spinner with no loading delay renders identically twice"
            ))
        func aSpinnerStateRendersIdenticallyTwice() async throws {
            let view = StateView<Int, Text>(
                state: .loading, retry: {}, openDiagnostics: {}, content: { Text("\($0)") })
            let first = try await render(view, variant: .light, size: .page)
            let second = try await render(view, variant: .light, size: .page)
            #expect(first == second)
            let blank = try await render(Color.clear, variant: .light, size: .page)
            #expect(first != blank, "the spinner must be drawn with no loading delay")
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected:
                    "The fixed environment reaches a view: UTC, en_US, fixtureNow, no loading delay, pinned settings"
            ))
        func theRenderEnvironmentIsFixed() async throws {
            final class Seen {
                var locale = ""
                var zone = ""
                var now: Date?
                var delay: Duration?
                var scheme: ColorScheme = .light
                var contrastIncreased = false
                var differentiate = true
                var reduceMotion = true
            }
            struct Reader: View {
                let seen: Seen
                @Environment(\.locale) var locale
                @Environment(\.timeZone) var zone
                @Environment(\.fixtureNow) var now
                @Environment(\.loadingDelay) var delay
                @Environment(\.colorScheme) var scheme
                @Environment(\.colorSchemeContrast) var contrast
                @Environment(\.accessibilityDifferentiateWithoutColor) var differentiate
                @Environment(\.accessibilityReduceMotion) var reduceMotion
                var body: some View {
                    seen.locale = locale.identifier
                    seen.zone = zone.identifier
                    seen.now = now
                    seen.delay = delay
                    seen.scheme = scheme
                    seen.contrastIncreased = contrast == .increased
                    seen.differentiate = differentiate
                    seen.reduceMotion = reduceMotion
                    return Color.clear
                }
            }
            let seen = Seen()
            _ = try await render(Reader(seen: seen), variant: .hcDark, size: .page)
            #expect(seen.locale == "en_US")
            #expect(seen.zone == "UTC" || seen.zone == "GMT")
            #expect(seen.now == FixtureClock.now)
            #expect(seen.delay == .zero)
            #expect(seen.scheme == .dark)
            #expect(seen.contrastIncreased)
            #expect(seen.differentiate == false)
            #expect(seen.reduceMotion == false)
        }

        @Test func theCanvasSizesFollowTheSpec() {
            #expect(SnapshotSize.page.width(for: .light) == 1044)
            #expect(SnapshotSize.page.width(for: .narrow) == 640)
            #expect(SnapshotSize.page.height == .fixed(748))
            let fitting = SnapshotSize.fitting(width: 900, narrowWidth: 500)
            #expect(fitting.width(for: .dark) == 900 && fitting.width(for: .narrow) == 500)
            #expect(fitting.height == .fitting)
            #expect(
                SnapshotVariant.standard.map(\.suffix) == [
                    "light", "dark", "hc-light", "hc-dark", "narrow", "rt",
                ])
        }

        // MARK: - Increase Contrast must change the image (K15, G5)

        @Test func aHighContrastImageEqualToItsStandardOneIsAnIssueUnlessFlagged() {
            let image = Self.solid(100)
            let same: [SnapshotVariant: SnapshotImage] = [
                .light: image, .dark: image, .hcLight: image, .hcDark: image,
            ]
            #expect(
                SnapshotContrast.issues(name: "page", images: same, invariant: false) == [
                    "page (hc-light): identical to light; the view does not react to Increase "
                        + "Contrast. If that is intended, pass contrastInvariant: true with "
                        + "contrastInvariantReason",
                    "page (hc-dark): identical to dark; the view does not react to Increase "
                        + "Contrast. If that is intended, pass contrastInvariant: true with "
                        + "contrastInvariantReason",
                ])
            #expect(SnapshotContrast.issues(name: "page", images: same, invariant: true).isEmpty)
        }

        @Test func aFlagOnAViewThatReactsToIncreaseContrastIsAnIssue() {
            let base = Self.solid(100)
            let other = Self.changing(base, by: 9)
            let reacts: [SnapshotVariant: SnapshotImage] = [
                .light: base, .dark: base, .hcLight: other, .hcDark: base,
            ]
            #expect(
                SnapshotContrast.issues(name: "page", images: reacts, invariant: false) == [
                    "page (hc-dark): identical to dark; the view does not react to Increase "
                        + "Contrast. If that is intended, pass contrastInvariant: true with "
                        + "contrastInvariantReason"
                ])
            #expect(
                SnapshotContrast.issues(name: "page", images: reacts, invariant: true) == [
                    "page: contrastInvariant is set but hc-light differs from light; remove it"
                ])
            let both: [SnapshotVariant: SnapshotImage] = [
                .light: base, .dark: base, .hcLight: other, .hcDark: other,
            ]
            #expect(SnapshotContrast.issues(name: "page", images: both, invariant: false).isEmpty)
            #expect(
                SnapshotContrast.issues(name: "page", images: both, invariant: true).count == 2)
        }

        @Test func aPairWithAVariantThatWasNotRenderedIsNotChecked() {
            let image = Self.solid(100)
            let onlyLight: [SnapshotVariant: SnapshotImage] = [.light: image, .hcLight: image]
            #expect(
                SnapshotContrast.issues(name: "page", images: onlyLight, invariant: false).count
                    == 1)
            let noPair: [SnapshotVariant: SnapshotImage] = [
                .light: image, .dark: image, .narrow: image,
            ]
            #expect(SnapshotContrast.issues(name: "page", images: noPair, invariant: false).isEmpty)
            #expect(SnapshotContrast.issues(name: "page", images: [:], invariant: true).isEmpty)
        }

        @Test func aFlagNeedsAReasonThatIsNotBlank() {
            for reason in [nil, "", "   \n"] {
                #expect(
                    SnapshotContrast.reasonIssue(name: "page", invariant: true, reason: reason)
                        == "page: contrastInvariant needs a reason")
            }
            #expect(
                SnapshotContrast.reasonIssue(name: "page", invariant: true, reason: "no borders")
                    == nil)
            #expect(
                SnapshotContrast.reasonIssue(name: "page", invariant: false, reason: nil) == nil)
        }

        /// The location of a throwaway suite file in `root`, so its references stay out of the repo.
        private static func location(in root: URL, suite: String) -> SourceLocation {
            SourceLocation(
                fileID: "StacklyUITests/\(suite).swift",
                filePath: root.appending(path: "\(suite).swift").path(percentEncoded: false),
                line: 1, column: 1)
        }

        /// Collects the messages of the known issues a matcher sees.
        nonisolated private final class IssueLog: @unchecked Sendable {
            private let lock = NSLock()
            private var storage: [String] = []

            var messages: [String] { lock.withLock { storage } }

            func matcher(_ issue: Issue) -> Bool {
                lock.withLock { storage.append(issue.comments.map(\.rawValue).joined()) }
                return true
            }
        }

        private static let contrastVariants: [SnapshotVariant] = [
            .light, .dark, .hcLight, .hcDark,
        ]

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected:
                    "assertSnapshot reports a view that ignores Increase Contrast and accepts it with contrastInvariant and a reason"
            ),
            .disabled(
                if: ProcessInfo.processInfo.environment["STACKLY_EVIDENCE_DIR"] != nil,
                "an evidence run never records references")
        )
        func assertSnapshotReportsAViewThatIgnoresIncreaseContrast() async throws {
            let (_, root) = try Self.temporaryStore()
            defer { try? FileManager.default.removeItem(at: root) }
            let location = Self.location(in: root, suite: "ContrastProbe")
            let size = SnapshotSize.fitting(width: 320, narrowWidth: 200)
            defer {
                try? FileManager.default.removeItem(
                    at: SourceTree.repositoryRoot.appending(
                        path: "target/snapshot-failures/ContrastProbe"))
            }

            // Record the references first, with the flag, so only the check under test speaks.
            await withKnownIssue {
                try await assertSnapshot(
                    PlainLabel(), named: "plain", size: size, variants: Self.contrastVariants,
                    contrastInvariant: true, contrastInvariantReason: "plain text, no borders",
                    sourceLocation: location)
            }
            // With the flag and a reason, the same view passes.
            try await assertSnapshot(
                PlainLabel(), named: "plain", size: size, variants: Self.contrastVariants,
                contrastInvariant: true, contrastInvariantReason: "plain text, no borders",
                sourceLocation: location)

            // Without it, both pairs are issues.
            let log = IssueLog()
            try await withKnownIssue {
                try await assertSnapshot(
                    PlainLabel(), named: "plain", size: size, variants: Self.contrastVariants,
                    sourceLocation: location)
            } matching: {
                log.matcher($0)
            }
            let messages = log.messages
            #expect(messages.count == 2)
            #expect(messages.contains { $0.hasPrefix("plain (hc-light): identical to light;") })
            #expect(messages.contains { $0.hasPrefix("plain (hc-dark): identical to dark;") })
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected:
                    "assertSnapshot rejects contrastInvariant without a reason before it renders, and a flag on a view that reacts"
            ),
            .disabled(
                if: ProcessInfo.processInfo.environment["STACKLY_EVIDENCE_DIR"] != nil,
                "an evidence run never records references")
        )
        func assertSnapshotRejectsAFlagWithoutAReasonOrOnAViewThatReacts() async throws {
            let (_, root) = try Self.temporaryStore()
            defer { try? FileManager.default.removeItem(at: root) }
            let location = Self.location(in: root, suite: "ContrastFlagProbe")
            let size = SnapshotSize.fitting(width: 320, narrowWidth: 200)
            defer {
                try? FileManager.default.removeItem(
                    at: SourceTree.repositoryRoot.appending(
                        path: "target/snapshot-failures/ContrastFlagProbe"))
            }

            let missingReason = IssueLog()
            try await withKnownIssue {
                try await assertSnapshot(
                    PlainLabel(), named: "plain", size: size, variants: Self.contrastVariants,
                    contrastInvariant: true, sourceLocation: location)
            } matching: {
                missingReason.matcher($0)
            }
            #expect(missingReason.messages == ["plain: contrastInvariant needs a reason"])
            let references = root.appending(path: "__Snapshots__/ContrastFlagProbe")
            #expect(
                FileManager.default.fileExists(atPath: references.path()) == false,
                "a flag without a reason must not render")

            // A flag on a view that reacts to Increase Contrast is an issue once recorded.
            await withKnownIssue {
                try await assertSnapshot(
                    Label(text: "one"), named: "label", size: size,
                    variants: Self.contrastVariants, sourceLocation: location)
            }
            let staleFlag = IssueLog()
            try await withKnownIssue {
                try await assertSnapshot(
                    Label(text: "one"), named: "label", size: size,
                    variants: Self.contrastVariants,
                    contrastInvariant: true, contrastInvariantReason: "stale flag",
                    sourceLocation: location)
            } matching: {
                staleFlag.matcher($0)
            }
            #expect(
                staleFlag.messages == [
                    "label: contrastInvariant is set but hc-light differs from light; remove it",
                    "label: contrastInvariant is set but hc-dark differs from dark; remove it",
                ])
        }

        // MARK: - The whole pipeline

        /// A view whose single text can change, for the end to end check.
        struct Label: View {
            let text: String
            @Environment(\.colorSchemeContrast) private var contrast
            var body: some View {
                Text(text).font(.largeTitle).padding(24)
                    .foregroundStyle(contrast == .increased ? Color.red : Color.primary)
            }
        }

        /// A view with nothing that reacts to Increase Contrast: a fixed color, no text, whose
        /// system colors would change.
        struct PlainLabel: View {
            var body: some View {
                Color(red: 0.2, green: 0.4, blue: 0.8).frame(width: 120, height: 60).padding(24)
            }
        }

        @Test(
            .evidence(
                covers: ["REQ-027"],
                expected:
                    "assertSnapshot records, then matches, then fails with artifacts, then clears them"
            ),
            .disabled(
                if: ProcessInfo.processInfo.environment["STACKLY_EVIDENCE_DIR"] != nil,
                "an evidence run never records references")
        )
        func assertSnapshotRecordsMatchesFailsAndClearsItsArtifacts() async throws {
            let (_, root) = try Self.temporaryStore()
            defer { try? FileManager.default.removeItem(at: root) }
            let suite = root.appending(path: "PipelineProbe.swift").path(percentEncoded: false)
            let location = SourceLocation(
                fileID: "StacklyUITests/PipelineProbe.swift", filePath: suite, line: 1, column: 1)
            let failures = SourceTree.repositoryRoot.appending(
                path: "target/snapshot-failures/PipelineProbe")
            defer { try? FileManager.default.removeItem(at: failures) }
            let size = SnapshotSize.fitting(width: 320, narrowWidth: 200)

            // 1. Nothing recorded yet: the five own references are written and each fails.
            await withKnownIssue {
                try await assertSnapshot(
                    Label(text: "one"), named: "label", size: size, sourceLocation: location)
            }
            let references = root.appending(path: "__Snapshots__/PipelineProbe")
            let written = try FileManager.default.contentsOfDirectory(atPath: references.path())
            #expect(
                written.sorted() == [
                    "label.dark.png", "label.hc-dark.png", "label.hc-light.png", "label.light.png",
                    "label.narrow.png",
                ])

            // 2. The same view matches all five, and Reduce Transparency equals light.
            try await assertSnapshot(
                Label(text: "one"), named: "label", size: size, sourceLocation: location)

            // 3. A changed view fails with artifacts.
            await withKnownIssue {
                try await assertSnapshot(
                    Label(text: "two"), named: "label", size: size, sourceLocation: location)
            }
            let artifacts = try FileManager.default.contentsOfDirectory(atPath: failures.path())
            #expect(artifacts.contains("label.light.actual.png"))
            #expect(artifacts.contains("label.light.diff.png"))

            // 4. The next call for the name starts by deleting them.
            try await assertSnapshot(
                Label(text: "one"), named: "label", size: size, sourceLocation: location)
            let after =
                (try? FileManager.default.contentsOfDirectory(atPath: failures.path())) ?? []
            #expect(after.isEmpty)
        }
    }
}
