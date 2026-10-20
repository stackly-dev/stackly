//! `providers/catalog-revisions.json`: every catalog revision with its
//! content digest and publish time (spec 0017, *Ledger revision*), the
//! same pattern as `protocols/contract-versions.json`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// One recorded catalog revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Entry {
    /// The catalog revision.
    pub revision: u64,
    /// When it was published, RFC 3339 in UTC.
    pub published_at: jiff::Timestamp,
    /// Its content digest.
    pub content_sha256: String,
}

/// The whole ledger, oldest revision first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Ledger {
    /// Every recorded revision.
    pub revisions: Vec<Entry>,
}

/// What `--update` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    /// The revision was already recorded with the same content.
    Unchanged,
    /// A new revision was appended.
    Added,
    /// A revision not yet on `main` was rewritten (`--amend`).
    Amended,
}

impl Ledger {
    /// Parses the ledger text.
    ///
    /// # Errors
    ///
    /// Fails on JSON that does not decode as the ledger.
    pub fn parse(text: &str) -> Result<Self> {
        serde_json::from_str(text).context("providers/catalog-revisions.json is invalid")
    }

    /// Writes the ledger as pretty JSON with one newline at the end.
    pub fn to_pretty_json(&self) -> Result<String> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        Ok(text)
    }

    /// Returns the entry of a revision.
    pub fn get(&self, revision: u64) -> Option<&Entry> {
        self.revisions
            .iter()
            .find(|entry| entry.revision == revision)
    }

    /// Checks that `current` is recorded exactly.
    ///
    /// # Errors
    ///
    /// Fails with the message that tells the maintainer what to do when the
    /// revision is missing or recorded with another digest or time.
    pub fn check(&self, current: &Entry) -> Result<()> {
        match self.get(current.revision) {
            Some(entry) if entry == current => Ok(()),
            _ => bail!(
                "catalog content changed: bump revision and published_at in providers/catalog.toml, then run cargo xtask catalog check --update"
            ),
        }
    }

    /// Records `current`. A new revision must be above every recorded one
    /// and published no earlier than the last; a recorded revision with
    /// other content is rewritten only with `amend`, and only while it is
    /// not in `on_main`, the ledger of `main`.
    ///
    /// # Errors
    ///
    /// Fails when the rule refuses the change.
    pub fn update(&mut self, current: Entry, amend: bool, on_main: &Self) -> Result<Update> {
        if let Some(index) = self
            .revisions
            .iter()
            .position(|entry| entry.revision == current.revision)
        {
            if self.revisions[index] == current {
                return Ok(Update::Unchanged);
            }
            if !amend {
                bail!(
                    "catalog revision {} is recorded with other content: bump revision and published_at in providers/catalog.toml",
                    current.revision
                );
            }
            if on_main.get(current.revision).is_some() {
                bail!(
                    "catalog revision {} is on main and cannot be amended: bump revision and published_at",
                    current.revision
                );
            }
            self.revisions[index] = current;
            return Ok(Update::Amended);
        }
        if let Some(last) = self.revisions.last() {
            if current.revision <= last.revision {
                bail!(
                    "catalog revision {} must be above the last recorded revision {}",
                    current.revision,
                    last.revision
                );
            }
            if current.published_at < last.published_at {
                bail!(
                    "published_at {} is before the last recorded revision {} ({})",
                    current.published_at,
                    last.revision,
                    last.published_at
                );
            }
        }
        self.revisions.push(current);
        Ok(Update::Added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(revision: u64, at: &str, digest: char) -> Entry {
        Entry {
            revision,
            published_at: at.parse().unwrap(),
            content_sha256: digest.to_string().repeat(64),
        }
    }

    #[test]
    fn a_recorded_revision_checks_only_with_the_same_content() {
        let ledger = Ledger {
            revisions: vec![entry(1, "2026-10-19T00:00:00Z", 'a')],
        };

        assert!(ledger.check(&entry(1, "2026-10-19T00:00:00Z", 'a')).is_ok());
        assert!(
            ledger
                .check(&entry(1, "2026-10-19T00:00:00Z", 'b'))
                .is_err()
        );
        assert!(
            ledger
                .check(&entry(1, "2026-10-20T00:00:00Z", 'a'))
                .is_err()
        );
        assert!(
            ledger
                .check(&entry(2, "2026-10-20T00:00:00Z", 'a'))
                .is_err()
        );
    }

    #[test]
    fn update_adds_a_higher_revision_and_refuses_a_lower_or_earlier_one() {
        let mut ledger = Ledger {
            revisions: vec![entry(2, "2026-10-19T00:00:00Z", 'a')],
        };
        let main = Ledger::default();

        assert!(
            ledger
                .update(entry(1, "2026-10-20T00:00:00Z", 'b'), false, &main)
                .is_err()
        );
        assert!(
            ledger
                .update(entry(3, "2026-10-18T00:00:00Z", 'b'), false, &main)
                .is_err()
        );
        assert_eq!(
            ledger
                .update(entry(3, "2026-10-20T00:00:00Z", 'b'), false, &main)
                .unwrap(),
            Update::Added
        );
        assert_eq!(
            ledger
                .update(entry(3, "2026-10-20T00:00:00Z", 'b'), false, &main)
                .unwrap(),
            Update::Unchanged
        );
        assert_eq!(ledger.revisions.len(), 2);
    }

    #[test]
    fn amend_rewrites_a_revision_only_while_it_is_not_on_main() {
        let mut ledger = Ledger {
            revisions: vec![
                entry(1, "2026-10-19T00:00:00Z", 'a'),
                entry(2, "2026-10-20T00:00:00Z", 'b'),
            ],
        };
        let main = Ledger {
            revisions: vec![entry(1, "2026-10-19T00:00:00Z", 'a')],
        };

        assert!(
            ledger
                .update(entry(2, "2026-10-20T00:00:00Z", 'c'), false, &main)
                .is_err()
        );
        assert_eq!(
            ledger
                .update(entry(2, "2026-10-20T00:00:00Z", 'c'), true, &main)
                .unwrap(),
            Update::Amended
        );
        assert!(
            ledger
                .update(entry(1, "2026-10-19T00:00:00Z", 'd'), true, &main)
                .is_err()
        );
        assert_eq!(ledger.get(2).unwrap().content_sha256, "c".repeat(64));
    }

    #[test]
    fn the_ledger_round_trips_through_its_pretty_json() {
        let ledger = Ledger {
            revisions: vec![entry(1, "2026-10-19T14:05:00Z", 'a')],
        };
        let text = ledger.to_pretty_json().unwrap();

        assert!(text.contains("\"publishedAt\": \"2026-10-19T14:05:00Z\""));
        assert_eq!(Ledger::parse(&text).unwrap(), ledger);
    }
}
