import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// `StatusLabel`, `ToneBadge`, `SectionStyle`, and `SectionIcon` (spec 0012 AC-5, AC-7,
/// AC-16, AC-19).
struct StatusViewTests {
    // MARK: - StatusLabel colors

    @Test(arguments: Tone.allCases)
    func standardLabelUsesCriticalTextOrTextSecondary(tone: Tone) {
        let expected: StatusInk = tone == .critical ? .text(.critical) : .text(.neutral)
        #expect(StatusContext.standard.labelInk(for: tone) == expected)
        #expect(Tone.neutral.text == StacklyColor.textSecondary)
    }

    @Test(arguments: Tone.allCases)
    func standardDetailUsesItsOwnToneText(tone: Tone) {
        #expect(StatusContext.standard.detailInk(for: tone) == .text(tone))
    }

    @Test(arguments: Tone.allCases)
    func rowLabelAndDetailUsePrimary(tone: Tone) {
        #expect(StatusContext.row.labelInk(for: tone) == .primary)
        #expect(StatusContext.row.detailInk(for: tone) == .primary)
    }

    @Test(arguments: StatusContext.allCases, Tone.allCases)
    func indicatorUsesToneFillOutsideASelectedRow(context: StatusContext, tone: Tone) {
        #expect(context.indicatorInk(for: tone, prominence: .standard) == .fill(tone))
    }

    @Test(arguments: Tone.allCases)
    func indicatorUsesPrimaryInASelectedRow(tone: Tone) {
        #expect(StatusContext.row.indicatorInk(for: tone, prominence: .increased) == .primary)
        #expect(
            StatusContext.standard.indicatorInk(for: tone, prominence: .increased) == .fill(tone))
    }

    // MARK: - StatusLabel content

    @Test func labelIsNeverCutAndTheDetailTruncatesFirst() {
        let withDetail = StatusAppearance.instance(
            lifecycle: .running,
            readiness: .notReady(detail: "Probe failed: connection refused after 3 attempts"),
            health: .unknown
        )
        let labelOnly = StatusAppearance(
            label: withDetail.label,
            tone: withDetail.tone,
            symbol: withDetail.symbol
        )
        let labelWidth = fittingWidth(of: StatusLabel(labelOnly), proposed: nil)
        let idealWidth = fittingWidth(of: StatusLabel(withDetail), proposed: nil)
        #expect(idealWidth > labelWidth + 100)

        let squeezedWidth = fittingWidth(of: StatusLabel(withDetail), proposed: 10)
        #expect(squeezedWidth >= labelWidth, "the label was cut at a narrow width")

        let partialWidth = (labelWidth + idealWidth) / 2
        let truncatedWidth = fittingWidth(of: StatusLabel(withDetail), proposed: partialWidth)
        #expect(truncatedWidth <= partialWidth + 0.5, "the detail did not truncate")
        #expect(truncatedWidth >= labelWidth)
    }

    @Test func statusChangeCrossfades() throws {
        let file = try statusSource(named: "StatusLabel.swift")
        #expect(file.codeLines.contains { $0.contains(".transition(StacklyMotion.crossfade)") })
    }

    @Test func statusViewsUseNoMoveSlideScaleOrOffset() throws {
        let patterns = [#"\.move\b"#, #"\.slide\b"#, #"\.scale\b"#, #"\.offset\b"#, "scaleEffect"]
        let regexes = try patterns.map { try Regex($0).wordBoundaryKind(.simple) }
        let files = try SourceTree.files(under: Self.statusFolder).filter { $0.hasSuffix(".swift") }
        #expect(files.isEmpty == false)
        for path in files {
            let text = try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
            for (index, line) in SourceFile(path: path, text: text).codeLines.enumerated() {
                for regex in regexes where line.contains(regex) {
                    Issue.record("\(path):\(index + 1) animates movement or scale: \(line)")
                }
            }
        }
    }

    // MARK: - ToneBadge

    @Test func toneBadgePadsNinePointsHorizontally() {
        #expect(ToneBadge.horizontalPadding == 9)
    }

    @Test func toneBadgeIsAtLeastItsScaledMinimumHeight() {
        let appearance = StatusAppearance.operation(state: .running, partial: false)
        let size = fittingSize(of: ToneBadge(appearance), proposed: nil)
        #expect(size.height >= ScaledSize.badgeHeight.base)
        #expect(ScaledSize.badgeHeight.textStyle == .subheadline)
    }

    // MARK: - Sections

    /// One row of the section icon table.
    nonisolated struct SectionRow: CustomTestStringConvertible, Sendable {
        let section: SectionStyle
        let symbol: String
        let color: NSColor

        var testDescription: String { "\(section)" }
    }

    nonisolated static let sectionRows = [
        SectionRow(section: .overview, symbol: "square.grid.2x2", color: .systemBlue),
        SectionRow(section: .instances, symbol: "server.rack", color: .systemGreen),
        SectionRow(section: .environments, symbol: "square.3.layers.3d", color: .systemPurple),
        SectionRow(section: .catalog, symbol: "book", color: .systemOrange),
        SectionRow(section: .runtimes, symbol: "shippingbox", color: .systemTeal),
        SectionRow(
            section: .operations,
            symbol: "arrow.triangle.2.circlepath",
            color: .systemIndigo
        ),
        SectionRow(section: .diagnostics, symbol: "waveform.path.ecg", color: .systemPink),
    ]

    @Test func everySectionHasOneRowInTheTable() {
        #expect(Self.sectionRows.map(\.section) == SectionStyle.allCases)
    }

    @Test(arguments: sectionRows)
    func sectionStyleMatchesItsTableRow(row: SectionRow) {
        #expect(row.section.symbol == row.symbol)
        #expect(row.section.systemColor == row.color)
    }

    @Test(arguments: sectionRows)
    func sectionSymbolExistsInSFSymbols(row: SectionRow) {

    }

    // MARK: - Helpers

    nonisolated static let statusFolder = "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Status"

    private func statusSource(named name: String) throws -> SourceFile {
        let path = Self.statusFolder + "/" + name
        let text = try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
        return SourceFile(path: path, text: text)
    }

    /// The size `view` takes for a proposed width, or its ideal size for `nil`.
    private func fittingSize(of view: some View, proposed width: CGFloat?) -> CGSize {
        let controller = NSHostingController(
            rootView: view.fixedSize(horizontal: false, vertical: true))
        return controller.sizeThatFits(
            in: CGSize(width: width ?? .greatestFiniteMagnitude, height: .greatestFiniteMagnitude)
        )
    }

    private func fittingWidth(of view: some View, proposed width: CGFloat?) -> CGFloat {
        fittingSize(of: view, proposed: width).width
    }
}
