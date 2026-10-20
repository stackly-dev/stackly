//! `cargo xtask check-deps`: the dependency direction and `unsafe` rules
//! from spec 0001 ("Aturan arah dependency"), the `stackly-evidence` rules
//! from spec 0002, the platform rules and source scan from spec 0004, the
//! `stackly-storage` and `rusqlite` rules from spec 0005, the
//! `stackly-ipc` and `stacklyd` rules and the socket scan from spec 0006,
//! the `stackly-fake-server` rules from spec 0009, and the secret rules
//! from spec 0011: the `expose` scan and where `security-framework` and
//! `rpassword` may be direct dependencies, and the CLI rules from spec
//! 0008: no process spawning paths, crates, or print macros in
//! `stackly-cli`, and no `codes::cli` outside the CLI and the protocol, and
//! the crate placement rules from spec 0015: the Objective-C bindings only
//! in `stackly-platform-macos`, and the clap completion and man page crates
//! only in `stackly-cli`.
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
/// The dummy server behind the `testing/fake` provider (spec 0009). Tests
/// find its binary through `stackly_testkit::fake_server::binary`.
const FAKE_SERVER: &str = "stackly-fake-server";
/// The IPC transport (spec 0006). Its only Stackly dependency is the
/// protocol.
const IPC: &str = "stackly-ipc";
const PROTOCOL: &str = "stackly-protocol";
/// The Stackly crates `stackly-ipc` may take as dev dependencies, on top of
/// [`PROTOCOL`].
const IPC_DEV_DEPS: [&str; 2] = [TESTKIT, EVIDENCE];
/// The daemon, a library plus bin whose library only its own `main.rs` and
/// `apps/daemon/tests/` use.
const DAEMON: &str = "stacklyd";
/// The crates that talk to the Security framework, which only the macOS
/// adapter may depend on (spec 0011).
const SECURITY_FRAMEWORK: [&str; 2] = ["security-framework", "security-framework-sys"];
/// The terminal password reader, which only the CLI may depend on
/// (spec 0011).
const RPASSWORD: &str = "rpassword";
/// The Objective-C binding crates behind `SMAppService`, which only the
/// macOS adapter may depend on (spec 0015).
const OBJC2: [&str; 3] = ["objc2", "objc2-foundation", "objc2-service-management"];
/// The clap crates that write shell completion scripts and man pages, which
/// only the CLI may depend on (spec 0015).
const CLAP_DOCS: [&str; 2] = ["clap_complete", "clap_mangen"];
const CLI: &str = "stackly-cli";
/// Crates that run or open other programs, which `stackly-cli` never takes
/// as a normal dependency (spec 0008, BR-011). Dev dependencies may.
const CLI_SPAWN_DEPS: [&str; 7] = [
    "libc",
    "nix",
    "duct",
    "subprocess",
    "xshell",
    "open",
    "opener",
];

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
        "{} dependency rule violation(s), see specs 0001, 0002, 0004, 0005, 0006, 0008, 0009, and 0011",
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
        let rules = source_scan::Rules::for_crate(package.name.as_str());
        violations.extend(source_scan::scan_package(root, package, rules)?);
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
    if package == FAKE_SERVER
        && is_stackly_crate(name)
        && !(name == TESTKIT && dependency.kind == DependencyKind::Development)
    {
        deny(
            "stackly-fake-server depends on no Stackly crate, except stackly-testkit as a dev dependency",
        );
    }
    if name == FAKE_SERVER {
        deny("no crate depends on stackly-fake-server; tests build it through stackly-testkit");
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
    if package == IPC && is_stackly_crate(name) {
        let allowed = match dependency.kind {
            DependencyKind::Normal => name == PROTOCOL,
            DependencyKind::Development => name == PROTOCOL || IPC_DEV_DEPS.contains(&name),
            _ => false,
        };
        if !allowed {
            deny(
                "stackly-ipc depends only on stackly-protocol \
                 (dev: stackly-testkit, stackly-evidence)",
            );
        }
    }
    if name == DAEMON {
        deny(
            "no crate depends on stacklyd; its library serves only its own main.rs \
             and apps/daemon/tests",
        );
    }
    if name == "rusqlite" && package != STORAGE {
        deny("only stackly-storage opens SQLite");
    }
    if SECURITY_FRAMEWORK.contains(&name) && package != MACOS_ADAPTER {
        deny("only stackly-platform-macos talks to the Security framework (spec 0011)");
    }
    if name == RPASSWORD && package != CLI {
        deny("only stackly-cli reads a secret from the terminal (spec 0011)");
    }
    if OBJC2.contains(&name) && package != MACOS_ADAPTER {
        deny("only stackly-platform-macos calls Objective-C APIs (spec 0015)");
    }
    if CLAP_DOCS.contains(&name) && package != CLI {
        deny("only stackly-cli writes shell completion and man pages (spec 0015)");
    }
    if package == CLI && dependency.kind == DependencyKind::Normal && CLI_SPAWN_DEPS.contains(&name)
    {
        deny(source_scan::SPAWN_RULE);
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

        let (crates, mut found) = find_violations(root).unwrap();
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

    #[test]
    fn stackly_fake_server_takes_no_stackly_crate_but_testkit_for_tests() {
        assert!(violations(FAKE_SERVER, TESTKIT, Some("dev")).is_empty());
        for name in ["serde_json", "sha2", "signal-hook"] {
            assert!(violations(FAKE_SERVER, name, None).is_empty(), "{name}");
        }
        let rule = "stackly-fake-server depends on no Stackly crate, except stackly-testkit as a dev dependency";
        for name in ["stackly-domain", "stackly-core", EVIDENCE, "stacklyd"] {
            for kind in [None, Some("dev"), Some("build")] {
                let found = violations(FAKE_SERVER, name, kind);
                assert!(
                    found.iter().any(|v| v.ends_with(rule)),
                    "{name} {kind:?}: {found:?}"
                );
            }
        }
        for kind in [None, Some("build")] {
            let found = violations(FAKE_SERVER, TESTKIT, kind);
            assert!(
                found.iter().any(|v| v.ends_with(rule)),
                "{kind:?}: {found:?}"
            );
        }
    }

    #[test]
    fn no_crate_depends_on_stackly_fake_server() {
        for package in [TESTKIT, "stackly-core", "stacklyd", "stackly-cli", "xtask"] {
            for (kind, label) in [
                (None, "normal"),
                (Some("dev"), "dev"),
                (Some("build"), "build"),
            ] {
                assert_eq!(
                    violations(package, FAKE_SERVER, kind),
                    [format!(
                        "{package} has a {label} dependency on stackly-fake-server: \
                         no crate depends on stackly-fake-server; tests build it through stackly-testkit"
                    )]
                );
            }
        }
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

        for name in source_scan::SCANNED
            .iter()
            .chain(&source_scan::SOCKET_SCANNED)
        {
            assert!(
                members.contains(name),
                "{name} is not a workspace member, so its source would never be scanned"
            );
        }
    }

    const IPC_RULE: &str =
        "stackly-ipc depends only on stackly-protocol (dev: stackly-testkit, stackly-evidence)";
    const DAEMON_RULE: &str = "no crate depends on stacklyd; its library serves only its own \
                               main.rs and apps/daemon/tests";

    #[test]
    fn stackly_ipc_depends_only_on_protocol_and_tests_with_testkit_and_evidence() {
        assert!(violations(IPC, PROTOCOL, None).is_empty());
        for name in [PROTOCOL, TESTKIT, EVIDENCE] {
            assert!(violations(IPC, name, Some("dev")).is_empty(), "{name}");
        }
        for name in ["tokio", "tracing", "uuid", "thiserror", "serde_json"] {
            assert!(violations(IPC, name, None).is_empty(), "{name}");
        }
        for name in [
            "stackly-core",
            "stackly-domain",
            "stackly-platform",
            STORAGE,
            DAEMON,
        ] {
            for kind in [None, Some("dev"), Some("build")] {
                let found = violations(IPC, name, kind);
                assert!(
                    found.iter().any(|v| v.ends_with(IPC_RULE)),
                    "{name} {kind:?}: {found:?}"
                );
            }
        }
        assert_eq!(
            violations(IPC, PROTOCOL, Some("build")),
            [format!(
                "stackly-ipc has a build dependency on stackly-protocol: {IPC_RULE}"
            )]
        );
    }

    #[test]
    fn no_crate_depends_on_stacklyd() {
        for package in ["stackly-cli", IPC, TESTKIT, "stackly-core", "xtask"] {
            for (kind, label) in [
                (None, "normal"),
                (Some("dev"), "dev"),
                (Some("build"), "build"),
            ] {
                let found = violations(package, DAEMON, kind);
                assert!(
                    found.contains(&format!(
                        "{package} has a {label} dependency on stacklyd: {DAEMON_RULE}"
                    )),
                    "{package} {kind:?}: {found:?}"
                );
            }
        }
    }

    #[test]
    fn the_daemon_and_the_cli_may_use_the_transport() {
        assert!(violations(DAEMON, IPC, None).is_empty());
        assert!(violations("stackly-cli", IPC, None).is_empty());
        assert!(violations("stackly-cli", PROTOCOL, None).is_empty());
    }

    /// Writes `path` under `root`, creating its folders.
    fn write_file(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn a_fixture_workspace_that_breaks_the_ipc_rules_fails_with_file_and_line() {
        let ev = stackly_evidence::evidence!(
            covers = ["AC-070", "REQ-047"],
            kind = stackly_evidence::Kind::Fixture,
            expected = "check-deps rejects a TCP or UDP socket name in the non test code of the transport, the daemon, and the CLI with file and line, a transport that depends on another Stackly crate, re-enables unsafe, or skips the workspace lints, and any crate that depends on stacklyd",
        );
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(
            root,
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\n\
             members = [\"crates/protocol\", \"crates/core\", \"crates/ipc\", \"apps/daemon\", \"apps/cli\"]\n\n\
             [workspace.lints.rust]\nunsafe_code = \"deny\"\n",
        );
        write_crate(root, "crates/protocol", PROTOCOL, "", false);
        write_crate(root, "crates/core", "stackly-core", "", false);
        write_file(
            root,
            "crates/ipc/Cargo.toml",
            "[package]\nname = \"stackly-ipc\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
             [dependencies]\nstackly-protocol = { path = \"../protocol\" }\n\
             stackly-core = { path = \"../core\" }\n",
        );
        write_file(
            root,
            "crates/ipc/src/lib.rs",
            // Built from parts, so this file never holds the opt out itself.
            &format!(
                "//! Fixture.\n#![allow({UNSAFE_LINT})]\n\
                 pub fn open() {{ let _ = std::net::UdpSocket::bind(\"127.0.0.1:0\"); }}\n"
            ),
        );
        write_crate(root, "apps/daemon", DAEMON, "", true);
        write_file(root, "apps/daemon/src/lib.rs", "//! Fixture.\n");
        write_file(
            root,
            "apps/daemon/src/main.rs",
            "//! Fixture.\nfn main() {\n    let _ = std::net::TcpListener::bind(\"127.0.0.1:0\");\n}\n",
        );
        write_file(
            root,
            "apps/daemon/tests/tcp.rs",
            "//! Tests may open TCP sockets.\n#[test]\nfn t() { let _ = std::net::TcpStream::connect(\"x\"); }\n",
        );
        write_crate(
            root,
            "apps/cli",
            "stackly-cli",
            "stacklyd = { path = \"../daemon\" }\nstackly-ipc = { path = \"../../crates/ipc\" }\n",
            true,
        );

        let (crates, mut found) = find_violations(root).unwrap();
        let unsafe_opt_out: Vec<String> = found
            .extract_if(.., |v| {
                v.starts_with("stackly-ipc re-enables `unsafe_code` in ")
            })
            .collect();
        found.sort();

        assert_eq!(crates, 5);
        assert_eq!(
            found,
            [
                format!("stackly-cli has a normal dependency on stacklyd: {DAEMON_RULE}"),
                "stackly-ipc crates/ipc/src/lib.rs:3: no tcp or udp sockets (spec 0006)".to_owned(),
                format!("stackly-ipc has a normal dependency on stackly-core: {IPC_RULE}"),
                "stackly-ipc must inherit the workspace lints with `[lints] workspace = true`"
                    .to_owned(),
                "stacklyd apps/daemon/src/main.rs:3: no tcp or udp sockets (spec 0006)".to_owned(),
            ]
        );
        assert_eq!(unsafe_opt_out.len(), 1, "{unsafe_opt_out:?}");
        assert!(
            unsafe_opt_out[0].ends_with("crates/ipc/src/lib.rs; only stackly-platform-macos may"),
            "{unsafe_opt_out:?}"
        );
        ev.pass();
    }

    const SECURITY_RULE: &str =
        "only stackly-platform-macos talks to the Security framework (spec 0011)";
    const RPASSWORD_RULE: &str = "only stackly-cli reads a secret from the terminal (spec 0011)";

    #[test]
    fn security_framework_and_rpassword_belong_to_one_crate_each() {
        for name in SECURITY_FRAMEWORK {
            assert!(violations(MACOS_ADAPTER, name, None).is_empty(), "{name}");
            for package in ["stackly-core", "stackly-platform", "stacklyd", CLI, TESTKIT] {
                for (kind, label) in [
                    (None, "normal"),
                    (Some("dev"), "dev"),
                    (Some("build"), "build"),
                ] {
                    assert_eq!(
                        violations(package, name, kind),
                        [format!(
                            "{package} has a {label} dependency on {name}: {SECURITY_RULE}"
                        )]
                    );
                }
            }
        }
        assert!(violations(CLI, RPASSWORD, None).is_empty());
        for package in ["stackly-core", "stacklyd", MACOS_ADAPTER, "xtask"] {
            for (kind, label) in [(None, "normal"), (Some("dev"), "dev")] {
                assert_eq!(
                    violations(package, RPASSWORD, kind),
                    [format!(
                        "{package} has a {label} dependency on rpassword: {RPASSWORD_RULE}"
                    )]
                );
            }
        }
    }

    const OBJC2_RULE: &str = "only stackly-platform-macos calls Objective-C APIs (spec 0015)";
    const CLAP_DOCS_RULE: &str =
        "only stackly-cli writes shell completion and man pages (spec 0015)";

    #[test]
    fn the_objc2_crates_belong_to_the_macos_adapter_and_the_clap_doc_crates_to_the_cli() {
        let kinds = [
            (None, "normal"),
            (Some("dev"), "dev"),
            (Some("build"), "build"),
        ];
        for name in OBJC2 {
            for (kind, _) in kinds {
                assert!(violations(MACOS_ADAPTER, name, kind).is_empty(), "{name}");
            }
            for package in ["stackly-platform", "stackly-core", "stacklyd", CLI, TESTKIT] {
                for (kind, label) in kinds {
                    assert_eq!(
                        violations(package, name, kind),
                        [format!(
                            "{package} has a {label} dependency on {name}: {OBJC2_RULE}"
                        )]
                    );
                }
            }
        }
        for name in CLAP_DOCS {
            for (kind, _) in kinds {
                assert!(violations(CLI, name, kind).is_empty(), "{name}");
            }
            for package in ["stacklyd", "stackly-core", MACOS_ADAPTER, "xtask"] {
                for (kind, label) in kinds {
                    assert_eq!(
                        violations(package, name, kind),
                        [format!(
                            "{package} has a {label} dependency on {name}: {CLAP_DOCS_RULE}"
                        )]
                    );
                }
            }
        }
    }

    #[test]
    fn a_fixture_workspace_that_breaks_the_secret_rules_fails_with_file_and_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(
            root,
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\n\
             members = [\"crates/core\", \"crates/testkit\", \"crates/platform-macos\", \"apps/cli\", \"apps/daemon\"]\n\n\
             [workspace.lints.rust]\nunsafe_code = \"deny\"\n",
        );
        write_crate(
            root,
            "crates/core",
            "stackly-core",
            "security-framework = \"3\"\n",
            false,
        );
        write_file(
            root,
            "crates/core/src/lib.rs",
            "//! Fixture.\nmod secret;\n\
             pub fn leak(v: &V) {\n    tracing::info!(secret = v.expose(), \"started\");\n}\n",
        );
        write_file(
            root,
            "crates/core/src/secret/mod.rs",
            "mod delivery;\nmod service;\n",
        );
        write_file(
            root,
            "crates/core/src/secret/delivery.rs",
            "pub fn stdin(v: &V) -> &[u8] { v.expose().as_bytes() }\n",
        );
        write_file(
            root,
            "crates/core/src/secret/service.rs",
            "\npub fn put(v: &V) -> usize { v.expose().len() }\n",
        );
        write_crate(root, "crates/testkit", TESTKIT, "", false);
        write_file(
            root,
            "crates/testkit/src/lib.rs",
            "//! Fixture.\nmod fake_secret;\n",
        );
        write_file(
            root,
            "crates/testkit/src/fake_secret.rs",
            "pub fn stored(v: &V) -> String { v.expose().to_owned() }\n",
        );
        write_crate(
            root,
            "crates/platform-macos",
            MACOS_ADAPTER,
            "security-framework = \"3\"\nsecurity-framework-sys = \"2\"\n",
            false,
        );
        write_crate(root, "apps/cli", CLI, "rpassword = \"7\"\n", true);
        write_crate(root, "apps/daemon", DAEMON, "rpassword = \"7\"\n", true);

        let (crates, mut found) = find_violations(root).unwrap();
        found.sort();

        assert_eq!(crates, 5);
        assert_eq!(
            found,
            [
                "stackly-core crates/core/src/lib.rs:4: secret expose outside the allowlist"
                    .to_owned(),
                "stackly-core crates/core/src/secret/service.rs:2: secret expose outside the allowlist"
                    .to_owned(),
                format!(
                    "stackly-core has a normal dependency on security-framework: {SECURITY_RULE}"
                ),
                format!("stacklyd has a normal dependency on rpassword: {RPASSWORD_RULE}"),
            ]
        );
    }

    const SPAWN_RULE: &str = "the cli never spawns processes (spec 0008, BR-011)";

    #[test]
    fn the_cli_never_takes_a_process_spawning_crate_as_a_normal_dependency() {
        for name in CLI_SPAWN_DEPS {
            assert_eq!(
                violations(CLI, name, None),
                [format!(
                    "stackly-cli has a normal dependency on {name}: {SPAWN_RULE}"
                )]
            );
            assert!(violations(CLI, name, Some("dev")).is_empty(), "{name}");
            assert!(violations("stacklyd", name, None).is_empty(), "{name}");
        }
        for name in [
            "anstream",
            "anstyle",
            "unicode-width",
            "jiff",
            "uuid",
            "tokio",
        ] {
            assert!(violations(CLI, name, None).is_empty(), "{name}");
        }
    }

    #[test]
    fn a_fixture_workspace_that_breaks_the_cli_rules_fails_with_file_and_line() {
        let ev = stackly_evidence::evidence!(
            covers = ["BR-011"],
            kind = stackly_evidence::Kind::Fixture,
            expected = "check-deps rejects every process spawning path, import, alias, glob, and crate in the cli, its print macros, and codes::cli outside the cli and the protocol, with file and line, while exit, ExitCode, id, clap::Command, a dev libc, and test code pass",
        );
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(
            root,
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\n\
             members = [\"crates/protocol\", \"apps/daemon\", \"apps/cli\"]\n\n\
             [workspace.lints.rust]\nunsafe_code = \"deny\"\n",
        );
        write_crate(root, "crates/protocol", PROTOCOL, "", false);
        write_file(
            root,
            "crates/protocol/src/lib.rs",
            "//! Fixture.\npub mod codes { pub mod cli { pub const USAGE: &str = \"cli.usage\"; } }\n\
             pub const A: &str = codes::cli::USAGE;\n",
        );
        write_crate(root, "apps/daemon", DAEMON, "", true);
        write_file(
            root,
            "apps/daemon/src/main.rs",
            "//! Fixture.\nuse stackly_protocol::codes::cli;\nfn main() {\n    println!(\"{}\", cli::USAGE);\n}\n",
        );
        write_file(
            root,
            "apps/cli/Cargo.toml",
            "[package]\nname = \"stackly-cli\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
             [dependencies]\nlibc = \"0.2\"\nduct = \"1\"\nclap = \"4\"\n\n\
             [dev-dependencies]\nnix = \"0.30\"\n\n[lints]\nworkspace = true\n",
        );
        write_file(
            root,
            "apps/cli/src/main.rs",
            "//! Fixture.\n\
             use std::process::Command;\n\
             use std::process as p;\n\
             use std::process::*;\n\
             use stackly_protocol::codes::cli;\n\
             fn main() -> std::process::ExitCode {\n\
             \x20   let _ = std::process::Command::new(\"x\");\n\
             \x20   let _ = p::Command::new(\"x\");\n\
             \x20   let _ = tokio::process::Command::new(\"x\");\n\
             \x20   println!(\"{} {}\", cli::USAGE, std::process::id());\n\
             \x20   let _ = clap::Command::new(\"stackly\");\n\
             \x20   if false { std::process::exit(2) }\n\
             \x20   std::process::ExitCode::SUCCESS\n\
             }\n",
        );
        write_file(
            root,
            "apps/cli/tests/spawn.rs",
            "//! Tests may spawn the binary.\n#[test]\nfn t() { let _ = std::process::Command::new(\"x\"); println!(\"x\"); }\n",
        );

        let (crates, mut found) = find_violations(root).unwrap();
        found.sort();

        assert_eq!(crates, 3);
        let print = "the cli writes through its output module, never print macros, which panic on a broken pipe (spec 0008, AC-10)";
        let codes = "only the cli and the protocol name codes::cli, so the daemon never sends a cli code (spec 0008, AC-14)";
        assert_eq!(
            found,
            [
                format!("stackly-cli apps/cli/src/main.rs:10: {print}"),
                format!("stackly-cli apps/cli/src/main.rs:2: {SPAWN_RULE}"),
                format!("stackly-cli apps/cli/src/main.rs:3: {SPAWN_RULE}"),
                format!("stackly-cli apps/cli/src/main.rs:4: {SPAWN_RULE}"),
                format!("stackly-cli apps/cli/src/main.rs:7: {SPAWN_RULE}"),
                format!("stackly-cli apps/cli/src/main.rs:9: {SPAWN_RULE}"),
                format!("stackly-cli has a normal dependency on duct: {SPAWN_RULE}"),
                format!("stackly-cli has a normal dependency on libc: {SPAWN_RULE}"),
                format!("stacklyd apps/daemon/src/main.rs:2: {codes}"),
            ]
        );
        ev.pass();
    }

    #[test]
    fn every_allowed_expose_module_of_this_workspace_holds_a_call_site() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask sits one directory below the repository root");
        let metadata = MetadataCommand::new()
            .manifest_path(root.join("Cargo.toml"))
            .no_deps()
            .exec()
            .unwrap();
        let packages = metadata.workspace_packages();

        for (name, modules) in source_scan::EXPOSE_ALLOWED {
            let package = packages
                .iter()
                .find(|package| package.name.as_str() == name)
                .unwrap_or_else(|| panic!("{name} is not a workspace member"));
            let nothing_allowed = source_scan::Rules {
                library: false,
                sockets: false,
                expose_allowed: &[],
                cli: false,
                no_cli_codes: false,
            };
            let calls = source_scan::scan_package(root, package, nothing_allowed).unwrap();
            for module in modules {
                let folder = format!("/src/{}", module.replace("::", "/"));
                assert!(
                    calls.iter().any(|call| call.contains(&folder)),
                    "{name} {module} holds no expose call, so it should leave the allowlist: {calls:?}"
                );
            }
        }
    }
}
