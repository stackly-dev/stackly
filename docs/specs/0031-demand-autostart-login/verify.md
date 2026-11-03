# Verify: Demand, autostart, dan login · spec 0031 · updated 2026-11-03

_Langkah diturunkan dari acceptance criteria spec 0031. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah._

## Milestone 1: Login menyalakan instance terpilih

Langkah build 2 sampai 7. Bagian AC-19 yang belum: Turn Off… di Diagnostics (langkah 12). Semua yang native (AC-21) belum.

### UI / manual

- [x] Buka `crates/platform-macos/src/session.rs` → `getaudit_addr` dibaca dengan satu blok `unsafe` ber `// SAFETY:`; audit session `0` atau negatif menjadi `LoginSessionError::Unavailable` → AC-1
- [x] Buka `crates/storage/migrations/0010_daemon_sessions.sql` → tabel `daemon_sessions` dengan CHECK `(ended_at IS NULL) = (end_reason IS NULL)`, unique index parsial `daemon_sessions_open`, dan unique index parsial `instance_demands_startup_session` → AC-2, AC-14
- [x] Buka `crates/core/src/session.rs` → `plan_session_rotation` fungsi murni dan `rotate_session` menjalankannya sebagai satu `commit_guarded`; tidak ada pemanggilan proses → AC-2, AC-3
- [x] Buka `apps/daemon/src/server.rs` (start slot) → urutan rotasi, `reconcile_in_session`, `queue_login_starts`, lalu `start_reconciled`; rotasi hanya untuk konteks launchd dan registry `ready` → AC-2, AC-7
- [x] Buka `crates/core/src/supervisor/startup.rs` `run_start` → start dengan `StartTrigger::Background` membaca desired lagi dan selesai `succeeded` tanpa menulis desired kalau bukan `running` → AC-7
- [x] Buka `protocols/contract-versions.json` → versi 2.4 tercatat; `ProtocolVersion::CURRENT` 2.4 → AC-17
- [x] Jalankan `target/debug/stackly instance autostart --help` → argumen `<target>` dan `on|off` → AC-18
- [ ] Buka sheet New Instance → toggle "Start at login" (default mati, teks "Separate from the daemon’s own login item.") di bagian yang sama dengan "Start after creating"; menyalakannya membuat perintah setara memuat `--autostart` dan `instance.create` membawa `startup: at_login` → AC-19
- [ ] Buka tab Summary sebuah instance → baris Autostart di kartu Details adalah toggle; teks kecil di bawahnya sesuai tabel *Copy* ("Stopped for this session. Starts at your next login.", "Starts at your next login.", "Won’t start at login: the background agent is off." atau "…needs approval in Login Items.") → AC-19

### Commands

- [x] `cargo test -p stackly-platform session` dan `cargo test -p stackly-platform-macos --lib session` → ID sesi `<boot>/<asid>` paling panjang 47 byte, audit session 0 ditolak, platform tanpa adapter menjawab `NotSupported`, macOS membaca sesi kalau proses punya → AC-1
- [x] `cargo test -p stackly-domain session` → `DaemonSession` dibuka, ditutup, dibuka lagi; kode alasan berakhir → AC-14
- [x] `cargo test -p stackly-storage` → migrasi 10 (fingerprint, fixture `registry-v10.sql`), suite konformasi `sessions` (satu sesi terbuka, `PruneSessions` menyisakan baris terbaru dan lease aktif, satu lease startup per sesi) lulus untuk SQLite dan fake → AC-2, AC-14
- [x] `cargo test -p stackly-core --test session_rotation` → tabel `first`, `new`, `continued`; rotasi kedua di sesi yang sama tidak membuat lease kedua; hanya 50 baris sesi tersisa → AC-2, AC-3, AC-5, AC-14
- [x] `cargo test -p stackly-core --test supervisor_start login` → antrean memakai pemicu `login` dan `reconcile`, `failed` yang tidak eligible tidak masuk, start sistem tidak menulis desired, proses yang `Gone` di sesi baru tidak respawn dan tidak menambah hitungan crash, proses `Proven` dengan desired `stopped` di stop dengan pemicu `session_rotation` → AC-4, AC-6, AC-7
- [x] `cargo test -p stacklyd --test supervisor login` → daemon launchd dengan `FakeLoginSessionInfo`: sesi pertama tidak menyalakan apa pun, login berikutnya menyalakan instance `at_login` sampai `ready` tanpa klien, stop pengguna bertahan di sesi yang sama, login berikutnya menyalakannya lagi; daemon manual tidak pernah membaca sesi; `instance.set_startup` dan `startup` di create → AC-3, AC-4, AC-7, AC-8 (bagian desired), AC-12 (bagian rotasi), AC-16
- [x] `cargo xtask codegen --check` → "20 generated files are up to date" → AC-17
- [x] `cargo test -p stackly-protocol` → fixture round trip untuk `instance.set_startup`, `autostart`, `session`, dan nilai tidak dikenal → AC-17
- [x] `swift test --package-path clients/swift` → semua lulus, termasuk fixture baru → AC-17
- [x] `cargo test -p stackly-cli` → `--autostart` mengirim `startup: at_login`, `instance autostart`, baris `Autostart` di `inspect`, baris `Session` di `daemon status` → AC-18
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter AutostartTests` → tabel teks Autostart, `setAutostart` mengirim `at_login` dan `manual`, tanpa daemon `notSent`; `CreateInstanceModelTests` → `startAtLogin` ikut ke preflight dan create; snapshot `instance-detail-autostart` dan sheet New Instance → AC-19
- [x] `xcrun swift-format lint --strict` dan `cargo xtask build-app` → bersih → AC-19
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih

### Acceptance-criteria coverage

- AC-1 · port, adapter macOS, fake, `UnsupportedPlatform`, cek Windows
- AC-2, AC-3, AC-5, AC-14 · `plan_session_rotation`, migrasi 10, pangkas 50 baris
- AC-4 · antrean start dengan pemicu `login` (fixture); bukti PostgreSQL 17 nyata menunggu record native langkah 14
- AC-6, AC-7 · `session_changed` di tabel rekonsiliasi, antrean level triggered, desired dibaca di bawah lock
- AC-12 · hanya bagian rotasi (konteks manual tidak merotasi); tabel sinyal menunggu langkah 11
- AC-16 · `instance.set_startup` dan `startup` di `instance.create`
- AC-19 · toggle di sheet New Instance dan di Summary beserta teksnya; teks "Won’t start at login" dari state agent dan sesi daemon; Turn Off… menunggu langkah 12
- AC-17 · bagian `startup`, `InstanceAutostart`, `DaemonSessionInfo`, kode `platform.session_unavailable`, 2.4; `agent_off` dan `instance.stop_all` menunggu langkah 9 dan 12
- AC-18 · `--autostart`, `instance autostart`, baris `inspect` dan `daemon status`; `daemon uninstall` menunggu langkah 12
- AC-8, AC-9, AC-10, AC-11, AC-13, AC-15, AC-20, AC-21 · belum, milik langkah lain

### Catatan

- Lease `startup` yang `suppressed` oleh stop pengguna dan lease `manual` ditulis oleh langkah 8; sampai itu, `autostart.session` hanya bernilai `active` atau `none`.
- Subsistem `session` hanya muncul di `daemon.get_health` saat `degraded` (sesi tidak terbaca, kode `platform.session_unavailable`).

## Milestone 2: Stop menahan autostart

Langkah build 8 dan 9. Yang belum: tabel sinyal dan Akhir sesi (langkah 11), `agent_off` dan Turn Off (langkah 12), timer secret (langkah 13), dan semua yang native (AC-21). `expectedInstanceIds`, `instance.stop_all_changed`, dan `stackly instance stop --all` milik spec 0032.

### UI / manual

- [x] Buka `crates/core/src/supervisor/stop.rs` `run_stop` → stop oleh pengguna membaca lease sekali, memakai `manual_stop_ops` (lease manual dilepas, lease startup sesi ini `suppressed`), dan menulisnya dalam commit yang sama dengan desired `stopped`; stop untuk instance yang desired nya `running` tanpa proses hanya mengubah registry dengan efek lease yang sama → AC-8
- [x] Buka `crates/core/src/supervisor/startup.rs` `manual_lease_ops` → hanya `StartTrigger::User` menulis lease manual (tanpa sesi di daemon manual); start sistem, restart, dan restart terencana tidak → AC-9
- [x] Buka `crates/core/src/instance/stop_all.rs` → `select` fungsi murni dengan tujuh aturan berurutan, dan `read_view` yang membaca ulang kalau generation berbeda di antara pembacaan → AC-10
- [x] Buka `crates/core/src/supervisor/stop_all.rs` → satu langkah `stop` yang `irreversible`, paling banyak `RECONCILE_START_CONCURRENCY` bersamaan, `select` dijalankan lagi sebelum setiap stop, cancel tidak memulai langkah baru dan sisanya `skipped` `operation.cancelled` → AC-10
- [x] Buka `apps/daemon/src/ipc/stop_all.rs` → `replay_operation` lebih dulu, `BeginRequest` resource `Manager` dengan `LockKey::Instance` per langkah dan `BusyPolicy::Queue`, `cancel_open_starts` sesudah begin, balasan dibangun dari keadaan akhir operasi (juga untuk replay) → AC-10
- [x] Buka `protocols/contract-versions.json` → versi 2.5 tercatat tanpa `--amend` → AC-17

### Commands

- [x] `cargo test -p stackly-core --test supervisor_start leases` → lease manual di start pengguna (dengan dan tanpa sesi), tidak ada di start daemon, stop melepas lease manual dan menekan lease startup, stop registry saja → AC-8, AC-9
- [x] `cargo test -p stackly-core --test stop_all_select` → setiap baris tabel *Pilihan stop all*, urutan nama lalu ID, demand dan lease startup sesi, tampilan satu instance → AC-10
- [x] `cargo test -p stackly-core --test supervisor_start stop_all` → operasi menghentikan setiap instance dan menulis desired `stopped`; cancel membiarkan stop yang jalan selesai, tidak memulai yang baru, dan sisanya `operation.cancelled` → AC-10
- [x] `cargo test -p stacklyd --test supervisor stop_all` dan `login` → preview dan `instance.stop_all` memilih himpunan yang sama, instance berlease environment `skipped` `supervisor.stop_requires_override` dan tetap jalan, kunci yang sama menjawab sama, pilihan kosong tetap membuat operasi, `autostart.session` menjadi `suppressed` dan instance tidak menyala lagi di sesi yang sama → AC-8, AC-10
- [x] `cargo xtask codegen --check` dan `cargo test -p stackly-protocol` → fixture `instance.preview_stop_all`, `instance.stop_all` (penuh dan sebagian), nilai tidak dikenal, kode `operation.cancelled`, 2.5 → AC-17
- [x] `swift test --package-path clients/swift` → semua lulus, 53 method punya fixture round trip → AC-17
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask evidence check`, dan cek Windows → bersih

### Acceptance-criteria coverage

- AC-8 · stop pengguna dengan lease dan sesi, stop registry saja, `autostart.session`
- AC-9 · lease manual di start pengguna
- AC-10 · `stop_all::select`, `instance.preview_stop_all`, `instance.stop_all`, operasi `stop_all_instances` (cancel start dan restart terbuka, cancel, balasan dari keadaan akhir, replay)
- AC-17 · bagian stop all dan `operation.cancelled`, protokol 2.5
- AC-11, AC-13, AC-15, AC-19 (Turn Off), AC-20, AC-21 · belum

### Catatan

- Langkah `already_stopping` dan `agent_off` dibangun di langkah 12; `select` sudah mengembalikannya dan preview menampilkannya.
- Restart instance (fitur 26) memakai stop tanpa lease: ia tidak menekan autostart.

## Milestone 3: Logout dan kembali

Langkah build 11. Langkah 1 (spike audit session) dan langkah 10 (spike logout) tidak dijalankan atas pilihan kamu pada 2026-11-03, jadi langkah 11 dibangun dengan asumsi desain spec 0031 (logout mengirim `SIGTERM` ke job launchd, dan `kern.bootsessionuuid` serta audit session stabil di sesi yang sama). Yang belum: `agent_off` dan Turn Off (langkah 12), timer secret (langkah 13), dan semua yang native (AC-21).

### UI / manual

- [x] Buka `crates/core/src/supervisor/session_end.rs` → `end_session` membatalkan operasi yang belum final selain stop, memulai satu `stop_instance` per instance dengan proses (`starting`, `running`, `stopping`) sekaligus, dan menunggu sampai semua final atau tenggat; `close_session` menulis `EndSession` sekali per run dan kegagalannya hanya di log → AC-11
- [x] Buka `crates/core/src/supervisor/stop.rs` `StopRequest::session_end` dan `startup.rs` `cancel_cleanup` → stop akhir sesi tanpa cek lease dan tanpa menulis desired; start yang dibatalkan akhir sesi juga tidak menulis desired `stopped` (`Supervisor::cancel_intent`) → AC-11
- [x] Buka `apps/daemon/src/startup.rs` `serve` → `SIGTERM` di daemon launchd tanpa `shutdown_requested` menjalankan `end_session` dari `t0` saat sinyal datang, lalu urutan berhenti spec 0006; `SIGINT`, `SIGHUP`, `daemon.shutdown`, dan restart terencana menutup baris `clean`; sinyal kedua memotong akhir sesi lalu urutan berhenti dipercepat → AC-11, AC-12
- [x] Buka `apps/daemon/src/session.rs` `ends_session` → tabel *Sinyal per konteks* satu fungsi: `SIGTERM` + konteks launchd + `!shutdown_pending()`; `shutdown_pending` sudah true untuk `daemon.shutdown` `stop` dan `restart`, dan rencana restart spec 0030 memakai jalur yang sama → AC-12
- [x] Buka `apps/daemon/src/shutdown.rs` → `StopSignals` mengikat `SIGHUP`; `LAUNCHD_EXIT_TIMEOUT` 30, `SESSION_END_MARGIN` 3, dan `stop_order_bound()` 12 detik (yang lebih panjang dari `SHUTDOWN_DRAIN` dan `OPERATION_SHUTDOWN_GRACE`, ditambah `SHUTDOWN_FLUSH` dan `LOG_CONTROL_TIMEOUT`) → AC-11
- [x] Buka `crates/core/src/session.rs` `plan_session_rotation` cabang `continued` → baris yang tertutup dibuka lagi (`ReopenSession`) dan tidak ada lease yang berubah → AC-13

### Commands

- [x] `cargo test -p stackly-core --test supervisor_start session_end` → akhir sesi menghentikan setiap instance dengan satu `SIGTERM`, desired dan lease tidak berubah, baris `clean` dan hanya ditulis sekali; stop yang melewati tenggat membiarkan instance `stopping` dan baris `interrupted`; start yang sedang berjalan dibatalkan dan desired tetap `running`; kembali di sesi yang sama membuka lagi baris dan antrean start hanya menyalakan yang dihentikan akhir sesi, bukan yang dihentikan pengguna → AC-11, AC-13
- [x] `cargo test -p stacklyd --test supervisor session_end` → logout lewat server nyata: proses berhenti, desired `running`, baris `clean`, lalu daemon baru di sesi yang sama membuka baris dan menyalakan lagi dengan lease `active`; `t0` yang sudah melewati budget menutup baris `interrupted`; `SIGINT`, `SIGHUP`, daemon manual, dan `SIGTERM` sesudah `daemon.shutdown` bukan logout → AC-11, AC-12, AC-13
- [x] `cargo test -p stacklyd --lib shutdown` → `SESSION_END_STOP_BUDGET` 15 + `stop_order_bound()` 12 + margin 3 ≤ `LAUNCHD_EXIT_TIMEOUT` 30 → AC-11
- [x] `cargo test -p stacklyd --test supervisor` dan `--test restart_plan` → seluruh suite tetap hijau (jalur restart terencana dan `agent_off` tidak berubah) → AC-12
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask evidence check`, `cargo xtask codegen --check`, dan cek Windows → bersih

### Langkah yang BLOCKED

- [ ] Langkah 1 · spike audit session lintas sleep dan dua logout → BLOCKED · reason: kamu memilih tidak melakukan sleep dan logout pada 2026-11-03 · owner: @ojiepermana. LaunchAgent `com.stackly.spike.asid` tetap terpasang dan plist nya tidak disentuh.
- [ ] Langkah 10 · spike logout nyata (`SIGTERM` ke job launchd, urutan waktu terhadap `ExitTimeOut`) → BLOCKED · reason: kamu memilih tidak melakukan logout pada 2026-11-03 · owner: @ojiepermana. Asumsi desain yang dipakai langkah 11: `SIGTERM` datang saat logout dan `SIGKILL` baru menyusul sesudah `ExitTimeOut` 30 detik.

### Acceptance-criteria coverage

- AC-11 · urutan *Akhir sesi* (langkah 1 sampai 5 di core dan daemon, langkah 6 oleh urutan berhenti spec 0006), budget 15 detik dari `t0`, relasi konstanta, baris `clean` atau `interrupted`, tanpa `SIGKILL` (hanya `SIGTERM` lewat strategi provider), tanpa suppression, tanpa mengubah desired
- AC-12 · tabel *Sinyal per konteks* dengan `shutdown_pending`; daemon manual tidak pernah menjalankan akhir sesi; `SIGHUP` kini ikut menghentikan daemon
- AC-13 · `ReopenSession` di rotasi `continued` dan antrean start sesudah akhir sesi
- AC-21 · hanya bagian spike, dan BLOCKED (langkah 1 dan 10 di atas)
- AC-15, AC-19 (Turn Off), AC-20 · belum

### Catatan

- Spec menyebut idempotency key `session_end:<SessionId>:<InstanceId>` untuk setiap stop. Engine hanya mengikat key pada `Origin::Request`, dan origin itu kena gerbang quiesce restart yang justru harus dibatalkan akhir sesi, jadi stop akhir sesi memakai `Origin::Background` tanpa key. Satu daemon hanya menjalankan akhir sesi sekali (sinyal kedua memotongnya), jadi tidak ada yang perlu di-replay.
- Instance `recovering` (jeda antar respawn) tidak punya proses, jadi tidak ikut stop; urutan berhenti spec 0006 mematikan respawn nya. Instance `unknown` dan `retained` juga tidak disentuh.
- Baris ditutup `interrupted` bila ada stop yang belum final saat tenggat atau gagal; selain itu `clean`. Instance yang masih `stopping` dibiarkan dan rekonsiliasi berikutnya membacanya sebagai `exited` (spec 0010).
- Kalau sinyal datang sebelum slot start selesai merekonsiliasi (supervisor belum siap), akhir sesi hanya menutup pintu dan tidak menghentikan instance, lalu urutan berhenti biasa berjalan.
- Kode memakai `StopIntent::PlannedRestart` untuk stop akhir sesi karena artinya sama (tanpa cek lease, tanpa menulis desired); `StopRequest::session_end` menamainya.

## Milestone 4: Agent off dan Keychain terkunci

Langkah build 12 dan 13. Langkah 13 (Keychain terkunci) dikerjakan lebih dulu; langkah 12 menunggu kontrak (`ShutdownMode::AgentOff`) dari F32.

### Langkah 13: Keychain terkunci (AC-15)

#### UI / manual

- [ ] Buka `crates/core/src/supervisor/secret_retry.rs` → satu task per supervisor, setiap `LOGIN_SECRET_RETRY_INTERVAL` (30 detik, `SupervisorConfig::login_secret_retry_interval`) membaca ulang secret instance yang blocker `needsSecret` nya hanya `store_locked` atau `store_busy`, lewat `SecretCaller::background()` (interaksi `Never`), tanpa operasi; start `secret_retry` dimulai hanya kalau secret terbaca, lewat `StartRequest::background` sehingga tidak dihitung ke budget restart → AC-15
- [ ] Buka `retry_one` → desired `stopped` menghapus blocker dan menghentikan timer; alasan yang berubah menjadi `missing` atau `access_denied` menghentikan pembacaan dan desired tidak diubah → AC-15
- [ ] Buka `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/SecretNeed.swift` `keychainWaitNote` dan `InstanceDetailBanner.swift` → banner "Needs a secret to start" memuat "The Keychain is locked. Stackly tries again every 30 seconds." hanya kalau semua alasan `store_locked` atau `store_busy`; tombol Enter Secret… tetap → AC-15

#### Commands

- [ ] `cargo test -p stackly-core --test supervisor_start secret_retry` → store terkunci: instance `stopped`, desired `running`, blocker `store_locked`, beberapa interval tanpa operasi baru dan tanpa spawn; buka kunci: tepat satu start dengan trigger `secret_retry`, blocker hilang; stop pengguna menghapus blocker dan tidak ada start; `access_denied` menghentikan pembacaan (jumlah panggilan store tidak naik) dan desired tetap `running` → AC-15
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter KeychainWaitTests` → kalimat hanya untuk `store_locked` dan `store_busy` → AC-15

#### Native

- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test supervisor native_keychain -- --ignored --test-threads 1` → keychain file di root sementara (login Keychain tidak disentuh), dikunci dengan `security lock-keychain`, lalu dibuka dengan `security unlock-keychain` → AC-15 · run 2026-11-03: PASS (macOS Keychain, subject `macos-keychain`). Pada run pertama tes ini GAGAL karena pembacaan dari keychain yang dikunci dari proses lain menjawab `Backend { code: -60008 }` (`errAuthorizationInternal`) walau `state()` sesudahnya `Locked`. Sesudah persetujuan user (2026-11-03) `map_status` di `crates/platform-macos/src/secret.rs` memetakan status lain apa pun ke `Locked` bila keychain terkunci (pemeriksaan state hanya sesudah panggilan gagal; pemeriksaan yang gagal tetap `Backend`), dengan unit test di tabel `map_status_follows_the_spec_table`. Run kedua: start login meninggalkan instance `stopped` dengan blocker `needsSecret`, dan sesudah `security unlock-keychain` start `secret_retry` menjalankannya dan blocker hilang.

### Langkah 12: Agent off dan Turn Off (AC-19, AC-20)

Protokol 3.1 (`ShutdownMode::AgentOff`, `ShutdownAccepted { exiting, stopAll }` tidak lagi `Copy`, kode `cli.agent_off_blocked`) dibangun di atas 3.0 dari F32.

#### UI / manual

- [ ] Buka `crates/core/src/operation/mod.rs` `begin_bypassing_quiesce` dan `scheduler.rs` → gerbang quiesce spec 0030 dilewati hanya lewat metode ini; `begin` biasa tetap menolak `Origin::Request` selama quiesce → AC-20
- [ ] Buka `apps/daemon/src/ipc/agent_off.rs` → `shutdown` (handler `daemon.shutdown` `agent_off`, sama di konteks launchd dan manual): `replay_operation` lebih dulu; `registry.recovery_mode`, `daemon.shutting_down`, `daemon.restart_pending` tanpa efek; satu `agent_off` per daemon dan gerbang ditutup; satu `stop_all_instances` (sumber `agent_off`) untuk himpunan `stop` ditambah `already_stopping`; `select` sekali lagi; keluar hanya kalau tidak ada `failed`, `skipped`, `stop`, `already_stopping`, atau `skipped` baru → AC-20
- [ ] Buka `apps/daemon/src/ipc/operation.rs` `start_operation_gated` → selama `agent_off` aktif gerbang menjawab `daemon.shutting_down`, bukan `daemon.restart_pending` → AC-20
- [ ] Buka `apps/cli/src/commands/daemon/uninstall.rs` → konfirmasi destruktif lewat `Confirm`, `--yes`, tidak mengirim apa pun bila preview punya `skipped`, unregister (`AgentEnv::unregister`) hanya sesudah `exiting: true` dan socket hilang, daemon tidak terjangkau: agent `requires_approval` atau `disabled` langsung unregister, `enabled` gagal dengan kesalahan terhubung → AC-18, AC-20
- [ ] Buka `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Agent/TurnOffPlan.swift` dan `TurnOffModel.swift` → judul "Turn Off Background Agent?", isi dari `items` plus `alreadyStopping` dan jumlah instance `at_login`, tombol nonaktif bila `skipped` tidak kosong; kunci idempotensi yang sama dipakai ulang sesudah jawaban yang hilang; agent di-unregister hanya bila `exiting` true dan daemon sudah hilang → AC-19, AC-20
- [ ] Buka Diagnostics, bagian Background Agent dengan agent `enabled` → tombol "Turn Off…" ada di samping Check Again; dialog konfirmasi destruktif mendaftar dampaknya; `exiting: false` menampilkan kalimat "Background agent is still on. …" → AC-19, AC-20

#### Commands

- [ ] `cargo test -p stacklyd --test supervisor agent_off` → lewat socket: tanpa kunci `operation.idempotency_key_required`; dengan kunci instance berhenti dan balasan `exiting: true` dengan `stopAll`, lalu daemon meminta stop; dalam proses: kunci yang sama menjawab sama, permintaan baru sesudahnya `daemon.shutting_down`; instance berlease environment membuat `exiting` false, gerbang dan klaim dicabut; quiesce restart menolak dengan `daemon.restart_pending` dan `agent_off` kedua dengan `daemon.shutting_down` → AC-20
- [ ] `cargo test -p stackly-cli` → enam skenario `daemon uninstall` (unregister sesudah exit, tetap terdaftar dengan `cli.agent_off_blocked`, `skipped` menahan sebelum mengirim, ditolak konfirmasi, tanpa daemon, tidak terdaftar) → AC-18, AC-20
- [ ] `cargo xtask codegen --check` dan `cargo test -p stackly-protocol` → fixture `daemon.shutdown` `agent_off` (permintaan, jawaban keluar, jawaban tetap), jawaban `stop` dan `restart` membawa `exiting: true`, kode `cli.agent_off_blocked`, 3.1 → AC-17
- [ ] `swift test --package-path clients/swift` dan `--filter TurnOffModelTests`, `TurnOffPlanTests` pada StacklyUI → semua lulus → AC-17, AC-19, AC-20
