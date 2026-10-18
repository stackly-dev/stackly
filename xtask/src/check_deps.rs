//! `cargo xtask check-deps`: the dependency direction and `unsafe` rules
//! from spec 0001 ("Aturan arah dependency"), the `stackly-evidence` rules
//! from spec 0002, and the platform rules and source scan from spec 0004.
//!
//! Only direct dependencies are checked, each with its kind (normal, dev,
//! build). Transitive dependencies are left alone: portability is proven by
//! the cross target `cargo check`, not by a list of banned crates.

mod source_scan;

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use cargo_metadata::{Dependency, DependencyKind, MetadataCommand, Package};

/// Crates that must build for every target, Windows included.
const PORTABLE: [&str; 4] = [
    "stackly-domain",
    "stackly-platform",
    "stackly-protocol",
    "stackly-core",
];
/// Crates that depend on no other Stackly crate. Domain and protocol may
/// still take `stackly-evidence` as a dev dependency.
const LEAVES: [&str; 3] = ["stackly-domain", "stackly-protocol", EVIDENCE];
/// Adapters the portable crates never see.
const ADAPTERS: [&str; 2] = ["stackly-platform-macos", "stackly-storage"];
/// Crates whose source may opt out of the workspace `unsafe_code` lint.
/// Add `stackly-storage` only once it is proven to need `unsafe`.
const UNSAFE_ALLOWED: [&str; 1] = ["stackly-platform-macos"];
const MACOS_ADAPTER: &str = "stackly-platform-macos";
const MACOS_ONLY_TARGET: &str = r#"cfg(target_os = "macos")"#;
const TESTKIT: &str = "stackly-testkit";
const EVIDENCE: &str = "stackly-evidence";
/// The only crates that may take `stackly-evidence` as a normal dependency.
const EVIDENCE_NORMAL_USERS: [&str; 2] = ["xtask", TESTKIT];
const UNSAFE_LINT: &str = "unsafe_code";

pub fn run(root: &Path) -> Result<()> {
    let (crates, violations) = find_violations(root)?;
    if violations.is_empty() {
        println!("check-deps: {crates} crates follow the dependency, unsafe, and source rules");
        return Ok(());
    }
    for violation in &violations {
        eprintln!("  {violation}");
    }
    bail!(
        "{} dependency rule violation(s), see specs 0001, 0002, and 0004",
        violations.len()
    )
}

/// Checks the workspace at `root` and returns its crate count and every
/// violation.
fn find_violations(root: &Path) -> Result<(usize, Vec<String>)> {
    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .context("cannot read `cargo metadata`")?;
    let packages = metadata.workspace_packages();

    let mut violations = check_workspace_lints(root)?;
    for package in &packages {
        for dependency in &package.dependencies {
            check_dependency(package.name.as_str(), dependency, &mut violations);
        }
        check_lints_inherited(package, &mut violations)?;
        if !UNSAFE_ALLOWED.contains(&package.name.as_str()) {
            check_unsafe_opt_out(package, &mut violations)?;
        }
        if source_scan::SCANNED.contains(&package.name.as_str()) {
            violations.extend(source_scan::scan_package(root, package)?);
        }
    }
    Ok((packages.len(), violations))
}

fn check_dependency(package: &str, dependency: &Dependency, violations: &mut Vec<String>) {
    let name = dependency.name.as_str();
    let target = dependency.target.as_ref().map(ToString::to_string);
    let mut deny = |rule: &str| {
        violations.push(format!(
            "{package} has a {} dependency on {name}: {rule}",
            kind_label(dependency.kind)
        ));
    };

    if PORTABLE.contains(&package) {
        if ADAPTERS.contains(&name) {
            deny("portable crates never depend on an adapter");
        }
        if target.as_deref().is_some_and(is_apple_target) {
            deny("portable crates never take a dependency that only exists on Apple targets");
        }
    }
    if LEAVES.contains(&package) {
        let dev_evidence = package != EVIDENCE
            && name == EVIDENCE
            && dependency.kind == DependencyKind::Development;
        if is_stackly_crate(name) && !dev_evidence {
            deny(if package == EVIDENCE {
                "stackly-evidence depends on no Stackly crate"
            } else {
                "this crate depends on no other Stackly crate, except stackly-evidence as a dev dependency"
            });
        }
        if matches!(name, "tokio" | "libc") {
            deny("tokio and libc are never direct dependencies of this crate");
        }
    }
    if package == "stackly-platform" && matches!(name, "tokio" | "libc") {
        deny("stackly-platform holds only portable traits and path logic, so never tokio or libc");
    }
    if package == "stackly-core" && matches!(name, "stackly-protocol" | "stackly-storage") {
        deny("stacklyd maps domain types to DTOs, so core never sees the protocol or storage");
    }
    if package == "stackly-cli" && matches!(name, "stackly-core" | "stackly-storage") {
        deny("the CLI never runs provider binaries or writes the registry");
    }
    if package == TESTKIT && name == "stackly-core" {
        deny("testkit never depends on core, so core tests see one copy of each type");
    }
    if name == TESTKIT && dependency.kind != DependencyKind::Development {
        deny("stackly-testkit is only ever a dev dependency");
    }
    if name == EVIDENCE {
        let allowed = dependency.kind == DependencyKind::Development
            || (dependency.kind == DependencyKind::Normal
                && EVIDENCE_NORMAL_USERS.contains(&package));
        if !allowed {
            deny(
                "stackly-evidence is a dev dependency everywhere except xtask and stackly-testkit",
            );
        }
    }
    if name == MACOS_ADAPTER && target.as_deref() != Some(MACOS_ONLY_TARGET) {
        deny("declare it under [target.'cfg(target_os = \"macos\")'.dependencies]");
    }
}

/// The workspace lint table must keep `unsafe_code` denied.
fn check_workspace_lints(root: &Path) -> Result<Vec<String>> {
    let manifest = read_manifest(&root.join("Cargo.toml"))?;
    let level = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("lints"))
        .and_then(|lints| lints.get("rust"))
        .and_then(|rust| rust.get(UNSAFE_LINT))
        .and_then(toml::Value::as_str);
    Ok(match level {
        Some("deny") => Vec::new(),
        _ => vec![format!(
            "workspace Cargo.toml must set [workspace.lints.rust] {UNSAFE_LINT} = \"deny\""
        )],
    })
}

/// Every member inherits the workspace lints instead of writing its own.
fn check_lints_inherited(package: &Package, violations: &mut Vec<String>) -> Result<()> {
    let manifest = read_manifest(package.manifest_path.as_std_path())?;
    let inherits = manifest
        .get("lints")
        .and_then(|lints| lints.get("workspace"))
        .and_then(toml::Value::as_bool);
    if inherits != Some(true) {
        violations.push(format!(
            "{} must inherit the workspace lints with `[lints] workspace = true`",
            package.name
        ));
    }
    Ok(())
}

/// Only the crates in [`UNSAFE_ALLOWED`] may switch the `unsafe_code` lint
/// back on in source.
fn check_unsafe_opt_out(package: &Package, violations: &mut Vec<String>) -> Result<()> {
    let dir = package
        .manifest_path
        .parent()
        .context("a manifest path always has a parent directory")?;
    let mut files = Vec::new();
    collect_rust_files(dir.as_std_path(), &mut files)?;
    for file in files {
        let source =
            fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
        if opts_out_of_unsafe_lint(&source) {
            violations.push(format!(
                "{} re-enables `{UNSAFE_LINT}` in {}; only {} may",
                package.name,
                file.display(),
                UNSAFE_ALLOWED.join(", ")
            ));
        }
    }
    Ok(())
}

fn collect_rust_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect_rust_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// True when an `allow(...)` or `expect(...)` attribute names the unsafe
/// lint, whatever the spacing or the other lints listed with it.
fn opts_out_of_unsafe_lint(source: &str) -> bool {
    let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    ["allow(", "expect("].iter().any(|attribute| {
        compact.match_indices(attribute).any(|(start, _)| {
            let args = &compact[start + attribute.len()..];
            let end = args.find(')').unwrap_or(args.len());
            args[..end].split(',').any(|lint| lint == UNSAFE_LINT)
        })
    })
}

fn read_manifest(path: &Path) -> Result<toml::Table> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    text.parse()
        .with_context(|| format!("cannot parse {}", path.display()))
}

fn is_stackly_crate(name: &str) -> bool {
    name.starts_with("stackly-") || name == "stacklyd"
}

fn is_apple_target(target: &str) -> bool {
    ["macos", "ios", "apple"]
        .iter()
        .any(|apple| target.contains(apple))
}

fn kind_label(kind: DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Normal => "normal",
        DependencyKind::Development => "dev",
        DependencyKind::Build => "build",
        _ => "unknown kind of",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One direct dependency as `cargo metadata` reports it. `kind` is null
    /// for a normal dependency, otherwise `"dev"` or `"build"`.
    fn dependency(name: &str, kind: Option<&str>) -> Dependency {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "source": null,
            "req": "*",
            "kind": kind,
            "optional": false,
            "uses_default_features": true,
            "features": [],
            "target": null,
            "rename": null,
            "registry": null,
            "path": null,
        }))
        .unwrap()
    }

    fn violations(package: &str, name: &str, kind: Option<&str>) -> Vec<String> {
        let mut violations = Vec::new();
        check_dependency(package, &dependency(name, kind), &mut violations);
        violations
    }

    #[test]
    fn stackly_evidence_is_a_normal_dependency_only_of_xtask_and_testkit() {
        assert!(violations("xtask", EVIDENCE, None).is_empty());
        assert!(violations(TESTKIT, EVIDENCE, None).is_empty());
        assert_eq!(
            violations("stackly-core", EVIDENCE, None),
            ["stackly-core has a normal dependency on stackly-evidence: \
              stackly-evidence is a dev dependency everywhere except xtask and stackly-testkit"]
        );
        assert_eq!(
            violations("stacklyd", EVIDENCE, Some("build")),
            ["stacklyd has a build dependency on stackly-evidence: \
              stackly-evidence is a dev dependency everywhere except xtask and stackly-testkit"]
        );
    }

    #[test]
    fn every_crate_may_take_stackly_evidence_as_a_dev_dependency() {
        for package in [
            "stackly-domain",
            "stackly-protocol",
            "stackly-platform",
            "stackly-core",
            "stackly-platform-macos",
            "stackly-storage",
            "stacklyd",
            "stackly-cli",
        ] {
            assert!(
                violations(package, EVIDENCE, Some("dev")).is_empty(),
                "{package}"
            );
        }
    }

    #[test]
    fn domain_and_protocol_take_no_other_stackly_crate_even_for_tests() {
        for package in ["stackly-domain", "stackly-protocol"] {
            let normal = violations(package, EVIDENCE, None);
            assert!(
                normal.iter().any(|v| v.ends_with(
                    "this crate depends on no other Stackly crate, except stackly-evidence as a dev dependency"
                )),
                "{package}: {normal:?}"
            );
            assert!(
                !violations(package, TESTKIT, Some("dev")).is_empty(),
                "{package}"
            );
        }
    }

    #[test]
    fn stackly_evidence_depends_on_no_stackly_crate_tokio_or_libc() {
        for name in ["stackly-domain", TESTKIT, "stacklyd"] {
            for kind in [None, Some("dev")] {
                let found = violations(EVIDENCE, name, kind);
                assert!(
                    found
                        .iter()
                        .any(|v| v.ends_with("stackly-evidence depends on no Stackly crate")),
                    "{name} {kind:?}: {found:?}"
                );
            }
        }
        for name in ["tokio", "libc"] {
            assert_eq!(
                violations(EVIDENCE, name, None),
                [format!(
                    "stackly-evidence has a normal dependency on {name}: \
                     tokio and libc are never direct dependencies of this crate"
                )]
            );
        }
        assert!(violations(EVIDENCE, "serde", None).is_empty());
        assert!(violations(EVIDENCE, "tempfile", None).is_empty());
    }

    #[test]
    fn stackly_platform_takes_neither_tokio_nor_libc() {
        for name in ["tokio", "libc"] {
            for (kind, label) in [(None, "normal"), (Some("dev"), "dev")] {
                assert_eq!(
                    violations("stackly-platform", name, kind),
                    [format!(
                        "stackly-platform has a {label} dependency on {name}: \
                         stackly-platform holds only portable traits and path logic, so never tokio or libc"
                    )]
                );
            }
        }
        assert!(violations("stackly-platform", "jiff", None).is_empty());
        assert!(violations("stackly-platform-macos", "libc", None).is_empty());
    }

    /// Writes a one crate workspace named `stackly-core` whose `src/lib.rs`
    /// is `lib`, and returns its root.
    fn fixture_workspace(lib: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/core\"]\n\n\
             [workspace.lints.rust]\nunsafe_code = \"deny\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("crates/core/src")).unwrap();
        fs::write(
            root.join("crates/core/Cargo.toml"),
            "[package]\nname = \"stackly-core\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
             [lints]\nworkspace = true\n",
        )
        .unwrap();
        fs::write(root.join("crates/core/src/lib.rs"), lib).unwrap();
        dir
    }

    #[test]
    fn a_crate_that_reads_the_environment_or_names_homebrew_fails_with_file_and_line() {
        let workspace = fixture_workspace(
            "//! A fixture crate.\n\
             pub fn root() -> Option<std::ffi::OsString> {\n\
                 std::env::var_os(\"HOME\")\n\
             }\n\
             pub const BREW: &str = \"/opt/homebrew\";\n",
        );

        let (crates, found) = find_violations(workspace.path()).unwrap();

        assert_eq!(crates, 1);
        assert_eq!(
            found,
            [
                "stackly-core crates/core/src/lib.rs:3: library crates never read the environment \
                 (`env::var_os`); a bin reads it once in main and passes the value in",
                "stackly-core crates/core/src/lib.rs:5: library crates never hardcode a machine path \
                 (`/opt/homebrew`); derive it from DirectoryLayout",
            ]
        );
    }

    #[test]
    fn a_clean_fixture_crate_passes() {
        let workspace =
            fixture_workspace("//! A fixture crate.\npub const NAME: &str = \"stackly\";\n");

        let (crates, found) = find_violations(workspace.path()).unwrap();

        assert_eq!((crates, found), (1, Vec::<String>::new()));
    }

    #[test]
    fn a_bin_target_is_scanned_but_tests_examples_and_benches_are_not() {
        let workspace = fixture_workspace("//! A fixture crate.\n");
        let core = workspace.path().join("crates/core");
        let leak = "fn main() { let _ = std::env::var(\"X\"); let _ = \"/tmp\"; }\n";
        fs::write(core.join("src/main.rs"), leak).unwrap();
        for dir in ["tests", "examples", "benches"] {
            fs::create_dir_all(core.join(dir)).unwrap();
            fs::write(core.join(dir).join("leak.rs"), leak).unwrap();
        }

        let (_, found) = find_violations(workspace.path()).unwrap();

        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found
                .iter()
                .all(|message| message.starts_with("stackly-core crates/core/src/main.rs:1: ")),
            "{found:?}"
        );
    }

    #[test]
    fn every_scanned_crate_is_a_member_of_this_workspace() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask sits one directory below the repository root");
        let metadata = MetadataCommand::new()
            .manifest_path(root.join("Cargo.toml"))
            .no_deps()
            .exec()
            .unwrap();
        let members: Vec<&str> = metadata
            .workspace_packages()
            .iter()
            .map(|package| package.name.as_str())
            .collect();

        for name in source_scan::SCANNED {
            assert!(
                members.contains(&name),
                "{name} is not a workspace member, so its source would never be scanned"
            );
        }
    }
}
