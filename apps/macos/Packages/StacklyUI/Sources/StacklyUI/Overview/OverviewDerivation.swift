import Foundation
import StacklyClient

/// The numbers and rows the Overview derives from the instances (spec 0022 AC-14: "model
/// turunan halaman dihitung ulang hanya saat input nya berubah").
/// The page body runs again for any store it reads, such as the operation center. This
/// keeps the last result and the input it came from, so an event that does not change an
/// instance, the catalog, or the failed refetches gives the same value without grouping
/// and sorting again.
final class OverviewDerivation {
    /// What the value depends on.
    private struct Input: Equatable {
        let summaries: [InstanceSummary]
        let providers: [ProviderSummary]?
        let failures: Set<UUID>
    }

    /// What the page shows of the instances.
    struct Value {
        /// The count of each group.
        let counts: InstanceCounts
        /// The rows of the Instances section, in the order of the page.
        let rows: [InstanceRowModel]
    }

    /// How many times this derivation computed a value: the counter the performance test
    /// reads, that must not rise for an event that changes no instance.
    @MainActor private(set) var recomputes = 0

    private var last: (input: Input, value: Value)?

    /// The value for the input: the one held when the input is the same, else a new one.
    /// - Parameters:
    ///   - summaries: The instances.
    ///   - providers: The catalog, or `nil` before it loaded.
    ///   - failures: The IDs whose last refetch failed.
    @MainActor
    func value(
        summaries: [InstanceSummary], providers: [ProviderSummary]?, failures: Set<UUID>
    ) -> Value {
        let input = Input(summaries: summaries, providers: providers, failures: failures)
        if let last, last.input == input { return last.value }
        recomputes += 1
        let value = Value(
            counts: InstanceCounts(of: summaries),
            rows: InstanceRowModel.rows(of: summaries, catalog: providers, failures: failures))
        last = (input, value)
        return value
    }
}
