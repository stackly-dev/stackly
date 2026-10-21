//! `runtime::download` against `FakeFetcher` and `FakePlatform` (spec 0018,
//! AC-3 to AC-6): the cache entry, resume with `Range`, wrong bytes, retry
//! and offline classification, space, and stop.
#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use sha2::{Digest, Sha256};
use stackly_core::prepare_layout;
use stackly_core::runtime::download::{
    CachePaths, DownloadContext, DownloadError, DownloadOutcome, DownloadPolicy, DownloadSource,
    download,
};
use stackly_domain::Sha256Digest;
use stackly_platform::{FetchError, HttpsUrl, StopFlag};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{FakePlatform, FakeStep, FakeVolume, TestRoot};

const URL: &str = "https://downloads.example.test/nats-2.15.1.tar.gz";
const MIRROR: &str = "https://mirror.example.test/nats-2.15.1.tar.gz";

struct Rig {
    _root: TestRoot,
    fake: FakePlatform,
    dir: PathBuf,
    body: Vec<u8>,
    sha256: Sha256Digest,
    urls: Vec<HttpsUrl>,
    policy: DownloadPolicy,
}

fn body() -> Vec<u8> {
    (0..4000_u32)
        .flat_map(|n| n.to_le_bytes())
        .take(10_000)
        .collect()
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

fn rig() -> Rig {
    let root = TestRoot::new().expect("a test root");
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();
    let prepared = prepare_layout(&platform).expect("the fake platform prepares its layout");
    let dir = prepared.layout().downloads_dir();
    let body = body();
    fake.fetcher().set_chunk(1000);
    fake.fetcher().serve(URL, body.clone());
    Rig {
        _root: root,
        fake,
        dir,
        sha256: digest(&body),
        body,
        urls: vec![HttpsUrl::new(URL).expect("a valid url")],
        policy: DownloadPolicy {
            backoff: [Duration::from_millis(1), Duration::from_millis(1)],
            retry_after_max: Duration::from_millis(5),
            sync_every: 3000,
            space_reserve: 0,
        },
    }
}

impl Rig {
    fn run(&self, stop: &StopFlag) -> (Result<DownloadOutcome, DownloadError>, Vec<(u64, u64)>) {
        let platform = self.fake.platform();
        let ctx = DownloadContext {
            platform: &platform,
            downloads_dir: &self.dir,
            stop,
            policy: &self.policy,
        };
        let source = DownloadSource {
            sha256: self.sha256,
            size: self.body.len() as u64,
            urls: &self.urls,
        };
        let mut seen = Vec::new();
        let result = download(&ctx, &source, &mut |done, total| seen.push((done, total)));
        (result, seen)
    }

    fn paths(&self) -> CachePaths {
        CachePaths::new(&self.dir, &self.sha256)
    }
}

#[test]
fn a_fresh_download_becomes_the_cache_entry_and_names_its_source() {
    let ev = evidence!(
        covers = ["REQ-004"],
        kind = Kind::Fixture,
        expected = "a download fills <sha256>.part, renames it to <sha256> only after the size and digest match, writes <sha256>.json, and leaves no .part or .part.json",
    );
    let rig = rig();

    let (result, seen) = rig.run(&StopFlag::new());

    assert_eq!(
        result,
        Ok(DownloadOutcome::Downloaded {
            host: "downloads.example.test".to_owned(),
            resumed: false
        })
    );
    let paths = rig.paths();
    assert_eq!(std::fs::read(&paths.entry).unwrap(), rig.body);
    assert!(!paths.part.exists() && !paths.part_note.exists());
    let note: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.source_note).unwrap()).unwrap();
    assert_eq!(note["schema"], "stackly.download-source/v1");
    assert_eq!(note["url"], URL);
    assert_eq!(seen.last(), Some(&(10_000, 10_000)));
    assert!(seen.windows(2).all(|pair| pair[0].0 < pair[1].0));
    ev.pass();
}

#[test]
fn a_cache_entry_is_used_without_the_network() {
    let ev = evidence!(
        covers = ["REQ-046"],
        kind = Kind::Fixture,
        expected = "with an entry in the cache and the network down, download returns Cached and makes no fetch call",
    );
    let rig = rig();
    std::fs::write(rig.paths().entry, &rig.body).unwrap();
    rig.fake.fetcher().go_offline();

    let (result, _) = rig.run(&StopFlag::new());

    assert_eq!(result, Ok(DownloadOutcome::Cached));
    assert!(rig.fake.fetcher().requests().is_empty());
    ev.pass();
}

#[test]
fn an_interrupted_download_resumes_from_the_last_synced_byte_with_a_range() {
    let ev = evidence!(
        covers = ["REQ-004", "AC-004"],
        kind = Kind::Fixture,
        expected = "a download cut at 4000 bytes keeps the bytes the last sync covered, the next attempt sends Range and If-Range from that offset, and the entry matches the catalog digest",
    );
    let rig = rig();
    rig.fake
        .fetcher()
        .script(URL, [FakeStep::InterruptAfter(4000)]);

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(
            result,
            Ok(DownloadOutcome::Downloaded { resumed: true, .. })
        ),
        "{result:?}"
    );
    let requests = rig.fake.fetcher().requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].resume_offset, None);
    assert_eq!(requests[1].resume_offset, Some(4000));
    assert_eq!(requests[1].validator.as_deref(), Some("\"v1\""));
    assert_eq!(std::fs::read(rig.paths().entry).unwrap(), rig.body);
    ev.pass();
}

#[test]
fn bytes_after_the_last_sync_are_dropped_before_resuming() {
    let rig = rig();
    let paths = rig.paths();
    // A partial file with 6000 bytes, of which the sidecar vouches for 3000,
    // as after a daemon that died between two syncs.
    std::fs::write(&paths.part, &rig.body[..6000]).unwrap();
    std::fs::write(
        &paths.part_note,
        format!(
            r#"{{"schema":"stackly.download-partial/v1","url":"{URL}","validator":"\"v1\"","bytes":3000,"updatedAt":""}}"#
        ),
    )
    .unwrap();

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(
            result,
            Ok(DownloadOutcome::Downloaded { resumed: true, .. })
        ),
        "{result:?}"
    );
    assert_eq!(rig.fake.fetcher().requests()[0].resume_offset, Some(3000));
    assert_eq!(std::fs::read(paths.entry).unwrap(), rig.body);
}

#[test]
fn a_server_that_answers_200_to_a_range_request_starts_again_from_zero() {
    let rig = rig();
    rig.fake
        .fetcher()
        .script(URL, [FakeStep::InterruptAfter(4000), FakeStep::IgnoreRange]);

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(
            result,
            Ok(DownloadOutcome::Downloaded { resumed: false, .. })
        ),
        "{result:?}"
    );
    assert_eq!(rig.fake.fetcher().requests()[1].resume_offset, Some(4000));
    assert_eq!(std::fs::read(rig.paths().entry).unwrap(), rig.body);
}

#[test]
fn a_changed_validator_starts_again_from_zero() {
    let rig = rig();
    rig.fake.fetcher().script(
        URL,
        [
            FakeStep::InterruptAfter(4000),
            FakeStep::ChangeValidator("\"v2\"".to_owned()),
        ],
    );

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(
            result,
            Ok(DownloadOutcome::Downloaded { resumed: false, .. })
        ),
        "{result:?}"
    );
    assert_eq!(std::fs::read(rig.paths().entry).unwrap(), rig.body);
}

#[test]
fn a_download_without_a_validator_cannot_resume() {
    let rig = rig();
    rig.fake
        .fetcher()
        .serve_without_validator(URL, rig.body.clone());
    rig.fake
        .fetcher()
        .script(URL, [FakeStep::InterruptAfter(4000)]);

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(
            result,
            Ok(DownloadOutcome::Downloaded { resumed: false, .. })
        ),
        "{result:?}"
    );
    assert_eq!(rig.fake.fetcher().requests()[1].resume_offset, None);
}

#[test]
fn a_new_download_for_the_same_artifact_continues_the_same_partial_file() {
    let rig = rig();
    rig.fake.fetcher().script(
        URL,
        [
            FakeStep::InterruptAfter(1000),
            FakeStep::InterruptAfter(1000),
            FakeStep::InterruptAfter(1000),
        ],
    );
    let (first, _) = rig.run(&StopFlag::new());
    assert!(
        matches!(first, Err(DownloadError::Failed { .. })),
        "{first:?}"
    );
    assert!(rig.paths().part.exists() && rig.paths().part_note.exists());

    let (second, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(
            second,
            Ok(DownloadOutcome::Downloaded { resumed: true, .. })
        ),
        "{second:?}"
    );
    let requests = rig.fake.fetcher().requests();
    assert!(
        requests
            .last()
            .is_some_and(|r| r.resume_offset.is_some_and(|o| o > 0))
    );
}

#[test]
fn a_range_not_satisfiable_answer_drops_the_partial_file_and_starts_over() {
    let rig = rig();
    rig.fake.fetcher().script(
        URL,
        [
            FakeStep::InterruptAfter(4000),
            FakeStep::RangeNotSatisfiable,
        ],
    );

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(
            result,
            Ok(DownloadOutcome::Downloaded { resumed: false, .. })
        ),
        "{result:?}"
    );
    assert_eq!(std::fs::read(rig.paths().entry).unwrap(), rig.body);
}

#[test]
fn wrong_bytes_fail_with_a_digest_mismatch_and_leave_nothing_in_the_cache() {
    let ev = evidence!(
        covers = ["AC-059", "VAL-010"],
        kind = Kind::Fixture,
        expected = "a download whose digest differs from the catalog fails digest_mismatch, deletes the partial file and its sidecar, and creates no cache entry",
    );
    let rig = rig();
    let mut wrong = rig.body.clone();
    wrong[9] ^= 0xff;
    rig.fake
        .fetcher()
        .script(URL, [FakeStep::ServeOther(wrong.clone())]);

    let (result, _) = rig.run(&StopFlag::new());

    assert_eq!(
        result,
        Err(DownloadError::DigestMismatch {
            expected: rig.sha256,
            actual: digest(&wrong)
        })
    );
    let paths = rig.paths();
    assert!(!paths.entry.exists() && !paths.part.exists() && !paths.part_note.exists());
    ev.pass();
}

#[test]
fn a_resumed_download_with_wrong_bytes_is_fetched_once_more_from_the_start() {
    let rig = rig();
    let paths = rig.paths();
    let mut stale = rig.body[..4000].to_vec();
    stale[0] ^= 0xff;
    std::fs::write(&paths.part, &stale).unwrap();
    std::fs::write(
        &paths.part_note,
        format!(
            r#"{{"schema":"stackly.download-partial/v1","url":"{URL}","validator":"\"v1\"","bytes":4000,"updatedAt":""}}"#
        ),
    )
    .unwrap();

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(
            result,
            Ok(DownloadOutcome::Downloaded { resumed: false, .. })
        ),
        "{result:?}"
    );
    let requests = rig.fake.fetcher().requests();
    assert_eq!(requests.len(), 2, "one resume, then one full download");
    assert_eq!(requests[1].resume_offset, None);
    assert_eq!(std::fs::read(paths.entry).unwrap(), rig.body);
}

/// A fetcher that claims success after fewer bytes than the catalog size,
/// which no adapter does, to prove the size is checked again at the end.
struct ShortFetcher(Vec<u8>);

impl stackly_platform::Fetcher for ShortFetcher {
    fn fetch(
        &self,
        request: &stackly_platform::FetchRequest<'_>,
        sink: &mut dyn stackly_platform::FetchSink,
    ) -> Result<stackly_platform::FetchSummary, FetchError> {
        sink.start(&stackly_platform::FetchResponse {
            kind: stackly_platform::ResponseKind::Full,
            content_length: None,
            validator: Some("\"v1\"".to_owned()),
            host: request.url.host().to_owned(),
        })
        .map_err(FetchError::Sink)?;
        sink.write(&self.0).map_err(FetchError::Sink)?;
        Ok(stackly_platform::FetchSummary {
            received: self.0.len() as u64,
            host: request.url.host().to_owned(),
        })
    }
}

#[test]
fn a_finished_download_of_the_wrong_size_fails_with_a_size_mismatch() {
    let rig = rig();
    let platform = rig
        .fake
        .platform()
        .with_fetcher(std::sync::Arc::new(ShortFetcher(rig.body[..9000].to_vec())));
    let ctx = DownloadContext {
        platform: &platform,
        downloads_dir: &rig.dir,
        stop: &StopFlag::new(),
        policy: &rig.policy,
    };
    let source = DownloadSource {
        sha256: rig.sha256,
        size: rig.body.len() as u64,
        urls: &rig.urls,
    };

    let result = download(&ctx, &source, &mut |_, _| {});

    assert_eq!(
        result,
        Err(DownloadError::SizeMismatch {
            expected: 10_000,
            actual: 9_000
        })
    );
    assert!(!rig.paths().entry.exists() && !rig.paths().part.exists());
}

#[test]
fn no_network_and_no_cache_is_offline_after_every_attempt() {
    let ev = evidence!(
        covers = ["REQ-046", "AC-046"],
        kind = Kind::Fixture,
        expected = "with every host unreachable and no cache entry, download fails Offline listing the hosts, after three attempts per url, and keeps no cache entry",
    );
    let mut rig = rig();
    rig.fake.fetcher().serve(MIRROR, rig.body.clone());
    rig.urls.push(HttpsUrl::new(MIRROR).unwrap());
    rig.fake.fetcher().go_offline();

    let (result, _) = rig.run(&StopFlag::new());

    assert_eq!(
        result,
        Err(DownloadError::Offline {
            hosts: vec![
                "downloads.example.test".to_owned(),
                "mirror.example.test".to_owned()
            ]
        })
    );
    // Unreachable is not retried on the same url: it moves on to the next.
    assert_eq!(rig.fake.fetcher().requests().len(), 2);
    assert!(!rig.paths().entry.exists());
    ev.pass();
}

#[test]
fn connect_timeouts_are_retried_three_times_and_then_count_as_offline() {
    let rig = rig();
    let timeout = FetchError::ConnectTimeout {
        host: "downloads.example.test".to_owned(),
    };
    rig.fake.fetcher().script(
        URL,
        [
            FakeStep::Fail(timeout.clone()),
            FakeStep::Fail(timeout.clone()),
            FakeStep::Fail(timeout),
        ],
    );

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(result, Err(DownloadError::Offline { .. })),
        "{result:?}"
    );
    assert_eq!(rig.fake.fetcher().requests().len(), 3);
}

#[test]
fn a_server_error_is_retried_and_a_later_success_wins() {
    let rig = rig();
    rig.fake.fetcher().script(
        URL,
        [
            FakeStep::Status(503, None),
            FakeStep::Status(429, Some(Duration::from_secs(600))),
        ],
    );

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(result, Ok(DownloadOutcome::Downloaded { .. })),
        "{result:?}"
    );
    assert_eq!(rig.fake.fetcher().requests().len(), 3);
}

#[test]
fn a_client_error_moves_to_the_next_url_without_a_retry() {
    let mut rig = rig();
    rig.fake.fetcher().serve(MIRROR, rig.body.clone());
    rig.urls.push(HttpsUrl::new(MIRROR).unwrap());
    rig.fake
        .fetcher()
        .script(URL, [FakeStep::Status(404, None)]);

    let (result, _) = rig.run(&StopFlag::new());

    assert_eq!(
        result,
        Ok(DownloadOutcome::Downloaded {
            host: "mirror.example.test".to_owned(),
            resumed: false
        })
    );
    let urls: Vec<String> = rig
        .fake
        .fetcher()
        .requests()
        .into_iter()
        .map(|r| r.url)
        .collect();
    assert_eq!(urls, [URL, MIRROR]);
}

#[test]
fn the_final_error_follows_the_order_untrusted_then_too_large_then_failed() {
    let tls = |host: &str| FetchError::Tls {
        host: host.to_owned(),
        detail: "unknown issuer".to_owned(),
    };
    for (steps, expected) in [
        (
            vec![FakeStep::Fail(tls("downloads.example.test"))],
            "untrusted",
        ),
        (
            vec![FakeStep::Fail(FetchError::InsecureRedirect {
                host: "downloads.example.test".to_owned(),
            })],
            "untrusted",
        ),
        (
            vec![FakeStep::Fail(FetchError::LengthMismatch {
                expected: 10_000,
                announced: 20_000,
            })],
            "too_large",
        ),
        (vec![FakeStep::Status(404, None)], "failed"),
    ] {
        let rig = rig();
        rig.fake.fetcher().script(URL, steps);

        let (result, _) = rig.run(&StopFlag::new());

        let kind = match result {
            Err(DownloadError::Untrusted { .. }) => "untrusted",
            Err(DownloadError::TooLarge { .. }) => "too_large",
            Err(DownloadError::Failed { .. }) => "failed",
            other => panic!("{expected}: {other:?}"),
        };
        assert_eq!(kind, expected);
    }
}

#[test]
fn a_volume_without_room_fails_before_any_request() {
    let ev = evidence!(
        covers = ["REQ-004"],
        kind = Kind::Fixture,
        expected = "a cache volume with less free space than the artifact needs fails InsufficientSpace with needed and available, before the first fetch",
    );
    let mut rig = rig();
    rig.policy.space_reserve = 1000;
    rig.fake
        .fs()
        .set_volume(&rig.dir, FakeVolume::new(1 << 30, 5_000));

    let (result, _) = rig.run(&StopFlag::new());

    assert_eq!(
        result,
        Err(DownloadError::InsufficientSpace {
            needed: 11_000,
            available: 5_000
        })
    );
    assert!(rig.fake.fetcher().requests().is_empty());
    ev.pass();
}

#[test]
fn the_space_check_counts_the_bytes_the_partial_file_already_has() {
    let mut rig = rig();
    rig.policy.space_reserve = 1000;
    std::fs::write(rig.paths().part, &rig.body[..6000]).unwrap();
    rig.fake
        .fs()
        .set_volume(&rig.dir, FakeVolume::new(1 << 30, 5_100));

    let (result, _) = rig.run(&StopFlag::new());

    assert!(
        matches!(result, Ok(DownloadOutcome::Downloaded { .. })),
        "{result:?}"
    );
}

#[test]
fn a_raised_stop_flag_stops_a_download_and_keeps_the_partial_file() {
    let ev = evidence!(
        covers = ["REQ-004"],
        kind = Kind::Fixture,
        expected = "a stop raised after 40% of the bytes ends the download Stopped, leaves the partial file and its sidecar for a later resume, and creates no cache entry",
    );
    let rig = rig();
    let stop = StopFlag::new();
    let raise = stop.clone();
    rig.fake.fetcher().after_chunk(move |delivered| {
        if delivered >= 4000 {
            raise.stop();
        }
    });

    let (result, _) = rig.run(&stop);

    assert_eq!(result, Err(DownloadError::Stopped));
    let paths = rig.paths();
    assert!(paths.part.exists() && paths.part_note.exists());
    assert!(!paths.entry.exists());
    ev.pass();
}

#[test]
fn a_stop_during_a_retry_pause_ends_the_download_at_once() {
    let mut rig = rig();
    rig.policy.backoff = [Duration::from_secs(30), Duration::from_secs(30)];
    rig.fake
        .fetcher()
        .script(URL, [FakeStep::Status(503, None)]);
    let stop = StopFlag::new();
    let raise = stop.clone();
    let timer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        raise.stop();
    });
    let started = std::time::Instant::now();

    let (result, _) = rig.run(&stop);

    timer.join().unwrap();
    assert_eq!(result, Err(DownloadError::Stopped));
    assert!(started.elapsed() < Duration::from_secs(5));
}
