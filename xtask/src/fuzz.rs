//! `cargo xtask fuzz list|build|seed|run`: the cargo-fuzz project in
//! `fuzz/` (spec 0006, *Fuzzing*).
//!
//! `fuzz/` is its own workspace and builds only with the dated nightly
//! [`FUZZ_TOOLCHAIN`], so the stable toolchain of the main workspace never
//! changes. `list`, `build`, and `run` check that this nightly and
//! [`CARGO_FUZZ_VERSION`] are installed, then call `cargo +<nightly> fuzz`
//! with `--fuzz-dir fuzz`. `seed` rebuilds `fuzz/seeds/<target>/` from the
//! shared fixtures and needs neither.
//!
//! Adding a target: put the harness in the owning crate's
//! `#[doc(hidden)] pub mod fuzzing`, add a `[[bin]]` named `<area>_<thing>`
//! and its file in `fuzz/fuzz_targets/`, add a row to [`TARGETS`] with its
//! seed source, run `cargo xtask fuzz seed`, and replay crash inputs from
//! the crate's `tests/fuzz_regressions.rs` on stable.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The dated nightly the fuzz project builds with. It must equal the
/// channel in `fuzz/rust-toolchain.toml`; change both together.
const FUZZ_TOOLCHAIN: &str = "nightly-2026-10-17";
/// The cargo-fuzz release these commands drive.
const CARGO_FUZZ_VERSION: &str = "0.13.2";
const FUZZ_DIR: &str = "fuzz";
const FUZZ_MANIFEST: &str = "fuzz/Cargo.toml";
const SEEDS_DIR: &str = "fuzz/seeds";
const CORPUS_DIR: &str = "fuzz/corpus";
const ARTIFACTS_DIR: &str = "fuzz/artifacts";
/// How long `run` fuzzes one target without `--time`: the 5 minutes per
/// target of Done when.
const DEFAULT_SECONDS: u64 = 300;
/// The longest input libFuzzer makes for the IPC targets. The `TooLarge`
/// path stays covered through the length in the header.
const IPC_MAX_LEN: usize = 65_536;
const FRAMING_VECTORS: &str = "protocols/fixtures/framing/vectors.json";
const IPC_FIXTURES: &str = "protocols/fixtures/ipc";
/// The first byte of every `ipc_frame_decode` seed. The harness reads its
/// chunk sizes from that byte, so the vector bytes follow it unchanged.
const FRAME_CHUNK_SEED: u8 = 0;

const USAGE: &str =
    "cargo xtask fuzz list | build [<target>] | seed | run <target> [--time <seconds>]";

/// One fuzz target.
struct Target {
    /// The `[[bin]]` name in `fuzz/Cargo.toml`, `<area>_<thing>`.
    name: &'static str,
    /// The crate that owns the harness; crash inputs go to its
    /// `tests/fuzz_regressions/<name>/`.
    crate_dir: &'static str,
    /// The longest input libFuzzer makes for this target.
    max_len: usize,
    /// Builds the seeds from the repository's fixtures.
    seeds: fn(&Path) -> Result<Vec<Seed>>,
}

/// Every fuzz target, in the order `seed` writes them.
const TARGETS: &[Target] = &[
    Target {
        name: "ipc_frame_decode",
        crate_dir: "crates/protocol",
        max_len: IPC_MAX_LEN,
        seeds: framing_vector_seeds,
    },
    Target {
        name: "ipc_request_decode",
        crate_dir: "crates/protocol",
        max_len: IPC_MAX_LEN,
        seeds: request_fixture_seeds,
    },
];

/// One seed file: its name in `fuzz/seeds/<target>/` and its bytes.
#[derive(Debug, PartialEq, Eq)]
struct Seed {
    name: String,
    bytes: Vec<u8>,
}

pub fn run(root: &Path, args: &[&str]) -> Result<()> {
    match args {
        ["list"] => list(root),
        ["build"] => build(root, None),
        ["build", name] => build(root, Some(target(name)?)),
        ["seed"] => seed(root),
        ["run", name, rest @ ..] => run_target(root, target(name)?, seconds(rest)?),
        _ => bail!("unknown fuzz task `{}`\n\nUsage: {USAGE}", args.join(" ")),
    }
}

fn list(root: &Path) -> Result<()> {
    check_pins(root)?;
    cargo_fuzz(root, &["list"])
}

fn build(root: &Path, target: Option<&Target>) -> Result<()> {
    check_pins(root)?;
    check_lock(root)?;
    match target {
        Some(target) => cargo_fuzz(root, &["build", target.name]),
        None => cargo_fuzz(root, &["build"]),
    }
}

fn seed(root: &Path) -> Result<()> {
    for target in TARGETS {
        let seeds = target_seeds(root, target)?;
        let dir = root.join(SEEDS_DIR).join(target.name);
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| format!("cannot clear {}", dir.display()))?;
        }
        fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
        for seed in &seeds {
            let path = dir.join(&seed.name);
            fs::write(&path, &seed.bytes)
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
        println!(
            "{}: {} seed(s) in {SEEDS_DIR}/{}",
            target.name,
            seeds.len(),
            target.name
        );
    }
    Ok(())
}

fn run_target(root: &Path, target: &Target, seconds: u64) -> Result<()> {
    check_pins(root)?;
    check_lock(root)?;
    let seeds = format!("{SEEDS_DIR}/{}", target.name);
    if !root.join(&seeds).is_dir() {
        bail!("{seeds} is missing; create it with `cargo xtask fuzz seed`");
    }
    if seeds_are_stale(root, target)? {
        eprintln!(
            "fuzz run: warning: {seeds} differs from the fixtures; refresh it with `cargo xtask fuzz seed`"
        );
    }
    let corpus = format!("{CORPUS_DIR}/{}", target.name);
    fs::create_dir_all(root.join(&corpus)).with_context(|| format!("cannot create {corpus}"))?;
    let max_len = format!("-max_len={}", target.max_len);
    let max_time = format!("-max_total_time={seconds}");
    let result = cargo_fuzz(
        root,
        &[
            "run",
            target.name,
            &corpus,
            &seeds,
            "--",
            &max_len,
            &max_time,
            "-print_final_stats=1",
        ],
    );
    if let Err(err) = result {
        bail!(
            "{err:#}\nthe crashing input is in {ARTIFACTS_DIR}/{name}/; copy it to {crate_dir}/tests/fuzz_regressions/{name}/ so `cargo test` replays it on stable",
            name = target.name,
            crate_dir = target.crate_dir,
        );
    }
    println!(
        "fuzz run: {} ran for {seconds} s without a crash",
        target.name
    );
    Ok(())
}

/// Finds a target by name.
fn target(name: &str) -> Result<&'static Target> {
    TARGETS
        .iter()
        .find(|target| target.name == name)
        .with_context(|| {
            let names: Vec<&str> = TARGETS.iter().map(|target| target.name).collect();
            format!(
                "unknown fuzz target `{name}`; the targets are {}",
                names.join(", ")
            )
        })
}

/// Reads `[--time <seconds>]`.
fn seconds(args: &[&str]) -> Result<u64> {
    match args {
        [] => Ok(DEFAULT_SECONDS),
        ["--time", value] => match value.parse::<u64>() {
            Ok(seconds) if seconds > 0 => Ok(seconds),
            _ => bail!("--time needs a whole number of seconds above 0, found `{value}`"),
        },
        _ => bail!("unexpected `{}`\n\nUsage: {USAGE}", args.join(" ")),
    }
}

fn check_pins(root: &Path) -> Result<()> {
    check_toolchain()?;
    check_cargo_fuzz(root)
}

fn check_toolchain() -> Result<()> {
    let install = format!("rustup toolchain install {FUZZ_TOOLCHAIN} --profile minimal");
    let output = Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .with_context(|| format!("cannot run `rustup`; install rustup, then run `{install}`"))?;
    let listed = String::from_utf8_lossy(&output.stdout);
    let prefix = format!("{FUZZ_TOOLCHAIN}-");
    if !output.status.success() || !listed.lines().any(|line| line.starts_with(&prefix)) {
        bail!("the fuzz toolchain {FUZZ_TOOLCHAIN} is not installed; install it with `{install}`");
    }
    Ok(())
}

fn check_cargo_fuzz(root: &Path) -> Result<()> {
    let install = format!("cargo install --locked cargo-fuzz --version {CARGO_FUZZ_VERSION}");
    let output = nightly_cargo(root)
        .args(["fuzz", "--version"])
        .output()
        .with_context(|| format!("cannot run `cargo fuzz`; install it with `{install}`"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let found = if output.status.success() {
        stdout.split_whitespace().nth(1).unwrap_or("unknown")
    } else {
        "none"
    };
    if found != CARGO_FUZZ_VERSION {
        bail!(
            "cargo-fuzz {CARGO_FUZZ_VERSION} is required, found {found}; install it with `{install}`"
        );
    }
    Ok(())
}

/// Fails when `fuzz/Cargo.lock` no longer matches `fuzz/Cargo.toml`, for
/// example after a dependency of a fuzzed crate changed.
fn check_lock(root: &Path) -> Result<()> {
    let status = nightly_cargo(root)
        .args([
            "fetch",
            "--locked",
            "--quiet",
            "--manifest-path",
            FUZZ_MANIFEST,
        ])
        .status()
        .with_context(|| format!("cannot run `cargo +{FUZZ_TOOLCHAIN} fetch`"))?;
    if !status.success() {
        bail!(
            "fuzz/Cargo.lock does not match fuzz/Cargo.toml; update it with `cargo +{FUZZ_TOOLCHAIN} fetch --manifest-path {FUZZ_MANIFEST}` and commit it"
        );
    }
    Ok(())
}

/// Runs `cargo +<nightly> fuzz <args>` with `--fuzz-dir fuzz`.
fn cargo_fuzz(root: &Path, args: &[&str]) -> Result<()> {
    let (command, rest) = args
        .split_first()
        .expect("every cargo fuzz call names a command");
    let status = nightly_cargo(root)
        .args(["fuzz", command, "--fuzz-dir", FUZZ_DIR])
        .args(rest)
        .status()
        .with_context(|| format!("cannot run `cargo +{FUZZ_TOOLCHAIN} fuzz`"))?;
    if !status.success() {
        bail!("`cargo fuzz {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

/// `cargo +<nightly>` from the repository root, without the toolchain
/// variables the outer `cargo xtask` set, so the nightly is used all the
/// way down.
fn nightly_cargo(root: &Path) -> Command {
    let mut command = Command::new("cargo");
    command.arg(format!("+{FUZZ_TOOLCHAIN}")).current_dir(root);
    for variable in ["RUSTUP_TOOLCHAIN", "CARGO", "RUSTC", "RUSTDOC"] {
        command.env_remove(variable);
    }
    command
}

/// The seeds of one target, checked for unique names and length.
fn target_seeds(root: &Path, target: &Target) -> Result<Vec<Seed>> {
    let seeds = (target.seeds)(root)?;
    if seeds.is_empty() {
        bail!("{} has no seeds; its fixtures are missing", target.name);
    }
    let mut names: Vec<&str> = seeds.iter().map(|seed| seed.name.as_str()).collect();
    names.sort_unstable();
    if let Some(pair) = names.windows(2).find(|pair| pair[0] == pair[1]) {
        bail!("{} has two seeds named `{}`", target.name, pair[0]);
    }
    if let Some(seed) = seeds.iter().find(|seed| seed.bytes.len() > target.max_len) {
        bail!(
            "seed `{}` of {} has {} bytes, above its max_len of {}",
            seed.name,
            target.name,
            seed.bytes.len(),
            target.max_len
        );
    }
    Ok(seeds)
}

/// Whether `fuzz/seeds/<target>/` differs from what `seed` would write.
fn seeds_are_stale(root: &Path, target: &Target) -> Result<bool> {
    let mut expected = target_seeds(root, target)?;
    expected.sort_by(|a, b| a.name.cmp(&b.name));
    let dir = root.join(SEEDS_DIR).join(target.name);
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
        let bytes = fs::read(entry.path())
            .with_context(|| format!("cannot read {}", entry.path().display()))?;
        found.push(Seed {
            name: entry.file_name().to_string_lossy().into_owned(),
            bytes,
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found != expected)
}

/// `ipc_frame_decode`: the bytes of every vector in
/// `protocols/fixtures/framing/vectors.json`, after the chunk seed byte.
fn framing_vector_seeds(root: &Path) -> Result<Vec<Seed>> {
    #[derive(Deserialize)]
    struct Vectors {
        vectors: Vec<Vector>,
    }
    #[derive(Deserialize)]
    struct Vector {
        name: String,
        hex: String,
    }
    let text = fs::read_to_string(root.join(FRAMING_VECTORS))
        .with_context(|| format!("cannot read {FRAMING_VECTORS}"))?;
    let vectors: Vectors =
        serde_json::from_str(&text).with_context(|| format!("cannot parse {FRAMING_VECTORS}"))?;
    vectors
        .vectors
        .into_iter()
        .map(|vector| {
            let mut bytes = vec![FRAME_CHUNK_SEED];
            bytes.extend(
                unhex(&vector.hex)
                    .with_context(|| format!("vector `{}` in {FRAMING_VECTORS}", vector.name))?,
            );
            Ok(Seed {
                name: slug(&vector.name),
                bytes,
            })
        })
        .collect()
}

/// `ipc_request_decode`: every fixture of type `Request` listed in
/// `protocols/fixtures/ipc/index.json`, rejected ones included.
fn request_fixture_seeds(root: &Path) -> Result<Vec<Seed>> {
    #[derive(Deserialize)]
    struct Index {
        fixtures: Vec<Fixture>,
    }
    #[derive(Deserialize)]
    struct Fixture {
        file: String,
        #[serde(rename = "type")]
        kind: String,
    }
    let index_path = format!("{IPC_FIXTURES}/index.json");
    let text = fs::read_to_string(root.join(&index_path))
        .with_context(|| format!("cannot read {index_path}"))?;
    let index: Index =
        serde_json::from_str(&text).with_context(|| format!("cannot parse {index_path}"))?;
    index
        .fixtures
        .into_iter()
        .filter(|fixture| fixture.kind == "Request")
        .map(|fixture| {
            let path = format!("{IPC_FIXTURES}/{}", fixture.file);
            let bytes =
                fs::read(root.join(&path)).with_context(|| format!("cannot read {path}"))?;
            Ok(Seed {
                name: fixture.file,
                bytes,
            })
        })
        .collect()
}

fn unhex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        bail!("odd hex length");
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            text.get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .with_context(|| format!("not a hex byte at offset {i}"))
        })
        .collect()
}

/// A file name from a vector name: lowercase ASCII letters and digits,
/// with one dash between words.
fn slug(name: &str) -> String {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_root;

    #[test]
    fn the_toolchain_file_pins_the_same_nightly_as_xtask() {
        let text = fs::read_to_string(repo_root().join("fuzz/rust-toolchain.toml")).unwrap();
        let file: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(file["toolchain"]["channel"].as_str(), Some(FUZZ_TOOLCHAIN));
    }

    #[test]
    fn every_manifest_target_has_a_row_and_a_source_file() {
        let root = repo_root();
        let text = fs::read_to_string(root.join(FUZZ_MANIFEST)).unwrap();
        let manifest: toml::Table = toml::from_str(&text).unwrap();
        let bins = manifest["bin"].as_array().unwrap();
        let mut names: Vec<&str> = bins
            .iter()
            .map(|bin| {
                let name = bin["name"].as_str().unwrap();
                let path = bin["path"].as_str().unwrap();
                assert_eq!(path, format!("fuzz_targets/{name}.rs"));
                assert!(
                    root.join(FUZZ_DIR).join(path).is_file(),
                    "{path} is missing"
                );
                name
            })
            .collect();
        names.sort_unstable();
        let mut rows: Vec<&str> = TARGETS.iter().map(|target| target.name).collect();
        rows.sort_unstable();
        assert_eq!(names, rows);
    }

    #[test]
    fn target_names_are_area_then_thing() {
        for target in TARGETS {
            let (area, thing) = target.name.split_once('_').unwrap();
            for part in [area, thing] {
                assert!(
                    !part.is_empty()
                        && part
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                    "`{}` is not <area>_<thing>",
                    target.name
                );
            }
            assert!(
                repo_root()
                    .join(target.crate_dir)
                    .join("Cargo.toml")
                    .is_file(),
                "{} is not a crate",
                target.crate_dir
            );
        }
    }

    #[test]
    fn every_target_gets_seeds_from_its_fixtures() {
        for target in TARGETS {
            let seeds = target_seeds(&repo_root(), target).unwrap();
            assert!(!seeds.is_empty(), "{} has no seeds", target.name);
        }
    }

    #[test]
    fn frame_seeds_are_the_chunk_seed_then_the_vector_bytes() {
        let seeds = framing_vector_seeds(&repo_root()).unwrap();
        let one = seeds.iter().find(|seed| seed.name == "one-frame").unwrap();
        assert_eq!(one.bytes[0], FRAME_CHUNK_SEED);
        assert_eq!(&one.bytes[1..5], &[0, 0, 0, 2]);
    }

    #[test]
    fn request_seeds_include_rejected_fixtures_and_skip_other_types() {
        let seeds = request_fixture_seeds(&repo_root()).unwrap();
        let names: Vec<&str> = seeds.iter().map(|seed| seed.name.as_str()).collect();
        assert!(names.contains(&"request-get-info.json"));
        assert!(names.contains(&"reject-too-deep.json"));
        assert!(
            names
                .iter()
                .all(|name| name.starts_with("request-") || name.starts_with("reject-"))
        );
    }

    #[test]
    fn the_time_defaults_to_five_minutes_and_must_be_positive() {
        assert_eq!(seconds(&[]).unwrap(), 300);
        assert_eq!(seconds(&["--time", "60"]).unwrap(), 60);
        assert!(seconds(&["--time", "0"]).is_err());
        assert!(seconds(&["--time", "soon"]).is_err());
        assert!(seconds(&["--time"]).is_err());
        assert!(seconds(&["--jobs", "4"]).is_err());
    }

    #[test]
    fn an_unknown_target_lists_the_known_ones() {
        let err = target("ipc_nothing").err().unwrap().to_string();
        assert!(
            err.contains("ipc_frame_decode, ipc_request_decode"),
            "{err}"
        );
        assert!(run(&repo_root(), &["run", "ipc_nothing"]).is_err());
        assert!(run(&repo_root(), &["seeds"]).is_err());
    }

    #[test]
    fn slugs_are_lowercase_words_joined_by_dashes() {
        assert_eq!(slug("two frames in one chunk"), "two-frames-in-one-chunk");
        assert_eq!(slug("Length of MAX + 1"), "length-of-max-1");
    }

    #[test]
    fn unhex_reads_pairs_and_refuses_odd_or_foreign_text() {
        assert_eq!(unhex("00ff10").unwrap(), vec![0, 255, 16]);
        assert!(unhex("abc").is_err());
        assert!(unhex("zz").is_err());
    }
}
