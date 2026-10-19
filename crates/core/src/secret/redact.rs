//! The [`Redactor`] (spec 0011, *Redaction*): the filter every text passes
//! before it leaves the daemon, as a log line, an error, a progress
//! message, or an event.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;
use std::sync::{LazyLock, PoisonError, RwLock};

use regex::Regex;
use stackly_platform::{SCRUB_ANYWHERE_LEN, SCRUB_MIN_LEN, SecretValue};

use crate::operation::Redact;

/// What every hidden value becomes, on every text surface. Its length never
/// depends on the value.
pub const MASK: &str = "****";

/// A sensitive key, case insensitive, after the start of a line or a
/// character that is not a letter or digit, maybe in quotes, then `=` or
/// `:` with optional spaces around it. The key alone, followed only by
/// spaces, does not match, so `password authentication failed` stays.
static SENSITIVE_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?im)(?:^|[^\p{L}\p{N}])["']?(password|passwd|pwd|secret|token|api_key|apikey|access_key|private_key|authorization|requirepass|masterauth|pgpassword|mysql_pwd|rediscli_auth)["']?[ \t]*[=:][ \t]*"#,
    )
    .expect("the sensitive key pattern is a valid regex")
});

/// The scheme of a URI and its `://`.
static URI_SCHEME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z][A-Za-z0-9+.\-]*://").expect("the URI scheme pattern is a valid regex")
});

/// Replaces secret values in text with [`MASK`], in three layers (AC-13):
///
/// 1. Every known value: one the daemon read, stored, copied, or generated
///    during this run, the old value before a replace included. A value of
///    [`SCRUB_ANYWHERE_LEN`] bytes or more is replaced wherever it appears.
///    A value of [`SCRUB_MIN_LEN`] up to 15 bytes is replaced only as a
///    whole token, with no letter or digit right before or after it, so
///    `postgres` inside `postgresql` stays. The longest value wins.
/// 2. The password in the userinfo of a URI: `<scheme>://<user>:****@<host>`.
///    The password runs from the first `:` after `://` to the last `@`
///    before `/`, `?`, `#`, whitespace, or the end of the line.
/// 3. The value after a sensitive key such as `password=`, `token:`, or
///    `"api_key": "..."`. A quoted value keeps its quotes; a bare value
///    ends before whitespace, `&`, `,`, `;`, `)`, `}`, `]`, or a quote; and
///    a bare value after `authorization` runs to the end of the line, so
///    `Authorization: Bearer x` is hidden whole.
///
/// The daemon shares one `Arc<Redactor>`. A value shorter than
/// [`SCRUB_MIN_LEN`] is never learned: only its type and the key patterns
/// hide it. An encoded, cut, or partly hidden value is not caught.
///
/// ```
/// use stackly_core::secret::Redactor;
/// use stackly_platform::SecretValue;
///
/// let redactor = Redactor::new();
/// redactor.learn(&SecretValue::new("correct-horse-battery-staple".to_owned())?);
///
/// assert_eq!(
///     redactor.redact("login with correct-horse-battery-staple failed"),
///     "login with **** failed"
/// );
/// assert_eq!(
///     redactor.redact("url=postgres://app:hunter2@127.0.0.1/db"),
///     "url=postgres://app:****@127.0.0.1/db"
/// );
/// assert_eq!(redactor.redact("PGPASSWORD=hunter2 psql"), "PGPASSWORD=**** psql");
/// # Ok::<(), stackly_platform::SecretValueError>(())
/// ```
pub struct Redactor {
    known: RwLock<Known>,
}

/// The learned values, longest first, with the first byte of each for a
/// quick skip while scanning.
struct Known {
    values: Vec<SecretValue>,
    first_bytes: [bool; 256],
}

/// Returns a second value equal to `value`, wiped from memory when it
/// drops like the first. The redactor keeps such a copy of every value it
/// learns, and the secret worker keeps one for a write it runs after its
/// caller may have stopped waiting. These two copies and
/// [`char_count`] are the only places outside matching and delivery that
/// read a value.
pub(crate) fn copy_value(value: &SecretValue) -> SecretValue {
    SecretValue::new(value.expose().to_owned())
        .expect("a secret value stays valid when it is copied")
}

/// Returns the number of Unicode characters of `value`, which the
/// `minLength` and `maxLength` of a secret field count (spec 0009).
pub(crate) fn char_count(value: &SecretValue) -> usize {
    value.expose().chars().count()
}

impl Redactor {
    /// Returns a redactor that knows no value yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            known: RwLock::new(Known {
                values: Vec::new(),
                first_bytes: [false; 256],
            }),
        }
    }

    /// Learns a value, so every later [`redact`](Self::redact) hides it.
    /// The daemon calls it as soon as a value is in its memory, before it
    /// is written anywhere. A value shorter than [`SCRUB_MIN_LEN`] is not
    /// learned, and learning a value twice keeps one copy. The copy is
    /// wiped from memory when the redactor drops.
    pub fn learn(&self, value: &SecretValue) {
        if value.len() < SCRUB_MIN_LEN {
            return;
        }
        let text = value.expose();
        let mut known = self.known.write().unwrap_or_else(PoisonError::into_inner);
        if known.values.iter().any(|seen| seen.expose() == text) {
            return;
        }
        let copy = copy_value(value);
        if let Some(&first) = text.as_bytes().first() {
            known.first_bytes[usize::from(first)] = true;
        }
        let at = known
            .values
            .partition_point(|seen| seen.len() >= copy.len());
        known.values.insert(at, copy);
    }

    /// Returns `text` with every known value, URI password, and value after
    /// a sensitive key replaced by [`MASK`]. Each layer looks at the text as
    /// given, so one layer never hides a match from another (a learned
    /// `postgres` does not stop the URI layer from seeing the password of
    /// `postgres://app:x@host`), and overlapping or touching matches become
    /// one mask. Text with nothing to hide comes back borrowed, and
    /// redacting twice gives the same text as once.
    #[must_use]
    pub fn redact<'t>(&self, text: &'t str) -> Cow<'t, str> {
        let mut hidden = self.known_values(text);
        hidden.extend(uri_passwords(text));
        hidden.extend(key_values(text));
        if hidden.is_empty() {
            return Cow::Borrowed(text);
        }
        hidden.sort_by_key(|range| range.start);
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        let mut merged: Option<Range<usize>> = None;
        for range in hidden {
            merged = match merged {
                Some(current) if range.start <= current.end => {
                    Some(current.start..current.end.max(range.end))
                }
                Some(current) => {
                    mask(&mut out, text, &mut copied, current);
                    Some(range)
                }
                None => Some(range),
            };
        }
        if let Some(last) = merged {
            mask(&mut out, text, &mut copied, last);
        }
        out.push_str(&text[copied..]);
        Cow::Owned(out)
    }

    /// Layer 2: where each known value appears, the longest first at each
    /// position.
    fn known_values(&self, text: &str) -> Vec<Range<usize>> {
        let known = self.known.read().unwrap_or_else(PoisonError::into_inner);
        let mut found = Vec::new();
        if known.values.is_empty() {
            return found;
        }
        let bytes = text.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            if known.first_bytes[usize::from(bytes[at])] {
                // A value starts with the first byte of a character, so a
                // match always starts and ends on a character boundary.
                let matched = known.values.iter().map(SecretValue::expose).find(|value| {
                    bytes[at..].starts_with(value.as_bytes())
                        && (value.len() >= SCRUB_ANYWHERE_LEN
                            || is_whole_token(text, at, at + value.len()))
                });
                if let Some(value) = matched {
                    found.push(at..at + value.len());
                    at += value.len();
                    continue;
                }
            }
            at += 1;
        }
        found
    }
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let known = self.known.read().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("Redactor")
            .field("known", &known.values.len())
            .finish()
    }
}

impl Redact for Redactor {
    fn redact(&self, text: &str) -> String {
        Self::redact(self, text).into_owned()
    }
}

/// True when no letter or digit sits right before `start` or right after
/// `end`.
fn is_whole_token(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

/// The byte offset of the end of the line `from` is on.
fn line_end(text: &str, from: usize) -> usize {
    text[from..]
        .find(['\n', '\r'])
        .map_or(text.len(), |offset| from + offset)
}

/// Writes what comes before `range` since `copied`, then [`MASK`].
fn mask(out: &mut String, text: &str, copied: &mut usize, range: Range<usize>) {
    out.push_str(&text[*copied..range.start]);
    out.push_str(MASK);
    *copied = range.end;
}

/// Layer 3, URIs: the password in the userinfo of each URI.
fn uri_passwords(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(scheme) = URI_SCHEME.find_at(text, from) {
        let authority_start = scheme.end();
        let authority_end = text[authority_start..]
            .find(|c: char| matches!(c, '/' | '?' | '#') || c.is_whitespace())
            .map_or(text.len(), |offset| authority_start + offset);
        let authority = &text[authority_start..authority_end];
        if let Some(at) = authority.rfind('@')
            && let Some(colon) = authority[..at].find(':')
        {
            found.push(authority_start + colon + 1..authority_start + at);
        }
        from = authority_end;
    }
    found
}

/// Layer 3, keys: the value after each sensitive key.
fn key_values(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(captures) = SENSITIVE_KEY.captures_at(text, from) {
        let (Some(whole), Some(key)) = (captures.get(0), captures.get(1)) else {
            break;
        };
        let start = whole.end();
        let rest = &text[start..];
        let (value, next) = match rest.chars().next() {
            Some(quote @ ('"' | '\'')) => {
                let inner = start + 1;
                let line = line_end(text, inner);
                match text[inner..line].find(quote) {
                    Some(offset) => (inner..inner + offset, inner + offset + 1),
                    None => (inner..line, line),
                }
            }
            _ if key.as_str().eq_ignore_ascii_case("authorization") => {
                let line = line_end(text, start);
                (start..line, line)
            }
            _ => {
                let end = rest
                    .find(|c: char| {
                        c.is_whitespace()
                            || matches!(c, '&' | ',' | ';' | ')' | '}' | ']' | '"' | '\'')
                    })
                    .map_or(text.len(), |offset| start + offset);
                (start..end, end)
            }
        };
        if !value.is_empty() {
            found.push(value);
        }
        // `next` is past the key, so the scan always moves on.
        from = next;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> SecretValue {
        SecretValue::new(text.to_owned()).unwrap()
    }

    fn knowing(values: &[&str]) -> Redactor {
        let redactor = Redactor::new();
        for text in values {
            redactor.learn(&value(text));
        }
        redactor
    }

    const LONG: &str = "Zq8vN2kLp4Rt7Wx1Yb3Mc5Df6Gh9Jk0Sa";

    #[test]
    fn the_grammar_table_of_the_spec_holds() {
        let redactor = knowing(&[LONG, "postgres"]);
        let cases = [
            (format!("token-{LONG}-tail"), "token-****-tail".to_owned()),
            (format!("x{LONG}y"), "x****y".to_owned()),
            ("role postgres denied".into(), "role **** denied".into()),
            ("user:postgres@".into(), "user:****@".into()),
            ("postgresql is up".into(), "postgresql is up".into()),
            (
                "mysql://admin:p@ss@db.local:3306/app".into(),
                "mysql://admin:****@db.local:3306/app".into(),
            ),
            (
                "mysql://admin@db.local/app".into(),
                "mysql://admin@db.local/app".into(),
            ),
            (
                "POSTGRES_PASSWORD=x".into(),
                "POSTGRES_PASSWORD=****".into(),
            ),
            (r#""password": "x""#.into(), r#""password": "****""#.into()),
            ("token: 'x'".into(), "token: '****'".into()),
            (
                "Authorization: Bearer x".into(),
                "Authorization: ****".into(),
            ),
            (
                "Authorization: Basic x y".into(),
                "Authorization: ****".into(),
            ),
            (
                r#"password authentication failed for user "app""#.into(),
                r#"password authentication failed for user "app""#.into(),
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(redactor.redact(&input), expected, "{input}");
        }
    }

    #[test]
    fn a_learned_short_value_also_hides_as_a_uri_scheme() {
        let redactor = knowing(&["postgres"]);

        assert_eq!(
            redactor.redact("postgres://admin:p@ss@db.local/app"),
            "****://admin:****@db.local/app"
        );
    }

    #[test]
    fn an_old_value_stays_hidden_after_a_replace() {
        let redactor = knowing(&["old-value-before-replace"]);
        redactor.learn(&value("new-value-after-replace"));

        assert_eq!(
            redactor.redact("was old-value-before-replace, now new-value-after-replace"),
            "was ****, now ****"
        );
    }

    #[test]
    fn the_longest_known_value_wins_where_two_start_together() {
        let redactor = knowing(&["abcdefgh", "abcdefgh-ijklmnop-qrst"]);

        assert_eq!(redactor.redact("[abcdefgh-ijklmnop-qrst]"), "[****]");
        assert_eq!(redactor.redact("[abcdefgh-x]"), "[****-x]");
    }

    #[test]
    fn a_short_value_hides_only_as_a_whole_token() {
        let redactor = knowing(&["hunter22"]);

        assert_eq!(redactor.redact("hunter22"), "****");
        assert_eq!(redactor.redact("(hunter22)"), "(****)");
        assert_eq!(redactor.redact("_hunter22_"), "_****_");
        assert_eq!(redactor.redact("xhunter22"), "xhunter22");
        assert_eq!(redactor.redact("hunter22x"), "hunter22x");
        assert_eq!(redactor.redact("éhunter22"), "éhunter22");
    }

    #[test]
    fn a_value_shorter_than_eight_bytes_is_never_learned() {
        let redactor = knowing(&["hunter2"]);

        assert_eq!(format!("{redactor:?}"), "Redactor { known: 0 }");
        assert_eq!(redactor.redact("hunter2"), "hunter2");
    }

    #[test]
    fn learning_a_value_twice_keeps_one_copy_and_debug_never_shows_it() {
        let redactor = knowing(&[LONG, LONG]);

        let debug = format!("{redactor:?}");
        assert_eq!(debug, "Redactor { known: 1 }");
        assert!(!debug.contains(LONG));
    }

    #[test]
    fn keys_match_after_a_non_alphanumeric_character_in_any_case() {
        let redactor = Redactor::new();
        let cases = [
            ("PASSWORD=a", "PASSWORD=****"),
            ("db_password = a b", "db_password = **** b"),
            ("xpassword=a", "xpassword=a"),
            ("password=a&token=b,c", "password=****&token=****,c"),
            ("{secret:a}", "{secret:****}"),
            ("[api_key=a]", "[api_key=****]"),
            ("requirepass: a;", "requirepass: ****;"),
            ("pgpassword=a", "pgpassword=****"),
            ("mysql_pwd=a", "mysql_pwd=****"),
            ("REDISCLI_AUTH=a", "REDISCLI_AUTH=****"),
            ("masterauth=a", "masterauth=****"),
            ("passwd:a", "passwd:****"),
            ("pwd=a", "pwd=****"),
            ("access_key=a", "access_key=****"),
            ("private_key=a", "private_key=****"),
            ("apikey=a", "apikey=****"),
            ("'token':'a'", "'token':'****'"),
            ("password=", "password="),
            ("password=\"\"", "password=\"\""),
            ("requirepass foo", "requirepass foo"),
            ("tokens=a", "tokens=a"),
        ];

        for (input, expected) in cases {
            assert_eq!(redactor.redact(input), expected, "{input}");
        }
    }

    #[test]
    fn a_quoted_value_without_its_closing_quote_hides_the_rest_of_the_line() {
        let redactor = Redactor::new();

        assert_eq!(
            redactor.redact("password=\"abc def\nnext line"),
            "password=\"****\nnext line"
        );
    }

    #[test]
    fn an_authorization_value_runs_to_the_end_of_its_line_only() {
        let redactor = Redactor::new();

        assert_eq!(
            redactor.redact("Authorization: Bearer abc\nHost: x"),
            "Authorization: ****\nHost: x"
        );
        assert_eq!(
            redactor.redact(r#"{"authorization": "Bearer abc", "user": "x"}"#),
            r#"{"authorization": "****", "user": "x"}"#
        );
    }

    #[test]
    fn uri_passwords_hide_up_to_the_last_at_sign_of_the_authority() {
        let redactor = Redactor::new();
        let cases = [
            ("redis://:pw@127.0.0.1:6379", "redis://:****@127.0.0.1:6379"),
            (
                "nats://u:p@h1 nats://v:q@h2",
                "nats://u:****@h1 nats://v:****@h2",
            ),
            ("x=mysql://root:a@b@c?ssl=1", "x=mysql://root:****@c?ssl=1"),
            ("http://example.com/a@b:c", "http://example.com/a@b:c"),
            ("https://user@host", "https://user@host"),
        ];

        for (input, expected) in cases {
            assert_eq!(redactor.redact(input), expected, "{input}");
        }
    }

    #[test]
    fn text_with_nothing_to_hide_comes_back_borrowed_and_redaction_is_stable() {
        let redactor = knowing(&[LONG, "postgres"]);
        let clean = "instance orders-db is ready on 127.0.0.1:5432";

        assert!(matches!(redactor.redact(clean), Cow::Borrowed(_)));
        for text in [
            format!("{LONG} postgres://a:{LONG}@h password={LONG} token: 'postgres'"),
            "Authorization: Bearer x\npassword=\"y\"".to_owned(),
        ] {
            let once = redactor.redact(&text).into_owned();
            assert!(!once.contains(LONG), "{once}");
            assert_eq!(redactor.redact(&once), once);
        }
    }

    #[test]
    fn the_engine_seam_returns_the_redacted_text() {
        let redactor = knowing(&[LONG]);
        let seam: &dyn Redact = &redactor;

        assert_eq!(seam.redact(&format!("failed: {LONG}")), "failed: ****");
    }
}
