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
- [x] `cargo test -p stackly-core --no-fail-fast` → semua test stop, restart, cancel start, Stop All, force stop, crash dan respawn, handover, dan wake yang ada lulus tanpa perubahan assertion → AC-5, AC-13, AC-14
- [x] `cargo test -p stacklyd --no-fail-fast` → lulus; precheck `instance.stop` menolak dengan item `environment` dari `environment_holders` → AC-13
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
- [x] `cargo test -p stackly-core --no-fail-fast` dan `cargo test -p stacklyd --no-fail-fast` → lulus; test stop, restart, dan Stop All lama tanpa perubahan assertion (perubahan test lama hanya argumen `StopAllOptions::default()` di `stop_all_select.rs`, field default baru di struct literal, dan `InstancePreviewStopAll(Default::default())`) → AC-14
- [x] `cargo test -p stackly-cli --no-fail-fast`, `-p stackly-domain`, `-p stackly-storage`, `-p stackly-testkit` → lulus
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
- [x] `cargo test -p stackly-cli --no-fail-fast` → lulus; golden lama yang berubah hanya `env_status.txt` (baris `lease:`)
- [x] `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask docs check` → lulus
- [x] Native, dengan binary `stackly` dan `stacklyd` di root sementara dan runtime NATS dari cache runtime xtask tanpa unduhan (binary daemon tidak mengenal `testing/fake` di manifest, jadi provider `fake` tidak bisa dipakai di sini; runtime dipasang dari cache lebih dulu, seperti `Site::open(Serve::Binary)` di `apps/daemon/tests/nats/harness.rs`): dua folder manifest, alpha dengan `official/nats` untuk `bus` dan `edge` (`edge` bergantung pada `bus`) dan beta dengan `instanceRef: alpha-bus`; `stackly env up ./alpha --wait`, `stackly env up ./beta --wait`, lalu `stackly env down ./alpha --wait` → stderr `stop alpha-edge` dan `keep alpha-bus: used by beta`, stdout "Down. 1 stopped, 1 kept running.", exit 0, dan `stackly env status beta` masih `lease: held on 1 instance since …`; `stackly instance stop alpha-bus` → exit 3 dengan `alpha-bus is used by beta (bus)`; `stackly instance stop alpha-bus --override-leases --wait` → "alpha-bus stopped. beta is no longer complete."; tanpa artefak di cache: `BLOCKED`, `owner: user` → AC-18, AC-19, AC-22

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

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter EnvironmentDown` → `EnvironmentDownModelTests` (preview membuka konfirmasi *Copy Down*, preview kosong memberi "Nothing to bring down." tanpa `env.down`, Bring Down mengirim ID `stop` dari preview, `env.down_changed` membuka konfirmasi lagi dengan "What Down will stop changed. Review it again.", penolakan lain jadi umpan balik, hasil Down yang diawasi muncul saat berakhir, setiap kalimat umpan balik termasuk `env.down_incomplete` dengan Show Operation dan `left_running` dengan Bring Down…) dan `EnvironmentDownPageTests` (Down nonaktif "Already bringing down", tetap aktif saat Up berjalan, Up… "Bringing down", banner "Bringing down… 1 of 2", judul dan label Partial di Activity) lulus → AC-20, AC-21
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter EnvironmentLeaseTests` → baris Lease (satu, banyak, tanpa lease, daemon lama), "· lease" di inspector, salinan konfirmasi override, dan Stop yang ditolak `supervisor.stop_requires_override` membuka konfirmasi lalu hanya Stop di konfirmasi itu mengirim `overrideLeases` true dengan key baru → AC-22, AC-23
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter EnvironmentOverrideTests` → sheet port dengan lease menampilkan peringatan dan "Change Anyway" lalu mengirim `overrideLeases` true, tanpa lease tidak mengirimnya; baris Stop All "Used by {environment}", kalimat environment terdampak (satu, banyak, tidak ada); Turn Off menulis instance ber lease dan tidak nonaktif karena lease → AC-23, AC-24
- [x] `swift test --package-path clients/swift --filter DaemonRestartClientTests` → `overrideLeases` true sampai di `instance.preview_stop_all`, `instance.stop_all`, dan `daemon.shutdown` `agent_off` → AC-24
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "EnvironmentDownSnapshotTests|EnvironmentLeaseSnapshotTests|EnvironmentOverrideSnapshotTests|EnvironmentUpToolbarSnapshotTests|MenuBarSnapshotTests"` → snapshot terang, gelap, dan sempit cocok: halaman saat Down berjalan, daftar "Bringing down…", Operations dengan Down Partial dan anak bersarang, baris Lease, inspector "Shared · lease", dan toolbar Validate, Down, Up… (alasan hanya tooltip, seperti toolbar asli), sheet port "Change Anyway", dan popover Stop All dengan `env-db` "Used by erp-development" → AC-20, AC-21, AC-22, AC-23, AC-24
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "FixtureScenarioTests|FixtureShellTests"` → skenario `environment-bringing-down` ada dan tenang di presence-nya → AC-25
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus
- [x] `cargo xtask build-app` → BUILD SUCCEEDED
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test env_down_native -- --ignored --test-threads=1` dari `HEAD` yang bersih → `PASS` dari cache runtime xtask (Up ERP enam instance, preview menghentikan enam dan menjaga nol, Down `succeeded`, enam `stopped` dengan folder data dan jumlah file data yang sama, lease 6 menjadi 0, Up kedua membawa enam instance yang sama ke `ready`), atau `BLOCKED` `owner: user` tanpa artefak; run developer `evidence/environment/01a1d6b2-8137-7226-821a-0bcd1d86b5ba` PASS (data ClickHouse `status`, `*.pid`, `*.lock`, `*.sock`, dan tablespace sementara MySQL dianggap file proses yang dihapus stop normal) → AC-26
- [x] `cargo xtask evidence run --kind fixture -- cargo test -p stacklyd --test env_down` → record fixture `PASS` untuk AC-5, AC-7, AC-11; run developer `evidence/environment/01a1d6b4-6089-722c-8443-30f3142c6828` 7 PASS → AC-26
- [x] `cargo xtask evidence check` → lulus

### Catatan `/check verify` 2026-11-14 (sesi `chk-f47-m4`, kode `a1cefc77`)

Verdict Milestone 4: **PASS untuk bagian otomatis** AC-20 sampai AC-24, bagian GUI AC-25, dan AC-26, dengan catatan pada AC-26 (temuan 1 dan 2). GUI nyata belum terbukti: 13 langkah `### UI / manual` tetap `NOT_RUN`, `owner: user` (`F47-1`). Checker tidak menjalankan app atau `cargo xtask ui-test`.

Dua kotak Commands dibiarkan kosong karena tidak diulang: suite penuh StacklyUI dan `cargo xtask build-app` (gerbang lead pada `a1cefc77` hijau). Dua kotak evidence dicentang karena checker menjalankan ulang test nya dan memeriksa record developer, bukan karena membuat record baru.

**Hasil perintah.** `--filter EnvironmentDown` 20 test di 4 suite; `EnvironmentLeaseTests` 5; `EnvironmentOverrideTests` 5; `DaemonRestartClientTests` 8; filter snapshot 15 test di 6 suite; `FixtureScenarioTests|FixtureShellTests` 31; `cargo test -p stacklyd --test env_down_native -- --ignored --test-threads=1` lulus 92,36 detik dari cache xtask tanpa unduhan; `cargo test -p stacklyd --test env_down` 7 lulus; `cargo xtask evidence check` lulus (74 record manual, 134 record run).

**Evidence.** Run native `01a1d6b2-8137-7226-821a-0bcd1d86b5ba`: `sourceDirty` false, `sourceCommit` `605dc763` leluhur `main`, `kind` `native`, satu `PASS`, perintah sama dengan langkah verify, enam subject `managed` dengan digest `versions.json`; `actual` mencatat jumlah file data per service (postgres 973, mysql 172, clickhouse 12, valkey 3, nats dan traefik 0). Run fixture `01a1d6b4-6089-722c-8443-30f3142c6828`: 7 `PASS` dari commit yang sama, tanpa masalah.

**Penilaian tafsiran AC-26** ("tidak ada file data yang hilang", dengan pengecualian file proses):

- Setiap pengecualian memang file yang ditulis server hanya selama ia berjalan dan dihapus oleh stop normal server itu sendiri, bukan data pengguna: `*.pid` (`postmaster.pid`, pid MySQL), `*.sock` dan `*.lock` soketnya, `status` ClickHouse (ditulis saat start, dihapus saat keluar), dan tablespace sementara MySQL (`ibtmp1`, `#innodb_temp/*`, dibuat ulang tiap start). Dua run pertama developer gagal justru karena file itu hilang oleh shutdown bersih, bukan oleh Down.
- Test tetap menangkap penghapusan data sungguhan. Mutasi lokal yang tidak di-commit: sesudah Down, `PG_VERSION` di folder data postgres dihapus; test gagal dengan "postgres: the data files stay; gone [\"PG_VERSION\"]". Mutasi dicabut sesudahnya.
- Batasnya: test membandingkan nama file, bukan isi. Mutasi kedua di run yang sama mengosongkan satu file data ClickHouse dan test tidak menyadarinya. "Up kedua kembali `ready` dengan data yang sama" juga hanya dibuktikan dengan instance yang sama kembali `ready` di folder yang sama; tidak ada data sentinel yang ditulis sebelum Down lalu dibaca sesudah Up.
- Terhadap Done when "`stackly env down` tidak pernah menghapus data atau runtime": terbukti bersama bukti lain (byte setiap file sama di probe `fake` langkah 3 sampai 5, ukuran file data dan runtime sama di probe native NATS, dan test fixture PRD AC-022). Test native sendiri tidak memeriksa folder runtime.

**Yang dibaca di kode app.** `EnvironmentDownModel` hanya memakai `env.preview_down` dan `env.down`, mengirim `expectedStopInstanceIds` dari preview yang ditampilkan, dan untuk `env.down_changed` membaca preview baru lalu membuka konfirmasi lagi; tidak ada logika pilihan stop sendiri. Stop dari setiap permukaan mengirim `overrideLeases` false lebih dulu; hanya tombol Stop di konfirmasi destructive yang mengirim true, dengan key baru. Sheet port mengirim `overrideLeases` true hanya saat verdict dry run memuat `leases` dan tombolnya sudah "Change Anyway"; penolakan `instance.endpoints_require_override` menilai ulang tanpa kirim ulang otomatis. Popover Stop All dan Turn Off memakai preview ber override dan mengirim `expectedInstanceIds` dari preview itu. Tidak ada label aksesibilitas, shortcut, atau identifier UI test baru.

**Snapshot yang dilihat.** Kartu "erp-development is down. 1 stopped, 2 kept running." dengan "Keeps running: pg17-dev (used by reporting), queue (started by you).", baris Lease "Lease held on 3 instances since …" di bawah Lock dan Local override (sama dengan teks mockup), inspector "Shared · lease", sheet port dengan banner dan "Change Anyway", popover Stop All dengan `env-db` "Used by erp-development" dan kalimat "erp-development will no longer be complete. Its lease stays.", banner "Bringing down… 1 of 2", dan toolbar Validate, Down, Up… (alasan nonaktif hanya tooltip, seperti toolbar asli).

**Tafsiran developer lain.** (1) Detail kartu "Keeps running: …" dari preview yang dikonfirmasi, hanya untuk Down yang dikirim jendela itu: diterima; ini menjawab temuan lama tentang environment yang masih `partially_ready` sesudah Down. (2) Tombol "Down" tanpa "…" dan `env.down_changed` sebagai baris pertama konfirmasi: diterima, sesuai teks spec. (3) Satu fungsi `overrideRequest` untuk semua permukaan Stop, permukaan tanpa tempat konfirmasi hanya melaporkan penolakan, sheet port tanpa kirim ulang otomatis: diterima. (4) Teks Turn Off dan `overrideLeases` true di `previewTurnOff` dan `agentOff`: diterima, sesuai AC-24.

Temuan, rendah:

1. AC-26 menulis "isi yang sama jumlah file nya", sedangkan test memeriksa "tidak ada file data yang hilang" dengan pengecualian file proses dan mengizinkan file tambahan. Tafsirannya masuk akal dan test menangkap penghapusan, tetapi teks AC perlu diselaraskan.
2. Test native tidak memeriksa isi data: file yang dikosongkan tidak terdeteksi, dan tidak ada data sentinel yang dibaca sesudah Up kedua. Pengecualian file proses juga berlaku per nama di kedalaman mana pun untuk semua provider (misalnya setiap file bernama `status`), bukan per provider.
3. Peringatan sheet port memakai "uses" dan "that environment" juga saat lebih dari satu environment memegang instance ("shop and blog uses this instance…").
4. Snapshot tidak bisa menunjukkan tombol default; "Cancel adalah default di dialog destructive" hanya bisa dinilai di langkah manual.

**Penilaian langkah manual.** Tiga belas langkah `F47-1` cukup untuk AC-20 sampai AC-24 dan bagian tampilan AC-12. Yang bisa otomatis tetapi belum: test ujung ke ujung Down terhadap `stacklyd` asli dari app, dan data sentinel di test native (temuan 2).

### Acceptance-criteria coverage

- AC-20 · tombol Down, `EnvironmentDownModel`, konfirmasi *Copy Down*, `env.down_changed`, "Nothing to bring down.", alasan nonaktif, Bring Down… di penolakan remove (langkah 8)
- AC-21 · detail kartu hasil menyebut tiap instance yang tetap jalan dengan alasannya dari preview yang dikonfirmasi ("used by {environment}", "started by you"), snapshot `environment-down-kept-feedback`; banner "Bringing down… {done} of {total}" dengan Cancel dan Show in Activity, "Bringing down…" di sidebar, daftar, dan Overview, kartu hasil *Copy Down*, judul dan label Partial di Activity (langkah 8)
- AC-22 · baris Lease dan "· lease" di inspector (langkah 9); bagian CLI milik milestone 3
- AC-23 · konfirmasi override untuk semua permukaan Stop (halaman instance, daftar, menu baris service, menu bar) lewat satu fungsi di `InstanceActionModel`, banner dan "Change Anyway" di sheet port dari `EndpointValidation.leases` (langkah 9)
- AC-24 · popover Stop All dan konfirmasi Turn Off dengan `overrideLeases` true, "Used by {environment}", kalimat environment terdampak (langkah 9)
- AC-25 · skenario fixture `environment-bringing-down` (langkah 8)
- AC-26 · test native `env_down_native.rs` dan record fixture `env_down` (langkah 10)

## Verify penuh 2026-11-14

Verdict fitur 47: **PASS untuk semua bagian otomatis** AC-1 sampai AC-26. `Build it` dan `Verify it` siap dicentang dengan sisa yang tercatat: GUI nyata `NOT_RUN` (13 langkah `### UI / manual` Milestone 4, `F47-1`, `owner: user`). Kode yang diperiksa `66bcb4e6` (protokol 4.6).

**Yang dicek ulang sesudah merge terakhir** (langkah `[x]` lama tidak diulang satu per satu): `stackly-core --lib environment::down` 18, `--test environment` 132, `--test stop_all_select` 18, `--test supervisor_start guarded_stop` 18; `stackly-protocol` lulus; `stacklyd --test env_down` 7 dan `--test supervisor lease_override` 5; `stackly-cli --test env down::` 7, `--test instance override_leases` 7, test unit 26; `codegen --check` bersih dengan kontrak 4.6; `check-deps` dan `evidence check` lulus; `swift test --filter 'EnvironmentDown|EnvironmentLeaseTests|EnvironmentOverrideTests'` 30 lulus. Test native `env_down_native` terakhir dijalankan checker pada `a1cefc77` (lulus 92,36 detik); sesudah itu hanya kode CLI yang berubah.

**Fix CLI `c125513c`.** `stackly instance stop --all` kini menamai tiap instance yang dipegang environment (`orders-db · won’t stop: used by erp`), menulis "Nothing was stopped." saat semuanya dilewati, dan tetap "Nothing is running." hanya saat memang tidak ada yang berjalan; empat golden baru dan test `override_leases` (7) lulus. Temuan 1 Milestone 3 tertutup.

**Kotak suite penuh** dicentang atas dasar gerbang lead, tidak diulang checker: Milestone 1 dan 2 (`6a41187d`: suite kecil 1140, `stackly-core` 1881, `stacklyd` 778, `stackly-cli` 678), Milestone 3 (`be6efa46`: `stackly-cli` 704; `c125513c`: 706), dan Milestone 4 (`a1cefc77`: StacklyUI penuh dan `build-app`). Langkah terakhir Commands Milestone 3 ditulis ulang menjadi langkah native yang dijalankan checker.

| AC | Hasil | Bukti utama |
| --- | --- | --- |
| AC-1 | PASS | `select` enam aturan, dipakai preview, precheck, driver, dan sapuan; probe preview |
| AC-2 | PASS | `env.down_changed` tanpa tulisan; Down tanpa manifest; Down tanpa yang dihentikan |
| AC-3 | PASS | pengatur, `BusyPolicy::Queue`, Down membatalkan Up (test daemon) |
| AC-4 | PASS | guard `release`; lease environment lain tetap `active` (probe) |
| AC-5 | PASS | PRD AC-022: PID sama dan `ready` sesudah Down pertama (probe `fake` dan native NATS) |
| AC-6 | PASS | urutan terbalik (`order`, probe) |
| AC-7 | PASS | byte file dan baris registry sama sebelum dan sesudah (probe) |
| AC-8 | PASS | `env.down_incomplete`, sapuan akhir, `left_running` (core dan daemon) |
| AC-9 | PASS | test cancel core |
| AC-10 | PASS | restart daemon di tengah Down, diulang 10 kali |
| AC-11 | PASS | Up dan Down serentak 50 putaran, diulang 4 kali |
| AC-12 | PASS | `env.get.lease`, `leaseActive`, klien putus |
| AC-13 | PASS | penolakan dengan `name`, `alias`, `ownership`; override; guard |
| AC-14 | PASS | restart dan reload tanpa override, lease tetap (probe) |
| AC-15 | PASS | dry run dengan `leases`, penolakan, override (probe) |
| AC-16 | PASS | preview dan Stop All dengan dan tanpa override; `instance.stop_all_changed` (probe) |
| AC-17 | PASS | `agent_off` dengan override lewat socket (test developer) |
| AC-18 | PASS | `stackly env down`: test dan probe native binary CLI |
| AC-19 | PASS | flag `--override-leases` di stop, stop all, endpoint set (probe native); `daemon uninstall` lewat test unit saja |
| AC-20 sampai AC-24 | PASS (otomatis) | Milestone 4: test, snapshot, fixture; GUI nyata `NOT_RUN` |
| AC-25 | PASS | protokol 4.5 dan 4.6, fixture, skenario `environment-bringing-down` |
| AC-26 | PASS dengan catatan | test native dijalankan ulang, evidence sah, mutasi hapus file tertangkap; lihat temuan 1 dan 2 |

**Trace.** REQ-022: AC-2, AC-4, AC-5, AC-6, AC-7, AC-8 lulus. REQ-028: AC-18, AC-19 lulus. BR-006: AC-7, AC-26 lulus. BR-014: AC-5, AC-11 lulus. BR-015: AC-13, AC-15, AC-16, AC-17 lulus; AC-23, AC-24 lulus otomatis dan GUI nya `NOT_RUN`. VAL-014: AC-7 lulus. AC-022: AC-4, AC-5 lulus. AC-073 bagian Down: AC-11 lulus. PRD 13.2: AC-4, AC-12, AC-13 lulus.

**Done when**, satu per satu:

| Kalimat | Hasil |
| --- | --- |
| Saat environment A down, lease B tetap ada dan instance tetap ready | terbukti (test fixture, probe `fake`, probe native NATS: PID sama, koneksi baru tetap dijawab) |
| Up A dan Down B bersamaan menghasilkan lease konsisten tanpa create ganda | terbukti (test 50 putaran, empat kali) |
| Lease per identitas environment, menutup CLI atau GUI tidak melepas lease | terbukti (klien putus di tengah Down, Ctrl-C di tengah Up, rotasi sesi, restart daemon) |
| Stop Services dari menu bar menampilkan lease environment yang terdampak | terbukti di daemon (`environments`, `affectedEnvironments`) dan di snapshot popover; GUI nyata `NOT_RUN` |
| Ganti port pada instance yang dipinjam ditolak atau meminta override dengan dampak yang jelas | terbukti di daemon dan CLI; sheet port lewat test dan snapshot, GUI nyata `NOT_RUN` |
| `stackly env down` tidak pernah menghapus data atau runtime | terbukti (byte file sama di probe `fake`, ukuran file data dan runtime sama di probe native CLI, test native ERP) |
| Konfirmasi Down menjelaskan instance yang tetap jalan, baris Lease mengikuti mockup | terbukti di CLI (`keep <instance>: used by …` dan `started by you`) dan di test serta snapshot app (teks baris Lease sama dengan mockup); GUI nyata `NOT_RUN` |

**Sisa `NOT_RUN` dan yang tidak dijalankan.**

- `NOT_RUN`, `owner: user`: 13 langkah `### UI / manual` Milestone 4 (`F47-1`), termasuk tombol default di dialog destructive.
- Tidak dijalankan: `stackly daemon uninstall --override-leases` terhadap sistem nyata (menyentuh LaunchAgent pengguna; hanya test unit), Ctrl-C di tengah `stackly env down --wait`, `--wait` yang kehabisan `--timeout`, dan probe native `agent_off` dengan override.

**Temuan rendah yang masih terbuka.**

1. AC-26 menulis "isi yang sama jumlah file nya"; test native memeriksa "tidak ada file data yang hilang" dengan pengecualian file proses. Teks AC perlu diselaraskan.
2. Test native Down membandingkan nama file saja (file yang dikosongkan lolos), tidak menulis dan membaca data sentinel, mengecualikan file proses per nama di kedalaman mana pun untuk semua provider, dan tidak memeriksa folder runtime.
3. Test `a_stop_that_times_out_ends_the_down_incomplete_and_the_next_down_finishes` tidak stabil: gagal 1 dari 12 kali dijalankan sendiri dan sekali di suite, dengan `env.down_changed` pada Down kedua. Sesudah `shutdown_server`, himpunan `stop` bisa berubah di antara preview dan `env.down`, dan helper test tidak mengulang. Perilaku daemon nya benar; yang perlu diperbaiki test nya (tunggu instance diam dulu, atau ulang sekali seperti CLI dan app).
4. `leaseActive` di summary instance bisa tertinggal sebentar selama Up atau Down masih berjalan; `env.get.lease` selalu segar.
5. `affectedEnvironments` menghitung setiap pemegang lease, sedangkan `environments` pada item hanya yang punya baris member.
6. Sesudah Down, environment yang instance bersamanya masih jalan tampil `partially_ready` dengan `lease: none` di `stackly env status`; kartu app sudah menjelaskannya, CLI belum.
7. Peringatan sheet port memakai bentuk tunggal juga untuk lebih dari satu environment.

Tertutup sejak catatan milestone: tiga tanda `// provisional: F47`, kode `env.*` di katalog protokol, test socket precheck `instance.stop`, dan "Nothing is running." pada `stop --all`.

**Celah test untuk `/test`** (lulus di probe checker atau terbaca di kode, belum dikunci test):

- Native: data sentinel sebelum Down dan sesudah Up kedua; perbandingan isi atau ukuran file, bukan hanya nama; pengecualian file proses per provider; folder runtime sama sebelum dan sesudah; Down dengan instance bersama lewat binary CLI (`env up` dua folder, `env down`, `env status` dengan `lease:`, `instance stop` tanpa dan dengan `--override-leases`, `instance endpoint set`).
- Daemon: dua Down serentak dari dua environment pada satu instance (paling banyak satu stop, satu berakhir `left_running`); cancel Down lewat socket (saat antre dan sesudah `release`); Down sesudah restart oleh pengguna (demand `manual`, `keep`); `instance.stop_all_changed` untuk himpunan preview bawaan yang dikirim dengan override; `fixed` pada port yang sedang dipakai endpoint `auto` dihitung perubahan dan `auto` ke `auto` `unchanged`; start manual dan Up sesudah stop ber override; perbandingan byte pohon file (test sekarang membandingkan ukuran).
- CLI: Ctrl-C di tengah `env down --wait`, `--wait` yang kehabisan `--timeout` (exit 8), `--json` `preview` terhadap daemon nyata, dan `daemon uninstall --override-leases` di luar test unit.
- App: test ujung ke ujung Down terhadap `stacklyd` asli; peringatan sheet port untuk lebih dari satu environment.
- Stabilitas: test `a_stop_that_times_out…` (temuan 3).
