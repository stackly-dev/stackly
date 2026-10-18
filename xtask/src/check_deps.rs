//! `cargo xtask check-deps`: the dependency direction and `unsafe` rules
//! from spec 0001 ("Aturan arah dependency"), the `stackly-evidence` rules
//! from spec 0002, and the `stackly-storage` and `rusqlite` rules from spec
//! 0005.
//!
//! Only direct dependencies are checked, each with its kind (normal, dev,
//! build). Transitive dependencies are left alone: portability is proven by
//! the cross target `cargo check`, not by a list of banned crates.

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
const UNSAFE_ALLOWED: [&str; 1] = ["stackly-platform-macos"];
/// The registry adapter, the only crate that opens SQLite.
const STORAGE: &str = "stackly-storage";
/// The Stackly crates `stackly-storage` may depend on.
const STORAGE_NORMAL_DEPS: [&str; 2] = ["stackly-domain", "stackly-platform"];
/// The Stackly crates `stackly-storage` may take as dev dependencies, on
/// top of [`STORAGE_NORMAL_DEPS`].
const STORAGE_DEV_DEPS: [&str; 2] = [TESTKIT, EVIDENCE];
/// The workspace binaries; of them, only `stacklyd` may use storage.
const BINARIES: [&str; 3] = ["stacklyd", "stackly-cli", "xtask"];
const MACOS_ADAPTER: &str = "stackly-platform-macos";
const MACOS_ONLY_TARGET: &str = r#"cfg(target_os = "macos")"#;
const TESTKIT: &str = "stackly-testkit";
const EVIDENCE: &str = "stackly-evidence";
/// The only crates that may take `stackly-evidence` as a normal dependency.
const EVIDENCE_NORMAL_USERS: [&str; 2] = ["xtask", TESTKIT];
const UNSAFE_LINT: &str = "unsafe_code";

pub fn run(root: &Path) -> Result<()> {
    let (crates, violations) = collect_violations(root)?;
    if violations.is_empty() {
        println!("check-deps: {crates} crates follow the dependency and unsafe rules");
        return Ok(());
    }
    for violation in &violations {
        eprintln!("  {violation}");
    }
    bail!(
        "{} dependency rule violation(s), see specs 0001 and 0002",
        violations.len()
    )
}

/// Checks every crate of the workspace at `root` and returns how many there
/// are and every rule they break.
fn collect_violations(root: &Path) -> Result<(usize, Vec<String>)> {
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
    if package == STORAGE && is_stackly_crate(name) {
        let allowed = STORAGE_NORMAL_DEPS.contains(&name)
            || (dependency.kind == DependencyKind::Development && STORAGE_DEV_DEPS.contains(&name));
        if !allowed {
            deny(
                "stackly-storage depends only on stackly-domain and stackly-platform \
                 (dev: stackly-testkit, stackly-evidence)",
            );
        }
    }
    if name == "rusqlite" && package != STORAGE {
        deny("only stackly-storage opens SQLite");
    }
    if name == STORAGE && BINARIES.contains(&package) && package != "stacklyd" {
        deny("of the binaries, only stacklyd uses the registry");
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
    fn storage_depends_only_on_domain_and_platform_and_tests_with_testkit_and_evidence() {
        for name in ["stackly-domain", "stackly-platform"] {
            assert!(violations(STORAGE, name, None).is_empty(), "{name}");
        }
        for name in [TESTKIT, EVIDENCE, "stackly-domain"] {
            assert!(violations(STORAGE, name, Some("dev")).is_empty(), "{name}");
        }
        for name in ["stackly-core", "stackly-protocol", MACOS_ADAPTER] {
            for kind in [None, Some("dev")] {
                let found = violations(STORAGE, name, kind);
                assert!(
                    found.iter().any(|v| v.ends_with(
                        "stackly-storage depends only on stackly-domain and stackly-platform \
                         (dev: stackly-testkit, stackly-evidence)"
                    )),
                    "{name} {kind:?}: {found:?}"
                );
            }
        }
        assert!(violations(STORAGE, "rusqlite", None).is_empty());
    }

    #[test]
    fn rusqlite_is_a_direct_dependency_of_storage_only() {
        for package in ["stacklyd", "stackly-core", TESTKIT, "xtask"] {
            for kind in [None, Some("dev")] {
                let found = violations(package, "rusqlite", kind);
                assert!(
                    found
                        .iter()
                        .any(|v| v.ends_with("only stackly-storage opens SQLite")),
                    "{package} {kind:?}: {found:?}"
                );
            }
        }
    }

    #[test]
    fn of_the_binaries_only_stacklyd_uses_storage() {
        assert!(violations("stacklyd", STORAGE, None).is_empty());
        for package in ["stackly-cli", "xtask"] {
            let found = violations(package, STORAGE, None);
            assert!(
                found
                    .iter()
                    .any(|v| v.ends_with("of the binaries, only stacklyd uses the registry")),
                "{package}: {found:?}"
            );
        }
    }

    /// Writes one crate of a fixture workspace.
    fn write_crate(root: &Path, dir: &str, name: &str, dependencies: &str, bin: bool) {
        let path = root.join(dir);
        std::fs::create_dir_all(path.join("src")).unwrap();
        std::fs::write(
            path.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
                 [dependencies]\n{dependencies}\n[lints]\nworkspace = true\n"
            ),
        )
        .unwrap();
        let (file, body) = if bin {
            ("main.rs", "//! Fixture.\nfn main() {}\n")
        } else {
            ("lib.rs", "//! Fixture.\n")
        };
        std::fs::write(path.join("src").join(file), body).unwrap();
    }

    #[test]
    fn a_fixture_workspace_that_breaks_the_storage_rules_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nresolver = \"3\"\nmembers = [\"domain\", \"core\", \"storage\", \"xtask\"]\n\n\
             [workspace.lints.rust]\nunsafe_code = \"deny\"\n",
        )
        .unwrap();
        write_crate(root, "domain", "stackly-domain", "", false);
        write_crate(root, "core", "stackly-core", "", false);
        write_crate(
            root,
            "storage",
            STORAGE,
            "stackly-domain = { path = \"../domain\" }\nstackly-core = { path = \"../core\" }\n\
             rusqlite = \"0.40\"\n",
            false,
        );
        write_crate(
            root,
            "xtask",
            "xtask",
            "stackly-storage = { path = \"../storage\" }\nrusqlite = \"0.40\"\n",
            true,
        );

        let (crates, mut found) = collect_violations(root).unwrap();
        found.sort();

        assert_eq!(crates, 4);
        assert_eq!(
            found,
            [
                "stackly-storage has a normal dependency on stackly-core: stackly-storage \
                 depends only on stackly-domain and stackly-platform (dev: stackly-testkit, \
                 stackly-evidence)",
                "xtask has a normal dependency on rusqlite: only stackly-storage opens SQLite",
                "xtask has a normal dependency on stackly-storage: of the binaries, only \
                 stacklyd uses the registry",
            ]
        );
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
}
