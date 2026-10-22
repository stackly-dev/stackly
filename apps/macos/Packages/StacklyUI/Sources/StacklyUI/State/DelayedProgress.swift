import SwiftUI

/// Whether a delayed spinner is visible: hidden at first, visible once the delay passed.
///
/// ``DelayedProgress`` runs it in its task, so the spinner never flashes when data arrives
/// within the delay. The sleep is injectable, so tests never wait for real time.
///
/// A zero delay is visible from the start and ``run()`` never sleeps: the spinner is part of
/// the first frame, with no task and no transition. A spinner inserted after the first frame
/// keeps changing in a snapshot window, so a snapshot of it would never settle (spec 0068).
@Observable
final class DelayedVisibility {
    /// The delay before a first load shows a spinner: 300 ms.
    static let defaultDelay: Duration = .milliseconds(300)

    /// Whether the spinner is visible: from the start when the delay is zero, else once the
    /// delay passed.
    private(set) var isVisible: Bool

    /// How long to wait before showing the spinner.
    let delay: Duration
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void

    /// Creates a visibility that is hidden, or visible when the delay is zero.
    ///
    /// - Parameters:
    ///   - delay: How long to wait before showing the spinner.
    ///   - sleep: Waits for a duration and throws when cancelled; `Task.sleep(for:)` by
    ///     default.
    init(
        delay: Duration = DelayedVisibility.defaultDelay,
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.delay = delay
        self.sleep = sleep
        isVisible = delay == .zero
    }

    /// Whether the spinner is part of the first frame, with no wait: the delay is zero.
    var showsInFirstFrame: Bool { delay == .zero }

    /// Hides the spinner, waits for the delay, then shows it. When the task is cancelled
    /// first (the data arrived), the spinner stays hidden. Each call starts the delay over.
    /// With a zero delay the spinner stays visible and nothing waits.
    func run() async {
        guard showsInFirstFrame == false else { return }
        isVisible = false
        do {
            try await sleep(delay)
        } catch {
            return
        }
        guard Task.isCancelled == false else { return }
        isVisible = true
    }
}

/// A centered `ProgressView` that appears only after 300 ms, so a fast load never
/// flashes a spinner. ``StateView`` shows it for the `loading` shape.
///
/// The delay starts when the view appears. ``StateView`` inserts a new one each time the
/// shape enters `loading`, so the delay starts over each time. The spinner keeps spinning
/// under Reduce Motion. With a zero delay (a snapshot window) the spinner is in the first
/// frame, with no task and no transition.
public struct DelayedProgress: View {
    @State private var visibility: DelayedVisibility

    /// Creates a delayed spinner with the 300 ms delay.
    public init() {
        self.init(visibility: DelayedVisibility())
    }

    /// Creates a delayed spinner that waits `delay` before it shows.
    init(delay: Duration) {
        self.init(visibility: DelayedVisibility(delay: delay))
    }

    init(visibility: DelayedVisibility) {
        _visibility = State(initialValue: visibility)
    }

    /// The view's content.
    public var body: some View {
        if visibility.showsInFirstFrame {
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            ZStack {
                if visibility.isVisible {
                    ProgressView()
                        .transition(StacklyMotion.crossfade)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .task {
                await visibility.run()
            }
        }
    }
}
