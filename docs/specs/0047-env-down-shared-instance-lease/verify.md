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

## Milestone 3: `stackly env down` dan flag override

Langkah Build plan 7: `stackly env down` dengan preview ke stderr, `env.down` dengan ID dari preview itu, `--wait` dengan satu baris per instance yang berakhir, dan satu ulangan otomatis untuk `env.down_changed`; baris `lease:` di `stackly env status`; `--override-leases` di `instance stop`, `instance stop --all`, `instance endpoint set`, dan `daemon uninstall`, dengan baris dan hint penolakan. Hanya `apps/cli/` dan bagian ini yang berubah. Protokol tetap 4.6, tidak ada kode `cli.*` baru, tidak ada migrasi. Completion dan man page dibuat clap dari definisi perintah, jadi `env down` dan flag baru ikut tanpa perubahan generator.

### UI / manual

- [x] Buka `apps/cli/src/commands/env/down.rs` → urutannya `env.preview_down`, baris preview ke stderr (`stop`, `keep <instance>: <alasan>`, `skip <instance>: <kalimat kode>`, `cancels the running up`, "Nothing to stop." untuk preview kosong), lalu `env.down` dengan `expectedStopInstanceIds` dari preview itu dan tanpa pertanyaan; `env.down_changed` membaca preview baru dan mengirim sekali lagi dengan key yang sama, lalu gagal (exit 3); target file dicocokkan dulu dengan `manifestLocation` di `env.list` dan baru di plan bila tidak ada yang cocok → AC-18
- [x] Di file yang sama → dengan `--wait`, sesudah operasi berhenti, satu baris per instance dari hasil operasi: `stopped` dari `result.resources`, `kept` dari item `partial` ber kode `env.instance_in_use`, dan `failed <instance>: <kode>`, `left running`, atau `cancelled` dari item `instance` error `env.down_incomplete`; stdout "Down. <s> stopped, <k> kept running." hanya saat `succeeded`, `<k>` = `keep` preview ditambah yang `kept` → AC-18
- [x] Buka `apps/cli/src/commands/env/status.rs` → baris `lease: held on <n> instances since <RFC 3339>` (`1 instance` untuk satu) atau `lease: none` tepat di bawah baris kepala, tidak ada baris saat `lease` kosong (daemon lama) → AC-22
- [x] Buka `apps/cli/src/commands/instance/lease.rs`, `action.rs`, `stop.rs`, `endpoint.rs`, `apps/cli/src/commands/daemon/uninstall.rs` → flag `--override-leases` mengirim `overrideLeases` true dan tidak bisa dipakai bersama `--force`; penolakan `supervisor.stop_requires_override` dan `instance.endpoints_require_override` menulis `<name> is used by <environment> (<alias>)` per item lalu hint perintah ulang dengan `--override-leases` dan `stackly env down <environment>`; stop sukses mencetak "<name> stopped. <environment> is no longer complete." per environment ber `leaseActive`; `stop --all` menamai environment di baris item dan skip, menambah `affects <environment> (<n> instances)`, dan memberi hint flag saat ada skip karena lease; `daemon uninstall` meneruskan flag ke preview dan `agent_off`, menamai environment, dan memberi hint flag saat diblokir lease → AC-19

### Commands

- [x] `cargo test -p stackly-cli --test env down::` → 7 lulus: tanpa `--wait` (golden `env_down.txt`, `env_down_stderr.txt`, `env_down.json`, ID `stop` preview di `env.down`, key `--idempotency-key`), `--wait` sukses (golden `env_down_wait.txt`, baris `stopped` sesudah preview, JSON dengan `environment`), instance `kept` sesudah preview, `env.down_incomplete` exit 7 (golden `env_down_incomplete_stderr.txt`), `env.down_changed` sekali lalu sukses dan dua kali lalu exit 3, preview kosong tetap mengirim `env.down` dengan daftar kosong, file terdaftar tanpa `env.plan` dan nama tak dikenal exit 10 → AC-18
- [x] `cargo build -p stacklyd -p xtask && cargo test -p stackly-cli --test env a_real_daemon_brings_down` → lulus terhadap `stacklyd` nyata: sesudah `env import`, `env status` mencetak `lease: none` di baris kedua; `env down ./shop --wait` menulis "Nothing to stop." dan "Down. 0 stopped, 0 kept running." dengan exit 0; manifest dihapus lalu `env down ./shop --json` tetap exit 0 untuk environment yang sama; tidak ada instance yang dibuat → AC-18, AC-22
- [x] `cargo test -p stackly-cli --test env status` → golden `env_status.txt` memuat `lease: held on 1 instance since 2026-11-13T09:00:00.000Z` → AC-22
- [x] `cargo test -p stackly-cli --test instance override_leases` → 6 lulus: stop ditolak exit 3 dengan baris dan dua hint (JSON tanpa baris), stop dengan override `--wait` mengirim `overrideLeases` true dan mencetak environment yang tidak lengkap, `--force` dengan `--override-leases` exit 2, `stop --all` dengan dan tanpa override (preview dan stop membawa nilainya, baris `Used by erp`, `affects erp (1 instance)`, hint flag), `endpoint set` ditolak exit 3 dengan hint perintah lengkap lalu dengan override mengirim `overrideLeases` true → AC-19
- [x] `cargo test -p stackly-cli --bin stackly -- env::down env::status instance::lease instance::stop daemon::uninstall` → lulus, termasuk `override_leases_reaches_the_preview_and_agent_off_and_names_the_environments` dan hint lease pada uninstall yang diblokir → AC-18, AC-19, AC-22
- [ ] `cargo test -p stackly-cli --no-fail-fast` → lulus; golden lama yang berubah hanya `env_status.txt` (baris `lease:`)
- [x] `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask docs check` → lulus
- [x] Dengan daemon nyata dan provider `fake` (seed `skeleton_seed`, dua environment yang berbagi satu instance): `stackly env up <a> --wait`, `stackly env up <b> --wait`, lalu `stackly env down <a> --wait` → stderr `keep <shared>: used by <b>`, exit 0, dan `stackly env status <b>` masih `lease: held on ...`; `stackly instance stop <shared>` → exit 3 dengan `<shared> is used by <b> (<alias>)`; `stackly instance stop <shared> --override-leases --wait` → "<shared> stopped. <b> is no longer complete." → AC-18, AC-19, AC-22

### Catatan `/check verify` 2026-11-14 (sesi `chk-f47-m3`, kode `fe88da0b`)

Verdict Milestone 3: **PASS** untuk AC-18, AC-19, dan bagian CLI AC-22. Sebelas kotak dijalankan atau dibaca checker dan lulus. Satu kotak kosong: suite penuh `cargo test -p stackly-cli --no-fail-fast` tidak diulang (gerbang lead: 704 lulus); `--test env` penuh 51 lulus.

**Hasil perintah.** `--test env down::` 7; `a_real_daemon_brings_down` 1; `--test env status` 4; `--test instance override_leases` 6; `--bin stackly -- env::down env::status instance::lease instance::stop daemon::uninstall` 25; `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo xtask docs check` lulus.

**Langkah terakhir Commands dijalankan dengan NATS asli, bukan `fake`.** Binary `stacklyd` tidak mengenal `testing/fake` di manifest (provider itu tidak ada di catalog binary), jadi langkah itu tidak bisa dijalankan seperti tertulis. Alurnya dijalankan dengan binary `stackly` terhadap binary `stacklyd` di root sementara, runtime NATS 2.15.1 dari cache xtask tanpa unduhan, dan keychain file milik root itu; root dibersihkan sesudahnya. alpha membuat `alpha-bus` dan `alpha-edge`, beta meminjam `alpha-bus`:

| Probe | Hasil |
| --- | --- |
| `env status` keduanya | `lease: held on 2 instances since <RFC 3339>` untuk alpha dan `lease: held on 1 instance since …` untuk beta, tepat di bawah baris kepala → AC-22 |
| `instance stop alpha-bus` | exit 3; `alpha-bus is used by alpha (bus)` dan `alpha-bus is used by beta (bus)`; hint `stackly instance stop alpha-bus --override-leases` dan `stackly env down <environment>` per environment; `--json` hanya envelope error dengan dua item, stderr kosong → AC-19 |
| `--override-leases` bersama `--force` | exit 2 dari clap → AC-19 |
| `instance endpoint set alpha-bus --port client=<port>` | `--dry-run` exit 0 dengan "used by alpha (bus): applying it needs --override-leases"; tanpa flag exit 3 dengan baris per environment dan hint perintah lengkap ber `--override-leases` → AC-19 |
| `instance stop --all --yes` tanpa flag | tidak menghentikan apa pun yang dipegang environment; hint `stackly instance stop --all --override-leases` (lihat temuan 1) → AC-19 |
| `env down ./alpha --wait` | tanpa pertanyaan; stderr `stop alpha-edge`, `keep alpha-bus: used by beta`, lalu `stopped alpha-edge`; stdout "Down. 1 stopped, 1 kept running."; exit 0 → AC-18 |
| Sesudah Down alpha | `alpha-bus` PID sama dan `ready`; alpha `lease: none`, beta masih `lease: held on 1 instance …`; ukuran setiap file data dan runtime sama (yang berubah hanya log stderr daemon) → AC-18, AC-22 |
| `env down alpha --json` | `result` berisi `operation`, `idempotencyKey`, `preview`, `environment` (null tanpa `--wait`); `preview` sama dengan `env.preview_down` daemon selain `environment.revision`, yang naik karena Down itu sendiri; stderr kosong → AC-18 |
| `env down ./alpha --wait` lagi | "Nothing to stop." dan "Down. 0 stopped, 1 kept running." → AC-18 |
| `instance stop alpha-bus --override-leases --wait` | "alpha-bus stopped. beta is no longer complete."; beta `Down  0 of 1 ready` dengan `lease: held on 1 instance …` (lease tidak dilepas) → AC-19 |
| `instance endpoint set … --override-leases --wait` | port pindah, beta kembali `Ready` dengan port baru, lease tetap → AC-19 |
| Restart `alpha-bus` oleh pengguna, lalu `env down beta --wait` | `keep alpha-bus: started by you`, "Down. 0 stopped, 1 kept running." → AC-18 |
| `env down nothere` | exit 10, `cli.environment_not_found` → AC-18 |

Tidak dijalankan: `stackly daemon uninstall --override-leases` terhadap sistem nyata (menyentuh LaunchAgent pengguna; hanya lewat test unit), Ctrl-C di tengah `env down --wait` (Down selesai terlalu cepat; klien putus tercakup test daemon), dan `--wait` yang kehabisan `--timeout`.

**Tafsiran developer.** (1) Baris per instance ditulis sesudah operasi Down berhenti, ringkasan hanya saat `succeeded`, ulangan `env.down_changed` dengan key yang sama, target file dicocokkan dulu dengan `manifestLocation`: diterima. (2) Baris dan hint penolakan, `--override-leases` bentrok dengan `--force`, hint flag pada `stop --all` dan `daemon uninstall`: diterima. (3) Bentuk baris `lease:`: diterima.

**Yang dibaca di kode.** `env down` tidak bertanya (K16) dan selalu mengirim `expectedStopInstanceIds` dari preview yang baru dibacanya. `stop --all` membaca preview dengan mode override yang sama dengan permintaan dan mengirim `expectedInstanceIds` dari preview itu. Tidak ada jalur CLI yang melepas lease selain `env down`.

Temuan, rendah:

1. `stackly instance stop --all` mencetak "Nothing is running." saat setiap instance yang berjalan dipegang environment. Instance yang dilewati dan nama environment nya tidak dicetak di jalur itu; hanya hint flag yang memberi tahu.
2. Langkah terakhir Commands tidak bisa dijalankan seperti tertulis dengan `skeleton_seed` dan provider `fake` terhadap binary daemon; teks langkahnya perlu menyebut provider resmi dari cache atau daemon di dalam proses.
3. Sesudah Down alpha, `env status` alpha menulis `Partially ready  1 of 2 ready` dengan `lease: none`, karena `alpha-bus` masih jalan untuk beta. Benar menurut aturan status; sama dengan temuan 2 langkah 3 sampai 5.

### Acceptance-criteria coverage

- AC-18: lengkap di CLI; per instance ditulis sesudah operasi berhenti (dari hasil dan item error operasi Down), tidak saat tiap anak selesai, karena operasi Down tidak membawa hasil per anak selama berjalan
- AC-19: lengkap di CLI, termasuk `daemon uninstall`
- AC-22: bagian CLI (`lease:` di `env status`); baris Lease dan "· lease" di app menunggu milestone 4
- AC-20, AC-21, AC-23, AC-24, AC-26 · milestone 4 (app, evidence native)
