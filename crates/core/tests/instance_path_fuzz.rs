//! The seeds of the `instance_path` fuzz target as a regression test (spec
//! 0020 AC-23, *Fuzz target*): `owned_path` runs on a tree with symlinks out
//! of the root, to another root, in a loop, to itself, Unicode names, and a
//! folder 64 levels deep, and every invariant holds.
#![cfg(unix)]

use stackly_core::instance::fuzzing::{OwnedPathFixtures, fixture_tree, owned_path};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_evidence::{Kind, evidence};
use stackly_platform::Platform;
use stackly_testkit::{FakePlatform, TestRoot};

struct Tree {
    _root: TestRoot,
    platform: Platform,
    prepared: PreparedLayout,
    fixtures: OwnedPathFixtures,
}

fn tree() -> Tree {
    let root = TestRoot::new().expect("a test root can be made");
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();
    let prepared = prepare_layout(&platform).expect("the fake roots prepare");
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&outside).expect("the outside folder can be made");
    let layout = prepared.layout();
    let fixtures = fixture_tree(&layout.instances_dir(), &outside, layout.roots().logs())
        .expect("the fixture tree can be made");
    Tree {
        _root: root,
        platform,
        prepared,
        fixtures,
    }
}

fn run(tree: &Tree, input: &[u8]) {
    owned_path(&tree.platform, &tree.prepared, &tree.fixtures, input);
}

#[test]
fn instance_path_seeds_hold_the_invariants() {
    let ev = evidence!(
        covers = ["VAL-007"],
        kind = Kind::Fixture,
        expected = "owned_path never panics on the seed paths, rejects relative and dotted paths as not normalized, accepts only paths strictly inside an owned root, and reports an overlap only for data folders that share an identity",
    );
    let tree = tree();
    let root = tree.fixtures.root.display().to_string();
    let deep = (0..64)
        .map(|n| format!("d{n}"))
        .collect::<Vec<_>>()
        .join("/");
    let seeds: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"/".to_vec(),
        b".".to_vec(),
        b"..".to_vec(),
        b"a/../b".to_vec(),
        b"./a".to_vec(),
        b"//".to_vec(),
        b"inside".to_vec(),
        b"link-out".to_vec(),
        b"link-out/x/y".to_vec(),
        b"link-other-root".to_vec(),
        b"link-other-root/anything".to_vec(),
        b"loop-a".to_vec(),
        b"loop-a/x".to_vec(),
        b"link-self/link-self/link-self/inside".to_vec(),
        b"link-inside/../inside".to_vec(),
        "ünï cödé/with space/x".as_bytes().to_vec(),
        format!("deep/{deep}").into_bytes(),
        format!("deep/{deep}/more").into_bytes(),
        b"instances/one/data".to_vec(),
        b"instances/one/data/sub".to_vec(),
        b"instances/one".to_vec(),
        b"instances".to_vec(),
        b"instances/one/../two/data".to_vec(),
        format!("{root}/instances/two/data/x").into_bytes(),
        format!("{root}/instances/../instances/one/data").into_bytes(),
        format!("{root}/link-self/instances/one/data").into_bytes(),
        format!("{root}/link-inside/x").into_bytes(),
        vec![0xff, 0xfe, b'/', 0x80],
        vec![0; 3],
        b"a\0b".to_vec(),
    ];
    for seed in &seeds {
        run(&tree, seed);
    }
    ev.pass();
}

#[test]
fn pseudo_random_paths_over_the_fixture_names_hold_the_invariants() {
    let tree = tree();
    let names = [
        "inside",
        "link-out",
        "link-other-root",
        "link-inside",
        "link-self",
        "loop-a",
        "loop-b",
        "deep",
        "d0",
        "d1",
        "instances",
        "one",
        "two",
        "data",
        "ünï cödé",
        "with space",
        "..",
        ".",
        "",
        "missing",
        "x",
    ];
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..400 {
        let count = usize::try_from(next() % 8).unwrap_or(0) + 1;
        let mut path = String::new();
        if next() % 3 == 0 {
            path.push('/');
        }
        for index in 0..count {
            if index > 0 {
                path.push('/');
            }
            let pick = usize::try_from(next()).unwrap_or(0) % names.len();
            path.push_str(names[pick]);
        }
        run(&tree, path.as_bytes());
    }
}
