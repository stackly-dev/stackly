# Verify: Log per instance · spec 0027 · updated 2026-11-02

_Steps derived from spec 0027 acceptance criteria and its Value sourcing table. `/check verify` runs these; `/test` locks the durable ones._

## Setup

- [x] Seed a dev root: `cargo build -p stacklyd -p stackly-fake-server && cargo run -q -p stacklyd --example skeleton_seed -- --root <R>`, run the printed `stackly instance create` command, then start the instance → an instance that writes to stdout and stderr

## UI / manual

- [x] Open the instance in the app, pick the Logs tab → newest lines with `HH:mm:ss.SSS` local time, source column, Follow on, footer `Cursor <seq> · Retained 5 files × 20 MiB per stream · Secrets are redacted before storage` → AC-11
- [x] Leave the tab open while the instance writes → new lines arrive at the bottom without a reload → AC-7, AC-11
- [x] Pick stdout, then stderr → only that stream shows; All shows both, merged by `seq` → AC-6, AC-11
- [ ] Type a filter in any case (for example `ERROR` versus `error`) → the same lines match; after about 300 ms the list reads again → AC-6, AC-11
- [ ] Type a filter that matches nothing → "No lines match this filter." → AC-11
- [ ] Scroll up while following → Follow turns off; turn it on → the newest page loads again and follow resumes → AC-11
- [x] Press Load Earlier until it turns off → older pages prepend; it is disabled once nothing older is left → AC-6, AC-11
- [ ] Copy Shown Lines, paste in a text editor → one line per row as `yyyy-MM-dd HH:mm:ss.SSS <source> <text>`, no day separators; right click a row, Copy Line → that row alone → AC-11
- [ ] Stop and start the instance with the tab open → `process exited with code 0` and `process started (pid N)` rows with source `stackly` → AC-5, AC-11
- [ ] Quit stacklyd while the tab is open → lines stay, the daemon banner shows; start it again → follow resumes from the last cursor without duplicates → AC-7, AC-11
- [ ] Make the store unreadable (for example `chmod 000` on a segment of a test root) → "Logs can’t be read" with the reason and Try Again → AC-11
- [ ] Open a new instance that has not written yet → "No output yet" → AC-11
- [x] Run `stackly instance logs <name> --follow` next to the Logs tab for the same instance, source, and filter → the same lines in the same order → AC-10, AC-11
- [ ] Have the dummy server print the fixture credential, including split at the 64 KiB cut → never in the tab, in Copy Shown Lines, or in the CLI output → AC-9

## Commands

- [x] `cargo test -p stackly-core --test logs_store` → passes (store, rotation, recovery, dedupe, read, follow) → AC-1, AC-2, AC-4, AC-5, AC-6, AC-12, AC-13
- [x] `cargo test -p stacklyd --test supervisor durable_logs` → passes (socket reads, follow, failing writes, recovery mode) → AC-1 to AC-9, AC-12 to AC-14
- [x] `cargo test -p stackly-cli --test instance logs_real` → passes → AC-9, AC-10
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "InstanceLogs"` → model tests and the six Logs snapshots pass → AC-7, AC-11
- [x] `swift test --package-path clients/swift --filter aLogFollowWhileNotConnectedIsNotSent` → passes → AC-7
- [x] `cargo xtask evidence run --kind fixture -- cargo test -p stacklyd --test supervisor slow_logs -- --nocapture` → against the `stacklyd` binary, `daemon.get_info` p99 under 100 ms, the daemon's footprint within 64 MiB of baseline, the fake at 95 percent or more of the lines due, one connection closed by `ipc.write_timeout`, the slow follower in `seq` order; plus the in process test that a follow behind a rotation gets `gap rotated` then ordered entries → AC-3, AC-7 (run 2026-11-02: PASS, p99 1 ms, +15 MiB)
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test postgresql postgresql_17_writes_ready -- --ignored` → `database system is ready to accept connections` on stderr as `normal`, the `FATAL` line of a wrong login as `error`, read through `stackly instance logs --json`; `BLOCKED` without the artifact in the xtask cache → AC-6, AC-8 (run 2026-11-02: PASS on 17.11)

## Value sourcing

- [x] `seq`: restart the daemon mid write → the next record's `seq` follows the last whole record, never repeats → AC-1, AC-13
- [x] `at`: compare a record's `at` with the daemon clock, not the process clock → read time in UTC to the millisecond → AC-1
- [x] `level`: lines `ERROR:`, `[FTL]`, `WARNING:`, `[WRN]` on stdout and a plain line on stderr → error, error, warning, warning, normal → AC-8
- [x] `src.pid`, `src.startedAt`, `src.end`: SIGKILL the daemon while the fake writes, restart, adopt → no line stored twice, none lost from the spool → AC-4
- [x] `truncated.bytes`: overflow the spool → a `truncated` row whose size matches the dropped bytes → AC-5
- [x] `started.pid`, `exited.code`, `exited.signal`: kill the process with a signal → `process exited with signal N` in red → AC-5
- [x] `storeId`: delete `captured/` → next read has a new `storeId`; an old cursor gets the `store_reset` gap → AC-5, AC-13
- [x] `retention`: footer numbers match `LOG_SEGMENTS_PER_STREAM` and `LOG_SEGMENT_MAX_BYTES` from the page, not a literal → AC-11
- [x] `storedBytes`: compare with the sizes of the segment files on disk → equal → AC-6
- [x] `subscriptionId`: open 8 event subscriptions plus log follows on one connection → the 9th is refused `events.too_many_subscriptions`; the tab turns Follow off and shows the reason as help → AC-7
- [ ] `ended`: delete the instance while following → the CLI exits 10 with `cli.instance_not_found`; the tab's page shows Instance Not Found → AC-7, AC-10
- [x] `instance_logs` health: make writes fail → `daemon.get_health` shows `degraded`, then `ok` 60 s after writes recover → AC-12
- [x] CLI local time: run with `TZ=Asia/Jakarta` and `TZ=UTC` → times shift by the zone → AC-10
- [ ] GUI local time and date: change the Mac time zone with the tab open → rows loaded afterwards use the new zone; a day separator appears where the local date changes → AC-11

## Acceptance criteria coverage

- AC-1 · Commands 1, 2; Value sourcing `seq`, `at`
- AC-2 · Commands 1
- AC-3 · Commands 6 (evidence run)
- AC-4 · Commands 1; Value sourcing `src`
- AC-5 · UI stop and start; Value sourcing `truncated`, `exited`, `storeId`
- AC-6 · UI source, filter, Load Earlier; Commands 1, 2
- AC-7 · UI follow, reconnect; Commands 4, 5; Value sourcing `subscriptionId`, `ended`
- AC-8 · Value sourcing `level`; Commands 7 (native)
- AC-9 · UI fixture credential; Commands 3
- AC-10 · UI CLI comparison; Value sourcing CLI time, `ended`
- AC-11 · every UI step; Commands 4
- AC-12 · Value sourcing health; Commands 2
- AC-13 · Value sourcing `seq`, `storeId`; Commands 1
- AC-14 · Commands 2 (recovery mode)
