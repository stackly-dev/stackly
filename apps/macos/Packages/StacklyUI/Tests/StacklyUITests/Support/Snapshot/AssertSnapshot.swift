import AppKit
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// Renders `view` in every variant and checks each image against its reference PNG
/// (spec 0068, AC-4 to AC-6).
///
/// Each variant renders in a ``SnapshotWindow`` with the fixed environment: locale
/// `en_US`, time zone UTC, the Gregorian calendar, `fixtureNow`, no loading delay, the
/// window background, and the color scheme from `fixtureAppearance`.
///
/// - Parameters:
///   - view: The view to render.
///   - name: The snapshot name: lowercase ASCII letters, digits, and `-`.
///   - size: The canvas.
///   - variants: The variants to render.
///   - sourceLocation: The suite file, which decides where the references live.
/// - Throws: The cancellation of a test that is `BLOCKED`, or a render error other than
///   a failed check, which is recorded as an issue.
@MainActor
func assertSnapshot<V: View>(
    _ view: V,
    named name: String,
    size: SnapshotSize = .page,
    variants: [SnapshotVariant] = SnapshotVariant.standard,
    sourceLocation: SourceLocation = #_sourceLocation
) async throws {
    guard SnapshotStore.isValid(name: name) else {
        Issue.record(
            "snapshot name “\(name)” must be lowercase ASCII letters, digits, and -",
            sourceLocation: sourceLocation)
        return
    }
    if let reason = SnapshotPreflight.blockedReason(.current()) {
        try Evidence.blocked(reason: reason, owner: "user")
    }
    let store = SnapshotStore.forSuite(at: sourceLocation.filePath)
    store.clearArtifacts(name: name)
    for variant in variants {
        do {
            let image = try await render(view, variant: variant, size: size)
            let outcome: SnapshotStore.Outcome
            outcome = try store.check(image, name: name, suffix: variant.suffix)
            if case .failure(let message) = outcome {
                Issue.record(
                    Comment(rawValue: "\(name) (\(variant.suffix)): \(message)"),
                    sourceLocation: sourceLocation)
            }
        } catch SnapshotError.didNotSettle(let last) {
            let path = store.writeActual(last, name: name, suffix: variant.suffix)
            Issue.record(
                Comment(
                    rawValue:
                        "\(name) (\(variant.suffix)): snapshot did not settle; last capture at \(path)"
                ),
                sourceLocation: sourceLocation)
        } catch let error as SnapshotRenderError {
            Issue.record(
                Comment(rawValue: "\(name) (\(variant.suffix)): \(error)"),
                sourceLocation: sourceLocation)
        }
    }
}

/// Renders one variant of `view` with the fixed snapshot environment.
@MainActor
func render<V: View>(
    _ view: V, variant: SnapshotVariant, size: SnapshotSize
) async throws -> SnapshotImage {
    let width = size.width(for: variant)
    let root = SnapshotRoot(content: view, variant: variant, width: width, height: size.height)
    return try await SnapshotRender.render(
        root, width: width, height: size.height, scheme: variant.appearance.scheme)
}

/// The fixed environment every snapshot renders in.
struct SnapshotRoot<Content: View>: View {
    let content: Content
    let variant: SnapshotVariant
    let width: CGFloat
    let height: SnapshotHeight

    static var calendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = SnapshotRoot.timeZone
        calendar.locale = SnapshotRoot.locale
        return calendar
    }

    static var locale: Locale { Locale(identifier: "en_US") }
    static var timeZone: TimeZone { TimeZone(identifier: "UTC") ?? .gmt }

    var body: some View {
        framed
            .background(Color(nsColor: .windowBackgroundColor))
            .environment(\.locale, Self.locale)
            .environment(\.timeZone, Self.timeZone)
            .environment(\.calendar, Self.calendar)
            .environment(\.fixtureNow, FixtureClock.now)
            .environment(\.loadingDelay, .zero)
            .fixtureAppearance(variant.appearance)
    }

    @ViewBuilder
    private var framed: some View {
        switch height {
        case .fixed(let points):
            content.frame(width: width, height: points, alignment: .topLeading)
        case .fitting:
            content.frame(width: width, alignment: .topLeading)
        }
    }
}
