import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// Placeholders, banners, and the mutation gate (spec 0012, State patterns; AC-10,
/// AC-11, AC-13, AC-14, AC-16, AC-20).
struct StateViewPartsTests {
    static let blocked = MutationAvailability.blocked(
        reason: "stacklyd is unavailable",
        since: Date(timeIntervalSince1970: 1_792_245_720)
    )

    // MARK: - Empty and placeholder content

    @Test func noResultsQuotesTheQueryAndOffersClearFilters() throws {
        var cleared = false
        let empty = EmptyContent.noResults(
            query: "mysql",
            message: "Check the spelling or try another status."
        ) { cleared = true }

        #expect(empty.kind == .noResults(query: "mysql"))
        #expect(empty.symbol == "magnifyingglass")
        #expect(empty.title == "No Results for “mysql”")
        #expect(empty.message == "Check the spelling or try another status.")
        #expect(empty.secondaryAction == nil)
        let clear = try #require(empty.primaryAction)
        #expect(clear.title == "Clear Filters")
        #expect(clear.mutates == false)
        clear.perform()
        #expect(cleared)
    }

    @Test func noDataKeepsTheScreensCopyAndActions() {
        let empty = EmptyContent.noData(
            symbol: "server.rack",
            title: "No Instances",
            message: "Each instance gets its own data directory, port and secret.",
            primaryAction: StateAction(title: "New Instance…", mutates: true) {},
            secondaryAction: StateAction(title: "Browse Catalog", mutates: false) {}
        )
        let content = PlaceholderContent(empty: empty)

        #expect(empty.kind == .noData)
        #expect(content.symbol == "server.rack")
        #expect(content.symbolStyle == .decorative)
        #expect(content.title == "No Instances")
        #expect(content.code == nil)
        #expect(content.primaryAction?.title == "New Instance…")
        #expect(content.secondaryAction?.title == "Browse Catalog")
    }

    @Test func failedPlaceholderShowsTheErrorWithRetryAndOpenDiagnostics() throws {
        var retried = false
        var opened = false
        let failure = FailureContent(
            title: "Couldn’t Load Instances",
            message: "the registry is busy",
            code: "E_REGISTRY_BUSY"
        )
        let content = PlaceholderContent(
            failure: failure,
            retry: { retried = true },
            openDiagnostics: { opened = true }
        )

        #expect(content.symbol == Tone.critical.symbol)
        #expect(content.symbolStyle == .critical)
        #expect(content.title == "Couldn’t Load Instances")
        #expect(content.message == "the registry is busy")
        #expect(content.code == "E_REGISTRY_BUSY")
        let retry = try #require(content.primaryAction)
        let diagnostics = try #require(content.secondaryAction)
        #expect(retry.title == "Retry")
        #expect(diagnostics.title == "Open Diagnostics")
        retry.perform()
        diagnostics.perform()
        #expect(retried)
        #expect(opened)
    }

    @Test func retryAndOpenDiagnosticsStayEnabledWhileMutationsAreBlocked() throws {
        let content = PlaceholderContent(
            failure: FailureContent(title: "Couldn’t Load Instances", message: "timed out"),
            retry: {},
            openDiagnostics: {}
        )
        let retry = try #require(content.primaryAction)
        let diagnostics = try #require(content.secondaryAction)

        #expect(retry.mutates == false)
        #expect(diagnostics.mutates == false)
        #expect(retry.disabledReason(in: Self.blocked) == nil)
        #expect(diagnostics.disabledReason(in: Self.blocked) == nil)
    }

    @Test func unsupportedPlaceholderShowsTheReasonWithoutActions() {
        let content = PlaceholderContent(
            unsupported: UnsupportedContent(
                title: "Backups Aren’t Supported",
                reason: "The Valkey provider doesn’t declare a backup capability."
            )
        )

        #expect(content.symbol == "nosign")
        #expect(content.symbolStyle == .decorative)
        #expect(content.title == "Backups Aren’t Supported")
        #expect(content.message == "The Valkey provider doesn’t declare a backup capability.")
        #expect(content.primaryAction == nil)
        #expect(content.secondaryAction == nil)
    }

    @Test func placeholderSizesMatchTheSpec() {
        #expect(StatePlaceholder.Size.page.titleFont == .title2.weight(.semibold))
        #expect(StatePlaceholder.Size.page.symbolSize == .placeholderPageSymbol)
        #expect(StatePlaceholder.Size.page.verticalPadding == 80)
        #expect(StatePlaceholder.Size.section.titleFont == .title3.weight(.semibold))
        #expect(StatePlaceholder.Size.section.symbolSize == .placeholderSectionSymbol)
        #expect(StatePlaceholder.Size.section.verticalPadding == 34)
    }

    // MARK: - Banner

    @Test(arguments: [0, 1, 5])
    func bannerShowsUpToFiveItemsWithoutShowAll(count: Int) {
        let split = StateBanner.split(Self.items(count), showsAll: false)
        #expect(split.shown.count == count)
        #expect(split.hiddenCount == 0)
    }

    @Test func bannerHidesItemsAfterTheFifthUntilShowAll() {
        let items = Self.items(7)

        let collapsed = StateBanner.split(items, showsAll: false)
        let expanded = StateBanner.split(items, showsAll: true)

        #expect(collapsed.shown.map(\.name) == items.prefix(5).map(\.name))
        #expect(collapsed.hiddenCount == 2)
        #expect(expanded.shown.map(\.name) == items.map(\.name))
        #expect(expanded.hiddenCount == 0)
    }

    static func items(_ count: Int) -> [PartialItem] {
        (0..<count).map { PartialItem(name: "service-\($0)", reason: "starting") }
    }

    // MARK: - Mutation gate

    @Test func mutationsAreAvailableUnlessTheShellBlocksThem() {
        #expect(EnvironmentValues().mutationAvailability == .available)
        #expect(MutationAvailability.available.blockedReason == nil)
        #expect(Self.blocked.blockedReason == "stacklyd is unavailable")
    }

    @Test func onlyMutatingActionsAreGated() {
        let mutating = StateAction(title: "New Instance…", mutates: true) {}
        let reading = StateAction(title: "Browse Catalog", mutates: false) {}

        #expect(mutating.disabledReason(in: Self.blocked) == "stacklyd is unavailable")
        #expect(mutating.disabledReason(in: .available) == nil)
        #expect(reading.disabledReason(in: Self.blocked) == nil)
        #expect(mutating.isBusy == false)
    }

    // MARK: - Symbols and motion

    @Test(arguments: ["magnifyingglass", "nosign", "info.circle"])
    func stateSymbolExistsOnThisMacOS(name: String) {
        #expect(NSImage(systemSymbolName: name, accessibilityDescription: nil) != nil)
    }

    /// AC-16: state views only crossfade; nothing moves or scales.
    @Test func stateViewsNeverAnimateMovementOrScale() throws {
        let pattern = try Regex(#"\.(move|slide|scale|offset)\b|\.(scaleEffect|offset)\("#)
            .wordBoundaryKind(.simple)
        let files = try SourceTree.swiftFiles().filter {
            $0.path.hasPrefix("apps/macos/Packages/StacklyUI/Sources/StacklyUI/State/")
        }
        #expect(files.isEmpty == false)
        for file in files {
            for (index, line) in file.codeLines.enumerated() where line.contains(pattern) {
                Issue.record("\(file.path):\(index + 1) animates movement or scale: \(line)")
            }
        }
    }
}
