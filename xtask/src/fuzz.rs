//! `cargo xtask fuzz list|build|seed|run`: the cargo-fuzz project in
//! `fuzz/` (spec 0006, *Fuzzing*), with the IPC targets of spec 0006, the
//! provider targets of spec 0009, and the `nats-readiness` target of spec
//! 0023.
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
use stackly_core::runtime::fuzzing as runtime_fuzzing;
use stackly_core::runtime::samples::{self, MachOBuilder};

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
/// The example packages and the core fixture package the provider targets
/// take their seeds from (spec 0009, *Fuzz target*), as short name and
/// folder.
const PROVIDER_PACKAGES: [(&str, &str); 4] = [
    ("fake", "providers/fake"),
    ("nats", "providers/nats"),
    ("traefik", "providers/traefik"),
    (
        "fake-command",
        "crates/core/tests/fixtures/packages/fake-command",
    ),
];
/// The longest `provider.yaml` or template input: the 256 KiB file limit.
const PROVIDER_FILE_MAX_LEN: usize = 262_144;
/// The longest settings schema input: the 128 KiB file limit.
const SETTINGS_SCHEMA_MAX_LEN: usize = 131_072;
/// The longest manifest field the interpolation target tries.
const INTERPOLATION_MAX_LEN: usize = 4096;
/// The `nats-readiness` transcripts and their index, the seed source of
/// `nats_readiness_protocol` (spec 0023, *Fuzz target*).
const NATS_TRANSCRIPTS: &str = "crates/core/tests/fixtures/nats-protocol";
/// The longest `nats_readiness_protocol` input: twice the 128 KiB read
/// limit of one attempt, so the fuzzer reaches that limit.
const NATS_PROTOCOL_MAX_LEN: usize = 262_144;
/// The first byte of every `nats_readiness_protocol` seed without its
/// JetStream bit: chunks of 128 bytes.
const NATS_CHUNK_SEED: u8 = 0xfe;
/// The longest `runtime_archive` input: the artifact the extractor reads,
/// whose limits (1 MiB of files) bound what a larger one adds.
const RUNTIME_ARCHIVE_MAX_LEN: usize = 1 << 20;
/// The longest `runtime_macho` input: a header and load commands. The
/// 4 MiB limit stays reachable through the sizes the header claims.
const RUNTIME_MACHO_MAX_LEN: usize = 65_536;

/// The longest `instance_path` input: a path a few names deep, well under
/// the 1024 byte `PATH_MAX` of macOS plus the 64 level fixture folder.
const INSTANCE_PATH_MAX_LEN: usize = 1024;

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
    Target {
        name: "ipc_server_frame_decode",
        crate_dir: "crates/protocol",
        max_len: IPC_MAX_LEN,
        seeds: server_frame_fixture_seeds,
    },
    Target {
        name: "provider_manifest",
        crate_dir: "crates/core",
        max_len: PROVIDER_FILE_MAX_LEN,
        seeds: provider_manifest_seeds,
    },
    Target {
        name: "provider_template",
        crate_dir: "crates/core",
        max_len: PROVIDER_FILE_MAX_LEN,
        seeds: provider_template_seeds,
    },
    Target {
        name: "provider_interpolation",
        crate_dir: "crates/core",
        max_len: INTERPOLATION_MAX_LEN,
        seeds: provider_interpolation_seeds,
    },
    Target {
        name: "provider_settings_schema",
        crate_dir: "crates/core",
        max_len: SETTINGS_SCHEMA_MAX_LEN,
        seeds: provider_settings_schema_seeds,
    },
    Target {
        name: "nats_readiness_protocol",
        crate_dir: "crates/core",
        max_len: NATS_PROTOCOL_MAX_LEN,
        seeds: nats_readiness_protocol_seeds,
    },
    Target {
        name: "runtime_archive",
        crate_dir: "crates/core",
        max_len: RUNTIME_ARCHIVE_MAX_LEN,
        seeds: runtime_archive_seeds,
    },
    Target {
        name: "runtime_macho",
        crate_dir: "crates/core",
        max_len: RUNTIME_MACHO_MAX_LEN,
        seeds: runtime_macho_seeds,
    },
    Target {
        name: "instance_path",
        crate_dir: "crates/core",
        max_len: INSTANCE_PATH_MAX_LEN,
        seeds: instance_path_seeds,
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

/// `runtime_archive`: every sample artifact of the extractor tests, after
/// the selector byte of its format and folder strip (spec 0018, AC-19).
fn runtime_archive_seeds(_root: &Path) -> Result<Vec<Seed>> {
    Ok(samples::archive_samples()
        .into_iter()
        .map(|sample| {
            let mut bytes = vec![runtime_fuzzing::archive_selector(
                sample.format,
                sample.spec.archive_root.is_some(),
            )];
            bytes.extend(sample.bytes);
            Seed {
                name: sample.name.to_owned(),
                bytes,
            }
        })
        .collect())
}

/// `runtime_macho`: thin and universal Mach-O files with every load
/// command kind the parser reads.
fn runtime_macho_seeds(_root: &Path) -> Result<Vec<Seed>> {
    let system = "/usr/lib/libSystem.B.dylib";
    let arm = || MachOBuilder::executable().min_os(13, 0).load(system);
    let x86 = || {
        MachOBuilder::executable()
            .x86_64()
            .min_os(13, 0)
            .load(system)
    };
    let files = [
        ("thin_executable", arm().build()),
        ("thin_x86_64", x86().build()),
        ("thin_arm64e", arm().arm64e().build()),
        (
            "thin_dylib",
            MachOBuilder::dylib("@rpath/libfoo.dylib")
                .load(system)
                .build(),
        ),
        (
            "every_library_kind",
            arm()
                .weak("/usr/lib/libz.1.dylib")
                .reexport("@rpath/libre.dylib")
                .lazy("@rpath/liblazy.dylib")
                .upward("/usr/lib/libup.dylib")
                .rpath("@loader_path/../lib")
                .build(),
        ),
        (
            "version_min_load_command",
            MachOBuilder::executable()
                .version_min(12, 0)
                .load(system)
                .build(),
        ),
        (
            "other_platform",
            MachOBuilder::executable().build_version(2, 17, 0).build(),
        ),
        (
            "universal_x86_64_and_arm64",
            samples::universal(&[x86().build(), arm().build()]),
        ),
        (
            "universal_x86_64_only",
            samples::universal(&[x86().build()]),
        ),
    ];
    Ok(files
        .into_iter()
        .map(|(name, bytes)| Seed {
            name: name.to_owned(),
            bytes,
        })
        .collect())
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
    ipc_fixture_seeds(root, "Request")
}

/// `ipc_server_frame_decode`: every fixture of type `ServerFrame` listed in
/// `protocols/fixtures/ipc/index.json`, rejected and skipped ones included.
fn server_frame_fixture_seeds(root: &Path) -> Result<Vec<Seed>> {
    ipc_fixture_seeds(root, "ServerFrame")
}

/// Every fixture of `type_name` listed in `protocols/fixtures/ipc/index.json`,
/// whatever it expects.
fn ipc_fixture_seeds(root: &Path, type_name: &str) -> Result<Vec<Seed>> {
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
        .filter(|fixture| fixture.kind == type_name)
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

/// `provider_manifest`: the `provider.yaml` of every example package.
fn provider_manifest_seeds(root: &Path) -> Result<Vec<Seed>> {
    PROVIDER_PACKAGES
        .iter()
        .map(|(name, dir)| {
            Ok(Seed {
                name: format!("{name}.provider.yaml"),
                bytes: read(root, &format!("{dir}/provider.yaml"))?,
            })
        })
        .collect()
}

/// `provider_template`: every template of the example packages after the
/// selector byte of its setup: 0 for `json`, 1 for `yaml`, 2 for
/// `natsConf`, and 3 for the traefik routes template, which the harness
/// renders with a list.
fn provider_template_seeds(root: &Path) -> Result<Vec<Seed>> {
    let mut seeds = Vec::new();
    for (name, dir) in PROVIDER_PACKAGES {
        let templates = format!("{dir}/templates");
        let mut files: Vec<String> = fs::read_dir(root.join(&templates))
            .with_context(|| format!("cannot read {templates}"))?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect::<Result<_>>()?;
        files.sort();
        for file in files {
            let selector: u8 = if name == "traefik" && file == "routes.yaml.tmpl" {
                3
            } else if file.ends_with(".json.tmpl") {
                0
            } else if file.ends_with(".conf.tmpl") {
                2
            } else {
                1
            };
            let mut bytes = vec![selector];
            bytes.extend(read(root, &format!("{templates}/{file}"))?);
            seeds.push(Seed {
                name: format!("{name}.{file}"),
                bytes,
            });
        }
    }
    Ok(seeds)
}

/// `provider_interpolation`: every quoted string holding `${` in the
/// manifests and settings schemas of the example packages, once each, in
/// the order they first appear.
fn provider_interpolation_seeds(root: &Path) -> Result<Vec<Seed>> {
    let mut texts: Vec<String> = Vec::new();
    for (_, dir) in PROVIDER_PACKAGES {
        for file in ["provider.yaml", "schemas/settings.schema.json"] {
            let text = String::from_utf8(read(root, &format!("{dir}/{file}"))?)
                .with_context(|| format!("{dir}/{file} is not UTF-8"))?;
            for quoted in text.split('"').skip(1).step_by(2) {
                if quoted.contains("${") && !texts.iter().any(|seen| seen == quoted) {
                    texts.push(quoted.to_owned());
                }
            }
        }
    }
    Ok(texts
        .into_iter()
        .enumerate()
        .map(|(index, text)| Seed {
            name: format!("field-{:02}", index + 1),
            bytes: text.into_bytes(),
        })
        .collect())
}

/// `provider_settings_schema`: the settings schema of every example
/// package.
fn provider_settings_schema_seeds(root: &Path) -> Result<Vec<Seed>> {
    PROVIDER_PACKAGES
        .iter()
        .map(|(name, dir)| {
            Ok(Seed {
                name: format!("{name}.settings.schema.json"),
                bytes: read(root, &format!("{dir}/schemas/settings.schema.json"))?,
            })
        })
        .collect()
}

/// Reads a repository file.
/// One seed per transcript in the index: the mode byte (128 byte chunks,
/// bit 0 for the transcript's JetStream expectation), then every byte the
/// server sent.
fn nats_readiness_protocol_seeds(root: &Path) -> Result<Vec<Seed>> {
    #[derive(Deserialize)]
    struct Index {
        transcripts: Vec<Transcript>,
    }
    #[derive(Deserialize)]
    struct Transcript {
        file: String,
        jetstream: bool,
    }
    let path = format!("{NATS_TRANSCRIPTS}/transcripts.json");
    let text =
        fs::read_to_string(root.join(&path)).with_context(|| format!("cannot read {path}"))?;
    let index: Index =
        serde_json::from_str(&text).with_context(|| format!("cannot parse {path}"))?;
    index
        .transcripts
        .into_iter()
        .map(|transcript| {
            let mut bytes = vec![NATS_CHUNK_SEED | u8::from(transcript.jetstream)];
            bytes.extend(read(
                root,
                &format!("{NATS_TRANSCRIPTS}/{}", transcript.file),
            )?);
            let stem = transcript
                .file
                .strip_suffix(".bin")
                .unwrap_or(&transcript.file);
            let mode = if transcript.jetstream {
                "jetstream"
            } else {
                "no-jetstream"
            };
            Ok(Seed {
                name: format!("{stem}.expect-{mode}.bin"),
                bytes,
            })
        })
        .collect()
}

fn read(root: &Path, path: &str) -> Result<Vec<u8>> {
    fs::read(root.join(path)).with_context(|| format!("cannot read {path}"))
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

/// `instance_path`: the paths of the regression test of the target
/// (`crates/core/tests/instance_path_fuzz.rs`) that do not name the
/// temporary folder of the fixture tree, which the harness joins to the
/// fixture root: symlinks out, to another root, in a loop, and to the
/// tree itself, Unicode and spaced names, the 64 level folder, and the data
/// folders.
fn instance_path_seeds(_root: &Path) -> Result<Vec<Seed>> {
    let deep = (0..64)
        .map(|level| format!("d{level}"))
        .collect::<Vec<_>>()
        .join("/");
    let paths: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("slash", b"/".to_vec()),
        ("dot", b".".to_vec()),
        ("dotdot", b"..".to_vec()),
        ("inner-dotdot", b"a/../b".to_vec()),
        ("dot-segment", b"./a".to_vec()),
        ("double-slash", b"//".to_vec()),
        ("inside", b"inside".to_vec()),
        ("link-out", b"link-out".to_vec()),
        ("link-out-below", b"link-out/x/y".to_vec()),
        ("link-other-root", b"link-other-root".to_vec()),
        (
            "link-other-root-below",
            b"link-other-root/anything".to_vec(),
        ),
        ("loop", b"loop-a".to_vec()),
        ("loop-below", b"loop-a/x".to_vec()),
        (
            "link-self-chain",
            b"link-self/link-self/link-self/inside".to_vec(),
        ),
        ("link-inside-dotdot", b"link-inside/../inside".to_vec()),
        ("unicode", "ünï cödé/with space/x".as_bytes().to_vec()),
        ("deep", format!("deep/{deep}").into_bytes()),
        ("deeper", format!("deep/{deep}/more").into_bytes()),
        ("data", b"instances/one/data".to_vec()),
        ("data-below", b"instances/one/data/sub".to_vec()),
        ("data-parent", b"instances/one".to_vec()),
        ("instances", b"instances".to_vec()),
        ("data-sibling", b"instances/one/../two/data".to_vec()),
        ("invalid-utf8", vec![0xff, 0xfe, b'/', 0x80]),
        ("nul-bytes", vec![0; 3]),
        ("nul-inside", b"a\0b".to_vec()),
    ];
    Ok(paths
        .into_iter()
        .map(|(name, bytes)| Seed {
            name: name.to_owned(),
            bytes,
        })
        .collect())
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
    fn server_frame_seeds_include_every_expectation_and_skip_other_types() {
        let seeds = server_frame_fixture_seeds(&repo_root()).unwrap();
        let names: Vec<&str> = seeds.iter().map(|seed| seed.name.as_str()).collect();
        assert!(names.contains(&"frame-response-get-info.json"));
        assert!(names.contains(&"frame-event-unknown-kind.json"));
        assert!(names.contains(&"frame-positional.json"));
        assert!(names.iter().all(|name| name.starts_with("frame-")));
        for seed in &seeds {
            let fixture = read(&repo_root(), &format!("{IPC_FIXTURES}/{}", seed.name)).unwrap();
            assert_eq!(
                seed.bytes, fixture,
                "{} is the fixture unchanged",
                seed.name
            );
        }
    }

    #[test]
    fn provider_seeds_come_from_every_example_package() {
        let root = repo_root();
        let manifests = provider_manifest_seeds(&root).unwrap();
        let names: Vec<&str> = manifests.iter().map(|seed| seed.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "fake.provider.yaml",
                "nats.provider.yaml",
                "traefik.provider.yaml",
                "fake-command.provider.yaml"
            ]
        );
        let templates = provider_template_seeds(&root).unwrap();
        let selector = |name: &str| {
            templates
                .iter()
                .find(|seed| seed.name == name)
                .map(|seed| seed.bytes[0])
                .unwrap()
        };
        assert_eq!(selector("fake.fake.json.tmpl"), 0);
        assert_eq!(selector("nats.nats.conf.tmpl"), 2);
        assert_eq!(selector("traefik.traefik.yaml.tmpl"), 1);
        assert_eq!(selector("traefik.routes.yaml.tmpl"), 3);
        let fields = provider_interpolation_seeds(&root).unwrap();
        let texts: Vec<&[u8]> = fields.iter().map(|seed| seed.bytes.as_slice()).collect();
        assert!(texts.contains(&b"--data=${instance.data}".as_slice()));
        assert!(texts.contains(&b"${instance.config}/dynamic".as_slice()));
        assert!(
            texts
                .iter()
                .all(|text| text.windows(2).any(|pair| pair == b"${"))
        );
        assert_eq!(provider_settings_schema_seeds(&root).unwrap().len(), 4);
    }

    #[test]
    fn nats_seeds_are_the_mode_byte_then_the_transcript() {
        let root = repo_root();
        let seeds = nats_readiness_protocol_seeds(&root).unwrap();
        assert_eq!(seeds.len(), 11);
        let transcript = read(&root, &format!("{NATS_TRANSCRIPTS}/real-js-on.bin")).unwrap();
        for (name, mode) in [
            ("real-js-on.expect-jetstream.bin", 0xff),
            ("real-js-on.expect-no-jetstream.bin", 0xfe),
        ] {
            let seed = seeds.iter().find(|seed| seed.name == name).unwrap();
            assert_eq!(seed.bytes[0], mode, "{name}");
            assert_eq!(seed.bytes[1..], transcript, "{name}");
        }
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
            err.contains("ipc_frame_decode, ipc_request_decode, ipc_server_frame_decode"),
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
