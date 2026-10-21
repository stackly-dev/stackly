//! `cargo xtask snapshot review [--base <rev>]`: a local page that shows, side by side,
//! the snapshot PNGs that changed since the merge base (spec 0068).
//!
//! The page lists every PNG under a `__Snapshots__` folder that was added, changed, or
//! deleted between `git merge-base <rev> HEAD` and the working tree, with the old image
//! from the merge base and the new one. The failure artifacts of the last snapshot run
//! (`target/snapshot-failures/`) are shown beside the reference they belong to.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};

/// The folder of the page, under the repository's `target/`.
const REVIEW_DIR: &str = "target/snapshot-review";
/// Where the snapshot helper writes the `actual` and `diff` images of a failed check.
const FAILURES_DIR: &str = "target/snapshot-failures";
/// The folder name every reference lives under.
const SNAPSHOT_FOLDER: &str = "__Snapshots__";

const USAGE: &str = "usage: cargo xtask snapshot review [--base <rev>]";

/// Runs `cargo xtask snapshot <args>`.
///
/// # Errors
///
/// Fails on an unknown subcommand, an unknown `--base` revision (with git's message), or
/// when a file cannot be read or written.
pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["review"] => review(root, "main"),
        ["review", "--base", base] => review(root, base),
        _ => bail!("unknown snapshot task `{}`\n{USAGE}", args.join(" ")),
    }
}

fn review(root: &Path, base: &str) -> Result<ExitCode> {
    match build_page(root, base)? {
        Outcome::NoChanges => println!("no snapshot changes"),
        Outcome::Page(index) => println!("{}", index.display()),
    }
    Ok(ExitCode::SUCCESS)
}

/// What a review produced.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// No snapshot PNG changed since the merge base.
    NoChanges,
    /// The absolute path of `index.html`.
    Page(PathBuf),
}

/// How a snapshot file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Added,
    Modified,
    Deleted,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "changed",
            Self::Deleted => "deleted",
        }
    }
}

/// One changed snapshot PNG, by its path from the repository root.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    path: String,
    status: Status,
}

fn build_page(root: &Path, base: &str) -> Result<Outcome> {
    let merge_base = git_text(root, &["merge-base", base, "HEAD"])?;
    let merge_base = merge_base.trim();
    let diff = git_bytes(
        root,
        &["diff", "--name-status", "--no-renames", "-z", merge_base],
    )?;
    let untracked = git_bytes(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let changes = merge_changes(parse_name_status(&diff), parse_untracked(&untracked));
    if changes.is_empty() {
        return Ok(Outcome::NoChanges);
    }

    let review = root.join(REVIEW_DIR);
    if review.exists() {
        fs::remove_dir_all(&review)
            .with_context(|| format!("cannot clear {}", review.display()))?;
    }
    fs::create_dir_all(&review)?;
    let mut rows = Vec::new();
    for change in changes {
        rows.push(mirror(root, &review, merge_base, change)?);
    }
    let index = review.join("index.html");
    fs::write(&index, render_index(base, &rows))
        .with_context(|| format!("cannot write {}", index.display()))?;
    Ok(Outcome::Page(index.canonicalize().unwrap_or(index)))
}

/// A change with the images the page shows for it, as paths relative to `index.html`.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    change: Change,
    base: Option<String>,
    current: Option<String>,
    actual: Option<String>,
    diff: Option<String>,
}

/// Copies the images of `change` under `review` and returns the row that points at them.
fn mirror(root: &Path, review: &Path, merge_base: &str, change: Change) -> Result<Row> {
    let mut row = Row {
        base: None,
        current: None,
        actual: None,
        diff: None,
        change: change.clone(),
    };
    if change.status != Status::Added {
        let bytes = git_bytes(root, &["show", &format!("{merge_base}:{}", change.path)])?;
        let relative = format!("base/{}", change.path);
        write_under(review, &relative, &bytes)?;
        row.base = Some(relative);
    }
    if change.status != Status::Deleted {
        let bytes = fs::read(root.join(&change.path))
            .with_context(|| format!("cannot read {}", change.path))?;
        let relative = format!("current/{}", change.path);
        write_under(review, &relative, &bytes)?;
        row.current = Some(relative);
    }
    if let Some((actual, diff)) = failure_paths(&change.path) {
        for (kind, source, slot) in [
            ("actual", actual, &mut row.actual),
            ("diff", diff, &mut row.diff),
        ] {
            if let Ok(bytes) = fs::read(root.join(&source)) {
                let relative = format!("{kind}/{}", change.path);
                write_under(review, &relative, &bytes)?;
                *slot = Some(relative);
            }
        }
    }
    Ok(row)
}

fn write_under(review: &Path, relative: &str, bytes: &[u8]) -> Result<()> {
    let target = review.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&target, bytes).with_context(|| format!("cannot write {}", target.display()))
}

/// The paths, from the repository root, of the `actual` and `diff` artifacts of the
/// reference `path`: `…/__Snapshots__/<Suite>/<name>.<variant>.png` becomes
/// `target/snapshot-failures/<Suite>/<name>.<variant>.{actual,diff}.png`.
fn failure_paths(path: &str) -> Option<(String, String)> {
    let (_, rest) = path.split_once(&format!("{SNAPSHOT_FOLDER}/"))?;
    let stem = rest.strip_suffix(".png")?;
    Some((
        format!("{FAILURES_DIR}/{stem}.actual.png"),
        format!("{FAILURES_DIR}/{stem}.diff.png"),
    ))
}

/// Whether `path` is a PNG under a `__Snapshots__` folder.
fn is_snapshot(path: &str) -> bool {
    path.ends_with(".png")
        && (path.starts_with(&format!("{SNAPSHOT_FOLDER}/"))
            || path.contains(&format!("/{SNAPSHOT_FOLDER}/")))
}

/// The snapshot changes of `git diff --name-status --no-renames -z`: `A`, `M`, `D`, and
/// the type change `T` (shown as changed), each with one path.
fn parse_name_status(output: &[u8]) -> Vec<Change> {
    let mut fields = output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut changes = Vec::new();
    while let (Some(code), Some(path)) = (fields.next(), fields.next()) {
        let status = match code.first() {
            Some(b'A') => Status::Added,
            Some(b'D') => Status::Deleted,
            Some(b'M' | b'T') => Status::Modified,
            _ => continue,
        };
        let path = String::from_utf8_lossy(path).into_owned();
        if is_snapshot(&path) {
            changes.push(Change { path, status });
        }
    }
    changes
}

/// The untracked snapshot files of `git ls-files --others -z`, all added.
fn parse_untracked(output: &[u8]) -> Vec<Change> {
    output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .filter(|path| is_snapshot(path))
        .map(|path| Change {
            path,
            status: Status::Added,
        })
        .collect()
}

/// Both lists in one, ordered by path, one entry per path.
fn merge_changes(mut tracked: Vec<Change>, untracked: Vec<Change>) -> Vec<Change> {
    tracked.extend(untracked);
    tracked.sort_by(|a, b| a.path.cmp(&b.path));
    tracked.dedup_by(|a, b| a.path == b.path);
    tracked
}

/// Escapes text for HTML element content and double quoted attributes.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Percent encodes everything in a relative path except `/` and unreserved characters.
fn url_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn render_index(base: &str, rows: &[Row]) -> String {
    let mut html = String::new();
    let _ = write!(
        html,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>Snapshot review</title>\n\
         <style>body{{font:14px system-ui;margin:24px}}table{{border-collapse:collapse}}\
         td,th{{border:1px solid #8886;padding:8px;vertical-align:top}}\
         img{{max-width:420px;height:auto;display:block;background:#8883}}\
         code{{font-size:12px}}</style>\n</head>\n<body>\n\
         <h1>Snapshot review</h1>\n<p>{} changed since the merge base with <code>{}</code>.</p>\n\
         <table>\n<tr><th>File</th><th>Before</th><th>After</th><th>Last failure</th></tr>\n",
        rows.len(),
        escape_html(base)
    );
    for row in rows {
        let _ = write!(
            html,
            "<tr>\n<td><code>{}</code><br>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}{}</td>\n</tr>\n",
            escape_html(&row.change.path),
            row.change.status.label(),
            image_cell(row.base.as_deref(), "before"),
            image_cell(row.current.as_deref(), "after"),
            image_cell(row.actual.as_deref(), "actual"),
            image_cell(row.diff.as_deref(), "diff"),
        );
    }
    html.push_str("</table>\n</body>\n</html>\n");
    html
}

fn image_cell(relative: Option<&str>, alt: &str) -> String {
    match relative {
        Some(path) => format!("<img src=\"{}\" alt=\"{alt}\">", url_path(path)),
        None => String::new(),
    }
}

fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .context("cannot run `git`")?;
    if !output.status.success() {
        bail!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&git_bytes(root, args)?).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, status: Status) -> Change {
        Change {
            path: path.to_string(),
            status,
        }
    }

    const PNG: &str = "apps/App/Tests/__Snapshots__/Suite/page.light.png";

    #[test]
    fn only_pngs_under_a_snapshots_folder_count() {
        assert!(is_snapshot(PNG));
        assert!(is_snapshot("__Snapshots__/Suite/a.png"));
        assert!(!is_snapshot("apps/App/Tests/Suite/page.light.png"));
        assert!(!is_snapshot("apps/App/Tests/__Snapshots__/Suite/notes.txt"));
        assert!(!is_snapshot("apps/App/Tests/NotSnapshots/a.png"));
    }

    #[test]
    fn name_status_output_gives_added_changed_and_deleted_snapshots_only() {
        let output = format!("A\0{PNG}\0M\0{PNG}2\0D\0x/__Snapshots__/S/gone.png\0M\0src/lib.rs\0");
        assert_eq!(
            parse_name_status(output.as_bytes()),
            vec![
                change(PNG, Status::Added),
                change(&format!("{PNG}2"), Status::Modified),
                change("x/__Snapshots__/S/gone.png", Status::Deleted),
            ]
            .into_iter()
            .filter(|c| is_snapshot(&c.path))
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn untracked_files_are_added_and_merged_in_path_order_without_duplicates() {
        let untracked = parse_untracked(
            "b/__Snapshots__/S/new.png\0a/__Snapshots__/S/x.png\0notes.md\0".as_bytes(),
        );
        assert_eq!(untracked.len(), 2);
        let tracked = vec![
            change("a/__Snapshots__/S/x.png", Status::Modified),
            change("c/__Snapshots__/S/z.png", Status::Deleted),
        ];
        let merged = merge_changes(tracked, untracked);
        let paths: Vec<_> = merged.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "a/__Snapshots__/S/x.png",
                "b/__Snapshots__/S/new.png",
                "c/__Snapshots__/S/z.png"
            ]
        );
        assert_eq!(merged[0].status, Status::Modified, "tracked wins");
    }

    #[test]
    fn failure_artifacts_follow_the_suite_folder_and_the_reference_name() {
        assert_eq!(
            failure_paths(PNG),
            Some((
                "target/snapshot-failures/Suite/page.light.actual.png".to_string(),
                "target/snapshot-failures/Suite/page.light.diff.png".to_string()
            ))
        );
        assert_eq!(failure_paths("plain.png"), None);
    }

    #[test]
    fn html_escaping_covers_markup_and_quotes_and_urls_encode_spaces() {
        assert_eq!(
            escape_html("<a href=\"x\">&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;"
        );
        assert_eq!(url_path("base/a b/é#1.png"), "base/a%20b/%C3%A9%231.png");
    }

    #[test]
    fn the_page_has_one_row_per_file_with_escaped_paths_and_relative_images() {
        let rows = vec![Row {
            change: change("x/__Snapshots__/<S>/a&b.png", Status::Modified),
            base: Some("base/x/a&b.png".to_string()),
            current: Some("current/x/a b.png".to_string()),
            actual: None,
            diff: None,
        }];
        let html = render_index("main", &rows);
        assert!(html.contains("<code>x/__Snapshots__/&lt;S&gt;/a&amp;b.png</code>"));
        assert!(html.contains("src=\"base/x/a%26b.png\""));
        assert!(html.contains("src=\"current/x/a%20b.png\""));
        assert_eq!(html.matches("<tr>\n<td>").count(), 1);
        assert!(!html.contains("alt=\"actual\""));
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(root)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn write(root: &Path, path: &str, bytes: &[u8]) {
        let target = root.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }

    fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q", "-b", "main"]);
        write(root.path(), "T/__Snapshots__/S/keep.light.png", b"keep");
        write(root.path(), "T/__Snapshots__/S/edit.light.png", b"old");
        write(root.path(), "T/__Snapshots__/S/gone.light.png", b"gone");
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-q", "-m", "base"]);
        git(root.path(), &["checkout", "-q", "-b", "work"]);
        root
    }

    #[test]
    fn without_changes_the_review_says_so_and_makes_no_page() {
        let root = repository();
        assert_eq!(build_page(root.path(), "main").unwrap(), Outcome::NoChanges);
        assert!(!root.path().join(REVIEW_DIR).exists());
    }

    #[test]
    fn the_review_mirrors_old_and_new_images_for_changed_added_and_deleted_files() {
        let root = repository();
        let r = root.path();
        write(r, "T/__Snapshots__/S/edit.light.png", b"new");
        fs::remove_file(r.join("T/__Snapshots__/S/gone.light.png")).unwrap();
        write(r, "T/__Snapshots__/S/fresh.light.png", b"fresh");
        write(r, "T/__Snapshots__/S/edit.light.png.txt", b"not a png");
        write(
            r,
            "target/snapshot-failures/S/edit.light.actual.png",
            b"actual",
        );
        // A commit after the branch point counts too: the merge base is the fixed point.
        git(r, &["add", "T/__Snapshots__/S/edit.light.png"]);
        git(r, &["commit", "-q", "-m", "edit"]);

        let Outcome::Page(index) = build_page(r, "main").unwrap() else {
            panic!("a page was expected");
        };
        assert!(index.is_absolute() && index.ends_with("index.html"));
        let review = r.join(REVIEW_DIR);
        assert_eq!(
            fs::read(review.join("base/T/__Snapshots__/S/edit.light.png")).unwrap(),
            b"old"
        );
        assert_eq!(
            fs::read(review.join("current/T/__Snapshots__/S/edit.light.png")).unwrap(),
            b"new"
        );
        assert_eq!(
            fs::read(review.join("actual/T/__Snapshots__/S/edit.light.png")).unwrap(),
            b"actual"
        );
        assert!(
            review
                .join("base/T/__Snapshots__/S/gone.light.png")
                .exists()
        );
        assert!(
            !review
                .join("current/T/__Snapshots__/S/gone.light.png")
                .exists()
        );
        assert!(
            !review
                .join("base/T/__Snapshots__/S/fresh.light.png")
                .exists()
        );
        assert!(
            review
                .join("current/T/__Snapshots__/S/fresh.light.png")
                .exists()
        );
        assert!(
            !review
                .join("current/T/__Snapshots__/S/keep.light.png")
                .exists()
        );
        let html = fs::read_to_string(index).unwrap();
        let edit = html.find("edit.light.png").unwrap();
        let fresh = html.find("fresh.light.png").unwrap();
        let gone = html.find("gone.light.png").unwrap();
        assert!(edit < fresh && fresh < gone, "rows are ordered by path");
    }

    #[test]
    fn an_unknown_revision_fails_with_the_message_of_git() {
        let root = repository();
        let error = build_page(root.path(), "no-such-rev").unwrap_err();
        assert!(format!("{error:#}").contains("git merge-base no-such-rev HEAD"));
    }

    #[test]
    fn unknown_subcommands_are_refused() {
        let root = repository();
        assert!(run(root.path(), &["review", "--base"]).is_err());
        assert!(run(root.path(), &["prune"]).is_err());
    }
}
