import AppKit

/// Writes plain text to the general pasteboard: the only place besides
/// ``ConcealedPasteboard`` that touches `NSPasteboard` (a source rule guards it).
/// Use it, through ``CopyButton``, for addresses, IDs, paths, and commands. A secret
/// never goes through it; a secret copy always uses ``ConcealedPasteboard``.
public enum PlainPasteboard {
    /// Replaces the pasteboard's contents with `text` as a plain string.
    public static func write(_ text: String) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
    }
}
