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

- [x] Buka `crates/core/src/environment/down/select.rs` → `select` menerapkan enam baris *Pilihan Down* berurutan (retained, `evaluate_stop(EnvironmentDown)` keep, `unknown` blocked, idle, `needs_recovery` blocked, stop), tiap daftar urut nama lalu ID; `read_view` membaca member (termasuk `detached`) ditambah instance ber lease `active` environment itu, dengan baca ulang sampai generation sama → AC-1
- [x] Buka `crates/core/src/environment/down/mod.rs` → guard `release` melepas setiap lease `active` environment itu lewat `GuardView::leases_of_environment` dan menulis `Environment::begin_down` dalam satu `commit_guarded`; environment yang hilang menolak `env.not_found` tanpa menulis → AC-4
- [x] Buka `crates/core/src/environment/down/driver.rs` → anak dimulai lewat port `DownChildren` dalam urutan `down::order` (paling banyak `DOWN_STOP_CONCURRENCY` 4), tiap anak didahului `select` atas view satu instance, stop yang berjalan tidak pernah dibatalkan, anak yang antre lebih dari `CHILD_QUEUE_TIMEOUT_SECONDS` dibatalkan dan diputuskan sapuan akhir, cancel menghentikan anak baru dan melewati sapuan → AC-5, AC-6, AC-8, AC-9
- [x] Buka `apps/daemon/src/ipc/env/down.rs` → `env.down` menjalankan replay, `read_view`, gerbang `begin` yang menolak `env.down_changed` saat himpunan `stop` beda dari `expectedStopInstanceIds`, `BusyPolicy::Queue` dengan lock `EnvironmentName`, lalu task terpisah yang membatalkan setiap `environment_up` yang belum berakhir sebelum balasan dikirim → AC-2, AC-3
- [x] Buka `apps/daemon/src/ipc/instance/mod.rs` → `down_stop_check` dan `down_stop_request` (*Precheck stop anak*: ditolak, `Kept`, `AlreadyStopped`, atau `Proceed`; `BusyPolicy::Queue`; input `environment_down`) dan `apps/daemon/src/ipc/env/children.rs` mengimplementasikan `DownChildren` di atasnya → AC-5
- [x] Buka `apps/daemon/src/membership.rs` → `leaseActive` tiap entri dibaca dari lease `active` environment bersama baris environment; buku dimuat ulang saat Up atau Down berakhir → AC-12

### Langkah 3 sampai 5: Commands

- [x] `cargo test -p stackly-core --lib environment::down` → 18 test lulus: tabel `select` (lease environment lain, `manual`, `startup` membuat `keep`; lease `suppressed` bukan demand; keep menang atas blocked; `unknown` dan `needs_recovery` blocked; stopped dengan desired `running` atau start terbuka masuk `stop`), `order` (rantai, berlian, dependent di luar himpunan, dependent yang sudah berakhir, kapasitas 4, instance tanpa member terakhir, edge rusak), dan `outcome_of` (`env.down_incomplete` dengan `stopped`, `total`, item `instance`) → AC-1, AC-6, AC-8
- [x] `cargo test -p stackly-core --test environment down` → 10 test lulus: `read_view` dengan lease tanpa member, `release` yang hanya melepas lease environment itu dan mengosongkan `last_up` tanpa menyentuh member atau baris instance, urutan stop terbalik pada berlian dengan dependent yang gagal, stop yang guard nya `kept` memberi item `partial` `env.instance_in_use`, instance di luar himpunan yang dikonfirmasi dan instance `kept` yang kehilangan demand dilaporkan `left_running` `env.down_changed`, penolakan port dan instance idle, Down tanpa yang dihentikan, guard pada environment yang hilang, cancel sesudah `release` yang menunggu stop berjalan, dan cancel saat antre yang tidak melepas lease → AC-1, AC-4, AC-5, AC-6, AC-7, AC-8, AC-9
- [x] `cargo build -p stacklyd -p stackly-fake-server && cargo test -p stacklyd --test env_down` → 7 test lulus: PRD AC-022 (stop tanpa override ditolak dengan item `environment` ber `name`, `alias`, `ownership`; Down shop menjaga `shop-db` untuk blog dengan PID sama dan `ready`; Down blog menghentikannya; data, baris, dan pohon file tetap), penolakan `env.down_changed` dan `env.not_found` tanpa tulisan, replay key, Down tanpa manifest dan tanpa lease, Down selagi Up (Up `cancelled`, Up baru `resource.busy`), stop yang timeout memberi `env.down_incomplete` lalu Down kedua selesai, restart daemon di tengah Down, klien putus, dan Up alpha serentak dengan Down beta 50 kali → AC-2, AC-3, AC-4, AC-5, AC-7, AC-8, AC-10, AC-11, AC-12, AC-13
- [x] `cargo test -p stackly-protocol --no-fail-fast` → lulus, termasuk fixture `protocols/fixtures/env/down-preview.json` dan fixture IPC `env.preview_down`, `env.down`, `env.down_incomplete`, dan penolakan ID ganda → AC-25
- [x] `cargo xtask codegen --check` dan `cargo xtask fuzz seed` → tanpa perubahan file; `protocols/contract-versions.json` mencatat 4.5 → AC-25
- [x] `swift test --package-path clients/swift` dan `swift build --package-path apps/macos/Packages/StacklyUI --build-tests` → lulus dengan kontrak 4.5 → AC-25

### Catatan `/check verify` 2026-11-14 (sesi checker, kode `31b25de2`, langkah Build plan 3 sampai 5)

Verdict langkah 3 sampai 5: **PASS** untuk AC-1 sampai AC-12 dan AC-25 (bagian protokol 4.5). Dua belas kotak di atas dijalankan atau dibaca checker dan lulus.

**Hasil perintah.** `stackly-core --lib environment::down` 18 lulus; `--test environment` 109 lulus (`down::` 10); `stacklyd --test env_down` 7 lulus, test 50 putaran diulang tiga kali lagi dan test restart daemon di tengah Down sepuluh kali lagi, semuanya lulus; `stackly-protocol` lulus; `codegen --check` 26 file mutakhir dengan kontrak 4.5 tercatat; `fuzz seed` tanpa perubahan; `check-deps` dan `evidence check` lulus; `swift test` klien 285 lulus dan test StacklyUI terbangun dengan kontrak 4.5.

**Probe `stacklyd` asli** (provider `fake`, test probe sementara yang sudah dicabut): shop membuat `shop-db`, `shop-api`, `shop-web` berantai; blog meminjam `shop-db`.

| Probe | Hasil |
| --- | --- |
| `instance.stop` tanpa override pada `shop-db` | `supervisor.stop_requires_override`, dua item `environment` ber `environmentId`, `name`, `alias`, `ownership` (`blog` `shared`, `shop` `dedicated`) → AC-13 |
| `env.preview_down` shop | `stop` `shop-api`, `shop-web`; `keep` `shop-db` dengan demand `environment` dan pemegang `blog`; `leaseCount` 3 → AC-1 |
| `env.down` dengan himpunan lain | `env.down_changed` dengan hint; jumlah baris dan byte file sama → AC-2 |
| Down shop | `succeeded`; `shop-web` berhenti sebelum `shop-api` (urutan terbalik); `shop-db` tidak disentuh: PID sama, `ready`, `restart_total`, endpoint, revision, dan config revision sama; ketiga lease shop `released`, lease blog `active` → AC-4, AC-5, AC-6, PRD AC-022 |
| Sesudah Down shop | `env.get` shop `lease.activeCount` 0, blog 1; summary instance langsung `leaseActive` false untuk shop dan true untuk blog → AC-12 |
| Pohon file dan baris | byte setiap file di luar log, `run/`, dan registry sama sebelum dan sesudah kedua Down; jumlah baris `instances`, `runtimes`, member, dan `environments` sama → AC-7 |
| Down shop lagi | `succeeded` tanpa yang dihentikan (`shop-api`, `shop-web` `idle`) → AC-2 |
| Down blog | `shop-db` masuk `stop`, lalu `stopped` dengan desired `stopped` → AC-5 |

**Probe native** (NATS 2.15.1 asli dari cache xtask, tanpa unduhan; probe, bukan record evidence): alpha membuat `alpha-bus`, beta meminjamnya. Down alpha: `keep` untuk beta, PID sama, `ready`, dan server tetap menjawab koneksi TCP baru dengan baris `INFO` ber `server_id` yang sama; lease beta `active`. Down beta: server berhenti dan port menolak koneksi. Dua belas file data dan runtime sama sebelum dan sesudah.

**Tafsiran developer.** (1) Item `partial` `env.instance_in_use` hanya untuk instance himpunan `stop` yang berakhir `kept`: diterima, sesuai AC-8. (2) `DownKeep.environments` hanya pemegang yang punya baris member, sisanya tetap di `demand`: diterima. (3) `leaseActive` dimuat ulang saat Up atau Down berakhir: diterima dengan catatan 1. (4) Kontrak `stop_all`, `agent_off`, `change_endpoints` menunggu 4.6: diterima. (5) Fake `DownChildren` di test core: diterima.

**Yang dibaca di kode.** `select` menerapkan enam aturan berurutan dan dipakai preview, precheck, driver, dan sapuan akhir; guard `release` satu `commit_guarded`; driver menjalankan `select` atas satu instance sebelum tiap anak, tidak pernah membatalkan stop yang berjalan, dan abandon saat daemon berhenti; Down yang dibatalkan saat antre tidak melepas lease. Tanda `// provisional: F47` tinggal satu, di `stop_all.rs` untuk langkah 6.

**A1 dan A2.** K27 spec 0044 (`provider_single_instance`): kode dan test sesuai teks spec; dicatat di verify spec 0044. K22 spec 0045: dibuktikan lewat probe CLI terhadap binary daemon; dicatat di verify spec 0045.

Temuan, rendah:

1. Selama Up atau Down masih berjalan (sesudah `accept` atau `release`, sebelum operasi berakhir), `leaseActive` di summary instance bisa tertinggal sebentar karena buku keanggotaan baru dimuat ulang saat operasi berakhir. `env.get.lease` selalu segar. AC-12 menulis keduanya "dibaca dari registry"; nilainya benar begitu operasi selesai.
2. Sesudah Down shop, `env.get` shop berstatus `partially_ready` (satu dari tiga `ready`) karena `shop-db` masih jalan untuk blog. Benar menurut aturan status, tetapi layar Down (langkah 9) perlu menjelaskannya.

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

## Milestone 2: Override pada aksi lain

Langkah Build plan 6: `StopAllOptions` di `stop_all::select`, `override_leases` di `StopAllRequest`, input journal, dan pemeriksaan ulang `stop_all_step`; `overrideLeases` di `instance.preview_stop_all`, `instance.stop_all`, dan `daemon.shutdown` `agent_off`; `environments` dan `affectedEnvironments` di preview; `overrideLeases` dan `EndpointValidation.leases` di `instance.change_endpoints` dengan cek ulang di guard commit. Protokol naik ke 4.6 (minor). Tidak ada migrasi, tidak ada `EventKind` baru.

### UI / manual

- [x] Buka `crates/core/src/instance/stop_all.rs` → `select(view, StopAllOptions)`: aturan 6 hanya berlaku saat `override_leases` false; `StopAllEntry` dan `StopAllSkipped` membawa `environments` (environment dari lease `active`, tanpa ganda); aturan 1 sampai 5 tidak bergantung pada override → AC-16
- [x] Buka `crates/core/src/supervisor/stop_all.rs` → `StopAllRequest::overriding_leases`; `stop_all_step` meneruskan nilai yang sama sebagai `StopAllOptions` ke `select` nya dan sebagai `LeaseCheck::Manual { override_leases }` ke `stop_direct`; tanda `// provisional: F47` sudah tidak ada → AC-16, AC-17
- [x] Buka `apps/daemon/src/ipc/stop_all.rs` → preview membaca `StopAllPreviewParams.overrideLeases`; `environments` tiap item dan skip berasal dari lease `active` di view dan baris member environment (environment tanpa baris member tidak ditulis), `affectedEnvironments` menghitung instance di `items` per environment, urut nama; `instance.stop_all` memakai `overrideLeases` di `select`, di `StopAllRequest`, dan di input journal (`overrideLeases`), dan aturan `expectedInstanceIds` tetap → AC-16
- [x] Buka `apps/daemon/src/ipc/agent_off.rs` → `ShutdownParams.overrideLeases` menjadi `StopAllOptions` yang dipakai di pemanggilan `current_selection` awal dan akhir dan di `StopAllRequest`; `agent_off` tanpa options sama dengan `override_leases` false → AC-17
- [x] Buka `apps/daemon/src/ipc/instance/config.rs` dan `crates/core/src/supervisor/change.rs` → dry run `instance.change_endpoints` tidak pernah ditolak karena lease dan mengisi `leases`; permintaan yang memindahkan port (`plan.endpoint_changes` tidak kosong) tanpa override ditolak `instance.endpoints_require_override` sebelum operasi dibuat; guard commit `change_endpoints` membaca lease lagi di dalam transaksi, dan dengan override menulis catatan audit `overriddenEnvironments`; `apply_config` tidak membaca lease → AC-14, AC-15
- [x] Buka `crates/protocol/src/message.rs` dan `json.rs` → `instance.preview_stop_all` adalah varian ber params dengan `params optional`: params yang tidak ada, `null`, atau `{}` dibaca sebagai default, jadi klien sebelum 4.6 tetap sah → AC-16, AC-25

### Commands

- [x] `cargo test -p stackly-protocol --no-fail-fast` → lulus, termasuk fixture baru di `protocols/fixtures/ipc/index.json`: `request-instance-preview-stop-all.json` (tanpa params, `decode`), `-null`, `-empty`, `-override`, `request-instance-stop-all-override.json`, `request-shutdown-agent-off-override.json`, `request-instance-change-endpoints-override.json`, `frame-error-instance-endpoints-require-override.json`, dan dua fixture `-before-4-6` (balasan daemon lama tanpa field baru, `decode`) → AC-15, AC-16, AC-17, AC-25
- [x] `cargo test -p stackly-core --test stop_all_select` → 18 lulus; `the_override_moves_an_instance_an_environment_holds_from_skipped_to_stop` memeriksa tabel aturan 6 dengan override true dan false, aturan 1 sampai 5 tetap, `demand` memuat `environment`, `environments` satu entri per environment; lease `released` tidak dihitung → AC-16
- [x] `cargo test -p stackly-core --test supervisor_start guarded_stop` → 18 lulus; langkah Stop All dengan override menghentikan instance ber lease (juga lease yang dicatat sesudah pilihan), lease tetap `active`; ganti port yang bertemu lease di guard gagal `instance.endpoints_require_override` dengan item `environment` tanpa revision baru; dengan override berjalan dan audit `overriddenEnvironments` = `shop`; apply config pada instance ber lease tidak meminta override → AC-14, AC-15, AC-16
- [x] `cargo test -p stacklyd --test supervisor lease_override` → 5 lulus: preview tanpa dan dengan override (skip, `environments`, `affectedEnvironments`), `expectedInstanceIds` dari preview override tanpa override ditolak `instance.stop_all_changed` tanpa operasi, Stop All dengan override menghentikan keduanya dan `leaseActive` tetap; Stop All tanpa override melewati instance ber lease; `agent_off` dengan override menghentikan semuanya dan daemon keluar; `daemon.shutdown` lewat socket membawa `overrideLeases` ke `agent_off`; ganti port tanpa override ditolak dengan `count`, hint, dan item, `dryRun` membalas `leases`, permintaan sama dengan sekarang `unchanged`, dengan override berjalan dan audit memuat nama environment → AC-15, AC-16, AC-17
- [ ] `cargo test -p stackly-core --no-fail-fast` dan `cargo test -p stacklyd --no-fail-fast` → lulus; test stop, restart, dan Stop All lama tanpa perubahan assertion (perubahan test lama hanya argumen `StopAllOptions::default()` di `stop_all_select.rs`, field default baru di struct literal, dan `InstancePreviewStopAll(Default::default())`) → AC-14
- [ ] `cargo test -p stackly-cli --no-fail-fast`, `-p stackly-domain`, `-p stackly-storage`, `-p stackly-testkit` → lulus
- [x] `swift test --package-path clients/swift` dan `swift build --package-path apps/macos/Packages/StacklyUI --build-tests` → lulus; perubahan Swift hanya nilai bawaan `nil` untuk field baru dan versi 4.6 di `ContractTests` → AC-25
- [x] `cargo xtask check-deps`, `cargo xtask evidence check`, `cargo xtask codegen --check`, `cargo xtask fuzz seed` (tanpa diff), dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus

### Catatan `/check verify` 2026-11-14 (sesi `chk-f47-m2`, kode `6a41187d`)

Verdict Milestone 2: **PASS** untuk AC-14, AC-15, AC-16, AC-17, dan bagian kontrak AC-25 untuk protokol 4.6. Dua belas kotak dijalankan atau dibaca checker dan lulus. Dua kotak suite penuh (`stackly-core` dan `stacklyd`; `stackly-cli`, `stackly-domain`, `stackly-storage`, `stackly-testkit`) tidak diulang; gerbang lead pada `6a41187d` mencatat suite kecil 1140, `stackly-core` 1881, `stacklyd` 778, `stackly-cli` 678 tanpa gagal.

**Hasil perintah.** `stackly-protocol` lulus (241 di lib, fixture baru ikut); `stop_all_select` 18; `supervisor_start guarded_stop` 18; `stacklyd --test supervisor lease_override` 5; test lama `stacklyd --test stop_all` 5 dan `--test supervisor stop_all` 5 tetap lulus; `stacklyd --test environment` 18; `codegen --check` 26 file mutakhir dengan kontrak 4.6 tercatat; `fuzz seed` tanpa perubahan; `check-deps`, `evidence check`, dan cek Windows lulus; `swift test` klien 285 lulus dan test StacklyUI terbangun. Tanda `// provisional: F47` sudah tidak ada di `crates` dan `apps`.

**Probe `stacklyd` asli** (provider `fake`, lease dari Up sungguhan, test probe sementara yang sudah dicabut): shop membuat `shop-db` dan `shop-api`, blog meminjam `shop-db`, dan `solo` adalah instance pengguna.

| Probe | Hasil |
| --- | --- |
| `instance.preview_stop_all` bawaan | `items` hanya `solo`; `shop-api` dan `shop-db` `skipped` dengan `supervisor.stop_requires_override` dan nama environment pemegang (`shop`; `blog`, `shop`); `affectedEnvironments` kosong → AC-16 |
| Preview dengan `overrideLeases` | ketiganya di `items` dengan `environments`; `affectedEnvironments` `blog` 1 dan `shop` 2, urut nama → AC-16 |
| `instance.stop_all` dengan himpunan preview override tanpa override, dan sebaliknya | keduanya `instance.stop_all_changed`, nol operasi baru → AC-16 |
| `instance.restart` dan `instance.reload` pada `shop-db` | berhasil tanpa field override; endpoint sama; lease environment tetap `active` → AC-14 |
| `instance.change_endpoints` `dryRun` | tidak ditolak; `leases` berisi `blog` (`shared`) dan `shop` (`dedicated`) → AC-15 |
| Ganti port tanpa override | `instance.endpoints_require_override` dengan `count` 2, hint, dan dua item `environment` ber `name`, `alias`, `ownership`; nol operasi, port tetap → AC-15 |
| `auto` ke `auto`; `fixed` pada port yang sedang dipakai endpoint `auto` | yang pertama `unchanged` tanpa cek lease; yang kedua dihitung perubahan dan ditolak tanpa override → AC-15 |
| Ganti port dengan override | `succeeded`, port pindah, instance `running`, semua lease tetap `active` → AC-15 |
| `instance.stop_all` dengan override | tiga instance berhenti; lease environment tetap `active`, lease `manual` `released`; shop `down` dengan `lease.activeCount` 2 dan alasan `stopped` → AC-16 |
| Sesudah override | start manual `shop-db` menulis lease `manual` baru, lease environment tetap; Up shop berikutnya menyalakan `shop-api` dan shop kembali `ready` |

`agent_off` dengan override (AC-17) dan catatan audit `overriddenEnvironments` terbukti lewat test developer `lease_override` yang saya jalankan, tidak lewat probe sendiri. Klien sebelum 4.6 terbukti lewat fixture protokol: preview tanpa params, `null`, dan `{}` terbaca sebagai bawaan, dan balasan `-before-4-6` tanpa field baru terbaca. Probe native tidak dijalankan (opsional).

**Tafsiran developer.** (1) `environments` dari lease `active` dan baris member, environment tanpa baris member hanya di `demand`, `affectedEnvironments` dari `items`, preview dengan `params optional`: diterima. (2) "Benar benar mengubah port" berarti `plan.endpoint_changes` tidak kosong, termasuk `fixed` pada port yang sedang dipakai endpoint `auto`: diterima, sesuai bunyi *Override pada aksi lain* ("`port_request` nya berbeda dari yang aktif"). (3) `agent_off` tetap tanpa override dan `agent_off_with` dipakai `daemon.shutdown`: diterima.

**Tiga fix yang ikut.** Blocker `env.lock` kini menyebut provider ("official/valkey has no version matching 8") dan hint `env.lock_no_artifact` serta `env.lock_artifact_unknown` menyebut target ("this Mac (macos-aarch64)"); keduanya terlihat lewat `stackly env lock` dan `stackly env plan --locked` terhadap binary daemon. `differences` runtime: selector line `2.15` terhadap `2.15.1` tidak berbeda, exact lain berbeda, line lain berbeda, `stable` tidak pernah; empat test `edges::` lulus di `stackly-core --test environment` (122).

Temuan, rendah:

1. Restart oleh pengguna pada instance yang dipegang environment menulis lease `manual` (aturan spec 0031). Akibatnya Down environment itu sesudahnya membiarkan instance tetap jalan (`keep`, demand `manual`). Benar menurut aturan, tetapi layar Down perlu menjelaskannya.
2. `affectedEnvironments` menghitung setiap environment pemegang lease, sedangkan `environments` pada item hanya memuat yang punya baris member; pemegang tanpa member (keadaan langka) bisa muncul di yang pertama tetapi tidak di yang kedua.
3. View Stop All dan daftar environment dibaca dalam dua pembacaan terpisah; hanya memengaruhi teks preview.

### Acceptance-criteria coverage

- AC-14: guard `change_endpoints` hanya untuk ganti port; apply config, restart, reload, rename, dan force stop tidak menerima field override (restart dan force stop teruji di milestone 1 langkah 2)
- AC-15: daemon dan core lengkap; banner dan "Change Anyway" di sheet port menunggu milestone 4 (AC-23)
- AC-16, AC-17: daemon dan core lengkap; flag `--override-leases` CLI menunggu milestone 3 (AC-19), popover dan Turn Off menunggu milestone 4 (AC-24)
- AC-25 (bagian kontrak langkah 6): protokol 4.6, fixture, Swift hasil generate

## Milestone 4: Down, lease, dan override di app

Langkah Build plan 8 sampai 10 sudah dibangun di atas protokol 4.6: Down di halaman environment, baris Lease, "· lease" di inspector, konfirmasi override untuk semua permukaan Stop, banner dan "Change Anyway" di sheet port, dampak environment di popover Stop All dan konfirmasi Turn Off (keduanya memakai `overrideLeases` true), dan test native `apps/daemon/tests/env_down_native.rs`.

Tafsiran developer: *Copy Down* tidak punya kalimat untuk environment yang masih `partially_ready` sesudah Down karena instance bersama tetap jalan. Kartu umpan balik Down yang menjaga instance memakai detail "Keeps running: {nama} ({alasan}), …" dari preview yang dikonfirmasi (kalimat konfirmasi yang sama), lalu kalimat baris pertama konfirmasi "Shared and manually started instances keep running." Konfirmasi Turn Off menulis instance ber lease sebagai "These stop too: {nama} · Used by {environment}." sebelum kalimat spec "Environments keep their leases. Bring them up again after you turn Stackly back on."

### UI / manual

Langkah GUI nyata ini hanya dijalankan kamu. Prasyarat: `cargo build -p stacklyd`, `cargo xtask build-app`, daemon jalan dari build itu, dan dua manifest contoh di folder sendiri: `shop/stackly.yaml` yang membuat instance `shop-db` (provider apa saja dari catalog, misalnya `official/valkey`) dan `blog/stackly.yaml` yang meminjamnya lewat `instanceRef: shop-db`.

- [ ] Import kedua manifest lewat Environments, lalu Up… dan Bring Up pada shop, lalu pada blog → keduanya Ready; baris Lease di Details keduanya "Lease held on 1 instance since {jam}. Down stops only instances with no other lease or manual start."; inspector `shop-db` di Instances menulis "Dedicated · lease" → AC-22
- [ ] Di halaman shop tekan Down (di kiri Up…) → konfirmasi "Bring down “shop”?" dengan baris "Releases the lease and stops only instances nothing else needs. Shared and manually started instances keep running. Data is kept.", "Nothing needs to stop.", dan "Keeps running: shop-db (used by blog)."; tombol Cancel dan Bring Down → AC-20
- [ ] Tekan Bring Down → banner "Bringing down…" sebentar, sidebar dan daftar menulis "Bringing down…", lalu kartu "shop is down. 0 stopped, 1 kept running." dengan detail "Keeps running: shop-db (used by blog). Shared and manually started instances keep running."; `shop-db` tetap Running dengan PID yang sama; baris Lease shop "No lease. Up takes one on each service’s instance."; Operations menulis "Bring down environment shop" → AC-20, AC-21, AC-22
- [ ] Up shop lagi, lalu Down blog → konfirmasi blog menyebut `shop-db` di "Keeps running: shop-db (used by shop)."; Bring Down; lalu Down shop → konfirmasi "Stops: shop-db."; Bring Down → kartu "shop is down. 1 instance stopped. Data is kept."; `shop-db` Stopped → AC-20, AC-21
- [ ] Down blog lalu Up shop, lalu Restart `shop-db` dari halaman instance (restart pengguna menulis lease `manual`), lalu Down shop → konfirmasi "Keeps running: shop-db (started by you)."; Bring Down → kartu "shop is down. 0 stopped, 1 kept running." dengan detail "Keeps running: shop-db (started by you). Shared and manually started instances keep running."; `shop-db` tetap Running; Stop `shop-db` sesudahnya untuk langkah berikut → AC-20, AC-21
- [ ] Up shop, tekan Down, Bring Down, lalu tutup app (Quit) tepat saat banner "Bringing down… 0 of 1" terlihat, buka lagi → halaman shop menampilkan banner "Bringing down…" bila Down masih berjalan, atau status Down tanpa banner bila sudah selesai; Operations menunjukkan Down dengan anak `Stop shop-db` bersarang; tidak ada kartu hasil (kartu hanya untuk Down yang dikirim jendela itu) → AC-12, AC-21
- [ ] Down saat tidak ada yang perlu berhenti dan tidak ada lease (Down shop sekali lagi) → tidak ada konfirmasi, kartu "Nothing to bring down." → AC-20
- [ ] Remove Environment… shop saat lease masih aktif → kartu "Bring it down first." dengan tombol Bring Down…, yang membuka konfirmasi Down → AC-20
- [ ] Up shop, lalu Stop `shop-db` dari halaman instance, dari baris daftar Instances, dari menu baris service di halaman shop, dan dari baris menu bar → setiap kali dialog destructive "Stop “shop-db”?" dengan "It’s leased by shop. Stopping it leaves the environment partially ready. Data is kept." dan tombol Cancel (default) dan Stop; Cancel tidak mengirim apa pun; Stop menghentikannya, shop menjadi Partially Ready atau Down, dan lease tetap (baris Lease tetap "held on 1 instance") → AC-23
- [ ] Up shop, lalu Stop All Services… dari menu bar → baris `shop-db` "Used by shop" dan kalimat "shop will no longer be complete. Its lease stays."; Stop All menghentikannya; baris Lease shop tetap "held on 1 instance" → AC-24
- [ ] Up shop, lalu Diagnostics → Turn Off Background Agent… → konfirmasi tidak nonaktif karena lease, menulis "These stop too: shop-db · Used by shop. Environments keep their leases. Bring them up again after you turn Stackly back on."; tekan Cancel (jangan matikan agent kecuali kamu memang mau) → AC-24
- [ ] Up shop, lalu buka `shop-db` → Configuration → Edit Ports, ketik port bebas lain → banner caution "shop uses this instance. Changing its ports can break that environment until you update what connects to it." dan tombol "Change Anyway"; Change Anyway mengganti port, lease tetap → AC-23
- [ ] Offline (hentikan `stacklyd`) → Down nonaktif dengan alasan gerbang di banner jendela; saat Down berjalan, Down nonaktif dengan tooltip "Already bringing down" dan Up… dengan "Bringing down" → AC-20

### Commands

- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter EnvironmentDown` → `EnvironmentDownModelTests` (preview membuka konfirmasi *Copy Down*, preview kosong memberi "Nothing to bring down." tanpa `env.down`, Bring Down mengirim ID `stop` dari preview, `env.down_changed` membuka konfirmasi lagi dengan "What Down will stop changed. Review it again.", penolakan lain jadi umpan balik, hasil Down yang diawasi muncul saat berakhir, setiap kalimat umpan balik termasuk `env.down_incomplete` dengan Show Operation dan `left_running` dengan Bring Down…) dan `EnvironmentDownPageTests` (Down nonaktif "Already bringing down", tetap aktif saat Up berjalan, Up… "Bringing down", banner "Bringing down… 1 of 2", judul dan label Partial di Activity) lulus → AC-20, AC-21
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter EnvironmentLeaseTests` → baris Lease (satu, banyak, tanpa lease, daemon lama), "· lease" di inspector, salinan konfirmasi override, dan Stop yang ditolak `supervisor.stop_requires_override` membuka konfirmasi lalu hanya Stop di konfirmasi itu mengirim `overrideLeases` true dengan key baru → AC-22, AC-23
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter EnvironmentOverrideTests` → sheet port dengan lease menampilkan peringatan dan "Change Anyway" lalu mengirim `overrideLeases` true, tanpa lease tidak mengirimnya; baris Stop All "Used by {environment}", kalimat environment terdampak (satu, banyak, tidak ada); Turn Off menulis instance ber lease dan tidak nonaktif karena lease → AC-23, AC-24
- [ ] `swift test --package-path clients/swift --filter DaemonRestartClientTests` → `overrideLeases` true sampai di `instance.preview_stop_all`, `instance.stop_all`, dan `daemon.shutdown` `agent_off` → AC-24
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "EnvironmentDownSnapshotTests|EnvironmentLeaseSnapshotTests|EnvironmentOverrideSnapshotTests|EnvironmentUpToolbarSnapshotTests|MenuBarSnapshotTests"` → snapshot terang, gelap, dan sempit cocok: halaman saat Down berjalan, daftar "Bringing down…", Operations dengan Down Partial dan anak bersarang, baris Lease, inspector "Shared · lease", dan toolbar Validate, Down, Up… (alasan hanya tooltip, seperti toolbar asli), sheet port "Change Anyway", dan popover Stop All dengan `env-db` "Used by erp-development" → AC-20, AC-21, AC-22, AC-23, AC-24
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "FixtureScenarioTests|FixtureShellTests"` → skenario `environment-bringing-down` ada dan tenang di presence-nya → AC-25
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus
- [ ] `cargo xtask build-app` → BUILD SUCCEEDED
- [ ] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test env_down_native -- --ignored --test-threads=1` dari `HEAD` yang bersih → `PASS` dari cache runtime xtask (Up ERP enam instance, preview menghentikan enam dan menjaga nol, Down `succeeded`, enam `stopped` dengan folder data dan jumlah file data yang sama, lease 6 menjadi 0, Up kedua membawa enam instance yang sama ke `ready`), atau `BLOCKED` `owner: user` tanpa artefak; run developer `evidence/environment/01a1d6b2-8137-7226-821a-0bcd1d86b5ba` PASS (data ClickHouse `status`, `*.pid`, `*.lock`, `*.sock`, dan tablespace sementara MySQL dianggap file proses yang dihapus stop normal) → AC-26
- [ ] `cargo xtask evidence run --kind fixture -- cargo test -p stacklyd --test env_down` → record fixture `PASS` untuk AC-5, AC-7, AC-11; run developer `evidence/environment/01a1d6b4-6089-722c-8443-30f3142c6828` 7 PASS → AC-26
- [ ] `cargo xtask evidence check` → lulus

### Acceptance-criteria coverage

- AC-20 · tombol Down, `EnvironmentDownModel`, konfirmasi *Copy Down*, `env.down_changed`, "Nothing to bring down.", alasan nonaktif, Bring Down… di penolakan remove (langkah 8)
- AC-21 · detail kartu hasil menyebut tiap instance yang tetap jalan dengan alasannya dari preview yang dikonfirmasi ("used by {environment}", "started by you"), snapshot `environment-down-kept-feedback`; banner "Bringing down… {done} of {total}" dengan Cancel dan Show in Activity, "Bringing down…" di sidebar, daftar, dan Overview, kartu hasil *Copy Down*, judul dan label Partial di Activity (langkah 8)
- AC-22 · baris Lease dan "· lease" di inspector (langkah 9); bagian CLI milik milestone 3
- AC-23 · konfirmasi override untuk semua permukaan Stop (halaman instance, daftar, menu baris service, menu bar) lewat satu fungsi di `InstanceActionModel`, banner dan "Change Anyway" di sheet port dari `EndpointValidation.leases` (langkah 9)
- AC-24 · popover Stop All dan konfirmasi Turn Off dengan `overrideLeases` true, "Used by {environment}", kalimat environment terdampak (langkah 9)
- AC-25 · skenario fixture `environment-bringing-down` (langkah 8)
- AC-26 · test native `env_down_native.rs` dan record fixture `env_down` (langkah 10)
