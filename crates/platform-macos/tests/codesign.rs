//! `MacosCodeSigning` on real binaries (spec 0018, AC-11): the signature
//! kinds, strict verification, ad hoc signing, and quarantine. It runs
//! `/usr/bin/codesign` and `/usr/bin/xattr`, which are part of base macOS.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

use stackly_platform::{CodeSigning, SignatureKind};
use stackly_platform_macos::MacosCodeSigning;
use stackly_testkit::TestRoot;

/// Copies a system binary into the test root, signature included.
fn copy_of(root: &TestRoot, name: &str) -> PathBuf {
    let copy = root.path().join(name);
    std::fs::copy("/bin/ls", &copy).expect("copy a system binary");
    copy
}

fn run(program: &str, args: &[&str], path: &Path) {
    let status = Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .expect("run a base macOS tool");
    assert!(status.success(), "{program} {args:?} failed");
}

#[test]
fn a_system_binary_is_signed_by_apple_and_valid() {
    let root = TestRoot::new().expect("a test root");
    let ls = copy_of(&root, "ls");

    let found = MacosCodeSigning::new().assess(&ls).expect("assess");

    assert!(found.strict_valid, "{:?}", found.detail);
    assert!(
        matches!(found.signature, SignatureKind::OtherSigned { .. }),
        "{:?}",
        found.signature
    );
}

#[test]
fn an_unsigned_copy_is_signed_ad_hoc_and_then_verifies() {
    let root = TestRoot::new().expect("a test root");
    let ls = copy_of(&root, "ls");
    run("/usr/bin/codesign", &["--remove-signature"], &ls);
    let signing = MacosCodeSigning::new();

    let before = signing.assess(&ls).expect("assess");
    signing.sign_adhoc(std::slice::from_ref(&ls)).expect("sign");
    let after = signing.assess(&ls).expect("assess");

    assert_eq!(before.signature, SignatureKind::Unsigned);
    assert!(!before.strict_valid);
    assert_eq!(after.signature, SignatureKind::Adhoc);
    assert!(after.strict_valid, "{:?}", after.detail);
}

#[test]
fn one_changed_byte_fails_strict_verification_without_the_path_in_the_detail() {
    let root = TestRoot::new().expect("a test root");
    let ls = copy_of(&root, "ls");
    let signing = MacosCodeSigning::new();
    run("/usr/bin/codesign", &["--remove-signature"], &ls);
    signing.sign_adhoc(std::slice::from_ref(&ls)).expect("sign");
    let mut bytes = std::fs::read(&ls).expect("read the copy");
    let middle = bytes.len() / 3;
    bytes[middle] ^= 0xff;
    std::fs::write(&ls, bytes).expect("write the copy");

    let found = signing.assess(&ls).expect("assess");

    assert!(!found.strict_valid);
    let detail = found.detail.expect("a refusal has a detail");
    assert!(
        !detail.contains(root.path().to_str().expect("utf-8 path")),
        "{detail}"
    );
}

#[test]
fn a_binary_the_linker_signed_is_linker_signed() {
    let root = TestRoot::new().expect("a test root");
    let source = root.path().join("hello.c");
    std::fs::write(&source, "int main(void) { return 0; }\n").expect("write the source");
    let binary = root.path().join("hello");
    let built = Command::new("/usr/bin/cc")
        .arg("-o")
        .arg(&binary)
        .arg(&source)
        .status();
    if !built.is_ok_and(|status| status.success()) {
        eprintln!("skipped: no C compiler on this Mac");
        return;
    }

    let found = MacosCodeSigning::new().assess(&binary).expect("assess");

    assert_eq!(found.signature, SignatureKind::LinkerSigned);
    assert!(found.strict_valid, "{:?}", found.detail);
}

#[test]
fn a_missing_file_is_an_answer_not_a_crash() {
    let root = TestRoot::new().expect("a test root");

    let found = MacosCodeSigning::new().assess(&root.path().join("missing"));

    // codesign exits non zero for a path that does not exist: not valid.
    assert!(found.is_ok_and(|found| !found.strict_valid));
}

#[test]
fn quarantine_is_read_from_the_attribute_and_never_through_a_symlink() {
    let root = TestRoot::new().expect("a test root");
    let file = root.path().join("tool");
    std::fs::write(&file, b"x").expect("write a file");
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&file, &link).expect("make a symlink");
    let signing = MacosCodeSigning::new();

    assert_eq!(signing.quarantine(&file).expect("read"), None);
    let status = Command::new("/usr/bin/xattr")
        .args([
            "-w",
            "com.apple.quarantine",
            "0083;5f3c1a2b;Safari;1A2B3C4D",
        ])
        .arg(&file)
        .status()
        .expect("run xattr");
    assert!(status.success());

    let found = signing
        .quarantine(&file)
        .expect("read")
        .expect("quarantined");
    assert_eq!(found.agent.as_deref(), Some("Safari"));
    // The link has no attribute of its own, and the read does not follow it.
    assert_eq!(signing.quarantine(&link).expect("read"), None);
}
