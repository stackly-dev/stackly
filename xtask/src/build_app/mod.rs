//! `cargo xtask build-app [--release] [--identity <identity>] [--install
//! [<dir>]]` (spec 0015): the one assembler of a complete `Stackly.app`.
//!
//! The steps run in a staging folder under `target/app` and the finished
//! bundle replaces `target/app/<Config>/Stackly.app` by rename, so that path
//! never holds a half built bundle: a step that fails removes the staging
//! folder and the task exits non zero before anything was signed or
//! published.
//!
//! No step registers an agent, touches Login Items, or writes outside
//! `target/` and the install folder.

mod assemble;
mod check;
mod install;
mod plist;
mod sign;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

pub use install::default_install_dir;

const PROJECT: &str = "apps/macos/Stackly.xcodeproj";
const SCHEME: &str = "Stackly";
/// Kept under `target/` so app builds stay out of git and out of
/// `~/Library/Developer/Xcode/DerivedData`.
const DERIVED_DATA: &str = "target/xcode";
/// Where finished and staged bundles go, under `target/`.
const APP_DIR: &str = "target/app";
const BUNDLE_NAME: &str = "Stackly.app";
/// The prefix of a staging folder under [`APP_DIR`].
const STAGE_PREFIX: &str = ".stage-";

/// The build configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Configuration {
    Debug,
    Release,
}

impl Configuration {
    const fn name(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Release => "Release",
        }
    }
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Debug or Release.
    pub configuration: Configuration,
    /// `--identity`, else `STACKLY_SIGN_IDENTITY`, else ad hoc.
    pub identity: Option<String>,
    /// `--install [<dir>]`: `Some(None)` for the default folder.
    pub install: Option<Option<PathBuf>>,
}

/// The two executables `cargo build` made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binaries {
    /// `stacklyd`.
    pub daemon: PathBuf,
    /// `stackly`.
    pub cli: PathBuf,
}

/// Parses the arguments after `build-app`.
///
/// # Errors
///
/// An unknown flag, or `--identity` without a value.
pub fn parse_args(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        configuration: Configuration::Debug,
        identity: None,
        install: None,
    };
    let mut args = args.iter().copied().peekable();
    while let Some(arg) = args.next() {
        match arg {
            "--release" => options.configuration = Configuration::Release,
            "--identity" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--identity needs a signing identity"))?;
                options.identity = Some(value.to_owned());
            }
            "--install" => {
                let dir = args
                    .next_if(|next| !next.starts_with("--"))
                    .map(PathBuf::from);
                options.install = Some(dir);
            }
            other => bail!("unknown build-app argument `{other}`"),
        }
    }
    Ok(options)
}

/// Runs the build.
///
/// # Errors
///
/// The first step that fails; the staging folder is removed first.
pub fn run(root: &Path, options: &Options) -> Result<()> {
    let config = options.configuration.name();
    // Before any long step: a layout that cannot exist on this volume.
    check::names_are_distinct()?;
    let app_dir = root.join(APP_DIR);
    std::fs::create_dir_all(&app_dir)
        .with_context(|| format!("cannot create {}", app_dir.display()))?;
    // Step 0: staging folders an interrupted build left behind.
    clean_stages(&app_dir)?;

    let version = workspace_version(root)?;
    // Step 1: the Rust executables.
    let binaries = build_rust(root, options.configuration)?;
    // Step 2: the app, unsigned.
    let product = build_xcode(root, options.configuration, &version)?;

    let output_dir = app_dir.join(config);
    let identity = sign::identity(options.identity.as_deref());
    let finished = publish(&app_dir, &output_dir, |stage| {
        let bundle = stage.join(BUNDLE_NAME);
        ditto(&product, &bundle)?;
        // Step 3: assembly.
        assemble::assemble(&bundle, &binaries, root)?;
        // Step 4: what the bundle holds.
        check::contents(&bundle, &version)?;
        // Steps 5 and 6: signing and its proof.
        sign::sign(&bundle, &identity)?;
        sign::verify(&bundle)?;
        Ok(())
    })?;
    // Step 7 done: the path to print is ours, not Xcode's.
    println!("built {}", finished.display());

    // Step 8.
    if let Some(dir) = &options.install {
        let dir = match dir {
            Some(dir) => dir.clone(),
            None => default_install_dir()?,
        };
        install::install(&finished, &dir)?;
    }
    Ok(())
}

/// Removes the `.stage-*` folders under `app_dir`, and only those.
fn clean_stages(app_dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(app_dir)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGE_PREFIX)
        {
            std::fs::remove_dir_all(entry.path())
                .with_context(|| format!("cannot remove {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Makes a staging folder under `app_dir`, runs `build` in it, and on
/// success moves `Stackly.app` from it to `output_dir`, replacing the old
/// bundle by rename. On failure the staging folder is removed and
/// `output_dir` stays as it was. Returns the path of the published bundle.
fn publish(
    app_dir: &Path,
    output_dir: &Path,
    build: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    let stage = app_dir.join(format!("{STAGE_PREFIX}{}", std::process::id()));
    if stage.exists() {
        std::fs::remove_dir_all(&stage)?;
    }
    std::fs::create_dir_all(&stage)?;
    let result = build(&stage).and_then(|()| swap(&stage.join(BUNDLE_NAME), output_dir));
    let _ = std::fs::remove_dir_all(&stage);
    result
}

/// Puts `bundle` at `output_dir/Stackly.app`: the old bundle is moved
/// aside, the new one renamed in, and the old one removed.
fn swap(bundle: &Path, output_dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(output_dir)?;
    let target = output_dir.join(BUNDLE_NAME);
    let old = output_dir.join(format!(".{BUNDLE_NAME}.old-{}", std::process::id()));
    let had_old = target.exists();
    if had_old {
        std::fs::rename(&target, &old)
            .with_context(|| format!("cannot move {} aside", target.display()))?;
    }
    if let Err(error) = std::fs::rename(bundle, &target) {
        if had_old {
            let _ = std::fs::rename(&old, &target);
        }
        return Err(error)
            .with_context(|| format!("cannot move the bundle to {}", target.display()));
    }
    if had_old {
        std::fs::remove_dir_all(&old)
            .with_context(|| format!("cannot remove {}", old.display()))?;
    }
    Ok(target)
}

/// The workspace version, the one `stacklyd --version` prints.
fn workspace_version(root: &Path) -> Result<String> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .no_deps()
        .exec()
        .context("cannot read the cargo metadata")?;
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name == "stacklyd")
        .ok_or_else(|| anyhow!("the workspace has no stacklyd package"))?;
    Ok(package.version.to_string())
}

/// Builds `stacklyd` and `stackly` and finds their paths in the
/// `compiler-artifact` messages.
fn build_rust(root: &Path, configuration: Configuration) -> Result<Binaries> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.current_dir(root).args([
        "build",
        "--locked",
        "--message-format=json",
        "-p",
        "stacklyd",
        "-p",
        "stackly-cli",
    ]);
    if configuration == Configuration::Release {
        command.arg("--release");
    }
    let output = command
        .stderr(Stdio::inherit())
        .output()
        .context("cannot run cargo build")?;
    if !output.status.success() {
        bail!("cargo build failed with {}", output.status);
    }
    binaries_from(&String::from_utf8_lossy(&output.stdout))
}

/// Reads the executable paths of `stacklyd` and `stackly` from the JSON
/// lines `cargo build --message-format=json` wrote.
///
/// # Errors
///
/// A binary has no `compiler-artifact` message with an executable.
fn binaries_from(json_lines: &str) -> Result<Binaries> {
    let mut daemon = None;
    let mut cli = None;
    for line in json_lines.lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(executable) = message["executable"].as_str() else {
            continue;
        };
        let is_bin = message["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"));
        if !is_bin {
            continue;
        }
        match message["target"]["name"].as_str() {
            Some("stacklyd") => daemon = Some(PathBuf::from(executable)),
            Some("stackly") => cli = Some(PathBuf::from(executable)),
            _ => {}
        }
    }
    Ok(Binaries {
        daemon: daemon.ok_or_else(|| anyhow!("cargo built no stacklyd executable"))?,
        cli: cli.ok_or_else(|| anyhow!("cargo built no stackly executable"))?,
    })
}

/// Builds the app with `xcodebuild`, unsigned and without debug dylibs, and
/// returns the product path.
fn build_xcode(root: &Path, configuration: Configuration, version: &str) -> Result<PathBuf> {
    let config = configuration.name();
    let status = Command::new("xcodebuild")
        .current_dir(root)
        .args([
            "-project",
            PROJECT,
            "-scheme",
            SCHEME,
            "-configuration",
            config,
        ])
        .args(["-destination", "platform=macOS,arch=arm64"])
        .args(["-derivedDataPath", DERIVED_DATA, "build"])
        .args([
            "CODE_SIGNING_ALLOWED=NO",
            "ENABLE_DEBUG_DYLIB=NO",
            &format!("MARKETING_VERSION={version}"),
        ])
        .status()
        .context("cannot run xcodebuild; install Xcode 27 and select it with `xcode-select`")?;
    if !status.success() {
        bail!("xcodebuild failed with {status}");
    }
    let product = root
        .join(DERIVED_DATA)
        .join("Build/Products")
        .join(config)
        .join(BUNDLE_NAME);
    if !product.is_dir() {
        bail!("xcodebuild did not produce {}", product.display());
    }
    Ok(product)
}

/// Copies `from` to `to` with `ditto`, which keeps modes and symlinks.
fn ditto(from: &Path, to: &Path) -> Result<()> {
    let status = Command::new("ditto")
        .arg(from)
        .arg(to)
        .status()
        .context("cannot run ditto")?;
    if !status.success() {
        bail!(
            "ditto {} {} failed with {status}",
            from.display(),
            to.display()
        );
    }
    Ok(())
}

/// Runs `command` and returns its trimmed stdout, failing with its stderr.
fn capture(command: &mut Command) -> Result<String> {
    let output = command.output().context("cannot run a helper tool")?;
    if !output.status.success() {
        bail!(
            "{:?} failed with {}: {}",
            command.get_program(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flags_parse_in_any_order_and_install_takes_an_optional_folder() {
        assert_eq!(
            parse_args(&[]).unwrap(),
            Options {
                configuration: Configuration::Debug,
                identity: None,
                install: None
            }
        );
        let options = parse_args(&["--install", "--release", "--identity", "Dev ID"]).unwrap();
        assert_eq!(options.configuration, Configuration::Release);
        assert_eq!(options.identity.as_deref(), Some("Dev ID"));
        assert_eq!(options.install, Some(None));
        let options = parse_args(&["--install", "/tmp/apps", "--release"]).unwrap();
        assert_eq!(options.install, Some(Some(PathBuf::from("/tmp/apps"))));
        assert!(parse_args(&["--identity"]).is_err());
        assert!(parse_args(&["--nope"]).is_err());
    }

    #[test]
    fn executables_come_from_the_compiler_artifact_messages() {
        let lines = [
            r#"{"reason":"compiler-message","message":{}}"#,
            r#"{"reason":"compiler-artifact","target":{"name":"stackly_ipc","kind":["lib"]},"executable":null}"#,
            r#"{"reason":"compiler-artifact","target":{"name":"stacklyd","kind":["bin"]},"executable":"/t/debug/stacklyd"}"#,
            r#"{"reason":"compiler-artifact","target":{"name":"stackly","kind":["bin"]},"executable":"/t/debug/stackly"}"#,
            r#"{"reason":"build-finished","success":true}"#,
        ]
        .join("\n");
        assert_eq!(
            binaries_from(&lines).unwrap(),
            Binaries {
                daemon: PathBuf::from("/t/debug/stacklyd"),
                cli: PathBuf::from("/t/debug/stackly")
            }
        );
        assert!(binaries_from(r#"{"reason":"build-finished","success":true}"#).is_err());
    }

    #[test]
    fn a_failing_step_leaves_the_output_path_as_it_was_and_no_staging_folder() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("app");
        let output = app_dir.join("Debug");
        std::fs::create_dir_all(output.join(BUNDLE_NAME)).unwrap();
        std::fs::write(output.join(BUNDLE_NAME).join("old"), "old").unwrap();

        let result = publish(&app_dir, &output, |stage| {
            std::fs::create_dir_all(stage.join(BUNDLE_NAME))?;
            std::fs::write(stage.join(BUNDLE_NAME).join("new"), "new")?;
            bail!("the man page step failed")
        });

        assert!(result.is_err());
        assert!(
            output.join(BUNDLE_NAME).join("old").is_file(),
            "the old bundle stays"
        );
        assert!(!output.join(BUNDLE_NAME).join("new").exists());
        let leftovers: Vec<_> = std::fs::read_dir(&app_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["Debug"], "no staging folder is left");
    }

    #[test]
    fn a_failing_first_build_leaves_no_bundle_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("app");
        let output = app_dir.join("Release");

        let result = publish(&app_dir, &output, |_| bail!("a step failed"));

        assert!(result.is_err());
        assert!(!output.join(BUNDLE_NAME).exists());
    }

    #[test]
    fn a_good_build_replaces_the_old_bundle_without_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("app");
        let output = app_dir.join("Debug");
        std::fs::create_dir_all(output.join(BUNDLE_NAME)).unwrap();
        std::fs::write(output.join(BUNDLE_NAME).join("old"), "old").unwrap();

        let published = publish(&app_dir, &output, |stage| {
            std::fs::create_dir_all(stage.join(BUNDLE_NAME))?;
            std::fs::write(stage.join(BUNDLE_NAME).join("new"), "new")?;
            Ok(())
        })
        .unwrap();

        assert_eq!(published, output.join(BUNDLE_NAME));
        assert!(published.join("new").is_file());
        assert!(!published.join("old").exists());
        let mut names: Vec<_> = std::fs::read_dir(&output)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, [BUNDLE_NAME], "the old bundle is gone");
    }

    #[test]
    fn only_staging_folders_are_cleaned() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".stage-1/Stackly.app")).unwrap();
        std::fs::create_dir_all(dir.path().join("Debug/Stackly.app")).unwrap();

        clean_stages(dir.path()).unwrap();

        assert!(!dir.path().join(".stage-1").exists());
        assert!(dir.path().join("Debug/Stackly.app").is_dir());
    }
}
