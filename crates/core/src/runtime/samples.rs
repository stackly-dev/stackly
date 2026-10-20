//! Sample artifacts for tests, fuzz seeds, and regression inputs of the
//! runtime extractor (spec 0018). Not for production use: the builders
//! write exactly what they are told, broken archives included, so tests
//! can hand the extractor every malformed shape it must refuse.

use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use flate2::Compression;
use flate2::Crc;
use flate2::write::{DeflateEncoder, GzEncoder};
use stackly_domain::ArtifactFormat;
use stackly_platform::{EntryPath, ReadSeekFile};

use super::archive::{ExtractLimits, ExtractSpec, RejectReason};

const BLOCK: usize = 512;
/// The record size tar writers pad their output to.
const RECORD: usize = 10_240;
/// The longest name a ustar header holds without a GNU long name.
const NAME_FIELD: usize = 100;

/// An artifact held in memory, readable as a [`ReadSeekFile`].
#[derive(Debug, Clone, Default)]
pub struct MemoryFile(Cursor<Vec<u8>>);

impl MemoryFile {
    /// Wraps `bytes`, positioned at the start.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Cursor::new(bytes))
    }
}

impl Read for MemoryFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for MemoryFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

impl ReadSeekFile for MemoryFile {
    fn len(&self) -> u64 {
        self.0.get_ref().len() as u64
    }
}

/// Compresses `data` into one gzip member.
#[must_use]
pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(data)
        .expect("writing to a Vec never fails");
    encoder.finish().expect("finishing into a Vec never fails")
}

/// A tar stream, built member by member. Names longer than 100 bytes go
/// through a GNU long name member, like GNU tar writes them.
#[derive(Debug, Clone, Default)]
pub struct TarBuilder {
    bytes: Vec<u8>,
}

impl TarBuilder {
    /// An empty stream.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a folder.
    #[must_use]
    pub fn dir(self, name: &str, mode: u32) -> Self {
        self.member(b'5', name.as_bytes(), mode, b"", b"")
    }

    /// Adds a regular file.
    #[must_use]
    pub fn file(self, name: &str, mode: u32, data: &[u8]) -> Self {
        self.member(b'0', name.as_bytes(), mode, b"", data)
    }

    /// Adds a symlink.
    #[must_use]
    pub fn symlink(self, name: &str, target: &str) -> Self {
        self.member(b'2', name.as_bytes(), 0o777, target.as_bytes(), b"")
    }

    /// Adds a member of any type byte with raw name, link, and data.
    #[must_use]
    pub fn member(mut self, kind: u8, name: &[u8], mode: u32, link: &[u8], data: &[u8]) -> Self {
        if name.len() > NAME_FIELD {
            self = self.gnu_long(b'L', name);
        }
        if link.len() > NAME_FIELD {
            self = self.gnu_long(b'K', link);
        }
        self.bytes
            .extend_from_slice(&header(kind, name, mode, link, data.len() as u64));
        self.push_data(data);
        self
    }

    /// Adds a header that declares `size` bytes, followed by `data`
    /// padded to a block, whatever its length.
    #[must_use]
    pub fn header_with_size(
        mut self,
        kind: u8,
        name: &[u8],
        mode: u32,
        size: u64,
        data: &[u8],
    ) -> Self {
        self.bytes
            .extend_from_slice(&header(kind, name, mode, b"", size));
        self.push_data(data);
        self
    }

    /// Adds a PAX extended header for the next member.
    #[must_use]
    pub fn pax(mut self, records: &[(&str, &[u8])]) -> Self {
        let mut data = Vec::new();
        for (key, value) in records {
            let body = [b" ", key.as_bytes(), b"=", value, b"\n"].concat();
            // The length counts its own digits.
            let mut len = body.len() + 1;
            while len.to_string().len() + body.len() != len {
                len = len.to_string().len() + body.len();
            }
            data.extend_from_slice(len.to_string().as_bytes());
            data.extend_from_slice(&body);
        }
        self.bytes
            .extend_from_slice(&header(b'x', b"PaxHeader", 0o644, b"", data.len() as u64));
        self.push_data(&data);
        self
    }

    /// Adds a GNU long name (`L`) or long link (`K`) member.
    #[must_use]
    pub fn gnu_long(mut self, kind: u8, value: &[u8]) -> Self {
        let mut data = value.to_vec();
        data.push(0);
        self.bytes.extend_from_slice(&header(
            kind,
            b"././@LongLink",
            0o644,
            b"",
            data.len() as u64,
        ));
        self.push_data(&data);
        self
    }

    /// Appends raw bytes.
    #[must_use]
    pub fn raw(mut self, bytes: &[u8]) -> Self {
        self.bytes.extend_from_slice(bytes);
        self
    }

    /// Ends the stream with two zero blocks and pads it to a 10 KiB
    /// record.
    #[must_use]
    pub fn finish(mut self) -> Vec<u8> {
        self.bytes.extend_from_slice(&[0; 2 * BLOCK]);
        let padded = self.bytes.len().div_ceil(RECORD) * RECORD;
        self.bytes.resize(padded, 0);
        self.bytes
    }

    /// [`finish`](Self::finish), gzipped.
    #[must_use]
    pub fn finish_gz(self) -> Vec<u8> {
        gzip(&self.finish())
    }

    fn push_data(&mut self, data: &[u8]) {
        self.bytes.extend_from_slice(data);
        let padded = self.bytes.len().div_ceil(BLOCK) * BLOCK;
        self.bytes.resize(padded, 0);
    }
}

/// One ustar header block.
fn header(kind: u8, name: &[u8], mode: u32, link: &[u8], size: u64) -> [u8; BLOCK] {
    let mut block = [0_u8; BLOCK];
    let name = &name[..name.len().min(NAME_FIELD)];
    block[..name.len()].copy_from_slice(name);
    octal(&mut block[100..108], u64::from(mode));
    octal(&mut block[108..116], 0);
    octal(&mut block[116..124], 0);
    octal(&mut block[124..136], size);
    octal(&mut block[136..148], 0);
    block[156] = kind;
    let link = &link[..link.len().min(NAME_FIELD)];
    block[157..157 + link.len()].copy_from_slice(link);
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    block[148..156].copy_from_slice(b"        ");
    let sum: u32 = block.iter().map(|byte| u32::from(*byte)).sum();
    block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    block
}

fn octal(field: &mut [u8], value: u64) {
    let digits = field.len() - 1;
    let text = format!("{value:0digits$o}");
    field[..digits].copy_from_slice(&text.as_bytes()[text.len() - digits..]);
    field[digits] = 0;
}

/// The host byte of "version made by" for Unix.
const HOST_UNIX: u8 = 3;
/// The host byte of "version made by" for MS-DOS.
const HOST_DOS: u8 = 0;
/// Stored, no compression.
pub const METHOD_STORED: u16 = 0;
/// Deflate.
pub const METHOD_DEFLATE: u16 = 8;
/// Bzip2, which the extractor refuses.
pub const METHOD_BZIP2: u16 = 12;

/// One zip entry, with every field a test may want to break.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ZipEntry {
    /// The name in the central directory.
    pub name: Vec<u8>,
    /// The name in the local header, when it differs.
    pub local_name: Option<Vec<u8>>,
    /// The uncompressed contents.
    pub data: Vec<u8>,
    /// The compression method; data is deflated only for
    /// [`METHOD_DEFLATE`] and stored as is otherwise.
    pub method: u16,
    /// The host byte of "version made by": 3 for Unix, 0 for MS-DOS.
    pub host: u8,
    /// The `st_mode` in the high half of the external attributes.
    pub mode: u32,
    /// Sets the encryption flag.
    pub encrypted: bool,
    /// Writes sizes and CRC in a data descriptor instead of the local
    /// header.
    pub data_descriptor: bool,
    /// The uncompressed size the headers declare, when it differs.
    pub declared_size: Option<u64>,
}

impl ZipEntry {
    /// A deflated file with mode 0644, made on Unix.
    #[must_use]
    pub fn file(name: &str, data: &[u8]) -> Self {
        Self {
            name: name.as_bytes().to_vec(),
            local_name: None,
            data: data.to_vec(),
            method: METHOD_DEFLATE,
            host: HOST_UNIX,
            mode: 0o100_644,
            encrypted: false,
            data_descriptor: false,
            declared_size: None,
        }
    }

    /// A folder (`name/`), made on Unix.
    #[must_use]
    pub fn dir(name: &str) -> Self {
        Self {
            method: METHOD_STORED,
            mode: 0o040_755,
            ..Self::file(&format!("{name}/"), b"")
        }
    }

    /// A symlink, made on Unix.
    #[must_use]
    pub fn symlink(name: &str, target: &str) -> Self {
        Self {
            method: METHOD_STORED,
            mode: 0o120_777,
            ..Self::file(name, target.as_bytes())
        }
    }

    /// The same entry with this `st_mode`.
    #[must_use]
    pub fn mode(mut self, mode: u32) -> Self {
        self.mode = mode;
        self
    }

    /// The same entry as made on MS-DOS: its mode bits stay in the
    /// external attributes, where only a Unix reader would trust them.
    #[must_use]
    pub fn dos(mut self) -> Self {
        self.host = HOST_DOS;
        self
    }

    /// The same entry with another compression method.
    #[must_use]
    pub fn method(mut self, method: u16) -> Self {
        self.method = method;
        self
    }

    /// The same entry with another name in its local header.
    #[must_use]
    pub fn local_name(mut self, name: &str) -> Self {
        self.local_name = Some(name.as_bytes().to_vec());
        self
    }

    /// The same entry flagged as encrypted.
    #[must_use]
    pub fn encrypted(mut self) -> Self {
        self.encrypted = true;
        self
    }

    /// The same entry with a data descriptor.
    #[must_use]
    pub fn data_descriptor(mut self) -> Self {
        self.data_descriptor = true;
        self
    }

    /// The same entry declaring another uncompressed size.
    #[must_use]
    pub fn declared_size(mut self, size: u64) -> Self {
        self.declared_size = Some(size);
        self
    }
}

/// A zip archive, built entry by entry, with optional bytes before the
/// first entry and after the end record, and an optional central
/// directory record that points at the data of the first entry again.
#[derive(Debug, Clone, Default)]
pub struct ZipBuilder {
    prefix: Vec<u8>,
    entries: Vec<ZipEntry>,
    comment: Vec<u8>,
    trailing: Vec<u8>,
    overlap: Option<Vec<u8>>,
}

impl ZipBuilder {
    /// An empty archive.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry.
    #[must_use]
    pub fn entry(mut self, entry: ZipEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Writes `bytes` before the first entry, with offsets that count
    /// them.
    #[must_use]
    pub fn prefix(mut self, bytes: &[u8]) -> Self {
        self.prefix = bytes.to_vec();
        self
    }

    /// Sets the end record's comment.
    #[must_use]
    pub fn comment(mut self, bytes: &[u8]) -> Self {
        self.comment = bytes.to_vec();
        self
    }

    /// Writes `bytes` after the end record and its comment.
    #[must_use]
    pub fn trailing(mut self, bytes: &[u8]) -> Self {
        self.trailing = bytes.to_vec();
        self
    }

    /// Adds a central directory record named `name` that points at the
    /// local header of the first entry.
    #[must_use]
    pub fn overlap(mut self, name: &str) -> Self {
        self.overlap = Some(name.as_bytes().to_vec());
        self
    }

    /// Writes the archive.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        let mut out = self.prefix.clone();
        let mut central = Vec::new();
        let mut records = 0_u16;
        let mut first = None;
        for entry in &self.entries {
            let offset = out.len() as u64;
            let mut crc = Crc::new();
            crc.update(&entry.data);
            let stored = if entry.method == METHOD_DEFLATE {
                let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
                encoder
                    .write_all(&entry.data)
                    .expect("writing to a Vec never fails");
                encoder.finish().expect("finishing into a Vec never fails")
            } else {
                entry.data.clone()
            };
            let size = entry.declared_size.unwrap_or(entry.data.len() as u64);
            let fields = Fields {
                flags: u16::from(entry.encrypted) | (u16::from(entry.data_descriptor) << 3),
                method: entry.method,
                crc: crc.sum(),
                compressed: u32_of(stored.len() as u64),
                size: u32_of(size),
            };
            let local_name = entry.local_name.as_deref().unwrap_or(&entry.name);
            let local_fields = if entry.data_descriptor {
                Fields {
                    crc: 0,
                    compressed: 0,
                    size: 0,
                    ..fields
                }
            } else {
                fields
            };
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            out.extend_from_slice(&20_u16.to_le_bytes());
            local_fields.write(&mut out);
            out.extend_from_slice(&len16(local_name));
            out.extend_from_slice(&0_u16.to_le_bytes());
            out.extend_from_slice(local_name);
            out.extend_from_slice(&stored);
            if entry.data_descriptor {
                out.extend_from_slice(&[0x50, 0x4b, 0x07, 0x08]);
                out.extend_from_slice(&fields.crc.to_le_bytes());
                out.extend_from_slice(&fields.compressed.to_le_bytes());
                out.extend_from_slice(&fields.size.to_le_bytes());
            }
            let dos_dir = if entry.host == HOST_DOS && entry.name.ends_with(b"/") {
                0x10
            } else {
                0
            };
            let external = (entry.mode << 16) | dos_dir;
            central_record(
                &mut central,
                entry.host,
                fields,
                &entry.name,
                external,
                offset,
            );
            records += 1;
            first.get_or_insert(offset);
        }
        if let (Some(name), Some(offset)) = (&self.overlap, first) {
            let entry = &self.entries[0];
            let fields = Fields {
                flags: 0,
                method: entry.method,
                crc: 0,
                compressed: 0,
                size: 0,
            };
            central_record(
                &mut central,
                HOST_UNIX,
                fields,
                name,
                0o100_644 << 16,
                offset,
            );
            records += 1;
        }
        let central_at = out.len() as u64;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&records.to_le_bytes());
        out.extend_from_slice(&records.to_le_bytes());
        out.extend_from_slice(&u32_of(central.len() as u64).to_le_bytes());
        out.extend_from_slice(&u32_of(central_at).to_le_bytes());
        out.extend_from_slice(&len16(&self.comment));
        out.extend_from_slice(&self.comment);
        out.extend_from_slice(&self.trailing);
        out
    }
}

/// The header fields the local header and the central directory share.
#[derive(Debug, Clone, Copy)]
struct Fields {
    flags: u16,
    method: u16,
    crc: u32,
    compressed: u32,
    size: u32,
}

impl Fields {
    /// Writes flags through the uncompressed size: flags, method, time,
    /// date (1980-01-01), CRC, compressed and uncompressed size.
    fn write(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.method.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0x21_u16.to_le_bytes());
        out.extend_from_slice(&self.crc.to_le_bytes());
        out.extend_from_slice(&self.compressed.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
    }
}

fn central_record(
    out: &mut Vec<u8>,
    host: u8,
    fields: Fields,
    name: &[u8],
    external: u32,
    offset: u64,
) {
    out.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
    out.extend_from_slice(&[20, host]);
    out.extend_from_slice(&20_u16.to_le_bytes());
    fields.write(out);
    out.extend_from_slice(&len16(name));
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&external.to_le_bytes());
    out.extend_from_slice(&u32_of(offset).to_le_bytes());
    out.extend_from_slice(name);
}

fn len16(bytes: &[u8]) -> [u8; 2] {
    u16::try_from(bytes.len())
        .expect("sample names and comments are shorter than 64 KiB")
        .to_le_bytes()
}

fn u32_of(value: u64) -> u32 {
    u32::try_from(value).expect("sample archives stay below 4 GiB")
}

/// One sample artifact and what the extractor must do with it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ArchiveSample {
    /// A short snake_case name, also the fuzz seed's file name.
    pub name: &'static str,
    /// The artifact format.
    pub format: ArtifactFormat,
    /// The artifact bytes.
    pub bytes: Vec<u8>,
    /// What to extract.
    pub spec: ExtractSpec,
    /// `None` when the extraction must succeed, else the reason it must
    /// stop with.
    pub expected: Option<RejectReason>,
}

/// The limits of every sample but the limit cases: 1 MiB and 1000 entries.
#[must_use]
pub fn sample_limits() -> ExtractLimits {
    ExtractLimits::new(1 << 20, 1000)
}

fn entry_paths(paths: &[&str]) -> Vec<EntryPath> {
    paths
        .iter()
        .map(|path| EntryPath::new(path).expect("sample executables are valid entry paths"))
        .collect()
}

fn sample(
    name: &'static str,
    format: ArtifactFormat,
    bytes: Vec<u8>,
    root: Option<&str>,
    executables: &[&str],
    expected: Option<RejectReason>,
) -> ArchiveSample {
    ArchiveSample {
        name,
        format,
        bytes,
        spec: ExtractSpec::new(
            root.map(str::to_owned),
            entry_paths(executables),
            sample_limits(),
        ),
        expected,
    }
}

fn tar_case(name: &'static str, tar: TarBuilder, expected: RejectReason) -> ArchiveSample {
    sample(
        name,
        ArtifactFormat::TarGz,
        tar.finish_gz(),
        None,
        &[],
        Some(expected),
    )
}

fn zip_case(name: &'static str, zip: ZipBuilder, expected: RejectReason) -> ArchiveSample {
    sample(
        name,
        ArtifactFormat::Zip,
        zip.finish(),
        None,
        &[],
        Some(expected),
    )
}

/// A `tar_gz` artifact laid out like the MySQL one: `archiveRoot` `pkg`,
/// executables, a library with versioned symlinks, a symlink into
/// another folder, a setuid file, AppleDouble members, a PAX path, and a
/// GNU long name.
#[must_use]
pub fn tar_gz_layout() -> Vec<u8> {
    let long_name = format!("pkg/share/{}", "n".repeat(120));
    TarBuilder::new()
        .dir("pkg/", 0o755)
        .dir("pkg/bin/", 0o755)
        .file("pkg/bin/tool", 0o755, b"#!/bin/sh\necho tool 1.0\n")
        .file("pkg/lib/libz.1.dylib", 0o644, b"library bytes")
        .symlink("pkg/lib/libz.dylib", "libz.1.dylib")
        .symlink("pkg/bin/libz.1.dylib", "../lib/libz.1.dylib")
        .file("pkg/bin/helper", 0o4755, b"setuid helper")
        .file("pkg/bin/._tool", 0o644, b"apple double")
        .file("__MACOSX/pkg/bin/._tool", 0o644, b"apple double")
        .pax(&[
            ("path", b"pkg/share/from-pax.txt"),
            ("SCHILY.xattr.com.apple.quarantine", b"0081;x"),
        ])
        .file("pkg/share/ignored-name", 0o644, b"pax path wins")
        .file(&long_name, 0o644, b"gnu long name")
        .finish_gz()
}

/// A `zip` artifact: a Unix folder and executable listed without execute
/// bits, a deflated file, a stored file with a data descriptor, a file
/// made on MS-DOS that claims `S_IFLNK`, a Unix symlink, and AppleDouble
/// members.
#[must_use]
pub fn zip_layout() -> Vec<u8> {
    ZipBuilder::new()
        .entry(ZipEntry::dir("bin"))
        .entry(ZipEntry::file("bin/server", b"\xcf\xfa\xed\xfe server").mode(0o100_644))
        .entry(ZipEntry::file("lib/data.txt", &b"deflated ".repeat(64)))
        .entry(
            ZipEntry::file("lib/descriptor.txt", b"descriptor")
                .method(METHOD_STORED)
                .data_descriptor(),
        )
        .entry(
            ZipEntry::file("lib/dos-link", b"not a link")
                .mode(0o120_777)
                .dos(),
        )
        .entry(ZipEntry::symlink("lib/current", "data.txt"))
        .entry(ZipEntry::file("__MACOSX/bin/._server", b"apple double"))
        .entry(ZipEntry::file("bin/._server", b"apple double"))
        .comment(b"an end record comment is fine")
        .finish()
}

/// Every sample artifact: three that extract and one or more for each
/// [`RejectReason`].
#[must_use]
pub fn archive_samples() -> Vec<ArchiveSample> {
    use ArtifactFormat::{Binary, TarGz, Zip};
    use RejectReason as R;

    let file = |name: &str| TarBuilder::new().file(name, 0o644, b"x");
    let mut samples = vec![
        sample(
            "tar_gz_layout",
            TarGz,
            tar_gz_layout(),
            Some("pkg"),
            &["bin/tool"],
            None,
        ),
        sample("zip_layout", Zip, zip_layout(), None, &["bin/server"], None),
        sample(
            "binary_layout",
            Binary,
            b"\xcf\xfa\xed\xfe binary".to_vec(),
            None,
            &["server"],
            None,
        ),
        tar_case("tar_absolute_path", file("/etc/passwd"), R::AbsolutePath),
        tar_case(
            "tar_path_traversal",
            file("pkg/../../escaped"),
            R::PathTraversal,
        ),
        tar_case("tar_empty_component", file("pkg//x"), R::NameInvalid),
        tar_case("tar_backslash", file("pkg\\x"), R::NameInvalid),
        tar_case(
            "tar_not_utf8",
            TarBuilder::new().member(b'0', b"pkg/\xff", 0o644, b"", b"x"),
            R::NameInvalid,
        ),
        tar_case(
            "tar_nul_in_pax_path",
            TarBuilder::new()
                .pax(&[("path", b"pkg/a\0b")])
                .file("x", 0o644, b"x"),
            R::NameInvalid,
        ),
        tar_case("tar_name_too_long", file(&"n".repeat(300)), R::NameTooLong),
        tar_case(
            "tar_too_deep",
            file(&vec!["d"; 33].join("/")),
            R::NameTooLong,
        ),
        sample(
            "tar_unexpected_top_level",
            TarGz,
            TarBuilder::new()
                .file("pkg/bin/tool", 0o755, b"x")
                .file("other/x", 0o644, b"x")
                .finish_gz(),
            Some("pkg"),
            &[],
            Some(R::UnexpectedTopLevel),
        ),
        tar_case(
            "tar_reserved_name",
            file(".stackly-runtime.json"),
            R::ReservedName,
        ),
        tar_case(
            "tar_folder_with_data",
            TarBuilder::new().header_with_size(b'5', b"pkg/", 0o755, 4, b"data"),
            R::SizeMismatch,
        ),
        tar_case(
            "tar_hard_link",
            file("pkg/a").member(b'1', b"pkg/b", 0o644, b"pkg/a", b""),
            R::HardLink,
        ),
        tar_case(
            "tar_char_device",
            TarBuilder::new().member(b'3', b"pkg/tty", 0o644, b"", b""),
            R::SpecialFile,
        ),
        tar_case(
            "tar_fifo",
            TarBuilder::new().member(b'6', b"pkg/fifo", 0o644, b"", b""),
            R::SpecialFile,
        ),
        tar_case(
            "tar_gnu_sparse",
            TarBuilder::new().member(b'S', b"pkg/sparse", 0o644, b"", b"x"),
            R::UnsupportedEntry,
        ),
        tar_case(
            "tar_pax_sparse",
            TarBuilder::new()
                .pax(&[("GNU.sparse.major", b"1")])
                .file("pkg/sparse", 0o644, b"x"),
            R::UnsupportedEntry,
        ),
        tar_case(
            "tar_symlink_absolute",
            TarBuilder::new().symlink("pkg/link", "/etc/passwd"),
            R::SymlinkEscape,
        ),
        tar_case(
            "tar_symlink_escape",
            TarBuilder::new()
                .dir("pkg/", 0o755)
                .symlink("pkg/link", "../../outside"),
            R::SymlinkEscape,
        ),
        tar_case(
            "tar_symlink_dangling",
            TarBuilder::new().symlink("link", "missing"),
            R::SymlinkDangling,
        ),
        tar_case(
            "tar_symlink_cycle",
            TarBuilder::new().symlink("a", "b").symlink("b", "a"),
            R::SymlinkChain,
        ),
        tar_case(
            "tar_entry_through_symlink",
            TarBuilder::new()
                .dir("real/", 0o755)
                .symlink("lib", "real")
                .file("lib/escaped", 0o644, b"x"),
            R::EntryThroughSymlink,
        ),
        tar_case(
            "tar_entry_through_planted_symlink",
            TarBuilder::new()
                .symlink("escape", "../sentinel")
                .file("escape/planted", 0o644, b"x"),
            R::EntryThroughSymlink,
        ),
        sample(
            "tar_executable_symlink",
            TarGz,
            TarBuilder::new()
                .file("bin/real", 0o755, b"x")
                .symlink("bin/tool", "real")
                .finish_gz(),
            None,
            &["bin/tool"],
            Some(R::ExecutableNotRegular),
        ),
        tar_case(
            "tar_duplicate_file",
            file("pkg/a").file("pkg/a", 0o644, b"y"),
            R::DuplicateEntry,
        ),
        tar_case(
            "tar_file_then_folder",
            file("pkg/a").file("pkg/a/b", 0o644, b"y"),
            R::DuplicateEntry,
        ),
        sample(
            "tar_trailing_bytes",
            TarGz,
            [file("a").finish_gz(), b"trailing".to_vec()].concat(),
            None,
            &[],
            Some(R::TrailingData),
        ),
        sample(
            "tar_second_gzip_member",
            TarGz,
            [file("a").finish_gz(), file("b").finish_gz()].concat(),
            None,
            &[],
            Some(R::TrailingData),
        ),
        sample(
            "tar_data_after_end_marker",
            TarGz,
            gzip(&[file("a").finish(), b"hidden".to_vec()].concat()),
            None,
            &[],
            Some(R::TrailingData),
        ),
        tar_case(
            "tar_pax_header_too_large",
            TarBuilder::new()
                .pax(&[("comment", &vec![b'c'; 70 << 10])])
                .file("a", 0o644, b"x"),
            R::HeaderTooLarge,
        ),
        sample(
            "tar_not_gzip",
            TarGz,
            file("a").finish(),
            None,
            &[],
            Some(R::Corrupt),
        ),
        sample(
            "tar_truncated_gzip",
            TarGz,
            {
                let bytes = file("a").finish_gz();
                bytes[..bytes.len() - 4].to_vec()
            },
            None,
            &[],
            Some(R::Corrupt),
        ),
        sample(
            "tar_bad_checksum",
            TarGz,
            {
                let mut tar = file("a").finish();
                tar[0] = b'b';
                gzip(&tar)
            },
            None,
            &[],
            Some(R::Corrupt),
        ),
        zip_case(
            "zip_prefix_data",
            ZipBuilder::new()
                .prefix(b"#!/bin/sh\nexit 0\n")
                .entry(ZipEntry::file("a", b"x")),
            R::PrefixData,
        ),
        zip_case(
            "zip_trailing_bytes",
            ZipBuilder::new()
                .entry(ZipEntry::file("a", b"x"))
                .comment(b"ok")
                .trailing(b"hidden"),
            R::TrailingData,
        ),
        zip_case(
            "zip_encrypted",
            ZipBuilder::new().entry(ZipEntry::file("a", b"x").method(METHOD_STORED).encrypted()),
            R::Encrypted,
        ),
        zip_case(
            "zip_bzip2",
            ZipBuilder::new().entry(ZipEntry::file("a", b"x").method(METHOD_BZIP2)),
            R::UnsupportedCompression,
        ),
        zip_case(
            "zip_overlapping_entries",
            ZipBuilder::new()
                .entry(ZipEntry::file("a", b"x"))
                .overlap("b"),
            R::OverlappingEntries,
        ),
        zip_case(
            "zip_name_mismatch",
            ZipBuilder::new().entry(ZipEntry::file("bin/tool", b"x").local_name("bin/other")),
            R::NameMismatch,
        ),
        zip_case(
            "zip_duplicate_name",
            ZipBuilder::new()
                .entry(ZipEntry::file("a", b"x"))
                .entry(ZipEntry::file("a", b"y")),
            R::DuplicateEntry,
        ),
        zip_case(
            "zip_size_mismatch",
            ZipBuilder::new().entry(ZipEntry::file("a", b"twelve bytes").declared_size(4)),
            R::SizeMismatch,
        ),
        zip_case(
            "zip_fifo",
            ZipBuilder::new().entry(ZipEntry::file("fifo", b"").mode(0o010_644)),
            R::SpecialFile,
        ),
        zip_case(
            "zip_path_traversal",
            ZipBuilder::new().entry(ZipEntry::file("../escaped", b"x")),
            R::PathTraversal,
        ),
        zip_case(
            "zip_absolute_path",
            ZipBuilder::new().entry(ZipEntry::file("/escaped/file", b"x")),
            R::AbsolutePath,
        ),
        zip_case(
            "zip_symlink_escape",
            ZipBuilder::new().entry(ZipEntry::symlink("link", "../outside")),
            R::SymlinkEscape,
        ),
        zip_case(
            "zip_entry_through_symlink",
            ZipBuilder::new()
                .entry(ZipEntry::dir("real"))
                .entry(ZipEntry::symlink("lib", "real"))
                .entry(ZipEntry::file("lib/x", b"x")),
            R::EntryThroughSymlink,
        ),
        zip_case(
            "zip_garbage",
            ZipBuilder::new().prefix(b"not a zip at all"),
            R::PrefixData,
        ),
        sample(
            "zip_random_bytes",
            Zip,
            b"PK\x03\x04 not really".to_vec(),
            None,
            &[],
            Some(R::Corrupt),
        ),
    ];
    let mut limited = |name, format, bytes: Vec<u8>, limits, expected| {
        samples.push(ArchiveSample {
            name,
            format,
            bytes,
            spec: ExtractSpec::new(None, Vec::new(), limits),
            expected: Some(expected),
        });
    };
    limited(
        "tar_too_many_entries",
        TarGz,
        file("a")
            .file("b", 0o644, b"x")
            .file("c", 0o644, b"x")
            .finish_gz(),
        ExtractLimits::new(1 << 20, 2),
        R::TooManyEntries,
    );
    limited(
        "tar_too_large",
        TarGz,
        TarBuilder::new().file("a", 0o644, &[0; 11]).finish_gz(),
        ExtractLimits::new(10, 100),
        R::TooLarge,
    );
    limited(
        "zip_too_many_entries",
        Zip,
        ZipBuilder::new()
            .entry(ZipEntry::file("a", b"x"))
            .entry(ZipEntry::file("b", b"x"))
            .entry(ZipEntry::file("c", b"x"))
            .finish(),
        ExtractLimits::new(1 << 20, 2),
        R::TooManyEntries,
    );
    limited(
        "zip_deflate_bomb",
        Zip,
        ZipBuilder::new()
            .entry(ZipEntry::file("bomb", &vec![0; 4 << 20]))
            .finish(),
        ExtractLimits::new(1 << 20, 100),
        R::TooLarge,
    );
    samples
}

const MH_MAGIC_64: u32 = 0xfeed_facf;
const CPU_TYPE_ARM64: u32 = 0x0100_000c;
const CPU_TYPE_X86_64: u32 = 0x0100_0007;
const CPU_SUBTYPE_ARM64E: u32 = 2;
const MH_EXECUTE: u32 = 2;
const MH_DYLIB: u32 = 6;
const FAT_MAGIC: u32 = 0xcafe_babe;
/// The alignment of each slice in a universal file: 16 KiB.
const SLICE_ALIGN: usize = 1 << 14;

/// A thin 64 bit little endian Mach-O file, built from its load
/// commands; it has no segments, which is all the parser reads.
#[derive(Debug, Clone)]
pub struct MachOBuilder {
    cputype: u32,
    cpusubtype: u32,
    file_type: u32,
    commands: Vec<Vec<u8>>,
}

impl MachOBuilder {
    /// An arm64 main executable.
    #[must_use]
    pub fn executable() -> Self {
        Self {
            cputype: CPU_TYPE_ARM64,
            cpusubtype: 0,
            file_type: MH_EXECUTE,
            commands: Vec::new(),
        }
    }

    /// An arm64 library with this `LC_ID_DYLIB`.
    #[must_use]
    pub fn dylib(install_name: &str) -> Self {
        Self {
            file_type: MH_DYLIB,
            ..Self::executable()
        }
        .dylib_command(0xd, install_name)
    }

    /// The same file for Intel.
    #[must_use]
    pub fn x86_64(mut self) -> Self {
        self.cputype = CPU_TYPE_X86_64;
        self.cpusubtype = 3;
        self
    }

    /// The same file for arm64e.
    #[must_use]
    pub fn arm64e(mut self) -> Self {
        self.cpusubtype = CPU_SUBTYPE_ARM64E;
        self
    }

    /// Adds `LC_BUILD_VERSION` for macOS `major.minor`.
    #[must_use]
    pub fn min_os(self, major: u32, minor: u32) -> Self {
        self.build_version(1, major, minor)
    }

    /// Adds `LC_BUILD_VERSION` for another platform (2 is iOS).
    #[must_use]
    pub fn build_version(mut self, platform: u32, major: u32, minor: u32) -> Self {
        let mut command = Vec::new();
        for word in [0x32, 24, platform, (major << 16) | (minor << 8), 0, 0] {
            command.extend_from_slice(&u32::to_le_bytes(word));
        }
        self.commands.push(command);
        self
    }

    /// Adds `LC_VERSION_MIN_MACOSX` for `major.minor`.
    #[must_use]
    pub fn version_min(mut self, major: u32, minor: u32) -> Self {
        let mut command = Vec::new();
        for word in [0x24, 16, (major << 16) | (minor << 8), 0] {
            command.extend_from_slice(&u32::to_le_bytes(word));
        }
        self.commands.push(command);
        self
    }

    /// Adds `LC_LOAD_DYLIB`.
    #[must_use]
    pub fn load(self, name: &str) -> Self {
        self.dylib_command(0xc, name)
    }

    /// Adds `LC_LOAD_WEAK_DYLIB`.
    #[must_use]
    pub fn weak(self, name: &str) -> Self {
        self.dylib_command(0x8000_0018, name)
    }

    /// Adds `LC_REEXPORT_DYLIB`.
    #[must_use]
    pub fn reexport(self, name: &str) -> Self {
        self.dylib_command(0x8000_001f, name)
    }

    /// Adds `LC_LAZY_LOAD_DYLIB`.
    #[must_use]
    pub fn lazy(self, name: &str) -> Self {
        self.dylib_command(0x20, name)
    }

    /// Adds `LC_LOAD_UPWARD_DYLIB`.
    #[must_use]
    pub fn upward(self, name: &str) -> Self {
        self.dylib_command(0x8000_0023, name)
    }

    /// Adds `LC_RPATH`.
    #[must_use]
    pub fn rpath(mut self, path: &str) -> Self {
        let mut command = Vec::new();
        let size = padded(12 + path.len() + 1);
        for word in [0x8000_001c, u32_of(size as u64), 12] {
            command.extend_from_slice(&u32::to_le_bytes(word));
        }
        command.extend_from_slice(path.as_bytes());
        command.resize(size, 0);
        self.commands.push(command);
        self
    }

    /// Adds a raw load command.
    #[must_use]
    pub fn command(mut self, bytes: Vec<u8>) -> Self {
        self.commands.push(bytes);
        self
    }

    /// Writes the file.
    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        let size: usize = self.commands.iter().map(Vec::len).sum();
        let mut out = Vec::new();
        for word in [
            MH_MAGIC_64,
            self.cputype,
            self.cpusubtype,
            self.file_type,
            u32_of(self.commands.len() as u64),
            u32_of(size as u64),
            0,
            0,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for command in &self.commands {
            out.extend_from_slice(command);
        }
        out
    }

    fn dylib_command(mut self, cmd: u32, name: &str) -> Self {
        let mut command = Vec::new();
        let size = padded(24 + name.len() + 1);
        for word in [cmd, u32_of(size as u64), 24, 2, 0x1_0000, 0x1_0000] {
            command.extend_from_slice(&u32::to_le_bytes(word));
        }
        command.extend_from_slice(name.as_bytes());
        command.resize(size, 0);
        self.commands.push(command);
        self
    }
}

/// Wraps thin Mach-O files into a universal file, each slice aligned to
/// 16 KiB, with the CPU each slice's own header names.
#[must_use]
pub fn universal(slices: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&FAT_MAGIC.to_be_bytes());
    out.extend_from_slice(&u32_of(slices.len() as u64).to_be_bytes());
    let mut offset = SLICE_ALIGN;
    let mut layout = Vec::new();
    for slice in slices {
        let cputype = u32::from_le_bytes(slice[4..8].try_into().expect("a slice has a header"));
        let cpusubtype = u32::from_le_bytes(slice[8..12].try_into().expect("a slice has a header"));
        for word in [
            cputype,
            cpusubtype,
            u32_of(offset as u64),
            u32_of(slice.len() as u64),
            14,
        ] {
            out.extend_from_slice(&word.to_be_bytes());
        }
        layout.push(offset);
        offset += slice.len().div_ceil(SLICE_ALIGN) * SLICE_ALIGN;
    }
    for (slice, at) in slices.iter().zip(layout) {
        out.resize(at, 0);
        out.extend_from_slice(slice);
    }
    out
}

/// Rounds a load command size up to 8 bytes.
fn padded(len: usize) -> usize {
    len.div_ceil(8) * 8
}
