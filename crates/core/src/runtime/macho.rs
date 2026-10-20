//! Mach-O headers and load commands (spec 0018, *Validasi binary* step 2),
//! read with the `object` crate from the header and load commands only.
//!
//! A thin 64 bit file and a universal file are both accepted. For a
//! universal file the architecture of every slice is listed, and only the
//! arm64 slice (or arm64e, when no arm64 slice exists) is parsed further:
//! the file type, the macOS minimum from `LC_BUILD_VERSION` or
//! `LC_VERSION_MIN_MACOSX`, `LC_ID_DYLIB`, every library load command, and
//! every `LC_RPATH`. The load commands of one slice may span at most
//! [`LOAD_COMMANDS_MAX_LEN`] bytes.

use std::cell::RefCell;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::rc::Rc;

use object::read::macho::{
    FatArch, LoadCommandVariant, MachHeader, MachOFatFile32, MachOFatFile64,
};
use object::{Endianness, ReadCache, ReadRef};
use stackly_domain::OsVersion;
use stackly_platform::ReadSeekFile;

/// The most bytes the header and load commands of one slice may span:
/// 4 MiB.
pub const LOAD_COMMANDS_MAX_LEN: u64 = 4 << 20;

/// The most slices a universal file may list; more is not a Mach-O file.
pub const FAT_ARCHES_MAX: u32 = 30;

const CPU_TYPE_ARM64: u32 = 0x0100_000c;
const CPU_TYPE_X86_64: u32 = 0x0100_0007;
const CPU_SUBTYPE_MASK: u32 = 0xff00_0000;
const CPU_SUBTYPE_ARM64E: u32 = 2;
const MH_EXECUTE: u32 = 2;
const PLATFORM_MACOS: u32 = 1;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
const LC_REEXPORT_DYLIB: u32 = 0x8000_001f;
const LC_LAZY_LOAD_DYLIB: u32 = 0x20;
const LC_LOAD_UPWARD_DYLIB: u32 = 0x8000_0023;
const LC_VERSION_MIN_MACOSX: u32 = 0x24;
const MACH_HEADER_64_LEN: u64 = 32;

/// What the first bytes of a file say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MachOKind {
    /// A thin 64 bit Mach-O file (`MH_MAGIC_64` or `MH_CIGAM_64`).
    Thin,
    /// A universal file (`FAT_MAGIC` or `FAT_MAGIC_64`) listing 1 to
    /// [`FAT_ARCHES_MAX`] slices.
    Universal,
}

/// Classifies a file by its first 8 bytes; `None` for anything else,
/// including a Java class file, which shares `FAT_MAGIC` but lists far
/// more "slices".
#[must_use]
pub fn classify(head: &[u8]) -> Option<MachOKind> {
    match head {
        [0xcf, 0xfa, 0xed, 0xfe, ..] | [0xfe, 0xed, 0xfa, 0xcf, ..] => Some(MachOKind::Thin),
        [0xca, 0xfe, 0xba, 0xbe | 0xbf, a, b, c, d, ..] => {
            let arches = u32::from_be_bytes([*a, *b, *c, *d]);
            (1..=FAT_ARCHES_MAX)
                .contains(&arches)
                .then_some(MachOKind::Universal)
        }
        _ => None,
    }
}

/// The CPU of one slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CpuArch {
    /// Apple Silicon.
    Arm64,
    /// Apple Silicon with pointer authentication.
    Arm64e,
    /// Intel 64 bit.
    X86_64,
    /// Any other CPU type and subtype.
    Other {
        /// The raw `cputype`.
        cputype: u32,
        /// The raw `cpusubtype`.
        cpusubtype: u32,
    },
}

impl CpuArch {
    fn from_raw(cputype: u32, cpusubtype: u32) -> Self {
        match (cputype, cpusubtype & !CPU_SUBTYPE_MASK) {
            (CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64E) => Self::Arm64e,
            (CPU_TYPE_ARM64, _) => Self::Arm64,
            (CPU_TYPE_X86_64, _) => Self::X86_64,
            _ => Self::Other {
                cputype,
                cpusubtype,
            },
        }
    }

    /// True for arm64 and arm64e, the slices an arm64 process loads.
    #[must_use]
    pub const fn is_arm64(self) -> bool {
        matches!(self, Self::Arm64 | Self::Arm64e)
    }
}

impl fmt::Display for CpuArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arm64 => f.write_str("arm64"),
            Self::Arm64e => f.write_str("arm64e"),
            Self::X86_64 => f.write_str("x86_64"),
            Self::Other {
                cputype,
                cpusubtype,
            } => write!(f, "cpu {cputype:#x}/{cpusubtype:#x}"),
        }
    }
}

/// How a library load command loads its library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryKind {
    /// `LC_LOAD_DYLIB`.
    Load,
    /// `LC_LOAD_WEAK_DYLIB`: the process starts without it.
    Weak,
    /// `LC_REEXPORT_DYLIB`.
    Reexport,
    /// `LC_LAZY_LOAD_DYLIB`.
    Lazy,
    /// `LC_LOAD_UPWARD_DYLIB`.
    Upward,
}

/// One library a slice loads.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Library {
    /// The install name as the load command gives it.
    pub name: String,
    /// The load command.
    pub kind: LibraryKind,
}

/// What the load commands of one slice say.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Slice {
    /// The CPU.
    pub arch: CpuArch,
    /// The Mach-O file type, such as 2 (`MH_EXECUTE`) for an executable.
    pub file_type: u32,
    /// The lowest macOS it runs on, when it says.
    pub min_os: Option<OsVersion>,
    /// `LC_ID_DYLIB`, for a library.
    pub install_name: Option<String>,
    /// Every library load command, in order.
    pub libraries: Vec<Library>,
    /// Every `LC_RPATH`, in order.
    pub rpaths: Vec<String>,
}

impl Slice {
    /// True for a main executable (`MH_EXECUTE`).
    #[must_use]
    pub const fn is_executable(&self) -> bool {
        self.file_type == MH_EXECUTE
    }
}

/// One parsed Mach-O file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MachO {
    /// The CPU of every slice, in file order; one for a thin file.
    pub arches: Vec<CpuArch>,
    /// The arm64 slice (arm64e when there is no arm64 one), or `None`
    /// when the file has neither.
    pub arm64: Option<Slice>,
}

/// Why a file could not be read as Mach-O.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MachOError {
    /// The first bytes are not a Mach-O magic number.
    #[error("not a mach-o file")]
    NotMachO,
    /// The header or a load command is malformed.
    #[error("malformed mach-o file: {reason}")]
    Malformed {
        /// What is wrong.
        reason: String,
    },
    /// The load commands of a slice span more than
    /// [`LOAD_COMMANDS_MAX_LEN`] bytes.
    #[error("the load commands span more than 4 MiB")]
    TooLarge,
    /// Reading the file itself failed.
    #[error("reading the file failed: {message}")]
    Read {
        /// The I/O error kind.
        kind: io::ErrorKind,
        /// The OS error message.
        message: String,
    },
}

/// Parses a Mach-O file from its start, reading only its headers and load
/// commands.
///
/// # Errors
///
/// Returns [`MachOError::NotMachO`] for another kind of file,
/// [`MachOError::Malformed`] or [`MachOError::TooLarge`] for a broken
/// one, and [`MachOError::Read`] when the file cannot be read.
pub fn parse(file: &mut dyn ReadSeekFile) -> Result<MachO, MachOError> {
    let len = file.len();
    let noted = Rc::new(RefCell::new(None));
    let cache = ReadCache::new(Noting {
        file,
        noted: Rc::clone(&noted),
    });
    let result = parse_ref(&cache, len);
    match (result, noted.borrow_mut().take()) {
        (Err(_), Some((kind, message))) => Err(MachOError::Read { kind, message }),
        (result, _) => result,
    }
}

/// Parses a Mach-O file held in memory; the fuzz target and tests use it.
///
/// # Errors
///
/// Returns the errors of [`parse`] but [`MachOError::Read`].
pub fn parse_bytes(bytes: &[u8]) -> Result<MachO, MachOError> {
    parse_ref(bytes, bytes.len() as u64)
}

fn parse_ref<'data, R: ReadRef<'data>>(data: R, len: u64) -> Result<MachO, MachOError> {
    let head = data
        .read_bytes_at(0, len.min(8))
        .map_err(|()| MachOError::NotMachO)?;
    match classify(head).ok_or(MachOError::NotMachO)? {
        MachOKind::Thin => {
            let slice = parse_slice(data, 0)?;
            Ok(MachO {
                arches: vec![slice.arch],
                arm64: slice.arch.is_arm64().then_some(slice),
            })
        }
        MachOKind::Universal => {
            let arches = if head[3] == 0xbf {
                fat_arches(&MachOFatFile64::parse(data).map_err(malformed)?)
            } else {
                fat_arches(&MachOFatFile32::parse(data).map_err(malformed)?)
            };
            let chosen = arches
                .iter()
                .find(|(arch, _)| *arch == CpuArch::Arm64)
                .or_else(|| arches.iter().find(|(arch, _)| *arch == CpuArch::Arm64e));
            let arm64 = chosen
                .map(|(arch, offset)| {
                    let slice = parse_slice(data, *offset)?;
                    if slice.arch == *arch {
                        Ok(slice)
                    } else {
                        Err(MachOError::Malformed {
                            reason: format!("the {arch} slice holds a {} header", slice.arch),
                        })
                    }
                })
                .transpose()?;
            Ok(MachO {
                arches: arches.into_iter().map(|(arch, _)| arch).collect(),
                arm64,
            })
        }
    }
}

fn fat_arches<Fat: FatArch>(
    file: &object::read::macho::MachOFatFile<'_, Fat>,
) -> Vec<(CpuArch, u64)> {
    file.arches()
        .iter()
        .map(|arch| {
            (
                CpuArch::from_raw(arch.cputype().0, arch.cpusubtype().0),
                arch.offset().into(),
            )
        })
        .collect()
}

/// Parses the 64 bit header and load commands of the slice at `offset`.
fn parse_slice<'data, R: ReadRef<'data>>(data: R, offset: u64) -> Result<Slice, MachOError> {
    let header =
        object::macho::MachHeader64::<Endianness>::parse(data, offset).map_err(malformed)?;
    let endian = header.endian().map_err(malformed)?;
    if MACH_HEADER_64_LEN + u64::from(header.sizeofcmds(endian)) > LOAD_COMMANDS_MAX_LEN {
        return Err(MachOError::TooLarge);
    }
    let mut slice = Slice {
        arch: CpuArch::from_raw(header.cputype(endian).0, header.cpusubtype(endian).0),
        file_type: header.filetype(endian).0,
        min_os: None,
        install_name: None,
        libraries: Vec::new(),
        rpaths: Vec::new(),
    };
    let mut commands = header
        .load_commands(endian, data, offset)
        .map_err(malformed)?;
    while let Some(command) = commands.next().map_err(malformed)? {
        match command.variant().map_err(malformed)? {
            LoadCommandVariant::Dylib(dylib) => {
                let kind = match command.cmd().0 {
                    LC_LOAD_DYLIB => LibraryKind::Load,
                    LC_LOAD_WEAK_DYLIB => LibraryKind::Weak,
                    LC_REEXPORT_DYLIB => LibraryKind::Reexport,
                    LC_LAZY_LOAD_DYLIB => LibraryKind::Lazy,
                    LC_LOAD_UPWARD_DYLIB => LibraryKind::Upward,
                    _ => continue,
                };
                let name = text(
                    command
                        .string(endian, dylib.dylib.name)
                        .map_err(malformed)?,
                )?;
                slice.libraries.push(Library { name, kind });
            }
            LoadCommandVariant::IdDylib(dylib) => {
                let name = text(
                    command
                        .string(endian, dylib.dylib.name)
                        .map_err(malformed)?,
                )?;
                slice.install_name = Some(name);
            }
            LoadCommandVariant::Rpath(rpath) => {
                slice.rpaths.push(text(
                    command.string(endian, rpath.path).map_err(malformed)?,
                )?);
            }
            LoadCommandVariant::BuildVersion(build, _)
                if build.platform.get(endian).0 == PLATFORM_MACOS =>
            {
                slice.min_os = Some(version(build.minos.get(endian).0)?);
            }
            LoadCommandVariant::VersionMin(min)
                if command.cmd().0 == LC_VERSION_MIN_MACOSX && slice.min_os.is_none() =>
            {
                slice.min_os = Some(version(min.version.get(endian).0)?);
            }
            _ => {}
        }
    }
    Ok(slice)
}

/// Decodes a version packed as `xxxx.yy.zz` in nibbles; a zero patch is
/// left out, as `vtool` prints it.
fn version(packed: u32) -> Result<OsVersion, MachOError> {
    let (major, minor, patch) = (packed >> 16, (packed >> 8) & 0xff, packed & 0xff);
    let text = if patch == 0 {
        format!("{major}.{minor}")
    } else {
        format!("{major}.{minor}.{patch}")
    };
    text.parse().map_err(|_| MachOError::Malformed {
        reason: format!("version {text} is not a macOS version"),
    })
}

fn text(bytes: &[u8]) -> Result<String, MachOError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| MachOError::Malformed {
        reason: "a load command string is not UTF-8".to_owned(),
    })
}

fn malformed(err: object::Error) -> MachOError {
    MachOError::Malformed {
        reason: err.to_string(),
    }
}

/// The file under a [`ReadCache`], noting the first I/O error it returned.
struct Noting<'a> {
    file: &'a mut dyn ReadSeekFile,
    noted: Rc<RefCell<Option<(io::ErrorKind, String)>>>,
}

impl Noting<'_> {
    fn note<T>(&self, result: io::Result<T>) -> io::Result<T> {
        if let Err(err) = &result {
            self.noted
                .borrow_mut()
                .get_or_insert_with(|| (err.kind(), err.to_string()));
        }
        result
    }
}

impl Read for Noting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let result = self.file.read(buf);
        self.note(result)
    }
}

impl Seek for Noting<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let result = self.file.seek(pos);
        self.note(result)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Seek, SeekFrom};

    use super::*;
    use crate::runtime::samples::{MachOBuilder, MemoryFile, universal};

    fn version(text: &str) -> Option<OsVersion> {
        Some(text.parse().unwrap())
    }

    #[test]
    fn magic_numbers_tell_thin_and_universal_files_from_others() {
        assert_eq!(
            classify(&MachOBuilder::executable().build()),
            Some(MachOKind::Thin)
        );
        assert_eq!(
            classify(&[0xfe, 0xed, 0xfa, 0xcf, 0, 0, 0, 0]),
            Some(MachOKind::Thin)
        );
        assert_eq!(
            classify(&universal(&[MachOBuilder::executable().build()])),
            Some(MachOKind::Universal)
        );
        // A Java class file: FAT_MAGIC, then minor 0 and major 52.
        assert_eq!(classify(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 52]), None);
        assert_eq!(classify(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 0]), None);
        assert_eq!(classify(&[0xce, 0xfa, 0xed, 0xfe, 0, 0, 0, 0]), None);
        assert_eq!(classify(b"\x1f\x8b\x08\0"), None);
        assert_eq!(classify(b""), None);
    }

    #[test]
    fn a_thin_arm64_file_gives_every_command_the_closure_needs() {
        let bytes = MachOBuilder::dylib("@rpath/libtool.1.dylib")
            .min_os(13, 0)
            .load("/usr/lib/libSystem.B.dylib")
            .weak("@rpath/libweak.dylib")
            .reexport("@loader_path/libre.dylib")
            .lazy("@executable_path/../lib/liblazy.dylib")
            .upward("@rpath/libup.dylib")
            .rpath("@loader_path/../lib")
            .rpath("/usr/lib/swift")
            .build();

        let parsed = parse_bytes(&bytes).unwrap();

        assert_eq!(parsed.arches, [CpuArch::Arm64]);
        let slice = parsed.arm64.unwrap();
        assert!(!slice.is_executable());
        assert_eq!(slice.min_os, version("13.0"));
        assert_eq!(
            slice.install_name.as_deref(),
            Some("@rpath/libtool.1.dylib")
        );
        let kinds: Vec<_> = slice
            .libraries
            .iter()
            .map(|library| (library.name.as_str(), library.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("/usr/lib/libSystem.B.dylib", LibraryKind::Load),
                ("@rpath/libweak.dylib", LibraryKind::Weak),
                ("@loader_path/libre.dylib", LibraryKind::Reexport),
                ("@executable_path/../lib/liblazy.dylib", LibraryKind::Lazy),
                ("@rpath/libup.dylib", LibraryKind::Upward),
            ]
        );
        assert_eq!(slice.rpaths, ["@loader_path/../lib", "/usr/lib/swift"]);
    }

    #[test]
    fn an_intel_only_file_lists_its_arch_without_an_arm64_slice() {
        let parsed = parse_bytes(&MachOBuilder::executable().x86_64().build()).unwrap();

        assert_eq!(parsed.arches, [CpuArch::X86_64]);
        assert_eq!(parsed.arm64, None);
        assert_eq!(CpuArch::X86_64.to_string(), "x86_64");
    }

    #[test]
    fn a_universal_file_parses_its_arm64_slice_and_prefers_arm64_over_arm64e() {
        let intel = MachOBuilder::executable().x86_64().min_os(10, 15).build();
        let arm = MachOBuilder::executable().min_os(11, 0).build();
        let arm64e = MachOBuilder::executable().arm64e().min_os(14, 2).build();

        let both = parse_bytes(&universal(&[intel.clone(), arm64e.clone(), arm])).unwrap();
        let only_e = parse_bytes(&universal(&[intel, arm64e])).unwrap();

        assert_eq!(
            both.arches,
            [CpuArch::X86_64, CpuArch::Arm64e, CpuArch::Arm64]
        );
        let slice = both.arm64.unwrap();
        assert_eq!(
            (slice.arch, slice.min_os),
            (CpuArch::Arm64, version("11.0"))
        );
        assert!(slice.is_executable());
        let slice = only_e.arm64.unwrap();
        assert_eq!(
            (slice.arch, slice.min_os),
            (CpuArch::Arm64e, version("14.2"))
        );
    }

    #[test]
    fn the_macos_minimum_comes_from_build_version_before_version_min() {
        let other_platform = MachOBuilder::executable().build_version(2, 17, 0).build();
        let old_style = MachOBuilder::executable().version_min(12, 3).build();
        let both = MachOBuilder::executable()
            .version_min(10, 9)
            .min_os(13, 0)
            .build();

        assert_eq!(
            parse_bytes(&other_platform).unwrap().arm64.unwrap().min_os,
            None
        );
        assert_eq!(
            parse_bytes(&old_style).unwrap().arm64.unwrap().min_os,
            version("12.3")
        );
        assert_eq!(
            parse_bytes(&both).unwrap().arm64.unwrap().min_os,
            version("13.0")
        );
    }

    #[test]
    fn broken_files_are_refused() {
        let good = MachOBuilder::executable().load("@rpath/libz.dylib").build();
        let mut huge = good.clone();
        huge[20..24].copy_from_slice(&u32::to_le_bytes(5 << 20));
        let mut not_utf8 = good.clone();
        let at = not_utf8.len() - 8;
        not_utf8[at] = 0xff;
        let mut lying_slice = universal(std::slice::from_ref(&good));
        // The fat header claims arm64e for an arm64 slice.
        lying_slice[12..16].copy_from_slice(&2_u32.to_be_bytes());

        assert_eq!(parse_bytes(b"#!/bin/sh\n"), Err(MachOError::NotMachO));
        assert_eq!(parse_bytes(&huge), Err(MachOError::TooLarge));
        assert!(matches!(
            parse_bytes(&good[..40]),
            Err(MachOError::Malformed { .. })
        ));
        assert!(matches!(
            parse_bytes(&not_utf8),
            Err(MachOError::Malformed { .. })
        ));
        assert!(matches!(
            parse_bytes(&lying_slice),
            Err(MachOError::Malformed { .. })
        ));
    }

    /// A file whose reads fail after the first 8 bytes.
    struct FailingFile(MemoryFile);

    impl Read for FailingFile {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0.stream_position()? >= 8 {
                return Err(io::Error::other("injected read failure"));
            }
            let limit = buf.len().min(8);
            self.0.read(&mut buf[..limit])
        }
    }

    impl Seek for FailingFile {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.0.seek(pos)
        }
    }

    impl ReadSeekFile for FailingFile {
        fn len(&self) -> u64 {
            self.0.len()
        }
    }

    #[test]
    fn a_file_reads_like_its_bytes_and_a_read_failure_is_not_malformed() {
        let bytes = universal(&[MachOBuilder::dylib("@rpath/liba.dylib")
            .min_os(13, 0)
            .build()]);

        assert_eq!(
            parse(&mut MemoryFile::new(bytes.clone())),
            parse_bytes(&bytes)
        );
        assert!(matches!(
            parse(&mut FailingFile(MemoryFile::new(bytes))),
            Err(MachOError::Read { .. })
        ));
    }
}
