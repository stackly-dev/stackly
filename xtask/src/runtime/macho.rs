//! Recognizing Mach-O and gzip files by their magic number, and reading the
//! minimum macOS version of an executable with `vtool` (spec 0003).

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// `vtool` from the Xcode command line tools; macOS only.
const VTOOL: &str = "/usr/bin/vtool";
/// The largest `nfat_arch` a real universal binary has. A Java class file
/// shares the `cafebabe` magic but stores its version (45 or more) there.
const MAX_FAT_ARCHS: u32 = 30;

/// What the first bytes of a download say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A thin or universal Mach-O file.
    MachO,
    /// A gzip stream, such as a `.tar.gz`.
    Gzip,
    /// Anything else.
    Other,
}

/// Reads the first eight bytes of `path` and classifies them.
pub fn file_kind(path: &Path) -> Result<FileKind> {
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut head = [0_u8; 8];
    let mut filled = 0;
    while filled < head.len() {
        match file.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err).with_context(|| format!("cannot read {}", path.display())),
        }
    }
    Ok(classify(&head[..filled]))
}

/// Classifies a file by its first bytes.
pub fn classify(head: &[u8]) -> FileKind {
    match head {
        // MH_MAGIC_64 and MH_MAGIC, stored little endian on arm64 and x86_64.
        [0xcf | 0xce, 0xfa, 0xed, 0xfe, ..] => FileKind::MachO,
        // FAT_MAGIC and FAT_MAGIC_64, stored big endian, then nfat_arch.
        [0xca, 0xfe, 0xba, 0xbe | 0xbf, a, b, c, d] => {
            let archs = u32::from_be_bytes([*a, *b, *c, *d]);
            if (1..=MAX_FAT_ARCHS).contains(&archs) {
                FileKind::MachO
            } else {
                FileKind::Other
            }
        }
        [0x1f, 0x8b, ..] => FileKind::Gzip,
        _ => FileKind::Other,
    }
}

/// The minimum macOS version of the arm64 slice of `path` (or of its only
/// slice when it is thin), from `vtool -show-build`. The architecture is
/// checked by the spike's static audit, not here.
pub fn min_os(path: &Path) -> Result<String> {
    let output = Command::new(VTOOL)
        .arg("-show-build")
        .arg(path)
        .output()
        .with_context(|| format!("cannot run {VTOOL}"))?;
    if !output.status.success() {
        bail!(
            "vtool cannot read {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_min_os(&String::from_utf8_lossy(&output.stdout))
        .with_context(|| format!("vtool shows no macOS minimum for {}", path.display()))
}

/// Picks the macOS minimum from `vtool -show-build` output.
///
/// A universal file prints one section per slice, each headed
/// `<path> (architecture <arch>):`; a thin file prints one section headed
/// `<path>:`. The `arm64` slice wins, then `arm64e`, then a thin file. In a
/// section, `minos` of an `LC_BUILD_VERSION` for platform `MACOS` counts,
/// or `version` of an older `LC_VERSION_MIN_MACOSX`.
pub fn parse_min_os(output: &str) -> Option<String> {
    // (architecture, minimum) per section; `None` architecture is thin.
    let mut sections: Vec<(Option<String>, Option<String>)> = Vec::new();
    let mut command = "";
    let mut platform = "";
    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) && line.ends_with(':') {
            let arch = line
                .rsplit_once("(architecture ")
                .and_then(|(_, rest)| rest.strip_suffix("):"))
                .map(str::to_owned);
            sections.push((arch, None));
            command = "";
            platform = "";
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(key), Some(value)) = (words.next(), words.next()) else {
            continue;
        };
        let Some((_, minimum)) = sections.last_mut() else {
            continue;
        };
        match key {
            "cmd" => {
                command = value;
                platform = "";
            }
            "platform" => platform = value,
            "minos" if command == "LC_BUILD_VERSION" && platform == "MACOS" => {
                minimum.get_or_insert_with(|| value.to_owned());
            }
            "version" if command == "LC_VERSION_MIN_MACOSX" => {
                minimum.get_or_insert_with(|| value.to_owned());
            }
            _ => {}
        }
    }
    let pick = |wanted: Option<&str>| {
        sections
            .iter()
            .find(|(arch, minimum)| arch.as_deref() == wanted && minimum.is_some())
            .and_then(|(_, minimum)| minimum.clone())
    };
    pick(Some("arm64"))
        .or_else(|| pick(Some("arm64e")))
        .or_else(|| pick(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIVERSAL: &str = "\
/bin/ls (architecture x86_64):
Load command 9
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform MACOS
    minos 10.14
      sdk 27.0
   ntools 1
     tool LD
  version 27037.1
/bin/ls (architecture arm64e):
Load command 11
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform MACOS
    minos 11.0
      sdk 27.0
   ntools 1
     tool LD
  version 27037.1
";

    #[test]
    fn the_arm64e_slice_of_a_universal_file_counts_over_x86_64() {
        assert_eq!(parse_min_os(UNIVERSAL).as_deref(), Some("11.0"));
    }

    #[test]
    fn a_thin_file_has_one_unnamed_section() {
        let thin = "\
/tmp/Stackly Spike/nats-server:
Load command 8
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform MACOS
    minos 12.0
      sdk 15.0
   ntools 0
";
        assert_eq!(parse_min_os(thin).as_deref(), Some("12.0"));
    }

    #[test]
    fn an_old_version_min_command_counts_but_tool_versions_do_not() {
        let old = "\
/tmp/mysqld (architecture arm64):
Load command 10
      cmd LC_VERSION_MIN_MACOSX
  cmdsize 16
  version 11.0
      sdk 13.0
";
        assert_eq!(parse_min_os(old).as_deref(), Some("11.0"));
        let only_tools = "\
/tmp/x:
Load command 9
      cmd LC_BUILD_VERSION
 platform IOS
    minos 17.0
     tool LD
  version 1053.12
";
        assert_eq!(parse_min_os(only_tools), None);
    }

    #[test]
    fn an_x86_64_only_file_has_no_answer() {
        let intel = UNIVERSAL
            .split("/bin/ls (architecture arm64e)")
            .next()
            .unwrap();
        assert_eq!(parse_min_os(intel), None);
    }

    #[test]
    fn magic_numbers_tell_mach_o_from_gzip_and_java() {
        assert_eq!(
            classify(&[0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 0x01]),
            FileKind::MachO
        );
        assert_eq!(
            classify(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2]),
            FileKind::MachO
        );
        assert_eq!(
            classify(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 0x41]),
            FileKind::Other
        );
        assert_eq!(classify(&[0x1f, 0x8b, 0x08, 0]), FileKind::Gzip);
        assert_eq!(classify(b"PK\x03\x04"), FileKind::Other);
        assert_eq!(classify(&[]), FileKind::Other);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vtool_reads_the_minimum_of_a_system_binary() {
        let minimum = min_os(Path::new("/usr/bin/true")).unwrap();
        assert!(
            minimum.split('.').all(|part| part.parse::<u32>().is_ok()),
            "{minimum}"
        );
    }
}
