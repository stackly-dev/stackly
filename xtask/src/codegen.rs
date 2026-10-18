//! `cargo xtask codegen [--check]`: Swift contract types and JSON Schemas
//! generated from `stackly-protocol`, plus the evidence JSON Schemas
//! generated from `stackly-evidence` (spec 0002).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The `typeshare-cli` release the committed Swift file was generated with.
const TYPESHARE_CLI_VERSION: &str = "1.13.4";
const PROTOCOL_SRC: &str = "crates/protocol/src";
const TYPESHARE_CONFIG: &str = "typeshare.toml";
const SWIFT_OUTPUT: &str = "clients/swift/Sources/StacklyClient/Generated/StacklyProtocol.swift";
const SCHEMA_DIR: &str = "protocols/schemas";
const EVIDENCE_SCHEMA_DIR: &str = "schemas/evidence";
/// Every folder whose `*.schema.json` files are all generated.
const SCHEMA_DIRS: [&str; 2] = [SCHEMA_DIR, EVIDENCE_SCHEMA_DIR];
const SCHEMA_SUFFIX: &str = ".schema.json";

/// Whether to write the generated files or only compare them.
pub enum Mode {
    Write,
    Check,
}

/// One generated file: its path relative to the repo root and its contents.
struct Generated {
    path: PathBuf,
    contents: String,
}

pub fn run(root: &Path, mode: Mode) -> Result<()> {
    check_typeshare_version()?;
    let mut generated = vec![generate_swift(root)?];
    generated.extend(generate_schemas()?);
    match mode {
        Mode::Write => write(root, &generated),
        Mode::Check => check(root, &generated),
    }
}

fn check_typeshare_version() -> Result<()> {
    let install = format!("cargo install --locked typeshare-cli --version {TYPESHARE_CLI_VERSION}");
    let output = Command::new("typeshare")
        .arg("--version")
        .output()
        .with_context(|| format!("cannot run `typeshare`; install it with `{install}`"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let found = stdout.split_whitespace().nth(1).unwrap_or("unknown");
    if !output.status.success() || found != TYPESHARE_CLI_VERSION {
        bail!(
            "typeshare-cli {TYPESHARE_CLI_VERSION} is required, found {found}; install it with `{install}`"
        );
    }
    Ok(())
}

fn generate_swift(root: &Path) -> Result<Generated> {
    let staging = tempfile::tempdir().context("cannot create a staging directory")?;
    let output = staging.path().join("StacklyProtocol.swift");
    let status = Command::new("typeshare")
        .arg(root.join(PROTOCOL_SRC))
        .args(["--lang", "swift"])
        .arg("--config-file")
        .arg(root.join(TYPESHARE_CONFIG))
        .arg("--output-file")
        .arg(&output)
        // typeshare logs every step at info level; keep only real problems.
        .env("RUST_LOG", "error")
        .status()
        .context("cannot run `typeshare`")?;
    if !status.success() {
        bail!("typeshare failed with {status}");
    }
    let contents =
        fs::read_to_string(&output).with_context(|| format!("cannot read {}", output.display()))?;
    Ok(Generated {
        path: PathBuf::from(SWIFT_OUTPUT),
        contents,
    })
}

fn generate_schemas() -> Result<Vec<Generated>> {
    let protocol = schema_files(SCHEMA_DIR, stackly_protocol::json_schemas())?;
    let evidence = schema_files(EVIDENCE_SCHEMA_DIR, stackly_evidence::json_schemas())?;
    Ok(protocol.into_iter().chain(evidence).collect())
}

fn schema_files(dir: &str, schemas: Vec<(&str, schemars::Schema)>) -> Result<Vec<Generated>> {
    schemas
        .into_iter()
        .map(|(stem, schema)| {
            let mut contents = serde_json::to_string_pretty(&schema)
                .with_context(|| format!("cannot serialize the {stem} schema"))?;
            contents.push('\n');
            Ok(Generated {
                path: Path::new(dir).join(format!("{stem}{SCHEMA_SUFFIX}")),
                contents,
            })
        })
        .collect()
}

fn write(root: &Path, generated: &[Generated]) -> Result<()> {
    for file in generated {
        let path = root.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        fs::write(&path, &file.contents)
            .with_context(|| format!("cannot write {}", path.display()))?;
        println!("wrote {}", file.path.display());
    }
    for stale in stale_schemas(root, generated)? {
        fs::remove_file(root.join(&stale))
            .with_context(|| format!("cannot remove {}", stale.display()))?;
        println!("removed {}", stale.display());
    }
    Ok(())
}

fn check(root: &Path, generated: &[Generated]) -> Result<()> {
    let mut drifted = Vec::new();
    for file in generated {
        let on_disk = fs::read_to_string(root.join(&file.path)).ok();
        if on_disk.as_deref() != Some(file.contents.as_str()) {
            drifted.push(file.path.clone());
        }
    }
    drifted.extend(stale_schemas(root, generated)?);
    if drifted.is_empty() {
        println!(
            "codegen: {} generated files are up to date",
            generated.len()
        );
        return Ok(());
    }
    for path in &drifted {
        eprintln!("  out of date: {}", path.display());
    }
    bail!(
        "generated files drifted from stackly-protocol or stackly-evidence; run `cargo xtask codegen` and commit the result"
    )
}

/// Schema files on disk that no type generates any more. Only the top level
/// of each schema folder is generated; `schemas/evidence/examples/` holds
/// hand written files and is left alone.
fn stale_schemas(root: &Path, generated: &[Generated]) -> Result<Vec<PathBuf>> {
    let mut stale = Vec::new();
    for schema_dir in SCHEMA_DIRS {
        let dir = root.join(schema_dir);
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
            let name = entry?.file_name();
            let Some(name) = name.to_str() else { continue };
            let path = Path::new(schema_dir).join(name);
            if name.ends_with(SCHEMA_SUFFIX) && !generated.iter().any(|file| file.path == path) {
                stale.push(path);
            }
        }
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask sits one directory below the repository root")
    }

    /// The generated files that belong to `schemas/evidence/`.
    fn evidence_schemas() -> Vec<Generated> {
        generate_schemas()
            .unwrap()
            .into_iter()
            .filter(|file| file.path.starts_with(EVIDENCE_SCHEMA_DIR))
            .collect()
    }

    #[test]
    fn the_four_evidence_schemas_are_generated_into_schemas_evidence() {
        let mut paths: Vec<_> = evidence_schemas()
            .into_iter()
            .map(|file| file.path)
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            [
                "schemas/evidence/baseline-host.schema.json",
                "schemas/evidence/record.schema.json",
                "schemas/evidence/report.schema.json",
                "schemas/evidence/run-context.schema.json",
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn committed_evidence_schemas_match_the_rust_types() {
        for file in evidence_schemas() {
            let on_disk = fs::read_to_string(repo_root().join(&file.path)).unwrap();
            assert!(
                on_disk == file.contents,
                "{} drifted from stackly-evidence; run `cargo xtask codegen`",
                file.path.display()
            );
        }
    }

    #[test]
    fn check_flags_a_drifted_schema_and_a_stale_one_but_never_the_examples() {
        let root = tempfile::tempdir().unwrap();
        let generated = evidence_schemas();
        write(root.path(), &generated).unwrap();
        let examples = root.path().join(EVIDENCE_SCHEMA_DIR).join("examples");
        fs::create_dir(&examples).unwrap();
        fs::write(examples.join("record-rust-fixture-pass.json"), "{}\n").unwrap();
        assert!(check(root.path(), &generated).is_ok());

        let record = root.path().join("schemas/evidence/record.schema.json");
        fs::write(&record, "{}\n").unwrap();
        assert!(check(root.path(), &generated).is_err());

        write(root.path(), &generated).unwrap();
        let stale = root.path().join("schemas/evidence/old.schema.json");
        fs::write(&stale, "{}\n").unwrap();
        assert_eq!(
            stale_schemas(root.path(), &generated).unwrap(),
            [PathBuf::from("schemas/evidence/old.schema.json")]
        );
        assert!(check(root.path(), &generated).is_err());

        write(root.path(), &generated).unwrap();
        assert!(!stale.exists(), "write removes a schema no type generates");
        assert!(examples.join("record-rust-fixture-pass.json").exists());
        assert!(check(root.path(), &generated).is_ok());
    }
}
