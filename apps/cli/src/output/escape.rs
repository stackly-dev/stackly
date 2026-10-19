//! Escaping text from the daemon or from arguments before it reaches a
//! terminal (spec 0008, AC-3), so a path, a provider message, or a name
//! someone else chose can never inject a terminal escape or flip text with
//! a bidi override.

use std::borrow::Cow;
use std::fmt::Write as _;

/// `text` with every C0 control (U+0000 to U+001F, tab and newline
/// included), DEL (U+007F), C1 control (U+0080 to U+009F), and bidi
/// override or isolate (U+202A to U+202E, U+2066 to U+2069) written as
/// `\u{..}` in lowercase hex. Text without any of them is borrowed.
#[must_use]
pub fn escape_display(text: &str) -> Cow<'_, str> {
    if !text.chars().any(needs_escape) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if needs_escape(c) {
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Whether `c` must not reach a terminal raw.
const fn needs_escape(c: char) -> bool {
    matches!(
        c,
        '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_del_c1_and_bidi_overrides_are_written_as_escapes() {
        assert_eq!(
            escape_display("red \u{1b}[31mtext\u{1b}[0m"),
            "red \\u{1b}[31mtext\\u{1b}[0m"
        );
        assert_eq!(
            escape_display("a\u{202e}b\u{2066}c"),
            "a\\u{202e}b\\u{2066}c"
        );
        assert_eq!(
            escape_display("line\nnext\ttab\r"),
            "line\\u{a}next\\u{9}tab\\u{d}"
        );
        assert_eq!(
            escape_display("\u{0}\u{7f}\u{85}\u{9b}"),
            "\\u{0}\\u{7f}\\u{85}\\u{9b}"
        );
    }

    #[test]
    fn plain_and_wide_text_is_borrowed_unchanged() {
        for text in ["", "postgres-16 ready", "数据库 データ", "café \u{202f} x"] {
            assert!(matches!(escape_display(text), Cow::Borrowed(_)), "{text:?}");
        }
        assert_eq!(
            escape_display("\u{2029}\u{202f}\u{206a}"),
            "\u{2029}\u{202f}\u{206a}"
        );
    }
}
