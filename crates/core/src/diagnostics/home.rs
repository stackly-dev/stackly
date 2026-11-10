//! The home folder written `~` (spec 0034, *Penggantian folder home*).
//!
//! `HOME` is read once when the daemon starts, raw and canonical. A `HOME`
//! that is unset, relative, or `/` masks nothing. Each occurrence of either
//! form becomes `~` when the next character is `/`, the end of the text,
//! or anything but a letter, a digit, `.`, `_`, or `-`, and the character
//! before it is not one of those either, so `/Users/ojie` never masks part
//! of `/Users/ojiepermana`. The JSON escaped and percent encoded forms are
//! left as they are.

use std::borrow::Cow;
use std::path::Path;

/// The forms of the home folder to write as `~`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomeMask {
    /// The forms, longest first, so the canonical form wins over a raw one
    /// it starts with.
    forms: Vec<String>,
}

impl HomeMask {
    /// A mask that changes nothing.
    #[must_use]
    pub const fn none() -> Self {
        Self { forms: Vec::new() }
    }

    /// The mask of the home folder `raw`, as `HOME` held it, and
    /// `canonical`, its canonical path. A form that is not absolute UTF-8,
    /// or is `/`, is left out; trailing slashes are dropped.
    #[must_use]
    pub fn new(raw: Option<&Path>, canonical: Option<&Path>) -> Self {
        let mut forms: Vec<String> = [raw, canonical]
            .into_iter()
            .flatten()
            .filter(|path| path.is_absolute())
            .filter_map(Path::to_str)
            .map(|text| text.trim_end_matches('/').to_owned())
            .filter(|text| !text.is_empty())
            .collect();
        forms.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        forms.dedup();
        Self { forms }
    }

    /// Whether the mask changes anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.forms.is_empty()
    }

    /// `text` with every bounded occurrence of the home folder written `~`.
    #[must_use]
    pub fn mask<'t>(&self, text: &'t str) -> Cow<'t, str> {
        if self.forms.is_empty() || !self.forms.iter().any(|form| text.contains(form.as_str())) {
            return Cow::Borrowed(text);
        }
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        let mut copied = 0;
        while at < text.len() {
            let found = self.forms.iter().find(|form| {
                text[at..].starts_with(form.as_str())
                    && !text[..at].chars().next_back().is_some_and(is_name_char)
                    && !text[at + form.len()..]
                        .chars()
                        .next()
                        .is_some_and(is_name_char)
            });
            if let Some(form) = found {
                out.push_str(&text[copied..at]);
                out.push('~');
                at += form.len();
                copied = at;
            } else {
                at += text[at..].chars().next().map_or(1, char::len_utf8);
            }
        }
        out.push_str(&text[copied..]);
        Cow::Owned(out)
    }
}

/// A character that continues a folder name: a letter, a digit, `.`, `_`,
/// or `-`.
fn is_name_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(raw: &str, canonical: Option<&str>) -> HomeMask {
        HomeMask::new(Some(Path::new(raw)), canonical.map(Path::new))
    }

    #[test]
    fn the_home_folder_is_masked_only_at_a_name_boundary() {
        let home = mask("/Users/ojie", None);
        assert_eq!(home.mask("/Users/ojie/Library/x"), "~/Library/x");
        assert_eq!(home.mask("/Users/ojie"), "~");
        assert_eq!(home.mask("at /Users/ojie: gone"), "at ~: gone");
        assert_eq!(home.mask("\"/Users/ojie\""), "\"~\"");
        assert_eq!(home.mask("file:///Users/ojie/x"), "file://~/x");
        assert_eq!(home.mask("/Users/ojiepermana/x"), "/Users/ojiepermana/x");
        assert_eq!(home.mask("/Users/ojie.old/x"), "/Users/ojie.old/x");
        assert_eq!(home.mask("/Users/ojie_x"), "/Users/ojie_x");
        assert_eq!(home.mask("/Users/ojie-2"), "/Users/ojie-2");
        assert_eq!(
            home.mask("x/Users/ojie/y"),
            "x/Users/ojie/y",
            "inside a longer name"
        );
        assert_eq!(home.mask("a /Users/ojie b:/Users/ojie/c"), "a ~ b:~/c");
    }

    #[test]
    fn encoded_forms_are_left_alone() {
        let home = mask("/Users/ojie", None);
        assert_eq!(home.mask("%2FUsers%2Fojie"), "%2FUsers%2Fojie");
        assert_eq!(home.mask("\\/Users\\/ojie"), "\\/Users\\/ojie");
    }

    #[test]
    fn the_raw_and_the_canonical_form_are_both_masked() {
        let home = mask("/var/home/ojie", Some("/private/var/home/ojie"));
        assert_eq!(
            home.mask("/private/var/home/ojie/a and /var/home/ojie/b"),
            "~/a and ~/b"
        );
    }

    #[test]
    fn an_unset_relative_or_root_home_masks_nothing() {
        assert!(HomeMask::new(None, None).is_empty());
        assert!(mask("relative/home", None).is_empty());
        assert!(mask("/", None).is_empty());
        assert_eq!(mask("/", None).mask("/Users/x"), "/Users/x");
        assert!(HomeMask::none().mask("/Users/x").as_ref() == "/Users/x");
    }

    #[test]
    fn a_trailing_slash_of_home_is_dropped() {
        let home = mask("/Users/ojie/", None);
        assert_eq!(home.mask("/Users/ojie/x"), "~/x");
    }

    #[test]
    fn text_without_the_home_folder_is_borrowed() {
        let home = mask("/Users/ojie", None);
        assert!(matches!(home.mask("nothing here ü"), Cow::Borrowed(_)));
    }
}
