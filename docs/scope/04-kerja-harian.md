# Scope 04 · Kerja harian

[Kembali ke index](index.md)

Journey yang paling sering dipakai setelah instance ada: stop, restart, lihat log, pantau metrik, ubah config, dan percaya bahwa instance tetap benar walau GUI ditutup, daemon restart, Mac login ulang, atau tidak ada koneksi internet. Fase ini selesai kalau seluruh pekerjaan harian bisa dilakukan dari GUI, CLI, dan menu bar dengan state yang jujur.

### 26. Stop, restart & reload · done

Lifecycle lengkap sesuai capability provider: stop graceful dengan timeout, restart yang diserialisasi, reload hanya kalau provider mendukung (sesuai capability dari spec fitur 9 dan 10), dan opsi force dengan risiko yang jelas setelah timeout.

**Done when:** stop normal menjalankan strategi provider dan melaporkan hasilnya; stop dan start yang bersamaan diserialisasi; perilaku reload dicoba pada PostgreSQL (mendukung reload) dan NATS (tidak mendukung), dan provider tanpa hot reload meminta restart terkontrol, bukan klaim reload palsu; stop dan restart tidak memerlukan jaringan; force kill tidak pernah menyasar proses yang identitasnya belum terverifikasi; `stackly instance restart` dan `reload` setara dengan GUI; alur stop dan inspect bisa diselesaikan dengan interaksi desktop; tombol start atau stop, restart, dan reload di toolbar detail instance mengikuti mockup UI, dan reload nonaktif beserta alasannya untuk provider yang tidak mendukungnya.

**Trace:** REQ-011, REQ-028, REQ-046 (offline), BR-016, AC-011, AC-026, AC-027, AC-046, AC-067, NFR-012

**Pointer:** spec [0026](../specs/0026-stop-restart-reload/index.md) · code in `crates/core/src/supervisor/{force,restart_op,reload,persist}.rs`, `apps/daemon/src/ipc/instance/mod.rs`, `apps/cli/src/commands/instance/action.rs`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Instances/InstanceLifecycleActions.swift`

- [x] Design it (spec): `/architect stop, restart & reload` · spec 0026, two cross checks (Sonnet, Fable 5.1) applied, accepted 2026-11-01
- [x] Build it: `/develop stop, restart & reload` · 4329277f on `feat/f26-stop-restart-reload`
  - [x] Domain, registry, platform, and core: `ExitKind` and migration `0006_instance_last_exit.sql`, `OperationKind::ForceStopInstance`, the `force_kill` port with `FORCE_KILL_ALLOWED` in `check-deps`, `OperationEngine::force_intent` and the `ExitKind` table on every `exited` path, `supervisor::force`, `supervisor::restart_op`, and the reload lifecycle codes (spec 0026 AC-2 to AC-8, AC-10; build steps 1 to 7)
  - [x] Contract, daemon, CLI, and fixture proof: `instance.restart`, `instance.reload`, `instance.force_stop` with their params, codes, `InstanceSummary.capabilities` and `lastExit`, protocol 1.16 and codegen, the `stacklyd` handlers, socket tests for force, restart, serialization, and the offline harness, then `stackly instance restart`, `reload`, and `stop --force` (spec 0026 AC-1, AC-2, AC-4, AC-6 to AC-13; build steps 8 to 11; after milestone 1)
  - [x] GUI: `InstanceLifecycleActions.resolve`, the Restart and Reload toolbar buttons, the stopping banner with the Force Stop confirmation, feedback cards, the Last exit row, the Instance menu items, and view fixtures (spec 0026 AC-14 to AC-18; build steps 12 to 15; after milestone 2)
  - [x] PostgreSQL 18 and NATS on this Mac: stop with an active connection, restart, and reload on PostgreSQL 18; reload refused and restart on NATS; the network off run after your approval (spec 0026 AC-1, AC-6, AC-10 to AC-12; build steps 16 to 18; after milestone 2; runtime artifacts in the xtask cache, else `BLOCKED`) · native PASS on this Mac (2026-11-01): PostgreSQL 18 stop clean with a session open, restart with the sentinel kept, reload with the same PID; NATS reload refused with no operation and restart; the network off run and the pointer flow moved to `docs/manual.md` section V (not run yet)
- [ ] Verify it: `/check verify stop, restart & reload` · skipped for this session by the user (2026-11-01)
- [ ] Test it: `/test stop, restart & reload` · skipped for this session by the user (2026-11-01)

### 27. Log per instance · in-progress

Log terpisah untuk tiap instance dengan follow, cursor, filter dasar, timestamp, pemisahan stdout dan stderr, batas ukuran, dan rotasi. Subscriber yang lambat tidak boleh menahan pipe proses.

**Done when:** log besar dengan client lambat tidak membuat daemon lambat atau memori tumbuh tanpa batas; rotasi berjalan (default usulan 5 file × 20 MiB); segmen yang hilang ditandai sebagai truncation; `stackly instance logs --follow` dan layar Logs setara; secret tidak pernah muncul; layar Logs mengikuti mockup UI (sumber All, stdout, dan stderr, filter, Follow, Load Earlier, serta cursor dan info retention di footer).

**Trace:** REQ-017, PAGE-07, NFR-004, AC-017, PRD bagian 35.2

**Pointer:** spec [0027](../specs/0027-log-per-instance/index.md)

- [x] Design it (spec): `/architect log per instance` · spec 0027, cross check (Sonnet) applied, accepted 2026-11-01
- [ ] Build it: `/develop log per instance`
  - [x] Store in core: the spec 0010 changes (`LogBatch.source`, `LogLine.end`, `LogSink::run_event`, redaction overlap at the 64 KiB cut), record format, `level_of`, filter, the per instance store with rotation, recovery, exact dedupe, the bounded write queue and writer thread, the page reader with merge and scan budget, the follow source, and the `FileSystem::create_file_private` and `open_append_private` port methods (spec 0027 AC-1 to AC-9, AC-12, AC-13; build steps 1 to 4; steps 1 and 2 can run as two parallel sessions) · code in `crates/core/src/logs/`, tests in `crates/core/tests/logs_store.rs` and `supervisor_spool.rs`
  - [x] Contract and daemon: `stackly-protocol::logs`, `LogSeq`, `instance.logs_read`, `instance.logs_follow`, `instance.logs_unfollow`, `ServerFrame::Log`, `instance.logs_unreadable`, subsystem `instance_logs`, minor bump and codegen, the spec 0009 `captured` rule, then `stacklyd::ipc::logs` with the shared subscription counter, the follow tasks, the `DurableLogSink` in `SinkSet`, and socket tests including crash, rotation, failing writes, and the fixture credential (spec 0027 AC-1 to AC-9, AC-12 to AC-14; build steps 5 and 6; after milestone 1) · protocol 1.18, code in `crates/protocol/src/logs.rs` and `apps/daemon/src/ipc/logs.rs`, tests in `apps/daemon/tests/supervisor/durable_logs.rs`; the crash test with `SpawnedDaemon` and the `ended` frame over the socket are not built yet (no delete method exists; AC-4 is covered by `crates/core/tests/logs_store.rs`)
  - [x] CLI: `stackly instance logs` with tail, follow, stream, grep, cursors, text and JSON output, Ctrl+C exit 0 (spec 0027 AC-9, AC-10; build step 7; after milestone 2) · code in `apps/cli/src/commands/instance/logs.rs`, test in `apps/cli/tests/instance/logs_real.rs`
  - [x] GUI: `StacklyClient` log methods and the `log` frame, then the Logs tab with `InstanceLogsModel`, Copy Shown Lines, every state, and the spec 0068 view fixtures (spec 0027 AC-7, AC-9, AC-11; build steps 8 and 9; after milestone 2; can run in parallel with milestone 3) · client in `clients/swift/Sources/StacklyClient/Link/` (`DaemonLink.followLogs`, `LogFollowSession`), tab in `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Instances/Logs/`, fixtures in `Fixtures/FixtureLogs.swift`, tests in `Tests/StacklyUITests/Instances/Logs/` and `SnapshotTests/InstanceLogsSnapshotTests.swift`
  - [ ] Evidence on this Mac: the slow client evidence run, the PostgreSQL 17 native test after your approval, and a manual GUI session comparing the Logs tab with `stackly instance logs --follow` (spec 0027 AC-3, AC-6, AC-8, AC-11; build step 10; after milestones 3 and 4; without the PostgreSQL artifact in the xtask runtime cache the native test ends `BLOCKED`) · slow client run PASS on 2026-11-02 against the `stacklyd` binary (get_info p99 1 ms, daemon footprint +15 MiB, fake at 100 percent, silent follower closed by `ipc.write_timeout`), test in `apps/daemon/tests/supervisor/slow_logs.rs` with the fake setting `logLineBytes`; PostgreSQL 17.11 native PASS on 2026-11-02, test in `apps/daemon/tests/postgresql/logs.rs`; the manual GUI session is still open
- [ ] Verify it: `/check verify log per instance`
- [ ] Test it: `/test log per instance`

### 28. Metrics & health · needs a decision

Status, readiness, process tree, uptime, restart count, kegagalan terakhir, CPU, memori, dan pemakaian disk terjadwal, dengan label cakupan yang jelas antara proses utama dan agregat child.

**Done when:** metrik yang tidak tersedia tampil unavailable, bukan nol, termasuk di `stackly instance metrics --json`; label menjelaskan cakupan agregasi untuk provider multiprocess; health punya timestamp dan alasan; cadence probe dan sampling default (usulan 5 detik) tidak melampaui anggaran NFR-003 pada 10 instance idle; setelah Mac bangun dari sleep, endpoint dicek ulang tanpa mengganti port fixed; layar Metrics mengikuti mockup UI, termasuk pilihan Main Process dan Process Tree, metrik unavailable yang diberi alasan, dan state tanpa metrik saat instance tidak running.

**Trace:** REQ-018, REQ-028, PAGE-07, NFR-003, AC-018, PRD bagian 19.5 dan 35.2

- [ ] Design it (spec): `/architect metrics & health`

### 29. Edit config, nama & endpoint · in-progress

Form typed dan advanced view dari schema provider, memakai renderer form yang sama dengan fitur 20, dengan diff, validasi, revision, dampak reload atau restart, histori minimal last known good, dan deteksi drift. Termasuk ganti nama (label saja; ID, runtime, credential reference, dan storage path tetap) dan ganti endpoint atau port dengan validasi yang sama seperti create.

**Done when:** config yang tidak valid tidak mengganti config aktif, dan error menunjuk field atau baris yang relevan; field managed penting tidak bisa ditimpa override tanpa validasi; drift tampil sebagai peringatan; form typed dan last known good diuji pada PostgreSQL, NATS, dan schema Traefik dengan `settings.routes` berbentuk daftar, tanpa patch SwiftUI; rename tidak memindahkan data atau mengubah ID; ganti port fixed melewati validasi konflik, menampilkan dampak restart, dan tercatat sebagai operasi ganti port (aturan untuk instance yang dipinjam environment menyusul di fitur 47); `stackly instance config get`, `validate`, dan `apply` setara dengan GUI; layar Configuration mengikuti mockup UI: mode Form dan Effective File, revision, label dampak, Revert, tombol Apply and Reload atau Apply and Restart, field managed yang terkunci, Pending Changes, History, dan banner drift yang menunggu pilihan eksplisit pengguna.

**Trace:** REQ-010, REQ-015, REQ-028, PAGE-06, VAL-012, BR-003, BR-017, AC-015, AC-026, AC-045, RISK-07, PRD bagian 7.1

**Pointer:** spec [0029](../specs/0029-edit-config-nama-endpoint/index.md)

- [x] Design it (spec): `/architect edit config, nama & endpoint` · spec 0029, cross check (Fable 5.1) applied, accepted 2026-11-02
- [ ] Build it: `/develop edit config, nama & endpoint`
  - [ ] Domain, registry, and core: config revision number, endpoint snapshot, `ready_at`, `origin`, the migration (next free number), `ActivateConfigRevision` through the aggregate, prune to 20, fake provider `0.4.0` with `fake.conf`, the config evaluator with two new `FieldCode` variants, per file history copies and drift checks with the `ConfigDrift` blocker, `applied_revision` and `MarkConfigRevisionReady`, and the `apply_config`, `change_endpoints`, `rename_instance`, and `resolve_config_drift` operations on spec 0026's reload and restart steps (spec 0029 AC-1 to AC-16, AC-20; build steps 1 to 7)
  - [ ] Contract, daemon, CLI, and fixture proof: six methods, DTOs, codes, protocol minor bump, fixtures, handlers, socket tests with the offline harness, and `stackly instance config get|validate|apply|rollback|keep-revision`, `rename`, `endpoint set` (spec 0029 AC-1 to AC-14, AC-17, AC-20, AC-21; build steps 8 to 11; after milestone 1)
  - [ ] GUI: `StacklyClient`, the Configuration tab (Form, Effective File, Pending Changes, History, drift and roll back banners), the rename and ports sheets, menu items, the Overview drift item, and view fixtures (spec 0029 AC-15, AC-16, AC-18, AC-19; build steps 12 to 15; after milestone 2; needs features 67 to 69 in `ui-macos.md`)
  - [ ] PostgreSQL 18, NATS, and Traefik on this Mac, then the network off run after your approval (spec 0029 AC-4, AC-5, AC-7, AC-8, AC-11, AC-14, AC-16, AC-21; build steps 16 to 19; after milestone 2; runtime artifacts in the xtask cache, else `BLOCKED`)
- [ ] Verify it: `/check verify edit config, nama & endpoint`
- [ ] Test it: `/test edit config, nama & endpoint`

### 30. Reconciliation & restart daemon terencana · needs a decision · GA

Hardening rekonsiliasi dasar dari fitur 10 untuk instance nyata: setelah daemon restart atau crash, rekonsiliasi proses, runtime, endpoint, storage, operasi, dan desired state, plus restart daemon terencana dengan jendela disruption yang terlihat.

**Done when:** menutup dan membuka GUI tidak mengubah PID atau identitas instance; setelah daemon crash tidak ada instance ganda atau data yang terhapus; PID yang kini milik executable lain tidak dibunuh, dan instance dilaporkan unknown beserta buktinya; `stackly daemon restart` terencana menghentikan instance terdampak dengan aman selama handover supervisor belum terbukti, memulihkan desired state sesudahnya, dan GUI serta CLI menampilkan jendela disruption, bukan klaim zero downtime; inventory tersedia dalam target NFR-007.

**Trace:** REQ-001, REQ-013, AC-001, AC-013, AC-067, NFR-007, OQ-007, RISK-03, PRD bagian 19.4 dan 36

- [ ] Design it (spec): `/architect reconciliation & restart daemon terencana`

### 31. Demand, autostart & login · needs a decision

Autostart daemon dan autostart instance sebagai dua setting berbeda, di atas model demand dengan tiga sumber (environment, manual, startup) dan suppression. Aturan evaluasi stop manual terhadap seluruh demand aktif diputuskan di sini; lease environment hanya menjadi sumber demand baru di fitur 46 dan 47.

**Done when:** setelah login dan izin tersedia, daemon serta instance terpilih ready; izin yang dicabut terlihat di UI; stop manual tidak langsung dinyalakan lagi oleh reconciler di sesi yang sama; Keychain yang terkunci menghasilkan needs secret, bukan auth kosong; logout mengakhiri sesi dengan perilaku instance yang terdefinisi dan batas disruption yang terdokumentasi, dan login berikutnya memulihkan desired state tanpa instance ganda.

**Trace:** REQ-014, REQ-022 (bagian evaluasi demand), BR-024, AC-014, AC-062, AC-069, PRD bagian 13.2, 17, 19.5, dan 32.2

- [ ] Design it (spec): `/architect demand, autostart & login`

### 32. Menu bar & Settings · needs a decision

Status ringkas di menu bar dengan aksi buka aplikasi, **Quit App** yang hanya menutup GUI, dan **Stop Services** sebagai aksi terpisah dengan impact preview. Halaman Settings untuk background startup, default autostart, sumber catalog dan daftar trust provider (import lokal hadir di fitur 37), path data, cache, dan log, retention, update channel (disimpan sebagai preferensi sampai fitur 61), dan privasi, plus notifikasi opsional yang mengikuti izin.

**Done when:** Quit App tidak menghentikan service; Stop Services menampilkan daftar instance yang akan berhenti berdasarkan demand aktif (fitur 31) sebelum berjalan; mengubah path data, cache, atau log hanya berlaku untuk instance baru dan tidak memindahkan data yang ada; Settings menjelaskan perilaku autostart pada login berikutnya; notifikasi bisa dimatikan dan hanya muncul setelah izin notifikasi macOS diberikan; telemetry mati secara default; popover menu bar dan jendela Settings (tab General, Instances, Sources, Storage, Updates) mengikuti mockup UI; Appearance di Settings mengikuti sistem secara default dengan pilihan Light atau Dark; baris setting milik fitur yang belum dibangun tidak tampil seolah sudah bekerja.

**Trace:** REQ-027 (menu bar, notifikasi), REQ-046 (privasi), PAGE-14 (bagian yang tersedia di fase 04), NFR-016

- [ ] Design it (spec): `/architect menu bar & Settings`

### 70. Buka instance di aplikasi klien · needs a decision

Menu aksi per service di halaman Environments, seperti di mockup UI, untuk membuka instance di aplikasi lain: klien database (mockup mencontohkan TablePlus, Postico, Sequel Ace, Redis Insight), CLI provider di Terminal, monitoring atau dashboard di browser, dan file routes Traefik di Finder. Tambahan dari mockup UI, belum ada di PRD.

**Done when:** daftar aksi berasal dari capability provider, bukan kode SwiftUI per provider; aplikasi yang tidak terpasang tidak ditawarkan; aksi nonaktif saat instance tidak running; password tidak pernah dititipkan lewat URL, argumen command, atau daftar proses, dan kalau sebuah aplikasi hanya menerima secret lewat cara itu, alurnya meminta aksi eksplisit dengan peringatan; CLI di Terminal memakai binary dari runtime instance itu, bukan dari PATH.

**Trace:** REQ-016, REQ-027, REQ-029, BR-018

- [ ] Design it (spec): `/architect buka instance di aplikasi klien`
