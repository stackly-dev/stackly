//! `cargo xtask provider new <name> [--out <dir>]` (spec 0036, AC-7,
//! *Scaffold*): copies the example package `providers/example/` into
//! `<dir>/<name>/` (default `providers/<name>/`) as `testing/<name>`.
//!
//! The copy holds every file `read_package` reads from the example, plus
//! `tests/scenarios.yaml` and `README.md`, with exactly three text
//! replacements: the `id:` line, the `displayName:` line, and the README
//! title. Each must match exactly once, so the example and the scaffold
//! never drift apart. The copy is written into a temporary folder next to
//! the destination and checked with `check_package`; only a publishable
//! package is renamed into place, and anything else leaves no file behind.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

/// The name the example package has, which a scaffold may not take.
const EXAMPLE: &str = "example";
/// The files of the example the scaffold copies besides the ones
/// `read_package` reads.
const EXTRA_FILES: [&str; 2] = ["tests/scenarios.yaml", "README.md"];
/// The rule a scaffold name follows: the name part of `ProviderName`.
const NAME_RULE: &str = "the name must be 1 to 63 characters: a lowercase letter, then lowercase letters, digits, and single hyphens";

/// One whole line the scaffold replaces in one file.
struct Replacement {
    /// The package file, relative to the package folder.
    file: &'static str,
    /// The line in the example.
    from: String,
    /// The line in the scaffold.
    to: String,
}

/// The three replacements of *Scaffold* step 3 for `name`.
fn replacements(name: &str) -> [Replacement; 3] {
    let title = capitalized(name);
    [
        Replacement {
            file: "provider.yaml",
            from: format!("  id: testing/{EXAMPLE}"),
            to: format!("  id: testing/{name}"),
        },
        Replacement {
            file: "provider.yaml",
            from: "  displayName: Example".to_owned(),
            to: format!("  displayName: {title}"),
        },
        Replacement {
            file: "README.md",
            from: "# Example provider".to_owned(),
            to: format!("# {title} provider"),
        },
    ]
}

/// `name` with its first letter in upper case.
fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_ascii_uppercase().to_string() + chars.as_str()
    })
}

/// Checks `name` against the name part rule of `ProviderName`, and that it
/// is not the example's own name.
///
/// # Errors
///
/// Returns the rule when `name` breaks it.
pub fn check_name(name: &str) -> Result<()> {
    if format!("testing/{name}")
        .parse::<stackly_domain::ProviderName>()
        .is_err()
        || name.contains('/')
    {
        bail!("invalid provider name {name:?}: {NAME_RULE}");
    }
    if name == EXAMPLE {
        bail!("invalid provider name {name:?}: the example package already has this name");
    }
    Ok(())
}

/// Replaces the one whole line `from` with `to` in `text`.
///
/// # Errors
///
/// Returns an error unless `from` is exactly one line of `text`.
fn replace_line(text: &str, file: &str, from: &str, to: &str) -> Result<String> {
    let count = text.lines().filter(|line| *line == from).count();
    if count != 1 {
        bail!(
            "providers/{EXAMPLE}/{file} has the line {from:?} {count} times, not once; the example and the scaffold have drifted apart"
        );
    }
    let mut out = String::with_capacity(text.len() + to.len());
    for line in text.split_inclusive('\n') {
        let (body, end) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        out.push_str(if body == from { to } else { body });
        out.push_str(end);
    }
    Ok(out)
}

/// The files of the scaffold for `name`: path relative to the package
/// folder, and the text after the replacements.
///
/// # Errors
///
/// Returns an error when the example cannot be read, a file is not UTF-8,
/// or a replacement does not match exactly once.
#[cfg(target_os = "macos")]
fn scaffold_files(example: &Path, name: &str) -> Result<Vec<(String, String)>> {
    use stackly_core::provider::read_package;
    use stackly_platform_macos::MacosFilesystem;

    let package = read_package(&MacosFilesystem, example).map_err(|err| {
        let lines: Vec<String> = err.issues.iter().map(ToString::to_string).collect();
        anyhow::anyhow!("{err}:\n{}", lines.join("\n"))
    })?;
    let mut files: Vec<(String, String)> = package
        .iter()
        .map(|(path, bytes)| {
            let text = String::from_utf8(bytes.to_vec())
                .with_context(|| format!("{path} of the example is not UTF-8"))?;
            Ok((path.to_owned(), text))
        })
        .collect::<Result<_>>()?;
    for extra in EXTRA_FILES {
        let path = example.join(extra);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        files.push((extra.to_owned(), text));
    }
    for replacement in replacements(name) {
        let (_, text) = files
            .iter_mut()
            .find(|(path, _)| path == replacement.file)
            .with_context(|| format!("the example has no {}", replacement.file))?;
        *text = replace_line(text, replacement.file, &replacement.from, &replacement.to)?;
    }
    files.sort();
    Ok(files)
}

/// What a successful scaffold made.
#[derive(Debug)]
pub struct Scaffolded {
    /// The new package folder.
    pub dir: PathBuf,
    /// `ok <id> <revision> <manifest_sha256>`, as `provider check` prints it.
    pub ok_line: String,
}

/// Copies the example package in `example` into `out/<name>/`.
///
/// # Errors
///
/// Returns an error, and leaves no new file behind, when `name` is not a
/// valid name, `out/<name>` already exists, `out` is not a folder, a
/// replacement does not match once, a write fails, or the copy is not
/// publishable.
#[cfg(target_os = "macos")]
pub fn scaffold(example: &Path, out: &Path, name: &str) -> Result<Scaffolded> {
    use stackly_core::provider::{NameCheck, check_package};
    use stackly_platform_macos::MacosFilesystem;

    check_name(name)?;
    let dest = out.join(name);
    if std::fs::symlink_metadata(&dest).is_ok() {
        bail!("{} already exists; nothing was written", dest.display());
    }
    if !out.is_dir() {
        bail!("{} is not a folder; nothing was written", out.display());
    }
    let files = scaffold_files(example, name)?;
    let temp = tempfile::Builder::new()
        .prefix(&format!(".{name}.new-"))
        .tempdir_in(out)
        .with_context(|| format!("cannot create a temporary folder in {}", out.display()))?;
    // The package folder inside the temporary folder carries the final
    // name, so `NameCheck::FolderName` checks the copy as it will be.
    let staged = temp.path().join(name);
    for (path, text) in &files {
        let target = staged.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        std::fs::write(&target, text)
            .with_context(|| format!("cannot write {}", target.display()))?;
    }
    let check = check_package(&MacosFilesystem, &staged, NameCheck::FolderName);
    let Some(package) = check.package.as_ref().filter(|_| check.publishable()) else {
        let lines: Vec<String> = check
            .findings
            .iter()
            .map(|finding| format!("{name}: {}", finding.issue))
            .collect();
        bail!(
            "the scaffold of {name} is not publishable; nothing was written:\n{}",
            lines.join("\n")
        );
    };
    let key = package.key();
    let ok_line = format!(
        "ok {} {} {}",
        key.provider(),
        key.revision(),
        key.manifest_sha256()
    );
    std::fs::rename(&staged, &dest)
        .with_context(|| format!("cannot move {} to {}", staged.display(), dest.display()))?;
    temp.close()
        .with_context(|| format!("cannot remove the temporary folder in {}", out.display()))?;
    Ok(Scaffolded { dir: dest, ok_line })
}

/// The arguments of `provider new`: the name and the `--out` folder.
fn parse_args<'a>(args: &[&'a str]) -> Result<(&'a str, Option<&'a str>)> {
    match args {
        [name] if !name.starts_with('-') => Ok((name, None)),
        [name, "--out", out] | ["--out", out, name] if !name.starts_with('-') => {
            Ok((name, Some(out)))
        }
        _ => bail!("usage: cargo xtask provider new <name> [--out <dir>]"),
    }
}

/// Runs `provider new` from the repository at `root`.
///
/// # Errors
///
/// Returns an error for bad arguments and every failure of [`scaffold`].
#[cfg(target_os = "macos")]
pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    let (name, out) = parse_args(args)?;
    let providers = root.join("providers");
    let out = match out {
        Some(dir) => std::env::current_dir()
            .context("cannot read the current directory")?
            .join(dir),
        None => providers.clone(),
    };
    let made = scaffold(&providers.join(EXAMPLE), &out, name)?;
    println!("{}", made.ok_line);
    // The folder as the user can type it from where they ran the task.
    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    let shown = made.dir.strip_prefix(&cwd).unwrap_or(&made.dir).display();
    println!("next:");
    println!("  cargo xtask provider check {shown}");
    println!("  cargo xtask provider test {shown}");
    println!("  read {shown}/README.md");
    let in_repo = match (out.canonicalize(), providers.canonicalize()) {
        (Ok(out), Ok(providers)) => out == providers,
        _ => false,
    };
    if in_repo {
        println!("  STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures");
    }
    Ok(ExitCode::SUCCESS)
}

/// `provider new` reads packages through `MacosFilesystem`.
///
/// # Errors
///
/// Always.
#[cfg(not(target_os = "macos"))]
pub fn run(_root: &Path, _args: &[&str]) -> Result<ExitCode> {
    bail!("provider new runs on macOS only, because it checks the copy through MacosFilesystem")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_breaks_the_name_part_rule_or_is_example_is_refused() {
        for name in [
            "",
            "My",
            "1tool",
            "my--tool",
            "tool-",
            "a/b",
            "my_tool",
            &"a".repeat(64),
        ] {
            let err = check_name(name).unwrap_err().to_string();
            assert!(err.contains(NAME_RULE), "{name}: {err}");
        }
        let err = check_name("example").unwrap_err().to_string();
        assert!(err.contains("already has this name"), "{err}");
        for name in ["mytool", "my-tool2", "a"] {
            check_name(name).unwrap();
        }
    }

    #[test]
    fn the_first_letter_of_the_name_is_capitalized() {
        assert_eq!(capitalized("mytool"), "Mytool");
        assert_eq!(capitalized("my-tool"), "My-tool");
    }

    #[test]
    fn a_line_is_replaced_only_when_it_is_there_exactly_once() {
        assert_eq!(
            replace_line("a\n  id: x\nb", "f", "  id: x", "  id: y").unwrap(),
            "a\n  id: y\nb"
        );
        assert!(replace_line("a\n", "f", "  id: x", "y").is_err());
        let err = replace_line("x\nx\n", "f", "x", "y")
            .unwrap_err()
            .to_string();
        assert!(err.contains("2 times"), "{err}");
        // A line that only starts with the text is not a match.
        assert!(replace_line("  id: xy\n", "f", "  id: x", "y").is_err());
    }

    #[test]
    fn the_arguments_are_a_name_and_an_optional_out_folder() {
        assert_eq!(parse_args(&["t"]).unwrap(), ("t", None));
        assert_eq!(parse_args(&["t", "--out", "d"]).unwrap(), ("t", Some("d")));
        assert_eq!(parse_args(&["--out", "d", "t"]).unwrap(), ("t", Some("d")));
        for args in [&[][..], &["--out", "d"], &["t", "--out"], &["a", "b"]] {
            assert!(parse_args(args).is_err(), "{args:?}");
        }
    }

    #[cfg(target_os = "macos")]
    mod macos {
        use std::collections::BTreeMap;
        use std::fs;

        use super::super::*;

        fn example() -> PathBuf {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../providers/example")
        }

        /// Every file under `dir`, relative to it, with its text.
        fn tree(dir: &Path) -> BTreeMap<String, String> {
            let mut out = BTreeMap::new();
            let mut pending = vec![dir.to_path_buf()];
            while let Some(folder) = pending.pop() {
                for entry in fs::read_dir(&folder).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        pending.push(path);
                    } else {
                        let rel = path
                            .strip_prefix(dir)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned();
                        out.insert(rel, fs::read_to_string(&path).unwrap());
                    }
                }
            }
            out
        }

        // covers: AC-7 (spec 0036, *Scaffold*)
        #[test]
        fn a_scaffold_differs_from_the_example_only_in_the_three_lines() {
            let root = stackly_testkit::TestRoot::new().unwrap();

            let made = scaffold(&example(), root.path(), "mytool").unwrap();

            assert_eq!(made.dir, root.path().join("mytool"));
            assert!(
                made.ok_line.starts_with("ok testing/mytool 0.1.0 "),
                "{}",
                made.ok_line
            );
            let names: Vec<_> = fs::read_dir(root.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(names, ["mytool"], "the temporary folder is gone");
            let copy = tree(&made.dir);
            let original = tree(&example());
            assert_eq!(
                copy.keys().collect::<Vec<_>>(),
                original.keys().collect::<Vec<_>>()
            );
            let mut changed = Vec::new();
            for (path, text) in &copy {
                let before: Vec<&str> = original[path].lines().collect();
                let after: Vec<&str> = text.lines().collect();
                assert_eq!(before.len(), after.len(), "{path}");
                assert_eq!(
                    original[path].ends_with('\n'),
                    text.ends_with('\n'),
                    "{path}"
                );
                for (old, new) in before.iter().zip(&after) {
                    if old != new {
                        changed.push((path.as_str(), *old, *new));
                    }
                }
            }
            assert_eq!(
                changed,
                [
                    ("README.md", "# Example provider", "# Mytool provider"),
                    (
                        "provider.yaml",
                        "  id: testing/example",
                        "  id: testing/mytool"
                    ),
                    (
                        "provider.yaml",
                        "  displayName: Example",
                        "  displayName: Mytool"
                    ),
                ]
            );
        }

        // covers: AC-7 (spec 0036, *Scaffold*: an existing folder)
        #[test]
        fn an_existing_destination_is_refused_without_writing() {
            let root = stackly_testkit::TestRoot::new().unwrap();
            fs::create_dir(root.path().join("mytool")).unwrap();
            fs::write(root.path().join("mytool/keep.txt"), "mine").unwrap();

            let err = scaffold(&example(), root.path(), "mytool").unwrap_err();

            assert!(err.to_string().contains("already exists"), "{err}");
            assert_eq!(
                tree(root.path()),
                BTreeMap::from([("mytool/keep.txt".to_owned(), "mine".to_owned())])
            );
        }

        // covers: AC-7 (spec 0036, *Scaffold*: an invalid name)
        #[test]
        fn an_invalid_name_writes_nothing() {
            let root = stackly_testkit::TestRoot::new().unwrap();

            for name in ["Bad", "example", "../up"] {
                assert!(scaffold(&example(), root.path(), name).is_err(), "{name}");
            }

            assert!(tree(root.path()).is_empty());
        }

        // covers: AC-7 (spec 0036, *Scaffold*: the guard against drift)
        #[test]
        fn an_example_whose_lines_drifted_fails_and_leaves_nothing() {
            let root = stackly_testkit::TestRoot::new().unwrap();
            let drifted = root.path().join("source/example");
            for (path, text) in tree(&example()) {
                let target = drifted.join(&path);
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                let text = if path == "README.md" {
                    text.replace("# Example provider", "# The example provider")
                } else {
                    text
                };
                fs::write(target, text).unwrap();
            }
            let out = root.path().join("out");
            fs::create_dir(&out).unwrap();

            let err = scaffold(&drifted, &out, "mytool").unwrap_err().to_string();

            assert!(err.contains("0 times"), "{err}");
            assert!(tree(&out).is_empty());
        }

        // covers: AC-7 (spec 0036, *Scaffold*: an unpublishable copy is removed)
        #[test]
        fn an_unpublishable_copy_is_removed_and_nothing_is_written() {
            let root = stackly_testkit::TestRoot::new().unwrap();
            let broken = root.path().join("source/example");
            for (path, text) in tree(&example()) {
                let target = broken.join(&path);
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                let text = if path == "tests/scenarios.yaml" {
                    text.replace("readyDelayMs: 1500", "readyDelayMs: 999999")
                } else {
                    text
                };
                fs::write(target, text).unwrap();
            }
            let out = root.path().join("out");
            fs::create_dir(&out).unwrap();

            let err = scaffold(&broken, &out, "mytool").unwrap_err().to_string();

            assert!(err.contains("not publishable"), "{err}");
            assert!(
                fs::read_dir(&out).unwrap().next().is_none(),
                "nothing is left in out"
            );
        }
    }
}
