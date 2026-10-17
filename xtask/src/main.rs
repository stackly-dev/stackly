//! Repo tasks for Stackly, run as `cargo xtask <task>`.

mod build_app;
mod check_deps;
mod codegen;
mod evidence;
mod runtime;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  codegen              Regenerate the Swift contract types and the JSON Schemas
  codegen --check      Fail when the committed generated files are out of date
  check-deps           Check crate dependency direction and the unsafe policy
  build-app            Build the macOS app with xcodebuild (scheme Stackly, Debug)
  build-app --release  Same, with the Release configuration
  evidence run --kind <fixture|native> [--out <folder>] -- <command>
                       Run a test command as evidence and write report.json
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
        ["codegen"] => done(codegen::run(&root, codegen::Mode::Write)),
        ["codegen", "--check"] => done(codegen::run(&root, codegen::Mode::Check)),
        ["check-deps"] => done(check_deps::run(&root)),
        ["build-app"] => done(build_app::run(&root, build_app::Configuration::Debug)),
        ["build-app", "--release"] => {
            done(build_app::run(&root, build_app::Configuration::Release))
        }
        ["evidence", rest @ ..] => evidence::run(rest),
        ["runtime", rest @ ..] => runtime::run(&root, rest),
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
