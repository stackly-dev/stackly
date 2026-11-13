# Verify: Env down, shared instance & lease · spec 0047 · updated 2026-11-13

_Langkah diturunkan dari acceptance criteria spec 0047. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27, dengan `export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`._

## Milestone 1: Down di daemon

Langkah Build plan 1 dan 2 sudah dibangun: kebijakan `environment_down`, `Environment::begin_down`, `leases_of_environment`, lalu stop ber guard lease. Langkah 3 sampai 5 (`environment::down`, protokol 4.5, handler `env.preview_down` dan `env.down`) menambah dua bagian di bawah. Protokol dan migrasi tidak berubah di langkah 1 dan 2; tidak ada migrasi di langkah 3 sampai 5.

### UI / manual

- [x] Buka `crates/domain/src/operation.rs` → `environment_down` tidak `resumable()`, `interrupt_policy()` nya `CloseInterrupted`, dan `is_coordinator()` benar untuk `environment_up` dan `environment_down` saja → AC-10, AC-25
- [x] Buka `crates/domain/src/environment.rs` → `Environment::begin_down(operation)` mengosongkan `last_up`, mengisi `last_operation`, dan menaikkan revision 1, tanpa menyentuh member → AC-4
- [x] Buka `crates/domain/src/registry/mod.rs` dan `guard.rs` → `Registry::leases_of_environment` dan `GuardView::leases_of_environment` (lease ber `source_kind` `environment` dan `source_id` itu, urut `acquired_at` lalu ID); implementasinya di `crates/storage/src/registry/read.rs` (`select_leases` dipakai bersama `leases`) dan di fake testkit → AC-4
- [x] Buka `crates/core/src/supervisor/stop.rs` → `LeaseCheck` (`None`, `Manual`, `EnvironmentDown`) diturunkan dari permintaan lewat `StopRequest::lease_check`, bukan dari `StopIntent`; `StopIntent::checks_leases()` sudah tidak ada; restart (`restart_op.rs`), start yang dibatalkan (`startup.rs`), dan restart terencana (`stop_for_plan`) memanggil `journal_stop` atau `stop_direct` dengan `LeaseCheck::None` → AC-5, AC-13, AC-14
- [x] Buka `crates/core/src/supervisor/persist.rs` → `commit_stop_decision` membaca lease di dalam guard, menjalankan `manual_stop_ops` atau `evaluate_stop(EnvironmentDown)`, keputusannya final (hanya penolakan niat force yang membangun ulang), dan `startup_lease_effect` diterbitkan dari op yang ditulis guard. Ketiga tempat commit (`journal_stop`, `settle_gone_process`, commit desired saja di `run_stop`) memakainya saat `LeaseCheck` bukan `None`; di `settle_gone_process` keputusan lease hanya menjaga desired dan op lease, dan `stopping` selalu menjadi `stopped` → AC-5, AC-13
- [x] Di `run_stop` dan `stop_direct` → dengan `LeaseCheck` selain `None`, `cancel_respawn` dipanggil sesudah `journal_stop` berhasil (bukan `Kept`, bukan penolakan); dengan `None` urutannya tidak berubah → AC-5, AC-13
- [x] `stop_all_step` (`crates/core/src/supervisor/stop_all.rs`) → memakai `LeaseCheck::Manual` dengan `override_leases` false bertanda `// provisional: F47`, dan `supervisor.stop_requires_override` dari guard menjadi `skipped`; pilihan override ada di langkah 6 → AC-16 (bagian guard)

### Commands

- [x] `cargo test -p stackly-domain --lib operation::tests` → `only_environment_up_and_environment_down_are_coordinators`, daftar resumable `install_runtime` dan `backup_instance`, dan daftar `CloseInterrupted` dengan `environment_down` lulus → AC-10, AC-25
- [x] `cargo test -p stackly-domain --lib environment::tests::begin_down_clears_the_last_up_and_records_the_down` → lulus → AC-4
- [x] `cargo test -p stackly-core --test operation_engine coordinator` → `an_interrupted_environment_down_closes_with_its_queued_child_stop_and_no_rebuild` (Down yang terputus dan anak `stop_instance` nya yang antre ditutup `failed` alasan `interrupted` tanpa hint pemulihan, lewat aturan tanpa factory yang sudah ada, `rebuild_failed` 1) dan `an_environment_down_runs_in_the_coordinator_slot_class_beside_a_busy_running_slot` lulus → AC-3 (kelas slot), AC-10
- [x] `cargo test -p stackly-core --test operation_engine` → 98 test lulus; empat test lama yang memakai `environment_down` sebagai contoh jenis biasa atau resumable kini memakai `import_environment` atau `backup_instance` dengan assertion yang sama → AC-25
- [x] `cargo test -p stackly-storage --test open journal` → `the_resumable_kinds_of_migration_2_match_the_domain` lulus: migrasi 2 membekukan `environment_down` resumable, dan journal menyimpan flag saat dibuat → AC-10
- [x] `cargo test -p stackly-storage` dan `cargo test -p stackly-testkit` → `registry_conformance` lulus pada SQLite dan fake, termasuk lease environment lintas instance urut `acquired_at`, yang `Unreleased` tanpa yang `released`, environment lain tidak ikut, environment tak dikenal kosong, dan baca yang sama dari dalam guard → AC-4
- [x] `cargo test -p stackly-core --test supervisor_start guarded_stop` → 13 test lulus: Down pada instance yang dipegang environment lain selesai `succeeded` dengan item `partial` `skipped` `env.instance_in_use`, tanpa sinyal, tanpa tulisan, lease tetap `active`, dan audit `kept: true`; Down pada instance yang hanya dipegang lease nya sendiri menghentikannya dan melepas lease itu; Down pada instance yang kamu nyalakan sendiri tetap jalan; stop klien tanpa override ditolak `supervisor.stop_requires_override` dengan item `environment` ber `environmentId`; dengan override berhenti, lease tetap `active`, audit `overriddenEnvironments`; lease yang dicatat sesudah cek awal tetap menolak di guard tanpa menghentikan apa pun; stop klien pada instance yang diam tidak meminta override dan lease `startup` yang di suppress guard menerbitkan event autostart; Down `kept` pada instance `stopping` yang prosesnya sudah pergi tetap menjadi `stopped` tanpa menulis desired; instance `recovering` yang Down nya `kept` atau yang stop nya ditolak guard tetap mendapat respawn; restart tidak meminta override; langkah Stop All melewati instance yang diambil environment sesudah pilihan → AC-5, AC-11 (guard), AC-13, AC-14 (restart), AC-16 (guard)
- [ ] `cargo test -p stackly-core --no-fail-fast` → semua test stop, restart, cancel start, Stop All, force stop, crash dan respawn, handover, dan wake yang ada lulus tanpa perubahan assertion → AC-5, AC-13, AC-14
- [ ] `cargo test -p stacklyd --no-fail-fast` → lulus; precheck `instance.stop` menolak dengan item `environment` dari `environment_holders` → AC-13
- [x] `cargo xtask check-deps`, `cargo xtask evidence check`, dan `cargo xtask codegen --check` → lulus tanpa perubahan file hasil generate
- [x] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0

### Catatan `/check verify` 2026-11-13 (sesi `chk-f47-m1`, kode `f4b19921`, langkah Build plan 1 dan 2)

Verdict langkah 1 dan 2: **PASS** untuk AC-4, AC-10, AC-25 (bagian domain dan storage) dan bagian guard AC-5, AC-11, AC-13, AC-14, AC-16. Semua kotak kosong saat checker mulai.

Dua langkah tidak dicentang: suite penuh `stackly-core` dan `stacklyd` tidak diulang (batas disk di brief; lead: 1763 dan 737 lulus). Sebagai gantinya checker menjalankan bagian yang menyentuh jalur stop: `stackly-core --test supervisor_start` seluruhnya (255, termasuk 13 `guarded_stop::` dan semua test stop, lease, restart, respawn, force stop yang lama), `--test stop_all_select --test session_rotation --test supervisor_probe --test supervisor_spool` (8, 16, 3, 18), `stacklyd --test supervisor` (162, 1 ignored), `--test stop_all --test restart_plan --test start_failure --test skeleton` (24, 10, 3, 5), dan `--test environment` (12).

Hasil lain: `stackly-domain --lib` 255; `stackly-core --test operation_engine` 98; `stackly-storage` seluruhnya (33, 78, 55); `stackly-testkit` lulus; `check-deps`, `evidence check`, `codegen --check` (kontrak tidak berubah), dan `cargo check` Windows lulus.

Probe `stacklyd` asli dengan dua instance manual (provider fake): start menulis satu lease `manual` `active` dan stop melepasnya; restart mengganti PID dan lease tetap `active`; proses dibunuh `SIGKILL` dari luar lalu `recovering`, respawn, `restart_total` 1, lease tetap; stop tepat sesudah proses mati berakhir `stopped` dengan desired `stopped`, tidak ada respawn sesudah 6 detik dan tidak ada instance tertinggal `stopping`; stop di dalam jeda respawn (`recovering`) berakhir `stopped` tanpa respawn; `stop --all --yes` menghentikan keduanya ("Stopped 2 services."). Log daemon tanpa `WARN` atau `ERROR` dari jalur ini dan tanpa `SIGKILL`.

Test lama yang diubah hanya berganti contoh: empat test engine memakai `backup_instance` atau `import_environment` menggantikan `environment_down` dengan assertion yang sama; test domain `checks_leases` dihapus bersama method nya (aturannya kini di `LeaseCheck` dan diuji `guarded_stop`); `crates/storage/tests/open/journal.rs` menambah `environment_down` ke pengecualian flag beku migrasi 2. Tidak ada test `supervisor_start` atau test daemon lama yang berubah.

Temuan, rendah atau catatan:

1. Item `supervisor.stop_requires_override` dari precheck `instance.stop` daemon tidak punya test lewat socket; yang teruji hanya guard core dan langkah Stop All. Item baru membawa `environmentId` saja; AC-13 (`name`, `alias`, `ownership`) belum terpenuhi sampai langkah 4, seperti yang ditandai `// provisional: F47`.
2. `env.instance_in_use` sudah dihasilkan core tetapi belum ada di katalog protokol (langkah 4).
3. Tiga tanda `// provisional: F47` (`error.rs`, `stop_all.rs`, `stop.rs`) harus hilang di langkah 4 dan 6.

### Langkah 3 sampai 5: UI / manual

- [ ] Buka `crates/core/src/environment/down/select.rs` → `select` menerapkan enam baris *Pilihan Down* berurutan (retained, `evaluate_stop(EnvironmentDown)` keep, `unknown` blocked, idle, `needs_recovery` blocked, stop), tiap daftar urut nama lalu ID; `read_view` membaca member (termasuk `detached`) ditambah instance ber lease `active` environment itu, dengan baca ulang sampai generation sama → AC-1
- [ ] Buka `crates/core/src/environment/down/mod.rs` → guard `release` melepas setiap lease `active` environment itu lewat `GuardView::leases_of_environment` dan menulis `Environment::begin_down` dalam satu `commit_guarded`; environment yang hilang menolak `env.not_found` tanpa menulis → AC-4
- [ ] Buka `crates/core/src/environment/down/driver.rs` → anak dimulai lewat port `DownChildren` dalam urutan `down::order` (paling banyak `DOWN_STOP_CONCURRENCY` 4), tiap anak didahului `select` atas view satu instance, stop yang berjalan tidak pernah dibatalkan, anak yang antre lebih dari `CHILD_QUEUE_TIMEOUT_SECONDS` dibatalkan dan diputuskan sapuan akhir, cancel menghentikan anak baru dan melewati sapuan → AC-5, AC-6, AC-8, AC-9
- [ ] Buka `apps/daemon/src/ipc/env/down.rs` → `env.down` menjalankan replay, `read_view`, gerbang `begin` yang menolak `env.down_changed` saat himpunan `stop` beda dari `expectedStopInstanceIds`, `BusyPolicy::Queue` dengan lock `EnvironmentName`, lalu task terpisah yang membatalkan setiap `environment_up` yang belum berakhir sebelum balasan dikirim → AC-2, AC-3
- [ ] Buka `apps/daemon/src/ipc/instance/mod.rs` → `down_stop_check` dan `down_stop_request` (*Precheck stop anak*: ditolak, `Kept`, `AlreadyStopped`, atau `Proceed`; `BusyPolicy::Queue`; input `environment_down`) dan `apps/daemon/src/ipc/env/children.rs` mengimplementasikan `DownChildren` di atasnya → AC-5
- [ ] Buka `apps/daemon/src/membership.rs` → `leaseActive` tiap entri dibaca dari lease `active` environment bersama baris environment; buku dimuat ulang saat Up atau Down berakhir → AC-12

### Langkah 3 sampai 5: Commands

- [ ] `cargo test -p stackly-core --lib environment::down` → 18 test lulus: tabel `select` (lease environment lain, `manual`, `startup` membuat `keep`; lease `suppressed` bukan demand; keep menang atas blocked; `unknown` dan `needs_recovery` blocked; stopped dengan desired `running` atau start terbuka masuk `stop`), `order` (rantai, berlian, dependent di luar himpunan, dependent yang sudah berakhir, kapasitas 4, instance tanpa member terakhir, edge rusak), dan `outcome_of` (`env.down_incomplete` dengan `stopped`, `total`, item `instance`) → AC-1, AC-6, AC-8
- [ ] `cargo test -p stackly-core --test environment down` → 10 test lulus: `read_view` dengan lease tanpa member, `release` yang hanya melepas lease environment itu dan mengosongkan `last_up` tanpa menyentuh member atau baris instance, urutan stop terbalik pada berlian dengan dependent yang gagal, stop yang guard nya `kept` memberi item `partial` `env.instance_in_use`, instance di luar himpunan yang dikonfirmasi dan instance `kept` yang kehilangan demand dilaporkan `left_running` `env.down_changed`, penolakan port dan instance idle, Down tanpa yang dihentikan, guard pada environment yang hilang, cancel sesudah `release` yang menunggu stop berjalan, dan cancel saat antre yang tidak melepas lease → AC-1, AC-4, AC-5, AC-6, AC-7, AC-8, AC-9
- [ ] `cargo build -p stacklyd -p stackly-fake-server && cargo test -p stacklyd --test env_down` → 7 test lulus: PRD AC-022 (stop tanpa override ditolak dengan item `environment` ber `name`, `alias`, `ownership`; Down shop menjaga `shop-db` untuk blog dengan PID sama dan `ready`; Down blog menghentikannya; data, baris, dan pohon file tetap), penolakan `env.down_changed` dan `env.not_found` tanpa tulisan, replay key, Down tanpa manifest dan tanpa lease, Down selagi Up (Up `cancelled`, Up baru `resource.busy`), stop yang timeout memberi `env.down_incomplete` lalu Down kedua selesai, restart daemon di tengah Down, klien putus, dan Up alpha serentak dengan Down beta 50 kali → AC-2, AC-3, AC-4, AC-5, AC-7, AC-8, AC-10, AC-11, AC-12, AC-13
- [ ] `cargo test -p stackly-protocol --no-fail-fast` → lulus, termasuk fixture `protocols/fixtures/env/down-preview.json` dan fixture IPC `env.preview_down`, `env.down`, `env.down_incomplete`, dan penolakan ID ganda → AC-25
- [ ] `cargo xtask codegen --check` dan `cargo xtask fuzz seed` → tanpa perubahan file; `protocols/contract-versions.json` mencatat 4.5 → AC-25
- [ ] `swift test --package-path clients/swift` dan `swift build --package-path apps/macos/Packages/StacklyUI --build-tests` → lulus dengan kontrak 4.5 → AC-25

### Acceptance-criteria coverage

- AC-4 · bagian domain dan storage: `begin_down`, `leases_of_environment` di registry, guard, fake, dan conformance; commit `release` sendiri di langkah 3
- AC-5 · guard stop ber `LeaseCheck::EnvironmentDown`, hasil `kept` yang eksplisit, `cancel_respawn` sesudah commit, `StopRequest::environment_down`, `ENVIRONMENT_DOWN_INPUT`; anak `stop_instance` dari driver dan test PRD AC-022 lewat socket di langkah 3 dan 5
- AC-10 · kebijakan `CloseInterrupted` dan penutupan anak stop yang antre oleh aturan yang ada; restart daemon di tengah Down lewat socket di langkah 5
- AC-11 · hanya bagian guard (lease yang muncul sebelum commit stop menahan stop); balapan Up dan Down 50 kali di langkah 5
- AC-13 · precheck dan guard, `EnvironmentHolder` dan item `environment`, audit `overriddenEnvironments`. Item hanya membawa `environmentId` sampai katalog protokol mendapat `name`, `alias`, dan `ownership` di langkah 4 (`// provisional: F47` di `crates/core/src/supervisor/error.rs`); rotasi sesi sesudah override diuji di langkah 5
- AC-14 · restart, start yang dibatalkan, rekonsiliasi, dan restart terencana memakai `LeaseCheck::None`; reload, apply config, rename, dan force stop tidak memanggil jalur stop ber guard
- AC-16 · hanya guard langkah Stop All dengan `override_leases` false; `StopAllOptions` dan `overrideLeases` di langkah 6
- AC-25 · bagian domain (`EnvironmentDown` tidak resumable, `CloseInterrupted`, pengatur); kontrak, fixture, dan kenaikan protokol di langkah 4
- AC-1, AC-2, AC-3, AC-6 sampai AC-9, AC-12 · langkah 3 sampai 5 (lihat dua bagian di atas); `leaseActive` dimuat ulang saat Up atau Down berakhir, belum pada commit `accept` atau `release` itu sendiri
- AC-25 · kontrak Down di protokol 4.5 (langkah 4); tambahan `stop_all`, `daemon`, dan `change_endpoints` menunggu langkah 6
- AC-15, AC-16 (pilihan override), AC-17 sampai AC-24, AC-26 · belum, milik langkah 6 sampai 10
