//! Content tests of the public docs of this repo (spec 0038): the license
//! in its four places, the required sections of `CONTRIBUTING.md`,
//! `SECURITY.md`, `GOVERNANCE.md`, the issue forms and the pull request
//! template, the provider guide pages, the package license notices, and the
//! `README.md` sections. `cargo xtask docs check` guards links, commands,
//! placeholders, schema coverage, and notice data (`docs_check.rs`); these
//! tests guard what that check does not read: which sections exist and the
//! sentences the spec requires word for word.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_yaml_ng::Value;

/// The contribution clause of spec 0038, *Lisensi*, word for word.
const CONTRIBUTION_CLAUSE: &str = "Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Stackly by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.";

/// The labels an issue form may use (spec 0038, *Label triage*).
const TRIAGE_LABELS: [&str; 5] = [
    "needs-triage",
    "core",
    "provider",
    "upstream",
    "configuration",
];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one directory below the repository root")
}

fn read(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|err| panic!("{rel} is in the repo: {err}"))
}

/// Every run of white space as one space, and blockquote marks dropped.
fn words(text: &str) -> String {
    text.lines()
        .map(|line| line.trim_start().trim_start_matches('>'))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `## ` headings of a Markdown file, in order, outside code fences.
fn h2_headings(text: &str) -> Vec<String> {
    let mut fenced = false;
    let mut found = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced && let Some(title) = line.strip_prefix("## ") {
            found.push(title.trim().to_owned());
        }
    }
    found
}

/// The body of the `## <title>` section, up to the next `## ` heading.
fn section(text: &str, title: &str) -> String {
    let heading = format!("## {title}");
    let mut lines = text.lines().skip_while(|line| line.trim() != heading);
    assert!(lines.next().is_some(), "no `{heading}` section");
    lines
        .take_while(|line| !line.starts_with("## "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Asserts that `rel` has exactly the `## ` headings in `expected`, in order.
fn assert_sections(rel: &str, expected: &[&str]) {
    assert_eq!(h2_headings(&read(rel)), expected, "the sections of {rel}");
}

/// The files of the public doc set (spec 0038, *Kumpulan dokumen publik*).
fn public_docs() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files: Vec<PathBuf> = [
        "README.md",
        "CONTRIBUTING.md",
        "CODE_OF_CONDUCT.md",
        "SECURITY.md",
        "GOVERNANCE.md",
    ]
    .iter()
    .map(|name| root.join(name))
    .filter(|path| path.is_file())
    .collect();
    let mut stack = vec![root.join(".github"), root.join("docs/providers")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("the folder is readable") {
            let path = entry.expect("the entry is readable").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    for entry in fs::read_dir(root.join("providers")).expect("providers/ is readable") {
        let folder = entry.expect("the entry is readable").path();
        for name in ["README.md", "LICENSE-NOTICES.md"] {
            if folder.join(name).is_file() {
                files.push(folder.join(name));
            }
        }
    }
    files.sort();
    files
}

fn yaml(rel: &str) -> Value {
    serde_yaml_ng::from_str(&read(rel)).unwrap_or_else(|err| panic!("{rel} is YAML: {err}"))
}

// covers: AC-1 (spec 0038)
#[test]
fn every_workspace_member_inherits_the_dual_license_and_the_repository_url() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .current_dir(repo_root())
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata lists packages");
    assert!(packages.len() >= 9, "{} workspace members", packages.len());
    for package in packages {
        let name = package["name"].as_str().unwrap_or("?");
        assert_eq!(
            package["license"].as_str(),
            Some("MIT OR Apache-2.0"),
            "the license of {name}"
        );
        assert_eq!(
            package["repository"].as_str(),
            Some("https://github.com/stackly-dev/stackly"),
            "the repository of {name}"
        );
    }
}

// covers: AC-1 (spec 0038)
#[test]
fn both_license_files_sit_at_the_root() {
    assert!(read("LICENSE-MIT").contains("MIT License"));
    assert!(read("LICENSE-APACHE").contains("Apache License"));
    assert!(read("LICENSE-APACHE").contains("Version 2.0"));
}

// covers: AC-1, AC-14 (spec 0038)
#[test]
fn the_readme_license_section_holds_the_contribution_clause_and_the_third_party_limit() {
    let license = words(&section(&read("README.md"), "License"));
    assert!(license.contains(CONTRIBUTION_CLAUSE), "{license}");
    assert!(license.contains("(LICENSE-APACHE)"), "{license}");
    assert!(license.contains("(LICENSE-MIT)"), "{license}");
    assert!(
        license.contains("It does not cover the third party software Stackly installs and runs"),
        "{license}"
    );
}

// covers: AC-1 (spec 0038)
#[test]
fn the_contributing_licensing_section_holds_the_clause_and_says_third_party_binaries_are_not_covered()
 {
    let licensing = words(&section(&read("CONTRIBUTING.md"), "Licensing"));
    assert!(licensing.contains(CONTRIBUTION_CLAUSE), "{licensing}");
    assert!(
        licensing.contains(
            "Stackly's license does not cover the third party software it installs and runs"
        ),
        "{licensing}"
    );
    assert!(
        licensing.contains("`runtimes/inventory.toml`"),
        "{licensing}"
    );
    assert!(licensing.contains("`LICENSE-NOTICES.md`"), "{licensing}");
    assert!(licensing.contains("AI tool"), "{licensing}");
}

// covers: AC-2 (spec 0038)
#[test]
fn contributing_has_every_required_section_in_order() {
    assert_sections(
        "CONTRIBUTING.md",
        &[
            "Ways to contribute",
            "Before you start",
            "Set up",
            "Build and check locally",
            "Run from source safely",
            "Contributing a provider",
            "Pull requests",
            "Where decisions live",
            "Licensing",
        ],
    );
}

// covers: AC-2 (spec 0038)
#[test]
fn contributing_lists_the_local_checks_because_there_is_no_ci() {
    let text = read("CONTRIBUTING.md");
    let checks = section(&text, "Build and check locally");
    assert!(checks.contains("This repository has no CI"), "{checks}");
    for command in [
        "cargo fmt --all --check",
        "cargo clippy",
        "cargo deny check",
        "cargo xtask check-deps",
        "cargo xtask codegen --check",
        "cargo xtask provider check",
        "cargo xtask provider test",
        "cargo xtask catalog check",
        "cargo xtask docs check",
    ] {
        assert!(
            checks.contains(command),
            "`{command}` in Build and check locally"
        );
    }
    let safely = section(&text, "Run from source safely");
    assert!(safely.contains("STACKLY_ROOT=$(mktemp -d)"), "{safely}");
    let provider = section(&text, "Contributing a provider");
    assert!(
        provider.contains("(docs/providers/README.md)"),
        "{provider}"
    );
}

// covers: AC-4 (spec 0038)
#[test]
fn security_has_every_required_section_in_order() {
    assert_sections(
        "SECURITY.md",
        &[
            "Reporting a vulnerability",
            "What to include",
            "Scope",
            "Supported versions",
            "What to expect",
            "Our process",
        ],
    );
}

// covers: AC-4 (spec 0038)
#[test]
fn security_names_the_private_channel_and_honest_targets() {
    let text = read("SECURITY.md");
    let reporting = words(&section(&text, "Reporting a vulnerability"));
    assert!(
        reporting.contains("https://github.com/stackly-dev/stackly/security/advisories/new"),
        "{reporting}"
    );
    assert!(
        reporting.contains("do not open an issue, a pull request, or a discussion"),
        "{reporting}"
    );
    let expect = words(&section(&text, "What to expect"));
    for target in ["7 days", "14 days", "90 days"] {
        assert!(expect.contains(target), "`{target}` in What to expect");
    }
    let process = words(&section(&text, "Our process"));
    assert!(process.contains("advisory"), "{process}");
}

// covers: AC-4 (spec 0038)
#[test]
fn the_issue_chooser_has_no_blank_issues_and_links_the_advisory_form() {
    let config = yaml(".github/ISSUE_TEMPLATE/config.yml");
    assert_eq!(config["blank_issues_enabled"], Value::Bool(false));
    let links = config["contact_links"]
        .as_sequence()
        .expect("config.yml has contact_links");
    assert!(
        links.iter().any(|link| link["url"].as_str()
            == Some("https://github.com/stackly-dev/stackly/security/advisories/new")),
        "{links:?}"
    );
}

// covers: AC-6 (spec 0038)
#[test]
fn governance_has_every_required_section_in_order() {
    assert_sections(
        "GOVERNANCE.md",
        &[
            "Roles",
            "How decisions are made",
            "Official and reviewed",
            "Provider owners",
            "Issue triage",
            "Compatibility and deprecation",
            "Review without CI",
            "Changing this document",
        ],
    );
}

/// The rows of the first Markdown table in `body`, as trimmed cells,
/// without the header and the separator.
fn table_rows(body: &str) -> Vec<Vec<String>> {
    body.lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim()
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_owned())
                .collect()
        })
        .collect()
}

// covers: AC-6 (spec 0038)
#[test]
fn governance_names_a_holder_for_every_role_and_has_no_codeowners_while_one_person_holds_them() {
    let roles = table_rows(&section(&read("GOVERNANCE.md"), "Roles"));
    let names: Vec<&str> = roles.iter().map(|row| row[0].as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Core maintainer",
            "Platform maintainer (macOS)",
            "Provider owner",
            "Security contact",
            "Release owner",
        ]
    );
    let holders: Vec<&str> = roles
        .iter()
        .map(|row| {
            row[2]
                .split_whitespace()
                .next()
                .expect("every role has a holder")
        })
        .collect();
    assert!(
        holders.iter().all(|holder| holder.starts_with('@')),
        "{holders:?}"
    );
    if holders.windows(2).all(|pair| pair[0] == pair[1]) {
        assert!(
            !repo_root().join(".github/CODEOWNERS").exists(),
            "no CODEOWNERS file until a second person holds a role"
        );
    }
}

// covers: AC-6 (spec 0038)
#[test]
fn governance_triage_table_maps_the_four_categories_to_their_labels() {
    let rows = table_rows(&section(&read("GOVERNANCE.md"), "Issue triage"));
    let labels: Vec<&str> = rows.iter().map(|row| row[1].as_str()).collect();
    assert_eq!(
        labels,
        vec!["`core`", "`provider`", "`upstream`", "`configuration`"]
    );
    let official = words(&section(&read("GOVERNANCE.md"), "Official and reviewed"));
    assert!(official.contains("`official`"), "{official}");
    assert!(official.contains("`reviewed`"), "{official}");
}

// covers: AC-7 (spec 0038)
#[test]
fn the_three_issue_forms_use_only_triage_labels() {
    let expected = [
        ("bug.yml", vec!["needs-triage"]),
        ("provider-proposal.yml", vec!["needs-triage", "provider"]),
        (
            "configuration-help.yml",
            vec!["needs-triage", "configuration"],
        ),
    ];
    for (file, wanted) in expected {
        let form = yaml(&format!(".github/ISSUE_TEMPLATE/{file}"));
        let labels: Vec<&str> = form["labels"]
            .as_sequence()
            .unwrap_or_else(|| panic!("{file} has labels"))
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(labels, wanted, "the labels of {file}");
        assert!(labels.iter().all(|label| TRIAGE_LABELS.contains(label)));
    }
    let mut forms: Vec<String> = fs::read_dir(repo_root().join(".github/ISSUE_TEMPLATE"))
        .expect("the issue template folder is readable")
        .map(|entry| {
            entry
                .expect("the entry is readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    forms.sort();
    assert_eq!(
        forms,
        vec![
            "bug.yml",
            "config.yml",
            "configuration-help.yml",
            "provider-proposal.yml"
        ]
    );
}

// covers: AC-7 (spec 0038)
#[test]
fn the_bug_form_asks_for_the_area_with_the_four_choices() {
    let form = yaml(".github/ISSUE_TEMPLATE/bug.yml");
    let body = form["body"].as_sequence().expect("bug.yml has a body");
    let area = body
        .iter()
        .find(|element| element["type"].as_str() == Some("dropdown"))
        .expect("bug.yml has a dropdown");
    let options: Vec<&str> = area["attributes"]["options"]
        .as_sequence()
        .expect("the dropdown has options")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        options,
        vec![
            "Core (app, daemon, CLI)",
            "A provider package",
            "An upstream runtime version",
            "Not sure",
        ]
    );
}

// covers: AC-7 (spec 0038)
#[test]
fn the_pull_request_template_has_every_required_section_and_checkbox() {
    let rel = ".github/PULL_REQUEST_TEMPLATE.md";
    assert_sections(
        rel,
        &[
            "What and why",
            "Kind of change",
            "Checks I ran locally",
            "Provider checklist",
            "Licensing",
        ],
    );
    let text = read(rel);
    let kinds = section(&text, "Kind of change");
    for kind in [
        "Provider package",
        "Core or daemon",
        "CLI",
        "macOS app",
        "Docs",
    ] {
        assert!(
            kinds.contains(&format!("- [ ] {kind}")),
            "`{kind}` checkbox"
        );
    }
    assert!(section(&text, "Checks I ran locally").contains("no CI"));
    assert!(section(&text, "Licensing").contains(
        "- [ ] I have the right to submit this and agree to license it under MIT OR Apache-2.0"
    ));
}

// covers: AC-11 (spec 0038)
#[test]
fn the_provider_guide_has_every_page_and_its_index_links_each() {
    let index = read("docs/providers/README.md");
    for page in [
        "authoring.md",
        "manifest-reference.md",
        "settings-reference.md",
        "versions-reference.md",
        "testing.md",
        "compatibility.md",
        "review.md",
        "trying-locally.md",
    ] {
        assert!(
            repo_root().join("docs/providers").join(page).is_file(),
            "docs/providers/{page} exists"
        );
        assert!(
            index.contains(&format!("]({page})")),
            "the index links {page}"
        );
    }
}

// covers: AC-8, AC-11 (spec 0038)
#[test]
fn the_quick_start_block_runs_exactly_the_four_commands_of_the_spec() {
    let index = read("docs/providers/README.md");
    let after: Vec<&str> = index
        .lines()
        .skip_while(|line| line.trim() != "<!-- docs-walkthrough -->")
        .skip(1)
        .collect();
    assert_eq!(after.first().map(|line| line.trim()), Some("```bash"));
    let block: Vec<&str> = after
        .iter()
        .skip(1)
        .take_while(|line| line.trim() != "```")
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert_eq!(
        block,
        vec![
            "cargo xtask provider check providers/example",
            "cargo xtask provider test providers/example",
            "cargo xtask provider new mytool",
            "cargo xtask provider test providers/mytool",
        ]
    );
    assert!(index.contains("target/provider-test/<runId>/report.json"));
}

// covers: AC-11 (spec 0038)
#[test]
fn the_compatibility_page_says_an_unsupported_schema_version_is_refused_before_any_lifecycle() {
    let page = words(&read("docs/providers/compatibility.md"));
    assert!(page.contains("`unsupported_api_version`"), "{page}");
    assert!(page.contains("no lifecycle step runs"), "{page}");
    assert!(page.contains("`deprecated`"), "{page}");
    let headings = h2_headings(&read("docs/providers/compatibility.md"));
    assert!(headings.iter().any(|heading| heading == "Deprecation"));
    assert!(
        headings
            .iter()
            .any(|heading| heading.starts_with("Moving to a new"))
    );
}

// covers: AC-11 (spec 0038)
#[test]
fn the_testing_page_gives_the_harness_exit_codes_report_and_cleanup() {
    let page = words(&read("docs/providers/testing.md"));
    for needed in [
        "cargo xtask provider check",
        "stackly provider validate",
        "cargo xtask provider test",
        "stackly.provider-conformance/v1",
        "--cleanup",
    ] {
        assert!(page.contains(needed), "`{needed}` on testing.md");
    }
    let codes = section(
        &read("docs/providers/testing.md"),
        "The conformance harness",
    );
    for row in ["| 0 | `pass`", "| 1 | `fail`", "| 2 | `blocked`"] {
        assert!(codes.contains(row), "exit code row `{row}`");
    }
}

// covers: AC-11 (spec 0038)
#[test]
fn the_authoring_and_review_pages_hold_their_required_parts() {
    let authoring = read("docs/providers/authoring.md");
    let moving = words(&section(&authoring, "Moving to the official catalog"));
    for needed in [
        "cargo xtask runtime pin",
        "cargo xtask codegen",
        "cargo xtask provider freeze",
        "providers/catalog.toml",
        "LICENSE-NOTICES.md",
        "metadata.revision",
    ] {
        assert!(
            moving.contains(needed),
            "`{needed}` in Moving to the official catalog"
        );
    }
    let review = read("docs/providers/review.md");
    assert_eq!(
        h2_headings(&review).first().map(String::as_str),
        Some("Read the diff first")
    );
    assert!(words(&review).contains("HANDOVER_PROVEN"));
}

// covers: AC-12 (spec 0038)
#[test]
fn the_trying_locally_page_names_the_import_flow_and_its_honest_limits() {
    let page = words(&read("docs/providers/trying-locally.md"));
    for command in [
        "stackly provider validate <dir>",
        "stackly provider import <dir> --dry-run",
        "stackly provider import <dir>",
        "stackly provider inspect <provider>",
        "stackly provider approve <provider>",
        "stackly provider revoke <provider>",
        "stackly provider remove <provider>",
        "stackly runtime install",
        "stackly instance create",
    ] {
        assert!(page.contains(command), "`{command}` on trying-locally.md");
    }
    assert!(page.contains("Import refuses the `official`, `testing`, and `stackly` namespaces"));
    assert!(page.contains("`STACKLY_CATALOG_DIR`"));
    assert!(page.contains("\"Local, not verified\""));
    assert!(page.contains("never Tested"));
    assert!(page.contains("real `sha256`"));
}

// covers: AC-12 (spec 0038)
#[test]
fn no_public_doc_uses_a_trust_word_spec_0037_rules_out() {
    for path in public_docs() {
        let text = fs::read_to_string(&path)
            .expect("a public doc is readable")
            .to_lowercase();
        for word in [
            "untrusted",
            "trusted publisher",
            "verified publisher",
            "sandboxed",
            "safe to run",
        ] {
            assert!(!text.contains(word), "{} uses `{word}`", path.display());
        }
    }
}

// covers: AC-13 (spec 0038)
#[test]
fn the_three_official_packages_carry_notices_with_the_three_headings_in_order() {
    for name in ["nats", "postgresql", "traefik"] {
        let rel = format!("providers/{name}/LICENSE-NOTICES.md");
        assert_sections(
            &rel,
            &["Package", "Runtime", "Third party material in this package"],
        );
        let text = read(&rel);
        assert!(
            section(&text, "Package").contains("`MIT OR Apache-2.0`"),
            "{rel} names the package license"
        );
        assert!(
            words(&section(&text, "Runtime")).contains("`runtimes/inventory.toml`"),
            "{rel} points at the inventory"
        );
    }
}

/// The `ok <id> <revision> <sha256>` line of `cargo xtask provider check`.
fn provider_check(dir: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .arg("provider")
        .arg("check")
        .arg(dir)
        .output()
        .expect("the xtask binary runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy folder is made");
    for entry in fs::read_dir(from).expect("the package folder is readable") {
        let entry = entry.expect("the entry is readable");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("the file is copied");
        }
    }
}

// covers: AC-13 (spec 0038)
#[test]
fn a_license_notice_never_changes_the_package_digest() {
    let temp = tempfile::tempdir().expect("a temp dir");
    for name in ["nats", "postgresql", "traefik"] {
        let copy = temp.path().join(name);
        copy_dir(&repo_root().join("providers").join(name), &copy);
        let with_notice = provider_check(&copy);
        fs::remove_file(copy.join("LICENSE-NOTICES.md")).expect("the notice is removed");
        let without_notice = provider_check(&copy);
        assert_eq!(with_notice, without_notice, "the digest of {name}");
        assert!(with_notice.starts_with(&format!("ok official/{name} ")));
    }
}

// covers: AC-13 (spec 0038)
#[test]
fn the_catalog_revision_ledger_still_matches_the_packages() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .args(["catalog", "check"])
        .output()
        .expect("the xtask binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.starts_with("ok catalog revision "), "{stdout}");
}

// covers: AC-14 (spec 0038)
#[test]
fn the_readme_drops_the_scaffold_status_and_points_at_the_root_files() {
    let text = read("README.md");
    assert!(!text.to_lowercase().contains("early scaffold"));
    let headings = h2_headings(&text);
    for needed in ["Contributing", "Security", "License"] {
        assert!(
            headings.iter().any(|heading| heading == needed),
            "README.md has `## {needed}`"
        );
    }
    let contributing = section(&text, "Contributing");
    assert!(contributing.contains("(CONTRIBUTING.md)"), "{contributing}");
    assert!(
        contributing.contains("(docs/providers/README.md)"),
        "{contributing}"
    );
    assert!(section(&text, "Security").contains("(SECURITY.md)"));
    let layout = section(&text, "Layout");
    assert!(
        layout
            .lines()
            .any(|line| line.starts_with("| `docs/`") && line.contains("`docs/providers/`")),
        "{layout}"
    );
}

// covers: AC-14 (spec 0038)
#[test]
fn no_public_doc_uses_common_indonesian_words() {
    // A cheap guard for "every public doc is in English": these words are
    // frequent in Indonesian prose and are not English words.
    let indonesian = ["yang", "untuk", "tidak", "dengan", "adalah", "atau", "dari"];
    for path in public_docs() {
        let text = fs::read_to_string(&path).expect("a public doc is readable");
        for word in text
            .split(|c: char| !c.is_alphanumeric())
            .map(str::to_lowercase)
        {
            assert!(
                !indonesian.contains(&word.as_str()),
                "{} uses the Indonesian word `{word}`",
                path.display()
            );
        }
    }
}
