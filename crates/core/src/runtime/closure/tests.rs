use stackly_evidence::{Kind, evidence};

use super::*;
use crate::runtime::archive::{ExtractSpec, scan};
use crate::runtime::macho::parse_bytes;
use crate::runtime::samples::{self, MachOBuilder, MemoryFile, TarBuilder, universal};
use stackly_domain::ArtifactFormat;

/// A tree built as a tar.gz, with the bytes of its Mach-O files kept to
/// parse them.
#[derive(Default)]
struct Sample {
    tar: TarBuilder,
    machos: Vec<(String, Vec<u8>)>,
}

impl Sample {
    fn macho(mut self, path: &str, bytes: Vec<u8>) -> Self {
        self.tar = self.tar.file(path, 0o755, &bytes);
        self.machos.push((path.to_owned(), bytes));
        self
    }

    fn file(mut self, path: &str, data: &[u8]) -> Self {
        self.tar = self.tar.file(path, 0o755, data);
        self
    }

    fn symlink(mut self, path: &str, target: &str) -> Self {
        self.tar = self.tar.symlink(path, target);
        self
    }

    fn check(
        &self,
        executables: &[&str],
        maintainer: bool,
        expected: Option<&[&str]>,
    ) -> Result<ClosureReport, ClosureError> {
        let executables: Vec<EntryPath> = executables
            .iter()
            .map(|path| EntryPath::new(path).unwrap())
            .collect();
        let spec = ExtractSpec::new(None, executables.clone(), samples::sample_limits());
        let report = scan(
            ArtifactFormat::TarGz,
            &mut MemoryFile::new(self.tar.clone().finish_gz()),
            &spec,
        )
        .unwrap();
        let machos: Vec<MachOEntry> = self
            .machos
            .iter()
            .map(|(path, bytes)| {
                MachOEntry::new(EntryPath::new(path).unwrap(), parse_bytes(bytes).unwrap())
            })
            .collect();
        let expected: Option<Vec<String>> =
            expected.map(|names| names.iter().map(|name| (*name).to_owned()).collect());
        let host: OsVersion = "27.0".parse().unwrap();
        check(&ClosureInput::new(
            &report.entries,
            &machos,
            &executables,
            &host,
            maintainer,
            expected.as_deref(),
        ))
    }
}

/// `bin/tool` loading a bundled `libz` through `@rpath`, plus system
/// libraries; `lib/libz.1.dylib` is a symlink to the real file.
fn bundled() -> Sample {
    Sample::default()
        .macho(
            "bin/tool",
            MachOBuilder::executable()
                .min_os(13, 0)
                .load("/usr/lib/libSystem.B.dylib")
                .load(
                    "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation",
                )
                .load("@rpath/libz.1.dylib")
                .rpath("@executable_path/../lib")
                .build(),
        )
        .macho(
            "lib/libz.1.3.1.dylib",
            MachOBuilder::dylib("@rpath/libz.1.dylib")
                .load("@rpath/libz.1.dylib")
                .load("/usr/lib/libSystem.B.dylib")
                .build(),
        )
        .symlink("lib/libz.1.dylib", "libz.1.3.1.dylib")
}

fn outside(path: &str, library: &str) -> Result<ClosureReport, ClosureError> {
    Err(ClosureError::DependencyOutsideRuntime {
        path: path.to_owned(),
        library: library.to_owned(),
    })
}

#[test]
fn a_bundled_library_found_through_rpath_matches_the_dependencies() {
    let ev = evidence!(
        covers = ["REQ-005", "AC-005"],
        kind = Kind::Fixture,
        expected = "a tree whose arm64 executable loads system libraries and a bundled libz through @rpath and a symlink passes the closure check with exactly libz.1.dylib as its non system library and no warning (REQ-005, AC-005)",
    );

    let report = bundled()
        .check(&["bin/tool"], true, Some(&["libz.1.dylib"]))
        .unwrap();

    assert_eq!(report.libraries, ["libz.1.dylib"]);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    ev.pass();
}

#[test]
fn loader_path_executable_path_and_symlinked_folders_resolve_inside_the_tree() {
    let sample = bundled()
        .macho(
            "lib/plugin/ext.so",
            MachOBuilder::dylib("ext.so")
                .load("@loader_path/../../Frameworks/Foo.framework/Versions/Current/Foo")
                .load("@executable_path/../lib/libz.1.dylib")
                .build(),
        )
        .macho(
            "Frameworks/Foo.framework/Versions/A/Foo",
            MachOBuilder::dylib("@rpath/Foo.framework/Versions/A/Foo").build(),
        )
        .symlink("Frameworks/Foo.framework/Versions/Current", "A");

    let report = sample
        .check(&["bin/tool"], false, Some(&["Foo", "libz.1.dylib"]))
        .unwrap();

    assert_eq!(report.libraries, ["Foo", "libz.1.dylib"]);
}

#[test]
fn an_executable_without_an_arm64_slice_is_an_architecture_mismatch() {
    let ev = evidence!(
        covers = ["REQ-005", "AC-005"],
        kind = Kind::Fixture,
        expected = "an executable with only an x86_64 slice, and one that is no Mach-O file at all, fail with architecture_mismatch naming the architectures found, while a universal x86_64 plus arm64 executable passes (REQ-005, AC-005)",
    );
    let intel = Sample::default().macho("server", MachOBuilder::executable().x86_64().build());
    let script = Sample::default().file("server", b"#!/bin/sh\nexit 0\n");
    let fat = Sample::default().macho(
        "server",
        universal(&[
            MachOBuilder::executable().x86_64().build(),
            MachOBuilder::executable().min_os(11, 0).build(),
        ]),
    );

    assert_eq!(
        intel.check(&["server"], false, Some(&[])),
        Err(ClosureError::ArchitectureMismatch {
            path: "server".to_owned(),
            arches: vec!["x86_64".to_owned()],
        })
    );
    assert_eq!(
        script.check(&["server"], false, Some(&[])),
        Err(ClosureError::ArchitectureMismatch {
            path: "server".to_owned(),
            arches: Vec::new(),
        })
    );
    assert!(fat.check(&["server"], false, Some(&[])).is_ok());
    ev.pass();
}

#[test]
fn an_executable_for_a_newer_macos_than_the_host_is_too_old() {
    let sample =
        Sample::default().macho("server", MachOBuilder::executable().min_os(99, 0).build());

    assert_eq!(
        sample.check(&["server"], false, Some(&[])),
        Err(ClosureError::OsTooOld {
            path: "server".to_owned(),
            required: "99.0".parse().unwrap(),
            host: "27.0".parse().unwrap(),
        })
    );
}

#[test]
fn libraries_outside_the_runtime_fail_the_closure() {
    let ev = evidence!(
        covers = ["REQ-005", "AC-005"],
        kind = Kind::Fixture,
        expected = "a reference to /opt/homebrew/lib/libfoo.dylib, an @rpath reference resolvable only through an absolute rpath outside the system, and a missing strong library fail with dependency_outside_runtime naming the file and library, while a missing weak library only warns (REQ-005, AC-005)",
    );
    let homebrew = Sample::default().macho(
        "server",
        MachOBuilder::executable()
            .load("/opt/homebrew/lib/libfoo.dylib")
            .build(),
    );
    let only_absolute = Sample::default().macho(
        "server",
        MachOBuilder::executable()
            .load("@rpath/libfoo.dylib")
            .rpath("/opt/homebrew/lib")
            .build(),
    );
    let missing = Sample::default().macho(
        "server",
        MachOBuilder::executable()
            .load("@rpath/libgone.dylib")
            .build(),
    );
    let weak = Sample::default().macho(
        "server",
        MachOBuilder::executable()
            .weak("@rpath/libgone.dylib")
            .build(),
    );

    assert_eq!(
        homebrew.check(&["server"], false, Some(&[])),
        outside("server", "/opt/homebrew/lib/libfoo.dylib")
    );
    assert_eq!(
        only_absolute.check(&["server"], false, Some(&[])),
        outside("server", "@rpath/libfoo.dylib")
    );
    assert_eq!(
        missing.check(&["server"], false, Some(&[])),
        outside("server", "@rpath/libgone.dylib")
    );
    let report = weak.check(&["server"], false, Some(&[])).unwrap();
    assert_eq!(
        report.warnings,
        ["server: weak library @rpath/libgone.dylib is missing"]
    );
    ev.pass();
}

#[test]
fn an_absolute_rpath_outside_the_system_warns_upstream_and_fails_a_maintainer_artifact() {
    let sample = Sample::default()
        .macho(
            "bin/tool",
            MachOBuilder::executable()
                .load("@rpath/libz.1.dylib")
                .rpath("/Volumes/build/lib")
                .rpath("/usr/lib/swift")
                .rpath("@executable_path/../lib")
                .build(),
        )
        .macho(
            "lib/libz.1.dylib",
            MachOBuilder::dylib("@rpath/libz.1.dylib").build(),
        );

    let upstream = sample
        .check(&["bin/tool"], false, Some(&["libz.1.dylib"]))
        .unwrap();
    let maintainer = sample.check(&["bin/tool"], true, Some(&["libz.1.dylib"]));

    assert_eq!(
        upstream.warnings,
        ["bin/tool: LC_RPATH /Volumes/build/lib is absolute outside the system"]
    );
    assert_eq!(maintainer, outside("bin/tool", "/Volumes/build/lib"));
}

#[test]
fn the_libraries_found_must_be_exactly_the_bundled_and_own_dependencies() {
    let ev = evidence!(
        covers = ["REQ-005", "AC-005"],
        kind = Kind::Fixture,
        expected = "a dependency list that names a library the tree lacks fails with dependency_mismatch listing it as missing, one that leaves out a library found fails listing it as unexpected, and without a dependency list the comparison is skipped with a warning (REQ-005, AC-005)",
    );

    assert_eq!(
        bundled().check(
            &["bin/tool"],
            true,
            Some(&["libz.1.dylib", "libssl.3.dylib"])
        ),
        Err(ClosureError::DependencyMismatch {
            missing: vec!["libssl.3.dylib".to_owned()],
            unexpected: Vec::new(),
        })
    );
    assert_eq!(
        bundled().check(&["bin/tool"], true, Some(&[])),
        Err(ClosureError::DependencyMismatch {
            missing: Vec::new(),
            unexpected: vec!["libz.1.dylib".to_owned()],
        })
    );
    let report = bundled().check(&["bin/tool"], true, None).unwrap();
    assert_eq!(
        report.warnings,
        ["dependencies not declared; the library set was not compared"]
    );
    ev.pass();
}

#[test]
fn a_mach_o_file_without_arm64_that_no_executable_names_only_warns() {
    let sample = bundled().macho(
        "lib/legacy.dylib",
        MachOBuilder::dylib("legacy").x86_64().build(),
    );

    let report = sample
        .check(&["bin/tool"], true, Some(&["libz.1.dylib"]))
        .unwrap();

    assert_eq!(
        report.warnings,
        ["lib/legacy.dylib: no arm64 slice (x86_64), never loaded by an arm64 process"]
    );
}

fn record(path: &str, kind: EntryKind, target: Option<&str>) -> EntryRecord {
    EntryRecord {
        path: EntryPath::new(path).unwrap(),
        kind,
        mode: None,
        size: 0,
        target: target.map(str::to_owned),
    }
}

#[test]
fn tree_resolution_follows_symlinks_but_never_leaves_the_tree_or_loops() {
    let entries = [
        record("a", EntryKind::Dir, None),
        record("a/f", EntryKind::File, None),
        record("a/up", EntryKind::Symlink, Some("..")),
        record("a/out", EntryKind::Symlink, Some("../..")),
        record("a/loop", EntryKind::Symlink, Some("loop")),
        record("a/dir", EntryKind::Symlink, Some(".")),
    ];
    let tree = Tree::new(&entries);

    assert!(tree.is_file(&["a"], "f"));
    assert!(tree.is_file(&["a"], "up/a/f"));
    assert!(tree.is_file(&["a"], "dir/dir/f"));
    assert!(tree.is_file(&[], "a/./f"));
    assert!(!tree.is_file(&["a"], "out/x/a/f"));
    assert!(!tree.is_file(&["a"], "loop"));
    assert!(!tree.is_file(&["a"], "../../a/f"));
    assert!(!tree.is_file(&["a"], "."));
    assert!(!tree.is_file(&["a"], "f/g"));
    assert!(!tree.is_file(&[], "a/dir/dir/dir/dir/dir/dir/dir/dir/dir/f"));
}
