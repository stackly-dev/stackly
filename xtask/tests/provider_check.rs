//! End to end tests of `cargo xtask provider check` (spec 0009, AC-14): the
//! real binary checks the repo packages, and broken copies of each package
//! in a test root make it exit 1 with one line per problem.
#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use stackly_testkit::TestRoot;

/// The packages every repo checkout ships.
const PACKAGES: [&str; 3] = ["fake", "nats", "traefik"];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one directory below the repository root")
}

/// `cargo xtask provider check <dirs>`, from the repo root.
fn provider_check(dirs: &[PathBuf]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .args(["provider", "check"])
        .args(dirs)
        .output()
        .expect("the xtask binary runs")
}

fn stdout_lines(output: &Output) -> Vec<String> {
    String::from_utf8(output.stdout.clone())
        .expect("stdout is UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Copies a folder tree with `std::fs`, as test setup.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy target can be created");
    for entry in fs::read_dir(from).expect("the package folder can be listed") {
        let entry = entry.expect("the entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("the file can be copied");
        }
    }
}

/// A copy of a repo package in `root`, with `provider.yaml` edited.
fn broken_copy(root: &TestRoot, package: &str, edit: &dyn Fn(&str) -> String) -> PathBuf {
    let copy = root.path().join("providers").join(package);
    copy_dir(&repo_root().join("providers").join(package), &copy);
    let manifest = copy.join("provider.yaml");
    let text = fs::read_to_string(&manifest).expect("the copied manifest can be read");
    let edited = edit(&text);
    assert_ne!(edited, text, "the edit changed {package}/provider.yaml");
    fs::write(&manifest, edited).expect("the copied manifest can be written");
    copy
}

#[test]
fn the_repo_packages_pass_with_one_ok_line_each() {
    let output = provider_check(&[]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = stdout_lines(&output);
    let ids: Vec<&str> = lines
        .iter()
        .map(|line| line.split(' ').nth(1).expect("an ok line has an ID"))
        .collect();
    assert_eq!(
        ids,
        [
            "testing/fake",
            "official/nats",
            "official/postgresql",
            "official/traefik"
        ]
    );
    for line in &lines {
        let parts: Vec<&str> = line.split(' ').collect();
        assert_eq!(parts.len(), 4, "{line}");
        assert_eq!(parts[0], "ok");
        assert_eq!(parts[3].len(), 64, "{line}");
        assert!(parts[3].bytes().all(|b| b.is_ascii_hexdigit()), "{line}");
    }
}

#[test]
fn an_unknown_primitive_in_a_copy_of_each_package_fails_with_one_line() {
    let root = TestRoot::new().expect("a test root can be created");
    let copies: Vec<PathBuf> = PACKAGES
        .iter()
        .map(|package| {
            broken_copy(&root, package, &|text| {
                text.replace("primitive: createManagedDirectories", "primitive: runShell")
            })
        })
        .collect();

    let output = provider_check(&copies);

    assert_eq!(output.status.code(), Some(1));
    let lines = stdout_lines(&output);
    assert_eq!(lines.len(), PACKAGES.len(), "{lines:#?}");
    for (line, package) in lines.iter().zip(PACKAGES) {
        assert!(
            line.starts_with(&format!(
                "{package}: provider.yaml lifecycle.initialize[0].primitive: unknown_primitive: "
            )),
            "{line}"
        );
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("3 problem(s) in 3 of 3 package(s)"),
        "{stderr}"
    );
}

#[test]
fn every_problem_in_a_package_gets_its_own_line_next_to_the_valid_ones() {
    let root = TestRoot::new().expect("a test root can be created");
    let broken = broken_copy(&root, "nats", &|text| {
        text.replace("primitive: renderConfig", "primitive: runShell")
            .replace("type: http", "type: script")
    });
    let valid = repo_root().join("providers/traefik");

    let output = provider_check(&[broken, valid]);

    assert_eq!(output.status.code(), Some(1));
    let lines = stdout_lines(&output);
    assert_eq!(lines.len(), 3, "{lines:#?}");
    assert!(lines[0].starts_with("nats: provider.yaml "), "{}", lines[0]);
    assert!(lines[0].contains(": unknown_primitive: "), "{}", lines[0]);
    assert!(lines[1].starts_with("nats: provider.yaml "), "{}", lines[1]);
    assert!(lines[1].contains(": unknown_probe: "), "{}", lines[1]);
    assert!(
        lines[2].starts_with("ok official/traefik 1.0.0 "),
        "{}",
        lines[2]
    );
}
