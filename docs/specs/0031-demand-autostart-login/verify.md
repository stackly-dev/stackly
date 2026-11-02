# Verify: Demand, autostart, dan login · spec 0031 · updated 2026-11-02

_Langkah diturunkan dari acceptance criteria spec 0031. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah._

## Milestone 1: Login menyalakan instance terpilih

Langkah build 2 sampai 7. Bagian AC-19 yang belum: Turn Off… di Diagnostics (langkah 12). Semua yang native (AC-21) belum.

### UI / manual

- [ ] Buka `crates/platform-macos/src/session.rs` → `getaudit_addr` dibaca dengan satu blok `unsafe` ber `// SAFETY:`; audit session `0` atau negatif menjadi `LoginSessionError::Unavailable` → AC-1
- [ ] Buka `crates/storage/migrations/0010_daemon_sessions.sql` → tabel `daemon_sessions` dengan CHECK `(ended_at IS NULL) = (end_reason IS NULL)`, unique index parsial `daemon_sessions_open`, dan unique index parsial `instance_demands_startup_session` → AC-2, AC-14
- [ ] Buka `crates/core/src/session.rs` → `plan_session_rotation` fungsi murni dan `rotate_session` menjalankannya sebagai satu `commit_guarded`; tidak ada pemanggilan proses → AC-2, AC-3
- [ ] Buka `apps/daemon/src/server.rs` (start slot) → urutan rotasi, `reconcile_in_session`, `queue_login_starts`, lalu `start_reconciled`; rotasi hanya untuk konteks launchd dan registry `ready` → AC-2, AC-7
- [ ] Buka `crates/core/src/supervisor/startup.rs` `run_start` → start dengan `StartTrigger::Background` membaca desired lagi dan selesai `succeeded` tanpa menulis desired kalau bukan `running` → AC-7
- [ ] Buka `protocols/contract-versions.json` → versi 2.4 tercatat; `ProtocolVersion::CURRENT` 2.4 → AC-17
- [ ] Jalankan `target/debug/stackly instance autostart --help` → argumen `<target>` dan `on|off` → AC-18
- [ ] Buka sheet New Instance → toggle "Start at login" (default mati, teks "Separate from the daemon’s own login item.") di bagian yang sama dengan "Start after creating"; menyalakannya membuat perintah setara memuat `--autostart` dan `instance.create` membawa `startup: at_login` → AC-19
- [ ] Buka tab Summary sebuah instance → baris Autostart di kartu Details adalah toggle; teks kecil di bawahnya sesuai tabel *Copy* ("Stopped for this session. Starts at your next login.", "Starts at your next login.", "Won’t start at login: the background agent is off." atau "…needs approval in Login Items.") → AC-19

### Commands

- [ ] `cargo test -p stackly-platform session` dan `cargo test -p stackly-platform-macos --lib session` → ID sesi `<boot>/<asid>` paling panjang 47 byte, audit session 0 ditolak, platform tanpa adapter menjawab `NotSupported`, macOS membaca sesi kalau proses punya → AC-1
- [ ] `cargo test -p stackly-domain session` → `DaemonSession` dibuka, ditutup, dibuka lagi; kode alasan berakhir → AC-14
- [ ] `cargo test -p stackly-storage` → migrasi 10 (fingerprint, fixture `registry-v10.sql`), suite konformasi `sessions` (satu sesi terbuka, `PruneSessions` menyisakan baris terbaru dan lease aktif, satu lease startup per sesi) lulus untuk SQLite dan fake → AC-2, AC-14
- [ ] `cargo test -p stackly-core --test session_rotation` → tabel `first`, `new`, `continued`; rotasi kedua di sesi yang sama tidak membuat lease kedua; hanya 50 baris sesi tersisa → AC-2, AC-3, AC-5, AC-14
- [ ] `cargo test -p stackly-core --test supervisor_start login` → antrean memakai pemicu `login` dan `reconcile`, `failed` yang tidak eligible tidak masuk, start sistem tidak menulis desired, proses yang `Gone` di sesi baru tidak respawn dan tidak menambah hitungan crash, proses `Proven` dengan desired `stopped` di stop dengan pemicu `session_rotation` → AC-4, AC-6, AC-7
- [ ] `cargo test -p stacklyd --test supervisor login` → daemon launchd dengan `FakeLoginSessionInfo`: sesi pertama tidak menyalakan apa pun, login berikutnya menyalakan instance `at_login` sampai `ready` tanpa klien, stop pengguna bertahan di sesi yang sama, login berikutnya menyalakannya lagi; daemon manual tidak pernah membaca sesi; `instance.set_startup` dan `startup` di create → AC-3, AC-4, AC-7, AC-8 (bagian desired), AC-12 (bagian rotasi), AC-16
- [ ] `cargo xtask codegen --check` → "20 generated files are up to date" → AC-17
- [ ] `cargo test -p stackly-protocol` → fixture round trip untuk `instance.set_startup`, `autostart`, `session`, dan nilai tidak dikenal → AC-17
- [ ] `swift test --package-path clients/swift` → semua lulus, termasuk fixture baru → AC-17
- [ ] `cargo test -p stackly-cli` → `--autostart` mengirim `startup: at_login`, `instance autostart`, baris `Autostart` di `inspect`, baris `Session` di `daemon status` → AC-18
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter AutostartTests` → tabel teks Autostart, `setAutostart` mengirim `at_login` dan `manual`, tanpa daemon `notSent`; `CreateInstanceModelTests` → `startAtLogin` ikut ke preflight dan create; snapshot `instance-detail-autostart` dan sheet New Instance → AC-19
- [ ] `xcrun swift-format lint --strict` dan `cargo xtask build-app` → bersih → AC-19
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih

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

- [ ] Buka `crates/core/src/supervisor/stop.rs` `run_stop` → stop oleh pengguna membaca lease sekali, memakai `manual_stop_ops` (lease manual dilepas, lease startup sesi ini `suppressed`), dan menulisnya dalam commit yang sama dengan desired `stopped`; stop untuk instance yang desired nya `running` tanpa proses hanya mengubah registry dengan efek lease yang sama → AC-8
- [ ] Buka `crates/core/src/supervisor/startup.rs` `manual_lease_ops` → hanya `StartTrigger::User` menulis lease manual (tanpa sesi di daemon manual); start sistem, restart, dan restart terencana tidak → AC-9
- [ ] Buka `crates/core/src/instance/stop_all.rs` → `select` fungsi murni dengan tujuh aturan berurutan, dan `read_view` yang membaca ulang kalau generation berbeda di antara pembacaan → AC-10
- [ ] Buka `crates/core/src/supervisor/stop_all.rs` → satu langkah `stop` yang `irreversible`, paling banyak `RECONCILE_START_CONCURRENCY` bersamaan, `select` dijalankan lagi sebelum setiap stop, cancel tidak memulai langkah baru dan sisanya `skipped` `operation.cancelled` → AC-10
- [ ] Buka `apps/daemon/src/ipc/stop_all.rs` → `replay_operation` lebih dulu, `BeginRequest` resource `Manager` dengan `LockKey::Instance` per langkah dan `BusyPolicy::Queue`, `cancel_open_starts` sesudah begin, balasan dibangun dari keadaan akhir operasi (juga untuk replay) → AC-10
- [ ] Buka `protocols/contract-versions.json` → versi 2.5 tercatat tanpa `--amend` → AC-17

### Commands

- [ ] `cargo test -p stackly-core --test supervisor_start leases` → lease manual di start pengguna (dengan dan tanpa sesi), tidak ada di start daemon, stop melepas lease manual dan menekan lease startup, stop registry saja → AC-8, AC-9
- [ ] `cargo test -p stackly-core --test stop_all_select` → setiap baris tabel *Pilihan stop all*, urutan nama lalu ID, demand dan lease startup sesi, tampilan satu instance → AC-10
- [ ] `cargo test -p stackly-core --test supervisor_start stop_all` → operasi menghentikan setiap instance dan menulis desired `stopped`; cancel membiarkan stop yang jalan selesai, tidak memulai yang baru, dan sisanya `operation.cancelled` → AC-10
- [ ] `cargo test -p stacklyd --test supervisor stop_all` dan `login` → preview dan `instance.stop_all` memilih himpunan yang sama, instance berlease environment `skipped` `supervisor.stop_requires_override` dan tetap jalan, kunci yang sama menjawab sama, pilihan kosong tetap membuat operasi, `autostart.session` menjadi `suppressed` dan instance tidak menyala lagi di sesi yang sama → AC-8, AC-10
- [ ] `cargo xtask codegen --check` dan `cargo test -p stackly-protocol` → fixture `instance.preview_stop_all`, `instance.stop_all` (penuh dan sebagian), nilai tidak dikenal, kode `operation.cancelled`, 2.5 → AC-17
- [ ] `swift test --package-path clients/swift` → semua lulus, 53 method punya fixture round trip → AC-17
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask evidence check`, dan cek Windows → bersih

### Acceptance-criteria coverage

- AC-8 · stop pengguna dengan lease dan sesi, stop registry saja, `autostart.session`
- AC-9 · lease manual di start pengguna
- AC-10 · `stop_all::select`, `instance.preview_stop_all`, `instance.stop_all`, operasi `stop_all_instances` (cancel start dan restart terbuka, cancel, balasan dari keadaan akhir, replay)
- AC-17 · bagian stop all dan `operation.cancelled`, protokol 2.5
- AC-11, AC-13, AC-15, AC-19 (Turn Off), AC-20, AC-21 · belum

### Catatan

- Langkah `already_stopping` dan `agent_off` dibangun di langkah 12; `select` sudah mengembalikannya dan preview menampilkannya.
- Restart instance (fitur 26) memakai stop tanpa lease: ia tidak menekan autostart.
