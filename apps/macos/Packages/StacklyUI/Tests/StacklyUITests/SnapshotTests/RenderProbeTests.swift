import AppKit
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// Probes that the render approach of spec 0068 works on this Mac: `cacheDisplay` of an
/// `NSHostingView` in a window that never shows, at scale 1 in sRGB. If one of these
/// fails on a Mac, stop and go back to `/architect` (step 1 of the build plan).
@Suite(.serialized)
struct RenderProbeTests {
    private static let size = CGSize(width: 480, height: 320)

    /// A view with the controls the probe of the spec lists: `List`, `Table`, `TextField`,
    /// and a `ProgressView`.
    private struct Controls: View {
        struct Row: Identifiable {
            let id: Int
            let name: String
        }
        @State private var text = "Instance name"
        private let rows = (1...3).map { Row(id: $0, name: "row \($0)") }

        var body: some View {
            VStack(alignment: .leading, spacing: 8) {
                TextField("Name", text: $text)
                HStack {
                    List(rows) { Text($0.name) }
                    Table(rows) { TableColumn("Name") { Text($0.name) } }
                }
                ProgressView()
                Button("Create") {}.buttonStyle(.borderedProminent)
            }
            .padding(12)
        }
    }

    private static func render<V: View>(
        _ view: V,
        _ appearance: FixtureAppearance = FixtureAppearance()
    ) async throws -> SnapshotImage {
        try await SnapshotRender.render(
            view.fixtureAppearance(appearance),
            width: size.width,
            height: .fixed(size.height),
            scheme: appearance.scheme
        )
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected:
                "A view with List, Table, TextField, and ProgressView renders into a bitmap at scale 1"
        )
    )
    func controlsRenderAtScaleOneInsideAWindowThatNeverShows() async throws {
        let image = try await Self.render(Controls())
        #expect(image.width == Int(Self.size.width))
        #expect(image.height == Int(Self.size.height))
        // A blank capture would be one color; the controls draw many.
        let distinct = Set(
            stride(from: 0, to: image.pixels.count, by: 4).map {
                [image.pixels[$0], image.pixels[$0 + 1], image.pixels[$0 + 2]]
            })
        #expect(distinct.count > 8)
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "Two renders of one view are identical pixel for pixel, spinner included"
        )
    )
    func twoRendersOfOneViewAreIdentical() async throws {
        let first = try await Self.render(Controls())
        let second = try await Self.render(Controls())
        #expect(first == second)
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "A spinner view settles; task and onAppear run in a window that never shows"
        )
    )
    func taskAndOnAppearRunInAWindowThatNeverShows() async throws {
        final class Flags {
            var appeared = false
            var tasked = false
        }
        let flags = Flags()
        struct Probe: View {
            let flags: Flags
            var body: some View {
                ProgressView()
                    .onAppear { flags.appeared = true }
                    .task { flags.tasked = true }
            }
        }
        _ = try await Self.render(Probe(flags: flags))
        #expect(flags.appeared)
        #expect(flags.tasked)
    }

    @Observable final class TitleModel {
        var title = "before"
    }

    private struct TitledText: View {
        let model: TitleModel
        var body: some View { Text(model.title).font(.largeTitle).padding() }
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "A change to an @Observable model after the view mounted shows in the capture"
        )
    )
    func anObservableChangeAfterMountShowsInTheCapture() async throws {
        let model = TitleModel()
        let before = try await Self.render(TitledText(model: model))
        model.title = "after"
        let after = try await Self.render(TitledText(model: model))
        #expect(before != after)
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected:
                "A view that keeps changing fails with 'snapshot did not settle' and keeps the last capture"
        )
    )
    func aViewThatKeepsChangingDoesNotSettle() async throws {
        struct Ticking: View {
            @State private var tick = 0
            var body: some View {
                Text("tick \(tick)")
                    .task {
                        while Task.isCancelled == false {
                            try? await Task.sleep(for: .milliseconds(20))
                            tick += 1
                        }
                    }
            }
        }
        do {
            _ = try await Self.render(Ticking())
            Issue.record("expected the render not to settle")
        } catch let error as SnapshotError {
            guard case .didNotSettle(let last) = error else {
                Issue.record("unexpected \(error)")
                return
            }
            #expect(last.width == Int(Self.size.width))
            #expect("\(error)" == "snapshot did not settle")
        }
    }

    // MARK: Guard for the underscored setters

    /// Reads the settings the guard cares about and draws them as text, so a setter that
    /// stopped working changes the capture.
    private struct SettingsReader: View {
        @Environment(\.colorSchemeContrast) private var contrast
        @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

        var body: some View {
            VStack(alignment: .leading) {
                Text("contrast \(contrast == .increased ? "increased" : "standard")")
                Text("reduce transparency \(reduceTransparency ? "on" : "off")")
            }
            .padding()
        }
    }

    /// Records what the environment says, with no drawing involved.
    private struct EnvironmentSpy: View {
        final class Seen {
            var increased = false
            var reduceTransparency = false
        }
        let seen: Seen
        @Environment(\.colorSchemeContrast) private var contrast
        @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

        var body: some View {
            seen.increased = contrast == .increased
            seen.reduceTransparency = reduceTransparency
            return Color.clear
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected:
                "The underscored environment setters reach the views of this SDK, so snapshots can pin Increase Contrast and Reduce Transparency"
        )
    )
    func theUnderscoredSettersStillReachTheViews() async throws {
        let sdk = ProcessInfo.processInfo.operatingSystemVersionString
        let highContrast = EnvironmentSpy.Seen()
        _ = try await Self.render(
            EnvironmentSpy(seen: highContrast), FixtureAppearance(increasedContrast: true))
        #expect(
            highContrast.increased,
            "a view does not read colorSchemeContrast == .increased on \(sdk); _colorSchemeContrast stopped working, go back to /architect"
        )
        let transparent = EnvironmentSpy.Seen()
        _ = try await Self.render(
            EnvironmentSpy(seen: transparent), FixtureAppearance(reduceTransparency: true))
        #expect(
            transparent.reduceTransparency,
            "a view does not read accessibilityReduceTransparency == true on \(sdk); _accessibilityReduceTransparency stopped working, go back to /architect"
        )
        let standard = EnvironmentSpy.Seen()
        _ = try await Self.render(EnvironmentSpy(seen: standard))
        #expect(standard.increased == false)
        #expect(standard.reduceTransparency == false)
    }
}
