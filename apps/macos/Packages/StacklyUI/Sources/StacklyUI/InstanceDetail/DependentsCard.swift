import StacklyClient
import SwiftUI

/// The Dependents card of the Summary tab (spec 0021, *Tab Summary*): the environments that
/// hold the instance, or "Nothing depends on this instance.".
struct DependentsCard: View {
    let detail: InstanceDetail?
    /// Whether the detail is still on its way, as opposed to failed.
    let isPending: Bool

    var body: some View {
        CardSection("Dependents") {
            if let detail {
                if detail.dependents.isEmpty {
                    Text(DetailFormat.noDependentsText)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
                ForEach(detail.dependents, id: \.environmentId) { dependent in
                    Text(DetailFormat.dependent(dependent))
                }
            } else if isPending {
                DetailPending()
            } else {
                Text("—").foregroundStyle(StacklyColor.textSecondary)
            }
        }

    }
}

/// A row that waits for the detail: a delayed spinner, so a fast load never flashes one.
struct DetailPending: View {
    @Environment(\.loadingDelay) private var loadingDelay

    var body: some View {
        DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
            .frame(width: 20, height: 20)

    }
}
