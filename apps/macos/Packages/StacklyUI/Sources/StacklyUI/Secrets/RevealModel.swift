import Foundation
import Observation
import StacklyClient

/// The state behind ``RevealableSecretText``: it reveals one or more references through
/// the daemon, shows the composed text for a while, and copies it to the pasteboard
/// (spec 0011 AC-19, AC-24, *Reveal dan copy*).
///
/// Reveal is all or nothing. Show and Copy ask the daemon for every reference, one after
/// the other; one failure cancels the action, shows and copies nothing, and drops the values
/// already received. The values live in a local dictionary for the length of one call to
/// `compose`; only the composed text of Show is kept, until Hide, 30 seconds, the view going
/// away, or the app becoming inactive. The model is owned by the view's `@State`, so it goes
/// with the view.
///
/// Hiding and cancelling are different things. Those four exits only conceal what is on
/// screen. A Show that still waits for the daemon when one of them comes in (for example the
/// app turns inactive because a Keychain dialog opened) drops its value and ends in
/// ``Phase/failed(_:)`` with a message that says why, never in a silent ``Phase/idle``. A Copy
/// that waits is an explicit request that shows nothing, so it finishes and writes the
/// pasteboard whatever happens to the screen meanwhile.
@Observable
public final class RevealModel {
    /// How long a shown value stays on screen: 30 seconds.
    public static let showDuration: Duration = .seconds(30)

    /// Reveals one reference for a purpose.
    public typealias Reveal =
        @Sendable (String, RevealPurpose) async throws(ClientError) -> RevealedSecret

    /// Builds the text from the revealed values, keyed by reference.
    public typealias Compose = ([String: RevealedSecret]) throws -> String

    /// What the model is doing, apart from the text.
    public enum Phase: Equatable, Sendable {
        /// Nothing runs.
        case idle
        /// Show or Copy waits for the daemon.
        case working
        /// The last action failed, with a safe message.
        case failed(String)
        /// The text was copied; the pasteboard clears itself.
        case copied
    }

    /// The references to reveal, without repeats, in the order given.
    public let references: [String]

    /// What the model is doing.
    public private(set) var phase = Phase.idle
    /// The composed text while it is shown, or `nil` while it is hidden.
    public private(set) var shownText: String?

    /// Whether the value is on screen.
    public var isShown: Bool { shownText != nil }

    @ObservationIgnored private let reveal: Reveal
    @ObservationIgnored private let compose: Compose
    @ObservationIgnored private let pasteboard: ConcealedPasteboard
    @ObservationIgnored private let showDuration: Duration
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private var hideTask: Task<Void, Never>?
    @ObservationIgnored private var generation = 0
    /// The purpose of the request that waits for the daemon, or `nil` when none does.
    @ObservationIgnored private var inFlight: RevealPurpose?
    /// Numbers each request, so a cancelled one that finishes late leaves a newer one alone.
    @ObservationIgnored private var flight = 0

    /// Creates a model.
    ///
    /// - Parameters:
    ///   - references: The references to reveal. A repeated reference counts once.
    ///   - reveal: Reveals one reference for a purpose.
    ///   - compose: Builds the text, for example a URI with the real password, from the
    ///     revealed values.
    ///   - pasteboard: Where Copy writes.
    ///   - showDuration: How long Show keeps the value; 30 seconds by default.
    ///   - sleep: Waits for a duration and throws when cancelled; `Task.sleep(for:)` by
    ///     default.
    public init(
        references: [String],
        reveal: @escaping Reveal,
        compose: @escaping Compose,
        pasteboard: ConcealedPasteboard = .shared,
        showDuration: Duration = RevealModel.showDuration,
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        var seen = Set<String>()
        self.references = references.filter { seen.insert($0).inserted }
        self.reveal = reveal
        self.compose = compose
        self.pasteboard = pasteboard
        self.showDuration = showDuration
        self.sleep = sleep
    }

    /// Reveals every reference for `.show`, then shows the composed text for 30 seconds.
    /// Nothing shows when any reveal or the composition fails.
    public func show() async {
        guard phase != .working else { return }
        conceal(because: nil)
        let mine = generation
        phase = .working
        let flight = begin(.show)
        defer { end(flight) }
        guard let text = await gather(purpose: .show, generation: mine) else { return }
        shownText = text
        phase = .idle
        armHide(generation: mine)
    }

    /// Reveals every reference for `.copy`, composes the text, and copies it with
    /// ``ConcealedPasteboard``. Nothing is copied when any reveal or the composition fails.
    ///
    /// Hide, the 30 second limit, and the app becoming inactive do not cancel a Copy that
    /// waits for the daemon: it shows nothing, so there is nothing to conceal. When the person
    /// copied something else after pressing Copy, the late value is not written and the phase
    /// is ``Phase/failed(_:)``.
    public func copy() async {
        guard phase != .working else { return }
        phase = .working
        let flight = begin(.copy)
        defer { end(flight) }
        let asked = pasteboard.changeCount
        guard let text = await gather(purpose: .copy, generation: nil) else { return }
        guard pasteboard.copy(text, unlessChangedSince: asked) else {
            phase = .failed("Not copied because the clipboard changed. Press Copy to try again.")
            return
        }
        phase = .copied
    }

    /// Hides the value and drops it. A Show that still waits for the daemon ends in
    /// ``Phase/failed(_:)``.
    public func hide() {
        conceal(because: "Hidden before the value arrived. Press Show to try again.")
    }

    /// The view went away: hide.
    public func viewDisappeared() {
        conceal(because: "Hidden because the view went away. Press Show to try again.")
    }

    /// The app is no longer the active app: hide.
    public func appBecameInactive() {
        conceal(
            because: "Hidden because Stackly went to the background. Press Show to try again.")
    }

    /// Drops the shown text, stops the timer, and cancels a Show that still waits. `reason`
    /// is what the user reads when such a Show is cancelled; `nil` means the caller is about
    /// to start a new action and nothing needs saying.
    private func conceal(because reason: String?) {
        generation += 1
        hideTask?.cancel()
        hideTask = nil
        shownText = nil
        if let reason, phase == .working, inFlight == .show {
            phase = .failed(reason)
        }
    }

    /// Reveals every reference and composes the text, or returns `nil` after setting
    /// ``phase`` to `.failed`. With a `generation`, the action stops as soon as a conceal
    /// bumps it: the values are dropped and ``phase`` stays as ``conceal(because:)`` left
    /// it. Without one, nothing cancels the action.
    private func gather(purpose: RevealPurpose, generation mine: Int?) async -> String? {
        var values: [String: RevealedSecret] = [:]
        for reference in references {
            do throws(ClientError) {
                values[reference] = try await reveal(reference, purpose)
            } catch {
                return fail(.failed(Self.message(for: error)), generation: mine)
            }
            if !isCurrent(mine) { return nil }
        }
        let text: String
        do {
            text = try compose(values)
        } catch {
            return fail(
                .failed("Couldn’t build the text from the revealed values."), generation: mine)
        }
        values.removeAll()
        return text
    }

    private func begin(_ purpose: RevealPurpose) -> Int {
        flight += 1
        inFlight = purpose
        return flight
    }

    private func end(_ finished: Int) {
        if flight == finished { inFlight = nil }
    }

    private func isCurrent(_ mine: Int?) -> Bool {
        mine.map { $0 == generation } ?? true
    }

    private func fail(_ failure: Phase, generation mine: Int?) -> String? {
        if isCurrent(mine) { phase = failure }
        return nil
    }

    private func armHide(generation mine: Int) {
        let duration = showDuration
        let sleep = sleep
        hideTask = Task { [weak self] in
            do {
                try await sleep(duration)
            } catch {
                return
            }
            guard let self, generation == mine else { return }
            conceal(because: nil)
        }
    }

    /// A safe message for a failed reveal: the daemon's own text, already redacted.
    static func message(for error: ClientError) -> String {
        switch error {
        case .notSent: "Couldn’t reach stacklyd. Nothing was revealed."
        case .outcomeUnknown: "stacklyd didn’t answer in time. Nothing is shown."
        case .rejected(let body):
            sentence(body.message) + (body.hint.map { " " + sentence($0) } ?? "")
        }
    }

    /// `text` with a capital first letter and a final period.
    static func sentence(_ text: String) -> String {
        guard let first = text.first else { return text }
        let capitalized = first.uppercased() + text.dropFirst()
        return capitalized.hasSuffix(".") ? capitalized : capitalized + "."
    }
}
