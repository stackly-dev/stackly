//! `providers/catalog.toml`, schema `stackly.catalog-source/v1`: the
//! curated list of the bundled catalog (spec 0017, *Sumber catalog di
//! repo*). Only this tool reads it; the daemon reads `catalog.json`.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use stackly_core::catalog::{IndexRevoked, SourceNote, SourcePlanned, SourceProvider, SourceSpec};

/// The only `schema` of `catalog.toml`.
pub const SOURCE_SCHEMA: &str = "stackly.catalog-source/v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSource {
    schema: String,
    revision: u64,
    published_at: toml::value::Datetime,
    #[serde(default)]
    provider: Vec<RawProvider>,
    #[serde(default)]
    planned: Vec<RawPlanned>,
    #[serde(default)]
    revoked: Vec<RawRevoked>,
}

/// One `[[revoked]]` (spec 0037, *Revocation di catalog*).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRevoked {
    id: String,
    revision: String,
    manifest_sha256: Option<String>,
    reason: String,
    since: toml::value::Datetime,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    id: String,
    mark: Option<String>,
    tile: Option<String>,
    #[serde(default)]
    platform_note: Vec<RawNote>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNote {
    os: String,
    status: String,
    note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlanned {
    id: String,
    display_name: String,
    category: String,
    description: String,
    note: Option<String>,
    mark: Option<String>,
    tile: Option<String>,
}

/// Parses the text of `catalog.toml` into the source the core assembles.
///
/// # Errors
///
/// Fails on TOML that does not parse, an unknown field, another `schema`,
/// or a `published_at` without a time zone offset.
pub fn parse(text: &str) -> Result<SourceSpec> {
    let raw: RawSource = toml::from_str(text).context("providers/catalog.toml is invalid")?;
    if raw.schema != SOURCE_SCHEMA {
        bail!(
            "providers/catalog.toml: schema must be {SOURCE_SCHEMA}, not {}",
            raw.schema
        );
    }
    let published_at = raw
        .published_at
        .to_string()
        .parse::<jiff::Timestamp>()
        .with_context(|| {
            format!(
                "providers/catalog.toml: published_at {} needs a date, a time, and an offset",
                raw.published_at
            )
        })?;
    let revoked = raw
        .revoked
        .into_iter()
        .map(|entry| {
            let since = match (entry.since.date, entry.since.time, entry.since.offset) {
                (Some(date), None, None) => jiff::civil::Date::new(
                    i16::try_from(date.year).unwrap_or(i16::MAX),
                    i8::try_from(date.month).unwrap_or(i8::MAX),
                    i8::try_from(date.day).unwrap_or(i8::MAX),
                )
                .ok(),
                _ => None,
            }
            .with_context(|| {
                format!(
                    "providers/catalog.toml: revoked {} since {} must be a date without a time",
                    entry.id, entry.since
                )
            })?;
            Ok(IndexRevoked {
                id: entry.id,
                revision: entry.revision,
                manifest_sha256: entry.manifest_sha256,
                reason: entry.reason,
                since,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(SourceSpec {
        revoked,
        revision: raw.revision,
        published_at,
        providers: raw
            .provider
            .into_iter()
            .map(|provider| SourceProvider {
                id: provider.id,
                mark: provider.mark,
                tile: provider.tile,
                platform_notes: provider
                    .platform_note
                    .into_iter()
                    .map(|note| SourceNote {
                        os: note.os,
                        status: note.status,
                        note: note.note,
                    })
                    .collect(),
            })
            .collect(),
        planned: raw
            .planned
            .into_iter()
            .map(|planned| SourcePlanned {
                id: planned.id,
                display_name: planned.display_name,
                category: planned.category,
                description: planned.description,
                note: planned.note,
                mark: planned.mark,
                tile: planned.tile,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
schema = "stackly.catalog-source/v1"
revision = 2
published_at = 2026-10-19T14:05:00+07:00

[[provider]]
id = "official/nats"
mark = "Na"
tile = "green"

[[provider.platform_note]]
os = "windows"
status = "planned"
note = "Windows releases exist upstream."

[[planned]]
id = "official/mysql"
display_name = "MySQL"
category = "database"
description = "Relational server."
"#;

    #[test]
    fn a_source_parses_with_its_offset_in_utc() {
        let spec = parse(SOURCE).unwrap();

        assert_eq!(spec.revision, 2);
        assert_eq!(spec.published_at.to_string(), "2026-10-19T07:05:00Z");
        assert_eq!(spec.providers[0].platform_notes[0].os, "windows");
        assert_eq!(spec.planned[0].mark, None);
    }

    // covers: AC-8 (spec 0037)
    #[test]
    fn a_revoked_entry_parses_with_its_date_and_needs_a_plain_date() {
        let revoked = format!(
            "{SOURCE}\n[[revoked]]\nid = \"official/nats\"\nrevision = \"1.0.0\"\nreason = \"Insecure default.\"\nsince = 2026-10-01\n"
        );
        let spec = parse(&revoked).unwrap();
        assert_eq!(spec.revoked[0].since.to_string(), "2026-10-01");
        assert_eq!(spec.revoked[0].manifest_sha256, None);

        assert!(
            parse(&revoked.replace("since = 2026-10-01", "since = 2026-10-01T00:00:00Z")).is_err()
        );
        assert!(parse(&revoked.replace("reason =", "reasons =")).is_err());
        assert!(parse(SOURCE).unwrap().revoked.is_empty());
    }

    #[test]
    fn unknown_fields_another_schema_and_local_times_are_refused() {
        assert!(parse(&SOURCE.replace("mark = \"Na\"", "markk = \"Na\"")).is_err());
        assert!(parse(&SOURCE.replace("catalog-source/v1", "catalog-source/v2")).is_err());
        assert!(
            parse(&SOURCE.replace("2026-10-19T14:05:00+07:00", "2026-10-19T14:05:00")).is_err()
        );
        assert!(parse(&SOURCE.replace("2026-10-19T14:05:00+07:00", "2026-10-19")).is_err());
    }
}
