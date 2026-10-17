//! A small writer of stored (uncompressed) zip archives for tests, so the
//! zip format of spec 0003 (AC-16) is tested without a zip crate: local
//! headers, a central directory with Unix modes, and the end record.

/// One member of a test archive.
pub struct Member {
    name: String,
    /// The full Unix mode, file type bits included.
    mode: u32,
    data: Vec<u8>,
}

/// A regular file with permission bits `mode`.
pub fn file(name: &str, mode: u32, data: &[u8]) -> Member {
    Member {
        name: name.to_owned(),
        mode: 0o100_000 | mode,
        data: data.to_vec(),
    }
}

/// A folder; `name` gets its trailing `/`.
pub fn dir(name: &str) -> Member {
    Member {
        name: format!("{}/", name.trim_end_matches('/')),
        mode: 0o040_755,
        data: Vec::new(),
    }
}

/// A symbolic link to `target`, stored the Unix way: the target is the
/// member's content.
pub fn symlink(name: &str, target: &str) -> Member {
    Member {
        name: name.to_owned(),
        mode: 0o120_777,
        data: target.as_bytes().to_vec(),
    }
}

/// The bytes of a zip archive holding `members` in order, uncompressed.
pub fn stored(members: &[Member]) -> Vec<u8> {
    // 2026-10-17 00:00 in MS-DOS date and time fields.
    const DATE: u16 = (46 << 9) | (10 << 5) | 17;
    const TIME: u16 = 0;
    // Version 1.0 suffices to extract a stored member; the high byte of
    // "version made by" says Unix, so readers take the mode from the
    // external attributes. Flag bit 11 marks UTF-8 names.
    const NEEDED: u16 = 10;
    const MADE_BY: u16 = (3 << 8) | 30;
    const UTF8: u16 = 1 << 11;

    let mut out = Vec::new();
    let mut central = Vec::new();
    for member in members {
        let offset = u32::try_from(out.len()).expect("a test archive stays below 4 GiB");
        let size = u32::try_from(member.data.len()).expect("a test member stays below 4 GiB");
        let name_len = u16::try_from(member.name.len()).expect("a test member name is short");
        let crc = crc32(&member.data);

        put32(&mut out, 0x0403_4b50);
        for field in [NEEDED, UTF8, 0, TIME, DATE] {
            put16(&mut out, field);
        }
        for field in [crc, size, size] {
            put32(&mut out, field);
        }
        put16(&mut out, name_len);
        put16(&mut out, 0);
        out.extend_from_slice(member.name.as_bytes());
        out.extend_from_slice(&member.data);

        put32(&mut central, 0x0201_4b50);
        for field in [MADE_BY, NEEDED, UTF8, 0, TIME, DATE] {
            put16(&mut central, field);
        }
        for field in [crc, size, size] {
            put32(&mut central, field);
        }
        // Name length, extra length, comment length, disk, internal
        // attributes.
        for field in [name_len, 0, 0, 0, 0] {
            put16(&mut central, field);
        }
        put32(&mut central, member.mode << 16);
        put32(&mut central, offset);
        central.extend_from_slice(member.name.as_bytes());
    }
    let central_offset = u32::try_from(out.len()).expect("a test archive stays below 4 GiB");
    let central_size = u32::try_from(central.len()).expect("a test archive stays below 4 GiB");
    let count = u16::try_from(members.len()).expect("a test archive has few members");
    out.extend_from_slice(&central);
    put32(&mut out, 0x0605_4b50);
    for field in [0, 0, count, count] {
        put16(&mut out, field);
    }
    put32(&mut out, central_size);
    put32(&mut out, central_offset);
    put16(&mut out, 0);
    out
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// The CRC-32 zip uses (reflected, polynomial 0xedb88320).
fn crc32(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[test]
fn crc32_matches_the_check_value() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
}
