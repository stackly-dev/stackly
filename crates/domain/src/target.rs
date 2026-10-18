use std::fmt;
use std::str::FromStr;

use crate::error::{ValueError, ValueKind};
use crate::text::serde_as_text;

/// An operating system that a host runs or an artifact is built for.
///
/// There is no WSL variant: an artifact built for WSL is a `linux` artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Os {
    /// Apple macOS.
    Macos,
    /// Microsoft Windows.
    Windows,
    /// Linux, including WSL.
    Linux,
}

impl Os {
    /// Every OS, in code order.
    pub const ALL: [Self; 3] = [Self::Macos, Self::Windows, Self::Linux];

    /// Returns the stable code, for example `macos`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for Os {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|os| os.as_code() == s)
            .ok_or(ValueError::new(
                ValueKind::Os,
                "must be macos, windows, or linux",
            ))
    }
}

/// A CPU architecture that a host runs or an artifact is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Arch {
    /// 64 bit ARM, such as Apple Silicon.
    Aarch64,
    /// 64 bit x86.
    X86_64,
}

impl Arch {
    /// Every architecture, in code order.
    pub const ALL: [Self; 2] = [Self::Aarch64, Self::X86_64];

    /// Returns the stable code, for example `aarch64`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64",
            Self::X86_64 => "x86_64",
        }
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for Arch {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|arch| arch.as_code() == s)
            .ok_or(ValueError::new(
                ValueKind::Arch,
                "must be aarch64 or x86_64",
            ))
    }
}

/// An OS and architecture pair, written `macos-aarch64`.
///
/// ```
/// use stackly_domain::{Arch, Os, Target};
///
/// let target: Target = "macos-aarch64".parse()?;
/// assert_eq!(target, Target::new(Os::Macos, Arch::Aarch64));
/// assert_eq!(target.to_string(), "macos-aarch64");
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Target {
    os: Os,
    arch: Arch,
}

impl Target {
    /// Pairs an OS with an architecture.
    #[must_use]
    pub const fn new(os: Os, arch: Arch) -> Self {
        Self { os, arch }
    }

    /// Returns the OS.
    #[must_use]
    pub const fn os(self) -> Os {
        self.os
    }

    /// Returns the architecture.
    #[must_use]
    pub const fn arch(self) -> Arch {
        self.arch
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.os, self.arch)
    }
}

impl FromStr for Target {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ValueError::new(ValueKind::Target, "must be <os>-<arch>");
        let (os, arch) = s.split_once('-').ok_or_else(invalid)?;
        Ok(Self::new(
            os.parse().map_err(|_| invalid())?,
            arch.parse().map_err(|_| invalid())?,
        ))
    }
}

/// Returns whether a provider runs natively on `host`: only when one of its
/// artifacts is built for exactly the host's OS and architecture (AC-074).
///
/// There is no substitution of any kind: a `linux` artifact, including one
/// built for WSL, never serves a Windows host, and a macOS artifact never
/// serves another OS.
///
/// ```
/// use stackly_domain::{Arch, Os, Target, native_support};
///
/// let mac = Target::new(Os::Macos, Arch::Aarch64);
/// let windows = Target::new(Os::Windows, Arch::Aarch64);
/// assert!(native_support(&[mac], mac));
/// assert!(!native_support(&[mac], windows));
/// ```
#[must_use]
pub fn native_support(artifact_targets: &[Target], host: Target) -> bool {
    artifact_targets.contains(&host)
}

serde_as_text! {
    Os => "an os: macos, windows, or linux",
    Arch => "an arch: aarch64 or x86_64",
    Target => "a target such as macos-aarch64",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_round_trips_through_its_text_form() {
        for os in Os::ALL {
            for arch in Arch::ALL {
                let target = Target::new(os, arch);

                assert_eq!(target.to_string().parse::<Target>().unwrap(), target);
            }
        }
        assert_eq!(
            Target::new(Os::Linux, Arch::X86_64).to_string(),
            "linux-x86_64"
        );
    }

    #[test]
    fn os_arch_and_target_serialize_as_their_codes() {
        let target = Target::new(Os::Macos, Arch::Aarch64);

        assert_eq!(serde_json::to_string(&target).unwrap(), "\"macos-aarch64\"");
        assert_eq!(serde_json::to_string(&Os::Windows).unwrap(), "\"windows\"");
        assert_eq!(
            serde_json::from_str::<Arch>("\"x86_64\"").unwrap(),
            Arch::X86_64
        );
        assert!(serde_json::from_str::<Os>("\"wsl\"").is_err());
    }

    #[test]
    fn unknown_os_arch_and_wsl_are_rejected() {
        let cases = [
            "",
            "macos",
            "wsl-x86_64",
            "macos-arm64",
            "MACOS-aarch64",
            "macos-aarch64-",
        ];
        for text in cases {
            let err = text.parse::<Target>().unwrap_err();
            assert_eq!(err.kind(), ValueKind::Target, "{text}");
        }
        assert_eq!("wsl".parse::<Os>().unwrap_err().kind(), ValueKind::Os);
        assert_eq!("arm64".parse::<Arch>().unwrap_err().kind(), ValueKind::Arch);
    }
}
