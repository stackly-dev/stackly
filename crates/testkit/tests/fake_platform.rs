//! Behavior of the fake clock, identity, platform facts, and the
//! `FakePlatform` bundle.
#![cfg(unix)]

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{Arch, Os, Target};
use stackly_platform::{CapabilityStatus, PlatformCapability, UserId};
use stackly_testkit::{FakeClock, FakePlatform, FakePlatformInfo, TestRoot};

#[test]
fn fake_clock_starts_at_2026_and_moves_only_when_told() {
    let clock = FakeClock::new();
    let platform_clock: &dyn stackly_platform::Clock = &clock;

    assert_eq!(platform_clock.now().to_string(), "2026-01-01T00:00:00Z");
    clock.advance(SignedDuration::from_secs(31));
    assert_eq!(platform_clock.now().to_string(), "2026-01-01T00:00:31Z");
    let later: Timestamp = "2026-03-01T12:00:00Z".parse().unwrap();
    clock.set(later);
    assert_eq!(platform_clock.now(), later);
}

#[test]
fn fake_platform_shares_its_clock_and_identity_with_the_bundle() {
    let root = TestRoot::new().unwrap();
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();

    fake.clock().advance(SignedDuration::from_hours(1));
    assert_eq!(platform.clock().now().to_string(), "2026-01-01T01:00:00Z");

    let owner = platform.identity().current_user().unwrap();
    fake.identity().set_current(UserId::Unix(4242));
    assert_eq!(
        platform.identity().current_user().unwrap(),
        UserId::Unix(4242)
    );
    assert_ne!(owner, UserId::Unix(4242));
}

#[test]
fn fake_identity_starts_as_the_owner_of_the_test_root() {
    use std::os::unix::fs::MetadataExt;

    let root = TestRoot::new().unwrap();
    let uid = std::fs::metadata(root.path()).unwrap().uid();

    let user = FakePlatform::new(&root)
        .platform()
        .identity()
        .current_user();

    assert_eq!(user.unwrap(), UserId::Unix(uid));
}

#[test]
fn fake_platform_info_reports_macos_capabilities_and_takes_overrides() {
    let root = TestRoot::new().unwrap();
    let info = FakePlatformInfo::new()
        .with_target(Target::new(Os::Linux, Arch::X86_64))
        .with_capability(
            PlatformCapability::SecretStore,
            CapabilityStatus::NotSupported {
                reason: "switched off",
            },
        );
    let default = FakePlatform::new(&root).platform();
    let custom = FakePlatform::new(&root).with_info(info).platform();

    assert_eq!(default.info().target().to_string(), "macos-aarch64");
    let caps = default.info().capabilities();
    for supported in [
        PlatformCapability::Directories,
        PlatformCapability::Filesystem,
        PlatformCapability::Clock,
        PlatformCapability::UserIdentity,
        PlatformCapability::SecretStore,
        PlatformCapability::ProcessSupervision,
    ] {
        assert!(caps.status(supported).is_supported(), "{supported}");
    }
    assert_eq!(
        caps.status(PlatformCapability::IpcEndpoint),
        CapabilityStatus::NotSupported {
            reason: "not built yet (feature 6)"
        }
    );
    assert_eq!(custom.info().target().to_string(), "linux-x86_64");
    assert_eq!(
        custom
            .info()
            .capabilities()
            .status(PlatformCapability::SecretStore),
        CapabilityStatus::NotSupported {
            reason: "switched off"
        }
    );
}

#[test]
fn with_roots_changes_the_resolved_roots_but_not_the_confinement() {
    let root = TestRoot::new().unwrap();
    let elsewhere = TestRoot::new().unwrap();
    let fake = FakePlatform::new(&root).with_roots(elsewhere.roots());

    let roots = fake.platform().directories().roots().unwrap();
    let outside = fake.platform().fs().symlink_metadata(elsewhere.path());

    assert_eq!(roots, elsewhere.roots());
    assert!(outside.is_err());
    assert_eq!(fake.fs().violations(), 1);
}
