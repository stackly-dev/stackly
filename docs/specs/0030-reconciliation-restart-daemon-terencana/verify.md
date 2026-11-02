# Verify: Rekonsiliasi dan restart daemon terencana · spec 0030 · updated 2026-11-02

_Langkah diturunkan dari acceptance criteria spec 0030. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah._

## Milestone 1: Contract and data

### UI / manual

- [x] Buka `crates/protocol/src/restart.rs` → `RestartPreview`, `RestartPlanView`, dan enum terbuka `PlanDecision`, `PlanReason`, `PlanState`, `PlanCloseReason`, `PlanOutcome` masing masing berakhir di `Unrecognized` → AC-18
- [x] Buka `protocols/contract-versions.json` → versi 2.0 dan 2.1 tercatat; setiap perubahan schema berikutnya di fitur ini memakai 2.2 dan tidak memakai `--amend` → AC-18
- [x] Buka `crates/storage/migrations/0009_daemon_restart_plan.sql` → tabel `daemon_restart_plan` dan `daemon_restart_plan_instance`, `instance_id` tanpa foreign key, unique index parsial `daemon_restart_plan_open` untuk state terbuka → AC-19

### Commands

- [x] `cargo xtask codegen --check` → "20 generated files are up to date" → AC-18
- [x] `cargo test -p stackly-protocol` → semua lulus, termasuk `ipc_fixtures` (fixture `daemon.restart_preview`, `daemon.shutdown` dengan `expectedPlanDigest`, event `daemon.restart_plan`, `daemon.reconciling` dengan `findings`, tiga error `daemon.restart_*`) dan tiga test fixture katalog, create, dan duplicate yang kini memakai digest paket nyata → AC-18
- [x] `swift test --package-path clients/swift` → 225 test lulus, termasuk fixture round trip untuk 43 method → AC-18
- [x] `cargo test -p stackly-domain` → state machine rencana (`hand_over`, `abort`, `take_over`, `record_outcome`, `expired` di boot baru), keputusan `plan_decision` menurut lifecycle, `StopIntent`, transisi `data_missing` dari `stopped` dan `failed`, `OperationKind::DaemonRestart` → AC-4, AC-9, AC-13
- [x] `cargo test -p stackly-storage --test open a_version_8` → migrasi 9 membuat dua tabel kosong; rencana kedua yang terbuka ditolak `UNIQUE`, `finished_at` yang tidak cocok dengan state ditolak `CHECK`, baris instance ikut terhapus bersama rencananya → AC-19
- [x] `cargo test -p stackly-storage --test registry` dan `cargo test -p stackly-testkit --test fake_registry` → suite konformasi `restart_plans`: satu rencana terbuka, update, dan prune menyisakan 20 rencana terminal tanpa menyentuh rencana terbuka → AC-19
- [x] `cargo test -p stacklyd --test server` → 84 test lulus; handshake memakai `ProtocolVersion::CURRENT.major` → AC-18
- [x] `cargo test -p stackly-cli --bin stackly there_is_one_constructor_per_cli_code_and_each_follows_the_catalog` → konstruktor `restart_aborted` dan `restart_restore_failed` mengikuti katalog → AC-15 (bagian kode)
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings` dan `cargo fmt --all --check` → bersih
- [x] `cargo xtask check-deps` → lulus; `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus

### Acceptance-criteria coverage

- AC-18 · seluruh kontrak: method, field, event, `findings`, kode, fixture, tipe Swift, bump mayor 2.0 (tambahan F70 jadi 2.1)
- AC-19 · migrasi 9, unique index parsial, retensi 20 rencana terminal di prune, suite konformasi
- AC-4 · hanya bagian domain (transisi `data_missing`, failure code); pemeriksaan folder data menunggu Milestone 2
- AC-9, AC-13 · hanya bagian domain (state machine rencana); alur daemon menunggu Milestone 3
- AC-1 sampai AC-3, AC-5 sampai AC-8, AC-10 sampai AC-12, AC-14 sampai AC-17 · belum, milik milestone lain

### Sisa kegagalan di luar milestone ini

`stackly-core catalog::status::tests::an_available_host_artifact_takes_its_support_from_its_records_and_notes_fill_empty_columns` dan `stackly-cli --test provider inspect::inspect_writes_the_identity_capabilities_matrix_and_artifacts` (F70 M3), `stacklyd ipc::operation::dto::tests::every_domain_code_has_a_known_wire_value` (F29 M2).

## Milestone 2: Daemon death without a plan

### UI / manual

- [x] Buka `crates/core/src/supervisor/prepare.rs` → `check_instance_folder` menerima paket yang di pin dan memanggil `data_missing` (folder data tidak ada, folder kosong, atau marker yang dideklarasikan paket hilang); `prepare` memanggilnya untuk `Checks::Full` sebelum `ensure_instance_run_dir` → AC-4
- [x] Buka `crates/core/src/supervisor/startup.rs` `error_failure_code` → `DataUnavailable` ke `data_missing` dan `RuntimeUnavailable` ke `runtime_unavailable` → AC-5
- [x] Buka `crates/core/src/supervisor/integrity.rs` → pemeriksaan data dan runtime hanya membaca (tanpa tulis, tanpa spawn), memakai `verify_executable` yang sama dengan startup → AC-4, AC-5
- [x] Catatan untuk checker: `rebind::check` memanggil `check_instance_folder` dengan paket yang sama, jadi marker yang hilang kini `supervisor.data_unavailable` untuk start dan rebind sekaligus (spec 0030 Neutral; spec 0025 AC-6 dan AC-8 berubah untuk kasus `marker_missing`) → AC-4

### Commands

- [x] `cargo test -p stackly-core --test supervisor_start integrity` → 8 test: data hilang dalam tiga bentuk dan tiga lifecycle menjadi `failed` `data_missing` tanpa spawn, tanpa folder baru, start ditolak `supervisor.data_unavailable`; proses `Proven` diadopsi dengan finding; executable runtime hilang memberi finding `runtime_missing`; respawn gagal `executable_invalid` atau `runtime_unavailable` → AC-4, AC-5
- [x] `cargo test -p stackly-core --test supervisor_start pid_reuse` → PID milik executable lain (`/usr/bin/sleep`) tidak pernah disinyal oleh rekonsiliasi, recheck, stop, force stop, dan start; lock dipegang pihak lain menjadi `unknown` dengan evidence `executable` gagal → AC-3
- [x] `cargo test -p stackly-core --test supervisor_start listener` → listener luar di port fixed: respawn gagal `endpoint_in_use` dengan `lastFailure` `endpoint_in_use`, start dari `stopped` gagal `endpoint_in_use`, tanpa spawn, tanpa sinyal, port tidak berubah → AC-6
- [x] `cargo test -p stackly-core --test supervisor_start data_compat` dan `cargo test -p stackly-core --test instance_rebind` → marker hilang kini `data_unavailable` untuk start dan rebind; `marker_mismatch`, `marker_unreadable`, dan `incompatible` tidak berubah → AC-4
- [x] `cargo test -p stackly-core --lib supervisor::reconcile` → tabel `plan_for_missing_data` untuk semua lifecycle tanpa proses → AC-4
- [x] `cargo test -p stackly-core --test supervisor_start` → 162 test lulus → AC-3 sampai AC-6
- [x] `cargo test -p stacklyd --test postgresql -- --ignored reconcile:: --test-threads=1` → native PostgreSQL 17 dan 18 dari cache runtime (tanpa unduhan): tiga test lulus dalam sekitar 2 menit → AC-1, AC-2, AC-4
- [x] `cargo test -p stacklyd --test postgresql -- --ignored mismatch:: --test-threads=1` → dua test native lulus dengan harapan baru `supervisor.data_unavailable` untuk `PG_VERSION` yang hilang → AC-4
- [x] `cargo test -p stacklyd --test supervisor` → 96 test lulus → AC-4
- [x] `cargo clippy -p stackly-core -p stacklyd --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih

### Acceptance-criteria coverage

- AC-1 · bagian native: `closing_every_client_keeps_three_postgresql_instances_unchanged` (id, pid, `startedAt`, `serverInstanceId`, `SELECT 1`, sentinel). Test integrasi Swift dengan fake dan record manual menunggu Milestone 5 dan 6
- AC-2 · `sigkill_leaves_one_postmaster_per_data_folder_and_every_sentinel_intact`: tiga cluster diadopsi dengan pid dan `startedAt` yang sama, satu postmaster per `instance.lock`, sentinel utuh, instance `stopped` tetap `stopped` dengan folder datanya identik byte per byte; pada cluster yang berjalan hanya file `pg_wal`, `pg_stat_tmp`, `pg_logical`, dan `pg_notify` milik server yang boleh berganti
- AC-3 · bagian rekonsiliasi, recheck, stop, force stop, dan start. Bagian rencana restart menunggu Milestone 3 (belum ada jalur stop rencana). Perilaku mengikuti tabel spec 0010: lock bebas menjadi `gone` (`stopped`), lock dipegang pihak lain menjadi `unknown` dengan blocker `dataInUse` dan evidence `executable` gagal, bukan `identityUnproven` seperti kalimat AC-3
- AC-4 · bagian rekonsiliasi dan start, fixture dan native. Bagian restore (tidak masuk daftar restore) menunggu Milestone 3
- AC-5 · finding `runtime_missing` untuk proses `Proven`, respawn gagal `executable_invalid` dan `runtime_unavailable`, tidak ada blocker baru. Start dari `stopped` gagal dengan kode itu tetapi tidak menulis `lastFailure` karena gerbangnya terjadi sebelum `start_begin` (jalur start yang ada); pencatatan untuk restore menunggu Milestone 3
- AC-6 · bagian respawn dan start. Restore dengan lifecycle `stopped` dan `lastFailure` menunggu Milestone 3
- AC-7 sampai AC-19 kecuali yang disebut di atas · belum, milik milestone lain

### Sisa kegagalan di luar milestone ini

`stackly-core catalog::status::tests::an_available_host_artifact_takes_its_support_from_its_records_and_notes_fill_empty_columns` (F70 M3) dan `stacklyd ipc::operation::dto::tests::every_domain_code_has_a_known_wire_value` (F29 M2) gagal sama seperti di `main`.

## Milestone 3: Planned restart in the daemon

### UI / manual

- [x] Buka `crates/core/src/supervisor/handover.rs` → `HANDOVER_PROVEN` memuat `testing/fake` line `0.3` dan `0.4` (evidence `spec-0015-ac-16`); `postgresql` dan `nats` line `1` ada karena test native AC-8 lulus (evidence `spec-0030-ac-8`). Pencocokan memakai ID provider penuh (`official/postgresql`), bukan nama pendek → AC-7, AC-8
- [x] Buka `crates/core/src/operation/scheduler.rs` `begin` → quiesce menolak `Origin::Request` selain `daemon_restart` dengan `BeginError::RestartPending` sebelum precheck; `Origin::Background` tidak tersentuh → AC-10
- [x] Buka `crates/core/src/supervisor/daemon_restart.rs` → operasi `daemon_restart` menulis rencana lewat `commit_guarded` (`write_plan_guard`, digest dihitung lagi di transaksi), menghentikan instance `stop_restore` dengan `StopIntent::PlannedRestart` (tanpa menulis desired, tanpa cek lease), lalu `hand_over`; stop yang gagal membuat rencana `aborted` dan `Restore` antre untuk instance yang sudah berhenti → AC-9, AC-11, AC-12
- [x] Buka `crates/core/src/supervisor/restore.rs` dan `reconcile.rs` → daemon baru mengambil alih rencana terbuka (`restoring` di boot sama, `expired` di boot baru), `Then::Restore` untuk `stopped` dengan desired `running`, `Then::PlannedStop` untuk `stopping` yang `Proven`, outcome per instance, `reconciled_at` di akhir rekonsiliasi → AC-6, AC-9, AC-13, AC-14
- [x] Catatan untuk checker: celah spec: folder data kosong tidak lagi dianggap hilang untuk paket tanpa data marker (NATS kosong sampai server pertama jalan); `prepare.rs::data_missing` berubah, diterima lead; tabel *Data* dan AC-4 disesuaikan lewat architect. Dijaga unit test `prepare.rs` (NATS kosong tidak hilang, PostgreSQL kosong hilang). `daemon.shutdown { mode: restart }` kini method yang memulai operasi, jadi wajib `idempotencyKey` (`operation.idempotency_key_required`). TUGAS MILESTONE 4: `stackly daemon restart` harus mengirim `idempotencyKey`; sampai itu restart lewat CLI ke daemon launchd gagal; test native memakai IPC langsung → AC-9, AC-15

### Commands

- [x] `cargo test -p stackly-core --lib supervisor::handover` → pencocokan line (mayor dari 1, mayor.minor di bawah 1.0, revision bukan semver), tabel keputusan semua lifecycle, mode recovery, digest → AC-7
- [x] `cargo test -p stacklyd --test restart_plan -- --test-threads=1` → 12 test: preview, restart yang menghentikan `stop_restore` dan membiarkan `keep`, request berulang dengan key sama, quiesce (`restart_pending` dan `restart_busy`), `force`, abort `stop_timeout` dengan start ulang, digest tidak cocok, adopsi dan restore oleh daemon berikutnya, boot baru `expired`, daemon berakhir saat rencana `stopping`, listener luar `endpoint_in_use`, instance yang desired nya sudah `stopped` di-`skipped` → AC-6, AC-7, AC-9 sampai AC-14
- [x] `cargo test -p stacklyd --test server` → 84 test lulus; test `daemon.shutdown` restart kini mengirim key dan menunggu `handle.started()` → AC-10
- [x] `cargo test -p stacklyd --test supervisor -- --test-threads=1` dan `cargo test -p stacklyd --test operations` → tidak ada regresi stop, restart, reload, force stop → AC-9
- [x] `cargo clippy -p stackly-core -p stackly-domain -p stacklyd --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd -- --ignored --test-threads 1 postgresql_17 postgresql_18 nats_survives` → 4 test native PASS (PostgreSQL 17, 18, NATS, dan fake sebagai bukti line `0.4`): PID dan `startedAt` sama melewati `kickstart -k`, `bootout`, dan restart terencana; setelah itu daemon baru mencatat outcome `adopted` → AC-8

### Acceptance-criteria coverage

- AC-7 · unit `handover.rs` (tabel keputusan, pencocokan line) dan `the_preview_sorts_the_running_instances_by_the_handover_list` (digest, launch context, tanpa operasi penghalang)
- AC-8 · tiga test native di `native_launchd.rs`; entri `postgresql` dan `nats` di `HANDOVER_PROVEN` hanya setelah lulus
- AC-9 · `a_restart_stops_what_it_cannot_hand_over_and_leaves_the_rest_running` (daemon lama) dan `the_next_daemon_adopts_the_kept_instance_and_starts_the_stopped_one_again` (daemon baru, outcome `adopted` dan `restored`, `readyAt` terisi)
- AC-10 · `an_operation_that_cannot_resume_refuses_the_restart_and_the_daemon_goes_on`, `a_new_request_is_refused_while_the_restart_waits_for_the_operations_in_flight`, `force_goes_on_although_a_create_is_still_running`. Operasi `daemon_restart` dimulai sesudah quiesce, bukan sebelumnya (tidak ada baris operasi `failed` untuk `restart_busy`)
- AC-11 · `an_instance_that_does_not_stop_aborts_the_restart_and_the_others_start_again`: rencana `aborted` `stop_timeout`, instance yang sudah berhenti hidup lagi sebagai proses baru lewat operasi sistem `start_instance` sesudah operasi `daemon_restart` berakhir `failed` (bukan di dalam operasi itu, karena langkah jurnal sebuah operasi tidak bisa diulang per instance); proses yang macet tetap diawasi, tanpa `SIGKILL`. Event `daemon.restart_plan` membawa `stoppedAt` kosong untuk instance yang macet; kode exit CLI menunggu milestone 4
- AC-12 · `a_plan_that_is_not_the_confirmed_one_is_refused_before_anything_stops` (digest di preview) dan test `plan.rs` di core (digest di transaksi, `DigestChanged` dari `write_plan_guard`)
- AC-13 · `a_new_boot_session_expires_the_plan_and_starts_nothing`
- AC-14 · `a_daemon_that_ends_while_the_plan_stops_leaves_it_to_the_next_one`: rencana `stopping` diteruskan sebagai `restoring`, instance yang sudah berhenti hidup lagi, instance yang macet dicoba berhenti sekali lagi dengan `PlannedRestart` (gagal `stop_timeout`, outcome `failed`), desired tidak berubah
- AC-6 · `a_foreign_listener_fails_the_restore_with_endpoint_in_use`: restore dari `stopped` gagal `endpoint_in_use`, `lastFailure` dicatat di instance yang tetap `stopped`, listener luar tidak tersentuh, port tidak berganti
- AC-3 (bagian rencana restart) · rencana tidak pernah menyinyal proses yang identitasnya tidak terbukti: `stop_for_plan` menolak `unknown` dan `Proven`-nya dijaga `signal_and_wait`; test PID reuse khusus rencana menunggu `/test`
- AC-5 (`lastFailure` untuk restore) · `settle_restored` menulis `runtime_unavailable` dan `executable_invalid` lewat `record_start_refusal`; dicakup bersama AC-6 oleh test listener, test khusus runtime menunggu `/test`
- AC-15 sampai AC-19 kecuali yang disebut di atas · belum, milik milestone lain

### Sisa kegagalan di luar milestone ini

`stackly-core catalog::status::tests::an_available_host_artifact_takes_its_support_from_its_records_and_notes_fill_empty_columns` (F70 M3) dan `stacklyd ipc::operation::dto::tests::every_domain_code_has_a_known_wire_value` (F29 M2) gagal sama seperti di `main`.

## Milestone 4: CLI and GUI

### UI / manual

- [ ] Di terminal, jalankan `stackly daemon restart` terhadap daemon launchd dari test root sendiri dengan satu instance `stop_restore` → rencana tampil di stderr, `[y/N]` muncul, `n` keluar 9, `y` menjalankan restart sampai rencana `finished` dan mencetak tabel per instance plus baris `Took` dengan tiga durasi → AC-15
- [ ] `stackly daemon status` sesudah restart → baris `Last restart` berbunyi `finished <umur>: N adopted, M restored`; tanpa restart baris itu tidak ada → AC-15
- [ ] Buka Settings (Stackly menu, Settings) dengan daemon launchd dari test root sendiri → tab General dengan bagian Daemon: baris `stacklyd` bersubtitle "Connected over Unix socket · protocol N" dan tombol Restart Daemon aktif; tanpa tab atau baris lain → AC-16
- [ ] Putuskan daemon (hentikan test root) → subtitle "Disconnected · reconnecting" dan tombol mati; jalankan daemon `manual` → subtitle "Runs outside launchd, so it can't restart itself" dan tombol mati → AC-16
- [ ] Tekan Restart Daemon dengan satu instance fake yang provider nya tidak ada di `HANDOVER_PROVEN` → sheet daftar "Keeps running" dan "Stops, then starts again" dengan alasan, plus "Operations in flight" bila ada; Cancel menutup tanpa efek → AC-16
- [ ] Tekan Restart di sheet → jendela utama menampilkan banner "stacklyd is restarting" (jumlah yang tetap jalan dan yang berhenti), baris instance `stop_restore` bertanda "Stopped for daemon restart"; sesudah daemon kembali banner ringkasan, lalu toast "Daemon restarted. N instances adopted, M started again." dan banner hilang → AC-16
- [ ] Mulai operasi create yang lama lalu tekan Restart → sheet menampilkan penolakan `daemon.restart_busy` dengan operasi penghalang dan tetap terbuka → AC-16
- [ ] Catatan: jangan pernah menjalankan langkah ini terhadap agent milik pengguna (`com.ojiepermana.stackly.daemon`); pakai `STACKLY_ROOT` test root sendiri → AC-8 (bagian CLI)

### Commands

- [x] `cargo test -p stackly-cli --bin stackly daemon::` → test unit restart: prompt (tanpa terminal exit 2, jawaban n exit 9, rencana tanpa `stop_restore` tidak bertanya), digest dan key di request, `--force`, `restart_plan_changed` (satu pertanyaan ulang di terminal, exit 3 tanpa terminal atau pada perubahan kedua), `aborted` exit 7, restore gagal exit 7, instance `unknown` hanya warning, rencana terbuka di batas exit 8, `--no-wait-instances`, Ctrl+C exit 130, daemon `manual` ditolak daemon sendiri tanpa pertanyaan, serta semua test penantian spec 0015 yang lama → AC-15
- [x] `cargo test -p stackly-cli --test daemon_restart_plan` → 11 test dengan daemon terskrip dan binary `stackly` nyata: hasil JSON dan teks, request membawa `expectedPlanDigest` dan idempotency key, `--yes` dan tanpa terminal, prompt pty (n exit 9, y lanjut), `plan_changed` exit 3, `aborted` exit 7, restore gagal exit 7 dengan seluruh rencana di `--json`, warning `cli.restart_instance_unknown`, `--no-wait-instances`, dan `daemon status` dengan `lastRestart` → AC-15
- [x] `cargo test -p stackly-cli` → seluruh suite hijau; golden `daemon_status.json` berisi `lastRestart` (null) dan urutan kunci `info, health, client, lastRestart, agent` → AC-15
- [x] `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, dan `cargo xtask check-deps` → bersih
- [x] `stackly completion zsh` dan `stackly manpage` memuat `--yes`, `--force`, dan `--no-wait-instances` untuk `daemon restart` (dihasilkan clap, tidak ada file tulisan tangan) → AC-15

- [x] `swift test --package-path clients/swift` → hijau, termasuk `DaemonRestartClientTests` (preview tanpa key, restart dengan digest, `force`, key, batas 45 detik, balasan salah jenis, penolakan lewat) → AC-16
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "RestartBannerContentTests|DaemonRestartModelTests|DaemonSettingsContentTests|DaemonModelRestartTests"` → banner murni per state, sheet dan penolakannya (`restart_plan_changed`, `restart_busy`, jawaban hilang memakai key yang sama), rencana lewat event dan handshake, tanda baris, toast sekali, dan dua panggilan lewat link → AC-16
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ShellReopenTests` → tiga instance fake `ready`: GUI ditutup lalu dibuka, `id`, `process.pid`, `process.startedAt`, `serverInstanceId`, dan `startedAt` daemon sama, `GET /health` 200, model baru mengambil ulang tiga instance (butuh `cargo build -p stacklyd -p stackly-fake-server` dan contoh `skeleton_seed`) → AC-1
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter RestartSnapshotTests` → snapshot light, dark, dan narrow: sheet (dengan operasi penghalang dan dengan penolakan), banner di tiap state, bagian Daemon di Settings (terhubung, putus, `manual`), dan status baris "Stopped for daemon restart"; lihat lewat `cargo xtask snapshot review` → AC-16
- [x] `cargo xtask build-app` dan `xcrun swift-format lint --strict` untuk file Swift yang berubah → bersih

### Acceptance-criteria coverage

- AC-15 · prompt, `--yes`, `--json`, `--force`, `--no-wait-instances`, dan exit code 2, 3, 7, 8, 9, 130 dicakup unit test `restart.rs` dan `daemon_restart_plan.rs`; durasi `stopping`, `daemonGap`, `restoring` dalam milidetik dari `handedOverAt`, `reconciledAt`, `finishedAt`; `daemon status` dengan `lastRestart` dicakup `daemon_status_shows_the_last_restart`
- AC-15 (batas) · `DAEMON_RESTORE_WAIT` 600 detik dicakup unit test (exit 8 membawa `seconds` 600) dengan jam yang di pause, bukan dengan menunggu 10 menit
- AC-8 (bagian CLI) · `stackly daemon restart` terhadap daemon launchd sungguhan belum dijalankan di milestone ini; test native `native_launchd.rs` milestone 3 memakai IPC langsung. Langkah manual di atas menunggu persetujuan lewat `/check verify`
- AC-11 (bagian CLI) · rencana `aborted` keluar 7 `cli.restart_aborted` dicakup `an_aborted_plan_exits_7_and_the_old_daemon_keeps_answering`
- AC-16 · bagian Daemon di tab General (baris `stacklyd`, subtitle, tombol nonaktif dengan alasan) dicakup `DaemonSettingsContentTests` dan snapshot; sheet konfirmasi, banner `restarting`, ringkasan, peringatan `aborted` dan `expired`, tanda baris, dan toast dicakup `RestartBannerContentTests`, `DaemonRestartModelTests`, `DaemonModelRestartTests`, dan `RestartSnapshotTests`; langkah manual di atas menunggu `/check verify`, record manual dari tombol Settings milik milestone 6
- AC-1 (bagian Swift) · `ShellReopenTests`; test daemon dengan PostgreSQL native dan record manual `native` menunggu milestone 2 dan 6

### Keputusan lokal yang perlu diketahui checker

- Tanpa terminal atau dengan `--json`, rencana yang menghentikan instance tetap butuh `--yes` (exit 2), walau stdin adalah terminal. Dengan `--yes` di terminal, `restart_plan_changed` tetap bertanya sekali lagi karena persetujuan lama berlaku untuk rencana lain.
- Batas tunggu daemon lama: 20 detik bila preview tidak punya instance `stop_restore`, `DAEMON_RESTORE_WAIT` (10 menit) bila ada. Sisa waktu rencana di daemon baru dihitung dari awal perintah.
- Rencana `expired` dibaca sebagai `cli.restart_restore_failed` (daemon hidup lagi, instance tidak kembali). Spec tidak menyebutnya; perlu keputusan bila ingin kode lain.
- Setiap percobaan restart memakai idempotency key baru (v7), termasuk percobaan ulang sesudah `restart_plan_changed`, karena digest di params membuat fingerprint request berbeda.

### Keputusan lokal GUI yang perlu diketahui checker

- Banner restart menggantikan banner daemon selama rencana terbuka (jendela disruption), karena banner daemon hanya akan berkata stacklyd hilang. Banner peringatan (`finished` dengan kegagalan, `aborted`, `expired`) memberi tempat ke banner daemon bila daemon punya banner sendiri, dan hilang oleh Dismiss atau setelah 10 menit (`RESTART_SUMMARY_TTL`). `finished` tanpa kegagalan hanya menghasilkan toast, dan toast hanya muncul untuk rencana yang jendela ini lihat terbuka atau yang jendela ini minta sendiri.
- Tanda baris "Stopped for daemon restart" dipasang pada instance `stop_restore` yang sudah berhenti (`stoppedAt` terisi) dan belum punya outcome, hanya di halaman Instances; baris Overview belum memakai tanda itu.
- Tombol Restart di sheet tidak menawarkan `force`; operasi yang menghalangi ditampilkan lalu orang menunggu atau membatalkan. `DaemonModel`, `MutationPath` tidak berubah selain properti `restart`; `daemon.shutdown { restart }` memakai batas 45 detik lewat `DaemonRestartClient`, bukan `MutationPath` (batas standar 10 detik terlalu pendek untuk quiesce 30 detik).
- Scene Settings di `StacklyApp.swift` kini memakai `SettingsRoot(model:)`; `SettingsPlaceholder` dihapus.

### Perbaikan setelah cross check (dev-f30-fix)

Tiga celah kode dari cross check spec 0030 dan satu bug dari run native, diperbaiki di daemon, core, dan domain. Tidak ada perubahan kontrak.

- **Restart di recovery mode** · `daemon.shutdown { restart }` dengan supervisor `Unavailable` (registry dalam recovery mode) kini diterima tanpa rencana (`planId` kosong) dan daemon keluar 75 tanpa menghentikan instance, seperti restart spec 0015; sebelumnya gagal `registry.unavailable` di `replay_operation`. `Starting` tetap `daemon.starting`. Test: `a_restart_in_recovery_mode_exits_75_without_a_plan` di `restart_plan.rs` (gagal tanpa perbaikan: `registry.unavailable`)
- **Replay** · request ulang dengan key yang sama untuk operasi `daemon_restart` yang `failed` mengembalikan error tersimpan (`Replay.failure`), yang `cancelled` mengembalikan `daemon.restart_busy`; sebelumnya selalu `ShutdownAccepted`. Test unit di `apps/daemon/src/ipc/restart.rs` (`a_replayed_restart_that_...`); belum ada test daemon end to end karena operasi `failed` atau `cancelled` butuh lock instance yang tertahan lebih dari 10 detik (`PREPARE_GRACE`)
- **Pemicu restore** · `PendingKind::Restore` dan stop `PlannedStop` menulis input jurnal `{"trigger":"planned_restart"}` (AC-9, *Alur* langkah 10); `Start`, `Stop`, dan `Respawn` tetap `reconcile`. Test: `the_next_daemon_adopts_the_kept_instance_and_starts_the_stopped_one_again` kini membaca jurnal start restore
- **Abort lalu `persist_plan` gagal** · sebelumnya `?` keluar sebelum `queue_restore`, sehingga instance yang sudah dihentikan tetap mati di daemon yang masih hidup sampai restart berikutnya. Kini kegagalan menulis rencana `aborted` dicatat (`warn`), instance tetap dinyalakan lagi, dan error asli stop dikembalikan; barisnya tetap `stopping` untuk diambil daemon berikutnya. Belum ada test (butuh kegagalan commit terarah pada `commit` ketiga di `daemon_restart_work`, yang tidak tersedia di test daemon)
- **Rencana kosong tidak selesai** (ditemukan `chk-f30-m4`, run native) · `RestartPlan::record_reconciled` kini memindah rencana ke `finished` bila tidak ada entri tersisa tanpa outcome (rencana kosong), dan `settle_plan_after_reconcile` melepas rencana, menyimpan, dan memancarkan event final; sebelumnya hanya `record_outcome` yang menyelesaikan rencana, sehingga rencana tanpa entri tetap `restoring`. Test: `a_plan_without_instances_finishes_when_the_new_daemon_reconciled` dan `a_plan_with_an_instance_left_stays_restoring_after_reconciliation` (domain), `a_restart_with_no_instance_in_the_plan_still_finishes` di `restart_plan.rs` (restart dengan nol instance selesai dan `instance.create` sesudahnya diterima; test gagal tanpa perbaikan)

```bash
cargo test -p stacklyd --lib restart::
cargo test -p stacklyd --test restart_plan -- --test-threads=1
cargo test -p stackly-domain --lib restart
```
