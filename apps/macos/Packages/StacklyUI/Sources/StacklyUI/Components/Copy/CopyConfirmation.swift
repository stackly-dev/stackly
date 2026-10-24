import Foundation
import Observation

/// The copy action and the two seconds of "Copied" that follow it (spec 0067, AC-4).
/// A second copy while the first one waits writes again and starts the two seconds over; the first wait then ends without touching the state.
@Observable
public final class CopyConfirmation {
    /// How long "Copied" stays: 2 seconds.
    public static let confirmationDuration: Duration = .seconds(2)

    /// Whether the button shows "Copied" now.
    public private(set) var isConfirming = false

    @ObservationIgnored private let write: @MainActor (String) -> Void
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private var generation = 0

    /// Creates a confirmation.
    /// - Parameters:
    ///   - write: Writes the text to the pasteboard; ``PlainPasteboard/write(_:)`` by     default.
    ///   - sleep: Waits for a duration and throws when cancelled; `Task.sleep(for:)` by     default.
    public init(
        write: @escaping @MainActor (String) -> Void = PlainPasteboard.write,

        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.write = write
        self.sleep = sleep
    }

    /// It returns after the two seconds, or at once when the task is cancelled or a newer
    /// copy took over.
    /// - Parameters:
    ///   - text: What to copy.
    public func copy(_ text: String) async {
        write(text)
        generation += 1
        let mine = generation
        isConfirming = true
        try? await sleep(Self.confirmationDuration)
        if generation == mine {
            isConfirming = false
        }
    }
}
