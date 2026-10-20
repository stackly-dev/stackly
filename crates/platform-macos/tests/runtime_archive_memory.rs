//! The peak heap use of the runtime extractor, measured with a counting
//! allocator while it writes archive bombs through `MacosTreeWriter`
//! (AC-060, NFR-004). The file holds one test, so no other test thread
//! allocates while it measures. Every path lives in a `TestRoot`.
#![cfg(target_os = "macos")]
// The counting allocator needs `unsafe impl GlobalAlloc`; this crate is the
// one that may opt out of the workspace lint (spec 0001).
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use stackly_core::runtime::archive::{
    ArchiveError, ExtractLimits, ExtractSpec, RejectReason, extract,
};
use stackly_core::runtime::samples::{MemoryFile, TarBuilder, ZipBuilder, ZipEntry};
use stackly_domain::ArtifactFormat;
use stackly_platform::{Filesystem, StopFlag};
use stackly_platform_macos::MacosFilesystem;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

/// Counts the bytes allocated now and the highest count since the last
/// reset.
struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System` with the same arguments, so
// the allocator contract holds as it does for `System`; the counters
// only observe the sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract, which
        // `System::alloc` shares.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `alloc` above with this `layout`.
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::SeqCst);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The peak bound of spec 0018: `max_entries × 2 KiB + 1 MiB`.
fn bound(limits: &ExtractLimits) -> usize {
    usize::try_from(limits.max_entries).expect("test limits fit in usize") * 2048 + (1 << 20)
}

/// Extracts `bytes` into a fresh stage and returns the result with the
/// peak heap growth during the call.
fn measure(
    format: ArtifactFormat,
    bytes: Vec<u8>,
    limits: ExtractLimits,
) -> (Result<u64, ArchiveError>, usize) {
    let root = TestRoot::new().expect("a test root can be created");
    let stage = root.path().join("stage");
    MacosFilesystem
        .create_dir_private(&stage)
        .expect("the stage folder can be created in a fresh test root");
    let mut writer = MacosFilesystem
        .open_tree_writer(&stage)
        .expect("a fresh stage is an empty private folder");
    let mut input = MemoryFile::new(bytes);
    let spec = ExtractSpec::new(None, Vec::new(), limits);
    let stop = StopFlag::new();
    let base = CURRENT.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let result =
        extract(format, &mut input, &spec, writer.as_mut(), &stop).map(|report| report.file_bytes);
    let peak = PEAK.load(Ordering::SeqCst) - base;
    (result, peak)
}

fn reason(result: &Result<u64, ArchiveError>) -> Option<RejectReason> {
    match result {
        Err(ArchiveError::Rejected { reason, .. }) => Some(*reason),
        _ => None,
    }
}

#[test]
fn archive_bombs_stop_at_their_limits_within_the_memory_bound() {
    let ev = evidence!(
        covers = ["AC-060", "NFR-004"],
        kind = Kind::Fixture,
        expected = "a 64 MiB file in a tar.gz, a zip entry inflating 64 MiB past a 1 MiB limit, a tar.gz holding 64 MiB of zeros under a 1 GiB limit, and a tar.gz of 5000 files (at and one past the entry limit) extract or stop with too_large or too_many_entries while the extractor's peak heap growth stays below max_entries x 2 KiB + 1 MiB (AC-060, NFR-004)",
    );
    let zeros = vec![0_u8; 64 << 20];
    let mut lines = Vec::new();
    let mut check = |label: &str,
                     format,
                     bytes: Vec<u8>,
                     limits: ExtractLimits,
                     expected: Option<RejectReason>| {
        let (result, peak) = measure(format, bytes, limits);
        assert_eq!(reason(&result), expected, "{label}: {result:?}");
        assert!(
            peak < bound(&limits),
            "{label}: peak {peak} bytes, bound {}",
            bound(&limits)
        );
        lines.push(format!("{label}: peak {} KiB", peak / 1024));
    };

    check(
        "tar.gz file past the byte limit",
        ArtifactFormat::TarGz,
        TarBuilder::new().file("bomb", 0o644, &zeros).finish_gz(),
        ExtractLimits::new(1 << 20, 100),
        Some(RejectReason::TooLarge),
    );
    check(
        "zip entry inflating past the byte limit",
        ArtifactFormat::Zip,
        ZipBuilder::new()
            .entry(ZipEntry::file("bomb", &zeros).declared_size(1 << 10))
            .finish(),
        ExtractLimits::new(1 << 20, 100),
        Some(RejectReason::SizeMismatch),
    );
    check(
        "tar.gz of 64 MiB within the byte limit",
        ArtifactFormat::TarGz,
        TarBuilder::new().file("big", 0o644, &zeros).finish_gz(),
        ExtractLimits::new(1 << 30, 100),
        None,
    );
    let many = (0..5000).fold(TarBuilder::new(), |tar, index| {
        tar.file(&format!("d{}/f{index}", index % 50), 0o644, b"x")
    });
    let many = many.finish_gz();
    check(
        "tar.gz of 5000 files at the entry limit",
        ArtifactFormat::TarGz,
        many.clone(),
        ExtractLimits::new(1 << 20, 5050),
        None,
    );
    check(
        "tar.gz of 5000 files past the entry limit",
        ArtifactFormat::TarGz,
        many,
        ExtractLimits::new(1 << 20, 5049),
        Some(RejectReason::TooManyEntries),
    );
    ev.pass_with(&lines.join("; "));
}
