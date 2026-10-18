//! `cargo xtask evidence new`: starts a manual record (spec 0002, "Record
//! manual").
//!
//! It writes `evidence/manual/<yyyy-mm-dd>-<slug>.json` as `NOT_RUN`, with
//! method `manual`, `evidencePath` pointing at the file itself, and the
//! context of the current checkout. The reviewer fills in the result later.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use stackly_evidence::{EvidenceRecord, Kind, RunContext, Source, Subject, write_json_atomic};

use super::check::MANUAL_DIR;
use super::context;

/// The longest slug taken from a title.
const SLUG_MAX: usize = 48;

const USAGE: &str = "cargo xtask evidence new --covers <ID>... --kind <fixture|native> --title <title> --expected <sentence> [--subject <provider>@<version>]...";

/// Parsed arguments of `evidence new`.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    covers: Vec<String>,
    kind: Kind,
    title: String,
    expected: String,
    subjects: Vec<(String, String)>,
}

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    let options = parse(args)?;
    let context = context::probe_manual(root)?;
    let relative = write(root, options, context, Timestamp::now())?;
    println!("{relative}");
    eprintln!(
        "evidence new: wrote a NOT_RUN manual record; do the steps on this commit, then fill in steps, actual, status, reviewer, and finishedAt"
    );
    Ok(ExitCode::SUCCESS)
}

/// Writes the manual record and returns its path from the repository root.
fn write(
    root: &Path,
    options: Options,
    context: RunContext,
    started_at: Timestamp,
) -> Result<String> {
    let slug = slug(&options.title);
    if slug.is_empty() {
        bail!("--title needs at least one ASCII letter or digit for the file name");
    }
    let dir = root.join(MANUAL_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("cannot create {MANUAL_DIR}"))?;
    let name = free_name(&dir, &started_at.strftime("%Y-%m-%d").to_string(), &slug);
    let relative = format!("{MANUAL_DIR}/{name}");

    let mut record = EvidenceRecord::manual(
        options.covers,
        options.kind,
        options.title,
        options.expected,
        started_at,
        relative.clone(),
    );
    record.subjects = options
        .subjects
        .into_iter()
        .map(|(provider, version)| Subject {
            provider,
            version,
            digest: None,
            source: Source::Managed,
        })
        .collect();
    record
        .validate()
        .context("the manual record breaks a rule")?;
    record.context = Some(context);
    write_json_atomic(&dir.join(&name), &record)
        .with_context(|| format!("cannot write {relative}"))?;
    Ok(relative)
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut covers = Vec::new();
    let mut kind = None;
    let mut title = None;
    let mut expected = None;
    let mut subjects = Vec::new();
    let mut rest = args.iter().peekable();
    while let Some(&flag) = rest.next() {
        match flag {
            "--covers" => {
                let before = covers.len();
                while let Some(&&id) = rest.peek().filter(|arg| !arg.starts_with("--")) {
                    covers.push(id.to_owned());
                    rest.next();
                }
                if covers.len() == before {
                    bail!("--covers needs at least one PRD ID\n\nUsage: {USAGE}");
                }
            }
            "--kind" => {
                let value = rest.next().context("--kind needs `fixture` or `native`")?;
                kind = Some(Kind::parse(value).with_context(|| {
                    format!("--kind must be `fixture` or `native`, found `{value}`")
                })?);
            }
            "--title" => title = Some(value(&mut rest, "--title", "a title")?),
            "--expected" => {
                expected = Some(value(&mut rest, "--expected", "one sentence of behavior")?);
            }
            "--subject" => {
                let subject = value(&mut rest, "--subject", "<provider>@<version>")?;
                let (provider, version) = subject
                    .split_once('@')
                    .filter(|(provider, version)| !provider.is_empty() && !version.is_empty())
                    .with_context(|| {
                        format!("--subject must be <provider>@<version>, found `{subject}`")
                    })?;
                subjects.push((provider.to_owned(), version.to_owned()));
            }
            other => bail!("unexpected argument `{other}`\n\nUsage: {USAGE}"),
        }
    }
    if covers.is_empty() {
        bail!("--covers is required\n\nUsage: {USAGE}");
    }
    Ok(Options {
        covers,
        kind: kind.with_context(|| {
            format!(
                "--kind fixture or --kind native is required; there is no default\n\nUsage: {USAGE}"
            )
        })?,
        title: title.with_context(|| format!("--title is required\n\nUsage: {USAGE}"))?,
        expected: expected.with_context(|| format!("--expected is required\n\nUsage: {USAGE}"))?,
        subjects,
    })
}

/// The value after `flag`, which must not be empty.
fn value<'a>(
    rest: &mut impl Iterator<Item = &'a &'a str>,
    flag: &str,
    what: &str,
) -> Result<String> {
    let value = rest
        .next()
        .with_context(|| format!("{flag} needs {what}"))?;
    if value.trim().is_empty() {
        bail!("{flag} needs {what}, found an empty value");
    }
    Ok((*value).to_owned())
}

/// The title in lowercase, with every run of characters other than ASCII
/// letters and digits turned into one `-`, trimmed of `-` at both ends, and
/// cut to [`SLUG_MAX`] characters.
fn slug(title: &str) -> String {
    let mut slug = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.truncate(SLUG_MAX);
    slug.trim_end_matches('-').to_owned()
}

/// `<date>-<slug>.json`, or with `-2`, `-3`, and so on when that file
/// already exists in `dir`.
fn free_name(dir: &Path, date: &str, slug: &str) -> String {
    let mut name = format!("{date}-{slug}.json");
    let mut n = 2;
    while dir.join(&name).exists() {
        name = format!("{date}-{slug}-{n}.json");
        n += 1;
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_take_several_ids_and_repeated_subjects() {
        let options = parse(&[
            "--covers",
            "AC-019",
            "NFR-005",
            "--kind",
            "native",
            "--title",
            "PostgreSQL 18 starts from the GUI",
            "--expected",
            "a managed instance reaches ready",
            "--subject",
            "postgresql@18.1",
            "--subject",
            "valkey@9.0.1",
        ])
        .unwrap();
        assert_eq!(options.covers, ["AC-019", "NFR-005"]);
        assert_eq!(options.kind, Kind::Native);
        assert_eq!(
            options.subjects,
            [
                ("postgresql".to_owned(), "18.1".to_owned()),
                ("valkey".to_owned(), "9.0.1".to_owned())
            ]
        );
    }

    #[test]
    fn kind_has_no_default_and_bad_values_are_refused() {
        let base = ["--covers", "AC-001", "--title", "t", "--expected", "e"];
        assert!(
            parse(&base)
                .unwrap_err()
                .to_string()
                .contains("there is no default")
        );
        let mut bad_subject = base.to_vec();
        bad_subject.extend(["--kind", "fixture", "--subject", "postgresql"]);
        assert!(parse(&bad_subject).is_err());
        let mut no_ids = vec!["--covers", "--kind", "fixture"];
        no_ids.extend(&base[2..]);
        assert!(parse(&no_ids).is_err());
        assert!(parse(&["--kind", "ci"]).is_err());
    }

    #[test]
    fn slug_lowercases_joins_runs_and_stops_at_48_characters() {
        assert_eq!(
            slug("Menu bar shows a stopped instance!"),
            "menu-bar-shows-a-stopped-instance"
        );
        assert_eq!(
            slug("  PostgreSQL 18 / Valkey 9.0  "),
            "postgresql-18-valkey-9-0"
        );
        assert_eq!(slug("Café über alles"), "caf-ber-alles");
        assert_eq!(slug("§§§"), "");
        let long = slug(&"word ".repeat(20));
        assert!(long.len() <= SLUG_MAX);
        assert!(!long.ends_with('-'));
    }

    #[test]
    fn an_existing_file_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            free_name(dir.path(), "2026-10-18", "x"),
            "2026-10-18-x.json"
        );
        fs::write(dir.path().join("2026-10-18-x.json"), "{}").unwrap();
        fs::write(dir.path().join("2026-10-18-x-2.json"), "{}").unwrap();
        assert_eq!(
            free_name(dir.path(), "2026-10-18", "x"),
            "2026-10-18-x-3.json"
        );
    }

    fn options(covers: &[&str], title: &str) -> Options {
        Options {
            covers: covers.iter().map(|id| (*id).to_owned()).collect(),
            kind: Kind::Native,
            title: title.to_owned(),
            expected: "a managed instance reaches ready after Start".to_owned(),
            subjects: vec![("postgresql".to_owned(), "18.1".to_owned())],
        }
    }

    fn context() -> RunContext {
        RunContext::new(
            "bc87d45",
            false,
            stackly_evidence::Host::new("macos", "aarch64"),
            stackly_evidence::Toolchain::default(),
        )
    }

    #[test]
    fn evidence_new_writes_a_valid_not_run_record_that_points_at_itself() {
        let dir = tempfile::tempdir().unwrap();
        let started_at: Timestamp = "2026-10-18T23:59:59Z".parse().unwrap();
        let title = "PostgreSQL 18 starts from the GUI";
        let relative = write(
            dir.path(),
            options(&["AC-019"], title),
            context(),
            started_at,
        )
        .unwrap();
        assert_eq!(
            relative,
            "evidence/manual/2026-10-18-postgresql-18-starts-from-the-gui.json"
        );

        let bytes = fs::read(dir.path().join(&relative)).unwrap();
        let record: EvidenceRecord = serde_json::from_slice(&bytes).unwrap();
        assert!(record.validate().is_ok());
        assert_eq!(record.status, stackly_evidence::Status::NotRun);
        assert_eq!(record.evidence_path, relative);
        assert_eq!(record.test.name, title);
        assert_eq!(record.subjects[0].source, Source::Managed);
        assert_eq!(record.subjects[0].digest, None);
        assert_eq!(record.context, Some(context()));

        let again = write(
            dir.path(),
            options(&["AC-019"], title),
            context(),
            started_at,
        )
        .unwrap();
        assert!(again.ends_with("-gui-2.json"));
    }

    #[test]
    fn evidence_new_refuses_bad_ids_and_selector_versions_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let now = Timestamp::now();
        let err = write(dir.path(), options(&["OQ-011"], "t"), context(), now).unwrap_err();
        assert!(format!("{err:#}").contains("covers entry `OQ-011` is not a PRD ID"));

        let mut selector = options(&["AC-019"], "t");
        selector.subjects = vec![("postgresql".to_owned(), "18".to_owned())];
        assert!(write(dir.path(), selector, context(), now).is_err());
        assert!(write(dir.path(), options(&["AC-019"], "§§"), context(), now).is_err());
        assert_eq!(dir.path().join(MANUAL_DIR).read_dir().unwrap().count(), 0);
    }
}
