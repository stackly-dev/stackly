//! The level of a line (spec 0027, *Aturan level*): a pure keyword rule on
//! the redacted text, over its first [`LOG_LEVEL_SCAN_CHARS`] characters.
//! The stream never decides it: a server that logs everything to stderr
//! still has `normal` lines.

use serde::{Deserialize, Serialize};

use super::LOG_LEVEL_SCAN_CHARS;

/// How serious a line looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Nothing matched.
    Normal,
    /// A warning keyword matched.
    Warning,
    /// An error keyword matched.
    Error,
}

const ERROR_TOKENS: [&str; 9] = [
    "ERROR", "ERR", "FATAL", "FTL", "PANIC", "CRITICAL", "CRIT", "EMERG", "ALERT",
];
const WARNING_TOKENS: [&str; 3] = ["WARNING", "WARN", "WRN"];
const ERROR_VALUES: [&str; 4] = ["error", "fatal", "panic", "critical"];
const WARNING_VALUES: [&str; 2] = ["warning", "warn"];

/// Returns the level of `text`. The first match in the text decides: an
/// upper case keyword bounded by characters that are neither letters nor
/// digits (`ERROR:`, `[WRN]`), or a `level=` or `"level":"` pair in any
/// case. Lower case words such as "0 errors" never match.
#[must_use]
pub fn level_of(text: &str) -> LogLevel {
    let end = text
        .char_indices()
        .nth(LOG_LEVEL_SCAN_CHARS)
        .map_or(text.len(), |(at, _)| at);
    let head = &text[..end];
    let token = first_token(head);
    let pair = first_pair(head);
    match (token, pair) {
        (Some((at, level)), Some((pair_at, pair_level))) => {
            if pair_at < at {
                pair_level
            } else {
                level
            }
        }
        (Some((_, level)), None) | (None, Some((_, level))) => level,
        (None, None) => LogLevel::Normal,
    }
}

/// The first bounded upper case keyword and where it starts.
fn first_token(head: &str) -> Option<(usize, LogLevel)> {
    let bytes = head.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_uppercase() {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_uppercase() {
            at += 1;
        }
        let bounded_before = head[..start]
            .chars()
            .next_back()
            .is_none_or(|before| !before.is_alphanumeric());
        let bounded_after = head[at..]
            .chars()
            .next()
            .is_none_or(|after| !after.is_alphanumeric());
        if !(bounded_before && bounded_after) {
            continue;
        }
        let token = &head[start..at];
        if ERROR_TOKENS.contains(&token) {
            return Some((start, LogLevel::Error));
        }
        if WARNING_TOKENS.contains(&token) {
            return Some((start, LogLevel::Warning));
        }
    }
    None
}

/// The first `level=<value>` or `"level":"<value>"` pair and where it
/// starts, in any case.
fn first_pair(head: &str) -> Option<(usize, LogLevel)> {
    let lower = head.to_ascii_lowercase();
    let mut best: Option<(usize, LogLevel)> = None;
    for key in ["level=", "\"level\":\""] {
        let mut from = 0;
        while let Some(found) = lower[from..].find(key) {
            let start = from + found;
            let value_at = start + key.len();
            let value: String = lower[value_at..]
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            let level = if ERROR_VALUES.contains(&value.as_str()) {
                Some(LogLevel::Error)
            } else if WARNING_VALUES.contains(&value.as_str()) {
                Some(LogLevel::Warning)
            } else {
                None
            };
            if let Some(level) = level {
                if best.is_none_or(|(at, _)| start < at) {
                    best = Some((start, level));
                }
                break;
            }
            from = value_at;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_postgresql_and_nats_lines_get_their_level() {
        let cases = [
            (
                "2026-11-01 07:03:22.118 UTC [4123] LOG:  database system is ready to accept connections",
                LogLevel::Normal,
            ),
            (
                "2026-11-01 07:03:22.118 UTC [4130] FATAL:  password authentication failed for user \"app\"",
                LogLevel::Error,
            ),
            (
                "2026-11-01 07:03:22.118 UTC [4130] ERROR:  relation \"x\" does not exist",
                LogLevel::Error,
            ),
            (
                "2026-11-01 07:03:22.118 UTC [4130] PANIC:  could not write to file",
                LogLevel::Error,
            ),
            (
                "2026-11-01 07:03:22.118 UTC [4130] WARNING:  there is no transaction in progress",
                LogLevel::Warning,
            ),
            (
                "[4123] 2026/11/01 07:03:22.118765 [INF] Server is ready",
                LogLevel::Normal,
            ),
            (
                "[4123] 2026/11/01 07:03:22.118765 [ERR] 127.0.0.1:52344 - cid:5 - authentication error",
                LogLevel::Error,
            ),
            (
                "[4123] 2026/11/01 07:03:22.118765 [FTL] Error listening on port",
                LogLevel::Error,
            ),
            (
                "[4123] 2026/11/01 07:03:22.118765 [WRN] Plaintext passwords detected",
                LogLevel::Warning,
            ),
        ];
        for (line, level) in cases {
            assert_eq!(level_of(line), level, "{line}");
        }
    }

    #[test]
    fn logfmt_and_json_pairs_match_in_any_case() {
        assert_eq!(level_of("ts=1 level=error msg=boom"), LogLevel::Error);
        assert_eq!(level_of("ts=1 LEVEL=Warn msg=slow"), LogLevel::Warning);
        assert_eq!(
            level_of("{\"level\":\"fatal\",\"msg\":\"x\"}"),
            LogLevel::Error
        );
        assert_eq!(level_of("{\"level\":\"warning\"}"), LogLevel::Warning);
        assert_eq!(level_of("level=info msg=ok"), LogLevel::Normal);
    }

    #[test]
    fn the_first_match_decides() {
        assert_eq!(level_of("level=warn msg=\"ERROR in x\""), LogLevel::Warning);
        assert_eq!(level_of("WARN: then level=error"), LogLevel::Warning);
    }

    #[test]
    fn plain_words_and_unbounded_capitals_never_match() {
        assert_eq!(
            level_of("checkpoint complete: 0 errors, 0 warnings"),
            LogLevel::Normal
        );
        assert_eq!(level_of("ERRORS are counted"), LogLevel::Normal);
        assert_eq!(level_of("PostgreSQL ERRor"), LogLevel::Normal);
        assert_eq!(level_of("ERROR1 code"), LogLevel::Normal);
        assert_eq!(level_of(""), LogLevel::Normal);
    }

    #[test]
    fn only_the_first_characters_are_looked_at() {
        let line = format!("{}ERROR:", "é".repeat(LOG_LEVEL_SCAN_CHARS));
        assert_eq!(level_of(&line), LogLevel::Normal);

        let line = format!("{} ERROR:", "a".repeat(LOG_LEVEL_SCAN_CHARS - 8));
        assert_eq!(level_of(&line), LogLevel::Error);
    }
}
