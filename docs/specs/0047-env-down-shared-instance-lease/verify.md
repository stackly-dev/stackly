# Verify: Env down, shared instance & lease · spec 0047 · updated 2026-11-13

_Langkah diturunkan dari acceptance criteria spec 0047. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27, dengan `export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`._

## Milestone 1: Down di daemon

Langkah Build plan 1 dan 2 sudah dibangun: kebijakan `environment_down`, `Environment::begin_down`, `leases_of_environment`, lalu stop ber guard lease. Langkah 3 sampai 5 (`environment::down`, protokol, handler `env.*`) menunggu spec 0046 milestone 2 dan menambah bagiannya di sini. Protokol dan migrasi tidak berubah di langkah 1 dan 2.

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

### Acceptance-criteria coverage

- AC-4 · bagian domain dan storage: `begin_down`, `leases_of_environment` di registry, guard, fake, dan conformance; commit `release` sendiri di langkah 3
- AC-5 · guard stop ber `LeaseCheck::EnvironmentDown`, hasil `kept` yang eksplisit, `cancel_respawn` sesudah commit, `StopRequest::environment_down`, `ENVIRONMENT_DOWN_INPUT`; anak `stop_instance` dari driver dan test PRD AC-022 lewat socket di langkah 3 dan 5
- AC-10 · kebijakan `CloseInterrupted` dan penutupan anak stop yang antre oleh aturan yang ada; restart daemon di tengah Down lewat socket di langkah 5
- AC-11 · hanya bagian guard (lease yang muncul sebelum commit stop menahan stop); balapan Up dan Down 50 kali di langkah 5
- AC-13 · precheck dan guard, `EnvironmentHolder` dan item `environment`, audit `overriddenEnvironments`. Item hanya membawa `environmentId` sampai katalog protokol mendapat `name`, `alias`, dan `ownership` di langkah 4 (`// provisional: F47` di `crates/core/src/supervisor/error.rs`); rotasi sesi sesudah override diuji di langkah 5
- AC-14 · restart, start yang dibatalkan, rekonsiliasi, dan restart terencana memakai `LeaseCheck::None`; reload, apply config, rename, dan force stop tidak memanggil jalur stop ber guard
- AC-16 · hanya guard langkah Stop All dengan `override_leases` false; `StopAllOptions` dan `overrideLeases` di langkah 6
- AC-25 · bagian domain (`EnvironmentDown` tidak resumable, `CloseInterrupted`, pengatur); kontrak, fixture, dan kenaikan protokol di langkah 4
- AC-1, AC-2, AC-3 (handler), AC-6 sampai AC-9, AC-12, AC-15, AC-17 sampai AC-24, AC-26 · belum, milik langkah 3 sampai 10
