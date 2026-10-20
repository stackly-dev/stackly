import AppKit
import Foundation

/// The part of `NSPasteboard` the concealed copy uses, so tests can watch the writes.
protocol PasteboardStore: AnyObject {
    var changeCount: Int { get }
    func prepareForNewContents(with options: NSPasteboard.ContentsOptions) -> Int
    func clearContents() -> Int
    func setString(_ string: String, forType dataType: NSPasteboard.PasteboardType) -> Bool
}

extension NSPasteboard: PasteboardStore {}

/// Writes a secret to the pasteboard so it stays on this Mac and clears itself (spec 0011,
/// *Reveal dan copy*).
///
/// A copy asks for `prepareForNewContents(with: .currentHostOnly)`, so it never travels
/// through Universal Clipboard, and adds the `org.nspasteboard.ConcealedType` and
/// `org.nspasteboard.TransientType` markers that clipboard managers honor. It remembers the
/// pasteboard's `changeCount`, and clears the pasteboard after 90 seconds or when the app
/// quits, but only while the `changeCount` is still the one it wrote: something the person
/// copied since stays.
///
/// One instance owns the pending clear, so a view that goes away does not cancel it. Use
/// ``shared`` in the app.
public final class ConcealedPasteboard {
    /// How long a copied secret stays on the pasteboard: 90 seconds.
    public static let clearDelay: Duration = .seconds(90)

    /// The pasteboard type that marks a concealed value.
    static let concealedType = NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")
    /// The pasteboard type that marks a transient value.
    static let transientType = NSPasteboard.PasteboardType("org.nspasteboard.TransientType")

    /// The instance the app uses: the general pasteboard, cleared when the app quits.
    public static let shared = ConcealedPasteboard(
        pasteboard: NSPasteboard.general, clearsOnTerminationOf: .default)

    private let pasteboard: any PasteboardStore
    private let delay: Duration
    private let sleep: @Sendable (Duration) async throws -> Void
    private var writtenChangeCount: Int?
    private var clearTask: Task<Void, Never>?

    /// Creates a concealed pasteboard.
    ///
    /// - Parameters:
    ///   - pasteboard: The pasteboard to write.
    ///   - delay: How long the value stays; 90 seconds by default.
    ///   - sleep: Waits for a duration and throws when cancelled; `Task.sleep(for:)` by
    ///     default.
    ///   - center: Where `NSApplication.willTerminateNotification` is posted, or `nil` to
    ///     not clear on quit.
    init(
        pasteboard: any PasteboardStore,
        delay: Duration = ConcealedPasteboard.clearDelay,
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        },
        clearsOnTerminationOf center: NotificationCenter? = nil
    ) {
        self.pasteboard = pasteboard
        self.delay = delay
        self.sleep = sleep
        if let center {
            // The observer lives with the center; it holds `self` weakly.
            _ = center.addObserver(
                forName: NSApplication.willTerminateNotification, object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.clearIfUnchanged() }
            }
        }
    }

    /// Whether a clear is still waiting.
    var hasPendingClear: Bool { writtenChangeCount != nil }

    /// The pasteboard's change count now. Read it when a copy starts and pass it to
    /// ``copy(_:unlessChangedSince:)`` when the value arrives.
    public var changeCount: Int { pasteboard.changeCount }

    /// Copies `text` and arms the clear. A second copy replaces the first and restarts the
    /// delay.
    ///
    /// - Parameter text: The secret.
    public func copy(_ text: String) {
        write(text)
    }

    /// Copies `text` only when the pasteboard is still at `changeCount`, so a copy that
    /// finishes late never overwrites something the person copied after asking for it.
    ///
    /// - Parameters:
    ///   - text: The secret.
    ///   - changeCount: The pasteboard's ``changeCount`` when the copy was asked for.
    /// - Returns: `true` when the text was written, `false` when the pasteboard had changed
    ///   and was left alone.
    @discardableResult
    public func copy(_ text: String, unlessChangedSince changeCount: Int) -> Bool {
        guard pasteboard.changeCount == changeCount else { return false }
        write(text)
        return true
    }

    private func write(_ text: String) {
        clearTask?.cancel()
        _ = pasteboard.prepareForNewContents(with: .currentHostOnly)
        _ = pasteboard.setString(text, forType: .string)
        _ = pasteboard.setString("", forType: Self.concealedType)
        _ = pasteboard.setString("", forType: Self.transientType)
        writtenChangeCount = pasteboard.changeCount
        let delay = delay
        let sleep = sleep
        clearTask = Task { [weak self] in
            do {
                try await sleep(delay)
            } catch {
                return
            }
            self?.clearIfUnchanged()
        }
    }

    /// Clears the pasteboard when nobody has changed it since the last copy, and forgets
    /// the copy either way.
    public func clearIfUnchanged() {
        clearTask?.cancel()
        clearTask = nil
        guard let written = writtenChangeCount else { return }
        writtenChangeCount = nil
        if pasteboard.changeCount == written {
            _ = pasteboard.clearContents()
        }
    }
}
