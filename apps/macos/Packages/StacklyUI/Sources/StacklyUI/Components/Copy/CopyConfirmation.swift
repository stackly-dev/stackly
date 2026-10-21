import Foundation
import Observation

/// The copy action and the two seconds of "Copied" that follow it (spec 0067, AC-4).
///
/// ``copy(_:announcement:)`` writes the text, announces it once, and holds
/// ``isConfirming`` for two seconds. A second copy while the first one waits writes again
/// and starts the two seconds over; the first wait then ends without touching the state.
/// The writer, the announcer, and the sleep are injected so tests run without a pasteboard,
/// a VoiceOver, or a clock.
@Observable
public final class CopyConfirmation {
    /// How long "Copied" stays: 2 seconds.
    public static let confirmationDuration: Duration = .seconds(2)

    /// Whether the button shows "Copied" now.
    public private(set) var isConfirming = false

    @ObservationIgnored private let write: @MainActor (String) -> Void
    @ObservationIgnored private let announce: @MainActor (String) -> Void
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private var generation = 0

    /// Creates a confirmation.
    ///
    /// - Parameters:
    ///   - write: Writes the text to the pasteboard; ``PlainPasteboard/write(_:)`` by
    ///     default.
    ///   - announce: Posts the VoiceOver announcement; ``announce(_:)`` by default.
    ///   - sleep: Waits for a duration and throws when cancelled; `Task.sleep(for:)` by
    ///     default.
    public init(
        write: @escaping @MainActor (String) -> Void = PlainPasteboard.write,
        announce: @escaping @MainActor (String) -> Void = announce(_:),
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.write = write
        self.announce = announce
        self.sleep = sleep
    }

    /// Writes `text`, announces `announcement`, and shows "Copied" for two seconds.
    ///
    /// It returns after the two seconds, or at once when the task is cancelled or a newer
    /// copy took over.
    ///
    /// - Parameters:
    ///   - text: What to copy.
    ///   - announcement: What VoiceOver says, once, after the write.
    public func copy(_ text: String, announcement: String) async {
        write(text)
        announce(announcement)
        generation += 1
        let mine = generation
        isConfirming = true
        try? await sleep(Self.confirmationDuration)
        if generation == mine {
            isConfirming = false
        }
    }
}
