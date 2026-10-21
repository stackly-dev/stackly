//! Repo tasks for Stackly, run as `cargo xtask <task>`.

mod build_app;
mod catalog;
mod check_deps;
mod codegen;
mod evidence;
mod fuzz;
mod provider;
mod runtime;
mod snapshot;
mod ui_test;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  codegen              Regenerate the Swift contract types, the JSON Schemas, the
                       error catalog, the contract digest, and each provider's
                       versions.json from runtimes/inventory.toml
  codegen --check      Fail when the committed generated files (contract, schemas,
                       provider versions.json) are out of date or the contract
                       changed without a new protocol version
  codegen --amend      Same as codegen, but rewrite the digest recorded for the
                       current protocol version (only for a version not on main)
  check-deps           Check crate dependency direction and the unsafe policy
  provider check [<dir>...]
                       Read and validate provider packages (default: every folder
                       in providers/) on macOS; exit 1 with one line per problem
  catalog build --out <dir>
                       Build the bundled catalog from providers/catalog.toml into
                       <dir> (macOS reads the packages); exit 1 with one line per problem
  catalog check [--update [--amend]]
                       Check providers/catalog.toml, its packages, and the revision
                       ledger; --update records the revision, --amend rewrites one
                       not yet on main
  build-app [--release] [--identity <identity>] [--install [<dir>]]
                       Build Stackly.app (stacklyd, stackly, the LaunchAgent plist,
                       man pages) under target/app/<Config>, signed ad hoc unless
                       --identity or STACKLY_SIGN_IDENTITY names one; --install copies
                       it to <dir> (default ~/Applications)
  evidence run --kind <fixture|native> [--out <folder>] -- <command>
                       Run a test command as evidence and write report.json
  evidence check       Check manual records, the baseline, and the native test markers
  evidence new --covers <ID>... --kind <fixture|native> --title <title>
               --expected <sentence> [--subject <provider>@<version>]...
                       Start a NOT_RUN manual record in evidence/manual/
  evidence baseline    Record this Mac as the benchmark baseline (macOS, not a VM)
  fuzz list            List the cargo-fuzz targets in fuzz/
  fuzz build [<target>]
                       Build the fuzz targets with the pinned nightly
  fuzz seed            Rebuild fuzz/seeds/ from the shared fixtures
  fuzz run <target> [--time <seconds>]
                       Fuzz one target, 300 seconds by default
  snapshot review [--base <rev>]
                       Write target/snapshot-review/index.html with the old and new
                       snapshot PNGs since the merge base with <rev> (default main)
  ui-test [--only <Class>[/<method>]]
                       Run the XCUITest target on fixture scenarios (macOS, needs
                       Automation Mode on and an unlocked screen; exit 2 otherwise)
  runtime check        Check runtimes/inventory.toml and evidence/runtime/
  runtime pin --url <https>... [--executable <path>] [--sha256-url <https>]
                       Download a runtime and print its pinned inventory values
  runtime verify [--id <id>]
                       Download every available runtime again and check it
  runtime spike --id <id> --result <file>
                       Run one runtime under a LaunchAgent (macOS); exit 0 PASS,
                       1 FAIL, 2 BLOCKED
  runtime spike --cleanup
                       Remove what interrupted spikes left behind (macOS)
  runtime spike --prune-cache
                       Delete cached downloads the inventory no longer pins
  runtime record <report.json>
                       Keep the native runtime evidence of a clean run
";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let root = repo_root();
    let done = |result: Result<()>| result.map(|()| ExitCode::SUCCESS);
    match args.as_slice() {
        ["codegen"] => done(codegen::run(&root, codegen::Mode::Write { amend: false })),
        ["codegen", "--check"] => done(codegen::run(&root, codegen::Mode::Check)),
        ["codegen", "--amend"] => done(codegen::run(&root, codegen::Mode::Write { amend: true })),
        ["check-deps"] => done(check_deps::run(&root)),
        ["build-app", rest @ ..] => {
            done(build_app::parse_args(rest).and_then(|options| build_app::run(&root, &options)))
        }
        ["evidence", rest @ ..] => evidence::run(&root, rest),
        ["fuzz", rest @ ..] => done(fuzz::run(&root, rest)),
        ["runtime", rest @ ..] => runtime::run(&root, rest),
        ["snapshot", rest @ ..] => snapshot::run(&root, rest),
        ["ui-test", rest @ ..] => ui_test::run(&root, rest),
        ["provider", rest @ ..] => provider::run(&root, rest),
        ["catalog", rest @ ..] => catalog::run(&root, rest),
        ["help" | "--help" | "-h"] => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        [] => bail!("no task given\n\n{USAGE}"),
        _ => bail!("unknown task `{}`\n\n{USAGE}", args.join(" ")),
    }
}

/// The repository root, one level above this crate.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one directory below the repository root")
        .to_path_buf()
}
