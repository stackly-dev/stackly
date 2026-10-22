//! Fuzz target `instance_path` (spec 0020, AC-23, VAL-007): the bytes as a
//! path that `check_owned_path` and `check_data_root_overlap` judge on a
//! fixture tree with symlinks out of the root, to another root, in a loop,
//! and to itself, names with Unicode and spaces, and a folder 64 levels
//! deep. The tree is built once, in a folder of the user temporary
//! directory named after the process, which a run leaves behind.

#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use stackly_core::instance::fuzzing::{OwnedPathFixtures, fixture_tree, owned_path};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_platform::Platform;
use stackly_platform_macos::MacosPlatform;

/// The platform and the fixture tree one run reuses for every input.
struct Setup {
    platform: Platform,
    prepared: PreparedLayout,
    fixtures: OwnedPathFixtures,
}

fn setup() -> &'static Setup {
    static SETUP: OnceLock<Setup> = OnceLock::new();
    SETUP.get_or_init(|| {
        let base = std::env::temp_dir().join(format!("stackly-fuzz-instance-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("the fixture folder can be made");
        let base = base
            .canonicalize()
            .expect("the fixture folder has a canonical path");
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).expect("the outside folder can be made");
        let root = base.join("root");
        std::fs::create_dir_all(&root).expect("the root folder can be made");
        let platform = MacosPlatform::new(Some(root.into())).expect("the folder is a valid root override");
        let prepared = prepare_layout(&platform).expect("the layout can be prepared");
        let layout = prepared.layout();
        let fixtures = fixture_tree(&layout.instances_dir(), &outside, layout.roots().logs())
            .expect("the fixture tree can be made");
        Setup {
            platform,
            prepared,
            fixtures,
        }
    })
}

fuzz_target!(|data: &[u8]| {
    let setup = setup();
    owned_path(&setup.platform, &setup.prepared, &setup.fixtures, data);
});
