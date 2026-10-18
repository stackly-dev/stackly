//! Native support on a host needs an artifact for exactly that host's OS
//! and architecture, with no substitution (AC-074).

use stackly_domain::{Arch, Os, Target, native_support};
use stackly_evidence::{Kind, evidence};

#[test]
fn windows_native_support_needs_a_windows_artifact() {
    let ev = evidence!(
        covers = ["AC-074"],
        kind = Kind::Fixture,
        expected = "on a windows-aarch64 host, a provider with only macOS artifacts, only linux artifacts (WSL builds included), or only a windows-x86_64 artifact is not natively supported, and one with a windows-aarch64 artifact is",
    );
    let host = Target::new(Os::Windows, Arch::Aarch64);
    let mac = Target::new(Os::Macos, Arch::Aarch64);
    let linux = [
        Target::new(Os::Linux, Arch::Aarch64),
        Target::new(Os::Linux, Arch::X86_64),
    ];

    assert!(!native_support(&[], host));
    assert!(!native_support(
        &[mac, Target::new(Os::Macos, Arch::X86_64)],
        host
    ));
    assert!(!native_support(&linux, host));
    assert!(!native_support(
        &[Target::new(Os::Windows, Arch::X86_64)],
        host
    ));
    assert!(native_support(&[mac, linux[0], host], host));
    assert!(native_support(&[mac], mac));
    ev.pass();
}
