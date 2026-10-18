//! Replays fuzz inputs on the stable toolchain (spec 0006, *Fuzzing*, and
//! spec 0009, *Fuzz target*): every input that ever crashed a provider
//! target, kept in `tests/fuzz_regressions/<target>/`, and the committed
//! seeds in `fuzz/seeds/<target>/`. The folder name picks the harness in
//! `stackly_core::provider::fuzzing`, the same function the cargo-fuzz
//! target calls, so a crash found on nightly fails `cargo test` here.

use std::fs;
use std::panic;
use std::path::{Path, PathBuf};

use stackly_core::provider::fuzzing;

/// A harness from `stackly_core::provider::fuzzing`; it panics on a
/// broken invariant.
type Harness = fn(&[u8]);

/// Every fuzz target of this crate and its harness.
const TARGETS: [(&str, Harness); 4] = [
    ("provider_manifest", fuzzing::manifest),
    ("provider_template", fuzzing::template),
    ("provider_interpolation", fuzzing::interpolation),
    ("provider_settings_schema", fuzzing::settings_schema),
];

fn regressions_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz_regressions")
}

fn seeds_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds")
}

/// The input files in `dir`, sorted, skipping hidden files. A missing
/// folder holds no inputs.
fn inputs(dir: &Path) -> Vec<PathBuf> {
    if !dir.exists() {
        return Vec::new();
    }
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()))
        .map(|entry| entry.expect("a directory entry can be read").path())
        .filter(|path| {
            path.is_file()
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    files.sort();
    files
}

/// Runs `harness` on every input in `dir`. Returns how many inputs ran and
/// one line for each input that panicked.
fn replay(dir: &Path, harness: Harness) -> (usize, Vec<String>) {
    let files = inputs(dir);
    let failures = files
        .iter()
        .filter_map(|path| {
            let bytes = fs::read(path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            let payload = panic::catch_unwind(|| harness(&bytes)).err()?;
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("a panic without a message");
            Some(format!("{}: {message}", path.display()))
        })
        .collect();
    (files.len(), failures)
}

#[test]
fn every_regression_input_passes_its_harness() {
    for (target, harness) in TARGETS {
        let (_, failures) = replay(&regressions_dir().join(target), harness);
        assert!(
            failures.is_empty(),
            "{target} still crashes on:\n{}",
            failures.join("\n")
        );
    }
}

#[test]
fn every_committed_seed_passes_its_harness() {
    for (target, harness) in TARGETS {
        let (count, failures) = replay(&seeds_dir().join(target), harness);
        assert!(
            count > 0,
            "fuzz/seeds/{target} is empty; rebuild it with `cargo xtask fuzz seed`"
        );
        assert!(
            failures.is_empty(),
            "{target} crashes on a seed:\n{}",
            failures.join("\n")
        );
    }
}

#[test]
fn every_regression_folder_names_a_target_of_this_crate() {
    let dir = regressions_dir();
    if !dir.exists() {
        return;
    }
    for entry in fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        assert!(
            entry.path().is_dir() && TARGETS.iter().any(|(target, _)| *target == name),
            "tests/fuzz_regressions/{name} is not the folder of a fuzz target of this crate, so it is never replayed"
        );
    }
}

#[test]
fn a_panicking_harness_is_reported_with_the_input_path() {
    fn always_panics(_: &[u8]) {
        panic!("invariant broke");
    }
    let (count, failures) = replay(&seeds_dir().join("provider_manifest"), always_panics);
    assert_eq!(failures.len(), count);
    assert!(failures[0].contains("provider_manifest"));
    assert!(failures[0].ends_with(": invariant broke"));
}
