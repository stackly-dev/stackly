# 0031. Demand, autostart, dan login

**Date**: 2026-11-02
**Status**: In Progress

## Summary

Spec ini menetapkan apa yang terjadi pada instance saat kamu login, stop, dan logout. Daemon mengenali sesi login lewat `SessionId` (UUID boot ditambah audit session macOS). Di setiap sesi baru, instance dengan autostart At login dinyalakan, dan instance dengan autostart Off tetap mati. Stop yang kamu lakukan menahan autostart sampai login berikutnya (suppression). Logout menghentikan instance dengan bersih di dalam jendela launchd. Autostart daemon tetap setting terpisah: mematikannya berarti unregister background agent, setelah daemon menghentikan instance dengan bersih. Stop All, Turn Off, dan preview Stop All memilih instance lewat satu fungsi murni, `stop_all::select`. Semua aturan sesi hanya berlaku untuk daemon yang dijalankan launchd; daemon manual dan test tetap berperilaku seperti spec 0010.

## Requirements

**User stories**:

- Sebagai pengembang, saya ingin `pg17-dev` siap setelah saya login tanpa membuka aplikasi, supaya pekerjaan pagi langsung jalan.
- Sebagai pengguna, saya ingin instance yang saya stop tetap mati sampai login berikutnya, supaya daemon tidak melawan keputusan saya (BR-024).
- Sebagai pengguna, saya ingin logout menghentikan database dengan bersih, supaya PostgreSQL tidak perlu crash recovery di login berikutnya.
- Sebagai pengguna, saya ingin mematikan background agent dengan tahu persis instance mana yang berhenti dan mana yang tidak menyala di login berikutnya.
- Sebagai pengguna, saya ingin melihat alasan instance tidak menyala saat Keychain terkunci atau izin background dicabut, bukan instance yang diam diam jalan tanpa password.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-069):

- **AC-1** (SessionId): port platform baru `LoginSessionInfo::current()` mengembalikan `LoginSession { id: SessionId, boot_id: BootId, audit_session: u32 }`, dengan `id` berbentuk `<kern.bootsessionuuid huruf kecil>/<audit session id desimal>` dari `getaudit_addr` (paling panjang 47 byte, valid untuk `SessionId` spec 0004). Audit session `0` atau `AU_DEFAUDITSID`, dan setiap error `getaudit_addr`, menjadi `LoginSessionError` dengan kode `platform.session_unavailable`. `MacosPlatform` membacanya sekali saat daemon start. `FakeLoginSessionInfo` di testkit bisa disetel per test, dan `UnsupportedPlatform` mengembalikan `NotSupported`. Domain, platform, protocol, dan core tetap lolos `cargo check` untuk `aarch64-pc-windows-msvc`.
- **AC-2** (rotasi sesi): hanya saat `LaunchContext::Launchd` (spec 0015) dan registry tidak dalam recovery mode, daemon menjalankan *Rotasi sesi* dalam satu transaksi `commit_guarded`, sesudah `recover` spec 0007 dan sebelum rekonsiliasi spec 0010. Hasilnya `first` (belum ada baris), `new` (id beda), atau `continued` (id sama). Konteks lain dan recovery mode menghasilkan `skipped` tanpa menulis apa pun. Daemon yang di restart berkali kali dalam satu sesi tidak pernah membuat baris kedua untuk sesi itu atau lease startup kedua untuk instance yang sama.
- **AC-3** (rilis pertama tidak mengganggu): rotasi `first` hanya menyisipkan baris sesi. Desired, lease, dan proses yang jalan tidak berubah, jadi daemon pertama yang membawa spec ini mengadopsi instance yang sedang jalan seperti spec 0010.
- **AC-4** (login menyalakan instance terpilih, AC-014): di rotasi `new`, setiap instance yang bukan `retained`, bukan `unknown`, dan sudah initialized, dengan `startup` `at_login`, mendapat satu lease `startup` `active` untuk sesi ini dan desired `running`. Antrean start (*Antrean start*) lalu menjalankannya dengan pemicu `login`, interaksi secret `Never`, dan budget restart baru. Dengan PostgreSQL 17 nyata, instance `at_login` mencapai readiness `ready` tanpa GUI terbuka.
- **AC-5** (autostart Off setelah login): di rotasi `new`, setiap instance `manual` tanpa lease environment `active` mendapat desired `stopped`, dan semua lease `manual` dan `startup` milik sesi lain menjadi `released`. Instance `manual` yang jalan saat logout tetap mati setelah login berikutnya.
- **AC-6** (sesi baru bukan crash): saat rotasi `new` di boot yang sama, rekonsiliasi menerima `session_changed` dan memperlakukan proses yang `Gone` seperti boot baru: `running` atau `recovering` menjadi `identity_lost` lalu `confirmed_stopped`, dan `starting` menjadi `start_failed { interrupted }`, tanpa respawn dan tanpa menambah hitungan crash. Proses yang `Proven` diadopsi; kalau desired nya kini `stopped`, ia di stop lewat operasi `stop_instance` dengan pemicu `session_rotation` tanpa suppression. Tidak pernah ada dua proses untuk satu instance.
- **AC-7** (antrean start level triggered): setiap daemon start dengan konteks launchd membangun satu antrean start dari keadaan registry sesudah rotasi dan rekonsiliasi. Antrean itu berisi instance dengan desired `running`, lifecycle `stopped` atau `failed` yang eligible (*Antrean start*), tanpa blocker identitas, dan bukan `retained` atau `unknown`. Antrean dideduplikasi per instance dengan daftar "jalankan lagi" spec 0010, memakai satu batas `RECONCILE_START_CONCURRENCY`, dan urut nama. Crash daemon tepat sesudah rotasi `new` tidak menghilangkan autostart sesi itu.
- **AC-8** (stop manual menahan autostart, AC-069, BR-024): `instance.stop` dari GUI atau CLI memanggil `evaluate_stop(Manual { session: Some(sesi sekarang), .. })`, sehingga lease `startup` `active` sesi ini menjadi `suppressed` dan lease `manual` dilepas dalam batch yang sama dengan desired `stopped`. Untuk instance dengan desired `running` tanpa proses hidup (misalnya menunggu Keychain), stop adalah perubahan registry saja dengan efek lease yang sama. Restart daemon di sesi yang sama tidak menyalakannya. Login berikutnya membuat lease startup baru dan menyalakannya lagi kalau policy masih `at_login`. Restart instance (fitur 26) tidak suppress.
- **AC-9** (lease manual): `instance.start` dengan pemicu pengguna menulis satu lease `manual` `active` untuk sesi sekarang dalam batch yang sama dengan desired `running`, kecuali lease itu sudah ada. Start dari restart policy, rekonsiliasi, login, atau retry secret tidak menulis lease manual. Tanpa sesi (konteks manual), lease ditulis tanpa `session_id`.
- **AC-10** (`instance.stop_all`): method baru `instance.stop_all { idempotencyKey }` (`recovery refused`) menjalankan satu operasi `stop_all_instances` dengan satu langkah per instance di himpunan `stop` dari `stackly_core::instance::stop_all::select` (*Pilihan stop all*), satu satunya aturan pilihan untuk method ini, `agent_off`, dan `instance.preview_stop_all`. Setiap langkah memakai jalur AC-8, bersamaan sampai `RECONCILE_START_CONCURRENCY`. Instance `unknown` dan instance dengan lease environment `active` masuk `skipped` dengan kode `supervisor.identity_unproven` atau kode override lease spec 0010 (aturan override milik fitur 47), dan instance yang sudah `stopping` tanpa operasi start atau restart terbuka masuk `already_stopping` tanpa langkah. Start dan restart yang terbuka di instance terpilih dibatalkan dulu (`operation.cancel`), lalu operasi menunggu lock yang masih dipegang operasi lain (`BusyPolicy::Queue`), dan setiap langkah memeriksa lagi dari state segar. `operation.cancel` berhenti memulai langkah baru; langkah yang sedang berjalan selesai, dan sisanya masuk `skipped` dengan `operation.cancelled`. Balasannya `StopAllResult { stopped, failed, skipped }` setelah operasi selesai, dan replay mengembalikan balasan yang sama (spec 0007). Spec 0032 menambah `expectedInstanceIds` (*Tambahan dari spec 0032*). Fitur 32 memakai method ini untuk Stop Services.
- **AC-11** (logout menghentikan instance dengan bersih): di konteks launchd, `SIGTERM` tanpa `daemon.shutdown` yang diterima lebih dulu di run daemon ini menjalankan *Akhir sesi* sesuai urutan bernomornya. Setiap instance dengan proses hidup di stop bersamaan lewat strategi provider, tanpa `SIGKILL`, tanpa suppression, dan tanpa mengubah desired, dengan tenggat `SESSION_END_STOP_BUDGET` 15 detik yang dihitung dari saat `SIGTERM` datang. Baris sesi ditutup `clean` kalau semua stop selesai, atau `interrupted` kalau ada yang masih `stopping`. Seluruh exit, termasuk urutan berhenti spec 0006, selesai sebelum `ExitTimeOut` 30 detik; test memeriksa relasi konstanta itu.
- **AC-12** (sinyal per konteks): perilaku setiap sinyal mengikuti tabel *Sinyal per konteks*. Di konteks manual (terminal, SSH, `SpawnedDaemon` di test), `SIGTERM` tetap mengikuti spec 0010 (instance dibiarkan untuk diadopsi), dan test AC-21 spec 0010 tetap berlaku di konteks itu.
- **AC-13** (kembali di sesi yang sama, BR-024): sesudah *Akhir sesi* di sesi yang sama (misalnya `launchctl kickstart -k`, `bootout` lalu bootstrap, `relaunch`, atau Turn On lagi), rotasi `continued` membuka lagi baris sesi itu, dan antrean start menyalakan instance yang dihentikan *Akhir sesi* (desired masih `running`). Instance yang kamu stop (desired `stopped`) tidak menyala.
- **AC-14** (sesi yang tidak ditutup dan retention): kalau baris terbaru masih terbuka saat rotasi `new`, baris itu ditutup dengan `end_reason` `unknown`. Tabel menyimpan paling banyak 50 baris, dan transaksi rotasi menghapus yang lebih tua beserta lease `manual` dan `startup` berstatus `released` yang `session_id` nya tidak lagi ada di 50 baris itu.
- **AC-15** (Keychain terkunci, AC-062): start sistem yang gagal karena secret dengan alasan `store_locked` atau `busy` meninggalkan instance `stopped` dengan blocker `NeedsSecret`, desired `running`, dan tanpa fallback ke auth kosong, trust, atau password hardcode. Untuk setiap instance dalam keadaan itu, actor supervisor memeriksa secret setiap `LOGIN_SECRET_RETRY_INTERVAL` 30 detik dengan interaksi `Never`, tanpa operasi. Saat secret terbaca, ia memulai `start_instance` dengan pemicu `secret_retry`, yang tidak dihitung ke budget restart. Timer berhenti saat desired menjadi `stopped`, saat start berhasil, atau saat alasan berubah menjadi `missing` atau `access_denied`. Pada dua alasan terakhir, desired tidak berubah dan Summary menampilkan alasannya. Start dari Summary boleh membuka dialog Keychain (spec 0011).
- **AC-16** (setting autostart instance): `instance.create` menerima `startup` opsional (default `manual`), dan method baru `instance.set_startup { instanceId, startup, expectedRevision, idempotencyKey }` (`recovery refused`) menulis policy lewat operasi `set_instance_startup` dan `commit_guarded`. Mengubah ke `at_login` tidak menyalakan apa pun dan tidak membuat lease. Mengubah ke `manual` melepas lease startup sesi ini (`active` atau `suppressed`) dan tidak menghentikan instance yang jalan. Nilai yang sama memeriksa `expectedRevision` kalau ada, lalu membalas summary yang tidak berubah tanpa operasi. Instance retained ditolak `supervisor.instance_retained`.
- **AC-17** (kontrak): `InstanceSummary` mendapat `autostart: InstanceAutostart { policy, session }`, dan `DaemonInfo` mendapat `session: DaemonSessionInfo`, dengan nilai seperti *Method IPC*. `ShutdownMode` mendapat `agent_off`, dengan method dan DTO di *Method IPC*, fixture `protocols/fixtures/`, kenaikan minor `ProtocolVersion::CURRENT` di commit yang sama, dan `cargo xtask codegen --check` bersih.
- **AC-18** (CLI): `stackly instance create --autostart`, `stackly instance autostart <target> on|off`, `stackly daemon uninstall [--yes]`, baris autostart di `stackly instance inspect`, dan baris session di `stackly daemon status` mengikuti *CLI*, dengan `--json` dan exit code dari kategori error.
- **AC-19** (GUI dan izin yang dicabut, AC-014): ada toggle Start at login di sheet New Instance, dan baris Autostart di tab Summary menjadi toggle dengan teks status sesuai *Copy*. Diagnostics Background Agent mendapat Turn Off… dengan konfirmasi daftar dampak. Saat daemon berjalan di konteks launchd pada root default dan agent tidak `enabled` (`requires_approval`, `disabled`, `not_registered`, `not_found`), instance `at_login` menampilkan "Won’t start at login" beserta alasannya. Toggle tetap bisa dipakai, dan Stackly tidak pernah mengubah registrasi OS tanpa aksi pengguna (spec 0015 AC-11).
- **AC-20** (matikan agent): Turn Off… dan `stackly daemon uninstall` menjalankan urutan *Matikan agent*: konfirmasi, `daemon.shutdown { mode: agent_off }`, lalu `unregister` hanya kalau daemon melaporkan `exiting: true`. Tidak ada instance yang tertinggal jalan tanpa supervisor, start baru tidak bisa menyelinap di antara stop dan exit, dan setting `startup` setiap instance tidak berubah.
- **AC-21** (bukti native): dua spike native menjawab pertanyaan di *Spike native* sebelum journey 1 dan 3 dibangun. Hasil yang berlawanan dengan asumsinya mengembalikan desain ini ke `/architect`. Record manual `native` di Mac ini, dijalankan setelah persetujuan kamu, membuktikan:
  - login (AC-4);
  - logout dengan PostgreSQL jalan lalu login lagi (AC-5, AC-6, AC-11);
  - reboot sekali;
  - stop lalu `kickstart -k` (AC-8, AC-13);
  - Login Items yang dicabut (AC-19);
  - Keychain yang dikunci lewat `security lock-keychain` sebelum `kickstart -k` (AC-15).

  Tanpa record itu, AC tersebut `BLOCKED`, bukan PASS.

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native.

## Decision

**Chosen option**: model demand berbasis sesi login, dengan start yang level triggered. Daemon launchd mengenali sesi sendiri. Di setiap sesi baru, ia menurunkan desired state dari policy startup. Stop dicatat sebagai suppression, dan instance dihentikan dengan bersih saat sesi berakhir. Setiap daemon start menyalakan apa pun yang desired nya `running` dan bisa jalan. Autostart daemon tetap registrasi OS lewat `SMAppService`.

Rotasi sesi adalah fungsi murni di core di atas `Lease` dan `evaluate_stop` dari spec 0004. Rotasi hanya menulis desired, lease, dan baris sesi; ia tidak pernah menyalakan sesuatu sendiri. Logout dideteksi dari `SIGTERM` tanpa `daemon.shutdown` di konteks launchd. Daemon autostart Off berarti `daemon.shutdown { mode: agent_off }` lalu unregister.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`)

## Feature design

### Trace PRD

| PRD | Di spec ini |
| --- | --- |
| REQ-014, AC-014 | AC-1 sampai AC-7, AC-19, AC-20 |
| BR-024, AC-069 | AC-8, AC-13 |
| AC-062 | AC-15 |
| REQ-022 (evaluasi demand) | AC-9, AC-10; lease environment milik fitur 46 dan 47 |
| PRD 13.2, 19.5 | *Rotasi sesi*, *Akhir sesi*, *Batas disruption* |
| PRD 32.2 (lifecycle login, logout, permission denied) | AC-21 |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| `Lease`, `evaluate_stop`, `should_autostart` | spec 0004 (dipakai; sumber `session` diputuskan di sini) |
| `instance_demands`, kolom `startup` | spec 0005 (dipakai), tabel `daemon_sessions` dan satu index lease baru di sini |
| `recover` operasi | spec 0007 (urutannya sebelum rotasi) |
| Rekonsiliasi, stop, restart policy, `SIGTERM` | spec 0010 (diubah, lihat *Usulan update untuk spec lain*) |
| Rencana restart daemon, quiesce, `ShutdownAccepted` | spec 0030 (dipakai; spec ini menyesuaikan diri, dan `agent_off` memakai gerbang quiesce yang sama) |
| `BackgroundAgent`, `LaunchContext`, banner agent, `daemon.shutdown` | spec 0015 (dipakai dan diperluas dengan `agent_off`) |
| Blocker `NeedsSecret`, interaksi Keychain | spec 0011 (dipakai) |
| Restart dan reload | fitur 26 (restart tidak suppress) |
| Restart daemon terencana dengan jendela disruption | fitur 30 |
| Menu bar, Stop Services, halaman Settings | fitur 32 (memakai `instance.stop_all`, `stop_all::select`, dan teks autostart dari sini) |
| `expectedInstanceIds`, `instance.stop_all_changed`, DTO `StopAllPreview`, `daemon_settings.default_startup`, `--no-autostart` | spec 0032 (memperluas kontrak ini, lihat *Tambahan dari spec 0032*; method `instance.preview_stop_all` dibangun di sini) |
| Lease environment, override stop | fitur 46 dan 47 |

### Model demand

Desired state tetap field sendiri (spec 0004). Lease menjelaskan alasan:

| Sumber | Dibuat | Berakhir | Sesi |
| --- | --- | --- | --- |
| `startup` | rotasi `new`, untuk instance `at_login` yang bisa jalan | `suppressed` oleh stop manual atau `stop_all`; `released` di rotasi `new` berikutnya atau saat policy menjadi `manual` | wajib, sesi sekarang |
| `manual` | `instance.start` pengguna (AC-9), dan juga restart pengguna spec 0026, karena separuh start nya memakai `StartRequest::user` (`restart_op.rs`) | `released` oleh stop manual, `stop_all`, atau rotasi `new` | sesi sekarang, atau tanpa sesi di konteks manual |
| `environment` | fitur 46 | fitur 47 | tanpa sesi, bertahan lintas sesi |

`stopped` dengan desired `running` adalah keadaan istirahat yang sah (misalnya sesudah *Akhir sesi*, atau saat menunggu Keychain). Exit proses yang dihentikan *Akhir sesi* tidak pernah dibaca sebagai crash, karena stop selalu lewat `stopping` dan exit saat `stopping` menjadi `exited` (spec 0010).

### Urutan daemon start

1. `recover` spec 0007 menutup operasi yang terputus.
2. Rencana restart terbuka spec 0030 dibaca (`restoring` di boot yang sama, `expired` di boot baru; *Alur restart terencana* langkah 9 spec 0030).
3. *Rotasi sesi* (konteks launchd saja, dan dilewati di recovery mode).
4. Rekonsiliasi spec 0010, dengan `session_changed` true hanya untuk rotasi `new`, plus aturan restore spec 0030. Karena rotasi berjalan lebih dulu, restore membaca desired sesudah rotasi.
5. `OperationEngine::start()`, lalu *Antrean start* (konteks launchd), atau daftar "jalankan lagi" spec 0010 apa adanya (konteks manual). Restore spec 0030 (`PendingKind::Restore`) berjalan di slot yang sama.

Gagal membaca `LoginSession` di konteks launchd membuat rotasi `skipped`, subsystem `session` `degraded` dengan kode `platform.session_unavailable`, dan antrean start hanya berisi daftar "jalankan lagi" spec 0010. Rotasi yang gagal ditulis (guard atau error registry) diperlakukan sama, bukan exit 1: subsystem `session` `degraded` dengan `platform.session_unavailable`, `DaemonInfo.session` null, dan hanya daftar "jalankan lagi" spec 0010 yang dinyalakan (`SessionStatus::Unavailable` di `apps/daemon/src/session.rs`; keputusan lead 2026-11-03, ikut kode).

### Rotasi sesi

Fungsi murni `plan_session_rotation(current: &LoginSession, latest: Option<&DaemonSession>, instances, leases, now) -> SessionRotation` di `stackly-core`, dijalankan sebagai guard `commit_guarded` di thread registry:

1. `latest` tidak ada → `first`: sisipkan baris baru, selesai.
2. `latest.id != current.id` → `new`:
   - tutup `latest` kalau masih terbuka (`unknown`), lalu sisipkan baris baru;
   - `release` setiap lease `startup` dan `manual` yang `session_id` nya bukan sesi sekarang;
   - untuk setiap instance `at_login` yang bukan `retained`, bukan `unknown`, dan sudah initialized: sisipkan lease `startup` `active` lalu `set_desired(running)`;
   - untuk setiap instance `manual` tanpa lease environment `active`: `set_desired(stopped)`;
   - pangkas baris sesi dan lease `released` sesuai AC-14.
3. `latest.id == current.id` → `continued`: kalau baris itu tertutup, buka lagi (`ended_at` dan `end_reason` menjadi null); selain itu tidak ada perubahan.

Setiap perubahan instance memakai satu `UpdateInstance` per mutasi dengan `expected` berurutan (spec 0010). Daemon menyimpan `LoginSession` dan hasil rotasi di memori untuk seluruh run.

### Antrean start

Satu antrean per daemon start (konteks launchd), dibangun sesudah rekonsiliasi dari registry. Daftar "jalankan lagi" spec 0010 digabung ke dalamnya, dideduplikasi per instance.

- **Masuk antrean**: desired `running`, bukan `retained` atau `unknown`, tanpa blocker `dataInUse` atau `IdentityUnproven`, dengan lifecycle `stopped`, atau `failed` dengan `last_failure` yang eligible.
- **`failed` eligible**: start yang terputus, budget restart habis, readiness timeout, `NeedsSecret`, dan konflik port. Konstanta `LOGIN_ELIGIBLE_FAILURES` di core memetakan kode yang ada ke daftar ini.
- **`failed` tidak eligible** (butuh aksi kamu): data tidak kompatibel (spec 0025), runtime tidak ada atau tidak `installed`, paket pinned tidak tersedia, dan init gagal. Instance ini tetap `failed` dengan `last_failure` nya, dan Summary menampilkannya seperti biasa.
- **Rencana restart**: instance `stop_restore` di rencana spec 0030 yang `restoring` dan yang `outcome` nya masih kosong tidak masuk antrean, apa pun pemicunya, karena restore spec 0030 (`PendingKind::Restore`, pemicu `planned_restart`) yang menyalakannya. Satu instance hanya dinyalakan satu jalur, dan selama rencana `restoring`, restore yang menang.
- **Pemicu**: `login` kalau rotasi `new`, `reconcile` untuk yang lain; instance yang ada di dua daftar memakai `login`. Recovering tetap lewat jalur respawn actor spec 0010.
- **Satu percobaan**: setiap instance dicoba sekali per daemon start dengan budget restart baru. Kegagalan selain `NeedsSecret` tidak dicoba lagi otomatis.
- **Desired diperiksa di bawah lock**: setiap start sistem (pemicu `reconcile`, `login`, `secret_retry`, dan `planned_restart` spec 0030) membaca desired lagi setelah lock instance dipegang. Kalau bukan `running` lagi (misalnya Stop All atau Turn Off lebih dulu), operasi selesai `succeeded` tanpa start dan tanpa menulis desired. Start pengguna tetap menulis desired `running`, dan respawn restart policy sudah membaca ulang (spec 0010).

### Sinyal per konteks

| Konteks | Sinyal | Perilaku |
| --- | --- | --- |
| launchd | `SIGTERM` dengan `shutdown_requested` false | *Akhir sesi* |
| launchd | `SIGTERM` sesudah `daemon.shutdown` `stop` diterima | spec 0010: instance dibiarkan, baris sesi ditutup `clean` |
| launchd | `SIGTERM` sesudah rencana restart spec 0030 di commit `stopping` | rencana restart spec 0030; baris sesi ditutup `clean` |
| launchd | `SIGTERM` sesudah `agent_off` memutuskan `exiting: true` | sama dengan `stop`; semua instance pilihan sudah berhenti |
| launchd | `SIGINT`, `SIGHUP` | sama dengan `SIGTERM` sesudah `daemon.shutdown` `stop` (bukan logout); `StopSignals` di `apps/daemon/src/shutdown.rs` kini juga mengikat `SIGHUP`, jadi `SIGHUP` menghentikan daemon (tabel sinyal spec 0006) |
| manual | `SIGTERM`, `SIGINT`, `SIGHUP` | spec 0006 dan 0010 apa adanya, tanpa sesi |
| semua | crash, `SIGKILL` | tidak ada handler; adopsi di start berikutnya (spec 0010 AC-21) |

`shutdown_requested` di memori menjadi true saat `daemon.shutdown` `stop` diterima, saat rencana restart spec 0030 di commit `stopping`, atau saat `agent_off` memutuskan `exiting: true`. "`daemon.shutdown` yang diterima lebih dulu" di AC-11 berarti flag ini true. `SIGTERM` yang datang lebih awal (selama quiesce restart, atau selama `agent_off` masih menghentikan instance) adalah *Akhir sesi*: langkah 2 nya membatalkan operasi itu, handler `daemon.shutdown` menjawab `daemon.shutting_down`, dan gerbang `agent_off` tidak dicabut lagi.

### Akhir sesi

Urutan bernomor sejak `SIGTERM` diterima (`t0`):

1. Server IPC menolak setiap request baru, termasuk `daemon.shutdown`, dengan `daemon.shutting_down`.
2. Operasi yang sedang berjalan dibatalkan lewat jalur cancel spec 0007 dan 0010 (start yang dibatalkan dengan proses hidup berakhir lewat alur *Stop* spec 0010).
3. Untuk setiap instance dengan proses hidup, mulai operasi `stop_instance` dengan pemicu `session_end` lewat `Origin::Background`, tanpa idempotency key dan tanpa `evaluate_stop`. Engine hanya mengikat key pada `Origin::Request`, dan origin itu kena gerbang quiesce restart yang justru dibatalkan akhir sesi; satu daemon menjalankan akhir sesi sekali (sinyal kedua memotongnya), jadi tidak ada yang perlu di replay. Stopnya memakai `StopRequest::session_end` (`StopIntent::PlannedRestart`: tanpa cek lease dan tanpa menulis desired). Instance yang lock nya masih dipegang operasi yang sedang dibatalkan menunggu lock itu di dalam tenggat yang sama.
4. Tunggu sampai semua stop commit, atau sampai `t0 + SESSION_END_STOP_BUDGET` (15 detik).
5. Tulis `EndSession { end_reason }`: `clean` kalau setiap instance dengan proses hidup mendapat operasi stop dan semuanya selesai `succeeded` sebelum tenggat; selain itu `interrupted`, yaitu ada stop yang belum final saat tenggat, stop yang selesai tetapi gagal, atau stop yang tidak bisa dimulai (`crates/core/src/supervisor/session_end.rs`). Gagal commit tidak memblokir exit; rotasi berikutnya menutup baris itu `unknown`.
6. Urutan berhenti spec 0006 (actor, probe, pembaca spool, dan watcher berhenti tanpa commit lifecycle, seperti spec 0010).

`SESSION_END_STOP_BUDGET` + batas urutan berhenti spec 0006 + margin 3 detik ≤ `ExitTimeOut` 30. Test konstanta memeriksa relasi ini, dan kalau batas spec 0006 ternyata lebih dari 12 detik, budget stop yang dikurangi. Instance yang masih `stopping` saat tenggat dibiarkan; rekonsiliasi berikutnya membaca `stopping` plus `Gone` sebagai `exited` (spec 0010).

### Pilihan stop all

Fungsi murni `stackly_core::instance::stop_all::select(view: &StopAllView) -> StopAllSelection { stop, already_stopping, skipped }` adalah satu satunya aturan pilihan. `instance.stop_all`, `agent_off`, `instance.preview_stop_all`, dan konfirmasi Turn Off memanggilnya, dan tidak ada pemanggil yang memilih instance dengan aturan sendiri. `StopAllView` diisi dalam satu closure `read` registry (satu snapshot): setiap instance (nama, lifecycle, desired, policy `startup`, retained), lease `active` dan `suppressed` nya, dan operasi yang belum selesai (`queued`, `running`, `cancel_requested`, atau `needs_recovery`) beserta key nya dari journal operasi. Spec 0032 menyebut input ini `RegistryView`. "Proses hidup" di sini berarti lifecycle `starting`, `running`, atau `stopping`, karena registry tidak menyimpan tanda hidup proses.

Aturan per instance dicek berurutan, dan aturan pertama yang cocok menang:

| # | Kondisi | Hasil |
| --- | --- | --- |
| 1 | retained | tidak dipilih |
| 2 | lifecycle `unknown` | `skipped`, `supervisor.identity_unproven` |
| 3 | lifecycle bukan `starting`, `running`, atau `stopping`, desired bukan `running`, dan tanpa operasi `start_instance` atau `restart_instance` terbuka | tidak dipilih |
| 4 | key nya dipegang operasi `needs_recovery` | `skipped`, `operation.recovery_required` |
| 5 | lifecycle `stopping`, tanpa operasi `start_instance` atau `restart_instance` terbuka | `already_stopping` |
| 6 | lease environment `active` | `skipped`, `supervisor.stop_requires_override` |
| 7 | lainnya | `stop` |

Setiap daftar urut nama, lalu ID. Aturan 3 dan 5 menghitung start dan restart yang terbuka karena restart spec 0026 menulis desired `stopped` di separuh stop nya lalu `running` lagi, dan start pengguna yang masih antre belum menulis desired. Tanpa itu, Stop All melewatkan instance yang sebentar lagi jalan. Aturan 4 menjaga `begin` dari `RecoveryRequired`, yang akan menggagalkan seluruh operasi.

### Operasi `stop_all_instances`

`OperationKind::StopAllInstances`: `resumable` false, `interrupt_policy` `CloseInterrupted`, resource `Manager`, `creates_resource` false, tanpa kompensasi (bentuk yang sama dengan `DaemonRestart` spec 0030). Karena resource nya `Manager`, dua Stop All, atau Stop All dan restart daemon, tidak pernah berjalan bersamaan.

1. Handler memanggil `replay_operation` dulu. Kalau meleset, ia menjalankan `select`, lalu `begin` dengan `Origin::Request`, input journal `{ source, steps, preSkipped }` (`source` `stop_all` atau `agent_off`, ID instance yang dapat langkah, dan `skipped` dari pilihan beserta kodenya), `extra_locks` berisi `LockKey::Instance` setiap instance di `steps`, dan `BusyPolicy::Queue`. Pilihan yang kosong tetap membuat operasi, supaya key terikat dan replay stabil. Input atau hasil di atas batas journal 16 KiB (sekitar 300 instance) ditolak `operation.payload_too_large`.
2. `begin` yang gagal dibalas sebagai error tanpa operasi: `operation.queue_full`, `operation.recovery_required` (operasi `needs_recovery` muncul di antara `select` dan `begin`, atau memegang `Manager`), dan `daemon.starting`. Untuk `agent_off`, gerbang dicabut lebih dulu.
3. Sesudah `begin`, daemon meminta `operation.cancel` untuk setiap operasi `start_instance` dan `restart_instance` yang terbuka di instance `steps`, seperti *Akhir sesi* langkah 2, supaya Stop All tidak tertahan start yang panjang. Start yang dibatalkan sesudah spawn berakhir lewat stop biasa (spec 0007, spec 0026 AC-9). Operasi lain di instance itu (stop, reload, force stop, edit config) ditunggu sampai selesai.
4. Setelah semua key dipegang, pekerjaan membuka satu langkah journal `stop` dan menghentikan instance lewat jalur stop supervisor langsung di bawah key itu (seperti spec 0030 *Alur restart terencana* langkah 6), dengan efek AC-8 (`evaluate_stop(Manual { session })`, efek lease, desired `stopped`) lalu *Stop* spec 0010 langkah 2 sampai 6: `recovering` membatalkan timer lalu `exited` tanpa proses, `stopping` mengirim stop lagi, dan tanpa proses hanya perubahan registry. Paling banyak `RECONCILE_START_CONCURRENCY` bersamaan, dengan `set_progress` (instance selesai dari total).
5. Tepat sebelum stop, setiap instance diperiksa lagi dengan `select` atas view satu instance: `skipped` dilaporkan dengan kodenya; instance yang sudah dihapus atau retained dibuang dari hasil, bukan penghalang; hasil lain menjalankan langkah 4. Instance yang sudah berhenti tetap mendapat efek lease dan masuk `stopped`.
6. Setiap instance berakhir `stopped`, `failed` dengan kode stop (misalnya `supervisor.stop_timeout`), atau `skipped`. `preSkipped` ikut di hasil; `already_stopping` tidak muncul di hasil `instance.stop_all`.
7. `operation.cancel` selagi berjalan: tidak ada stop baru yang mulai, stop yang berjalan selesai, dan sisanya `skipped` dengan `operation.cancelled`. Pekerjaan menyerap cancel itu dan kembali `Ok`, jadi operasi berakhir `succeeded` dengan `cancelRequestedAt` terisi. Operasi juga `succeeded` kalau ada yang `failed`: `result.resources` berisi instance `stopped`, dan `result.partial` satu item per instance `failed` atau `skipped` (outcome `failed` atau `skipped`, dengan `code`), jadi halaman Operations menampilkannya sebagai partial.
8. Balasan dibangun dari keadaan akhir operasi, sama untuk balasan pertama dan replay: `succeeded` dari `result`; `cancelled` (cancel saat masih antre, tanpa `result`) dari input, dengan setiap instance `skipped` `operation.cancelled`; `failed` (misalnya daemon berhenti di tengah, `CloseInterrupted`) sebagai error dengan kode kegagalannya. Replay yang datang selagi operasi berjalan menunggu sampai operasi selesai.
9. Client memakai timeout interaktif (CLI `RequestTimeout::Interactive`, 200 detik). Stop All dengan banyak instance bisa lebih lama; pada `ipc.call_timeout`, client mengirim ulang dengan key yang sama, dan replay menunggu operasi yang sama.

### Tambahan dari spec 0032

Spec 0032 (*Tambahan pada kontrak spec 0031*) memperluas kontrak ini. Fitur 31 membangun `stop_all::select` dengan `already_stopping`, cancel pada `stop_all_instances`, dan `instance.preview_stop_all` (baca saja, `recovery refused`, balasan `StopAllPreview` persis seperti *Method IPC* spec 0032), karena ketiganya bagian dari operasi yang dibangun di sini dan konfirmasi Turn Off membutuhkan preview yang sama. Fitur 32 membangun sisanya, karena butuh popover atau tabel `daemon_settings` spec 0032:

- `instance.stop_all` mendapat `expectedInstanceIds: [Uuid]` opsional (`#[serde(default)]`). Kalau ada, precheck (sesudah `replay_operation`) membandingkan himpunan `stop` dengan himpunan itu dan menolak `instance.stop_all_changed` kalau berbeda; operasi hanya membuat langkah untuk irisan keduanya. Tanpa field itu, perilaku tetap AC-10 (dipakai `agent_off`). Sidik jari idempotency spec 0007 ikut mencakup field ini.
- `instance.create` tanpa `startup` memakai `daemon_settings.default_startup` (default nya `manual`), bukan `manual` tetap (spec 0032 AC-18). Sampai tabel itu ada, default AC-16 berlaku.
- `stackly instance create` mendapat `--no-autostart`.

### Matikan agent

1. Client memanggil `instance.preview_stop_all` dan `instance.list`, lalu menampilkan konfirmasi:
   - instance yang akan berhenti (`items` dan `alreadyStopping` preview);
   - instance dengan `autostart.policy` `at_login` (dari `instance.list`) yang tidak akan menyala di login berikutnya;
   - kalau `skipped` preview tidak kosong: instance itu beserta alasannya, dan Turn Off nonaktif, karena daemon tidak akan keluar selama mereka ada. CLI keluar dengan kategori `conflict` tanpa mengirim apa pun.
2. `daemon.shutdown { mode: agent_off, idempotencyKey }`. Key wajib, seperti `restart` spec 0030, dan client memakai timeout interaktif seperti *Operasi `stop_all_instances`* langkah 9, karena balasannya baru datang setelah semua stop selesai. Perilakunya sama di konteks launchd dan manual.
   - Handler memanggil `replay_operation` lebih dulu, jadi kiriman ulang dengan key yang sama setelah timeout menunggu operasi yang sama, bukan ditolak gerbang.
   - Ditolak tanpa efek: `daemon.restart_pending` selama quiesce atau rencana restart `stopping` spec 0030; `daemon.shutting_down` sesudah `handed_over`, sesudah `stop` diterima, dan selama `agent_off` lain berjalan; `registry.recovery_mode` dalam recovery mode, karena tanpa registry daemon tidak bisa memilih atau menghentikan instance. Turn Off menampilkan kode itu, dan `daemon.shutdown { mode: stop }` tetap tersedia. Selama `agent_off` berjalan, `daemon.shutdown` `stop` dan `restart` juga ditolak `daemon.shutting_down`.
   - Sejak request diterima, gerbang quiesce spec 0030 di `OperationEngine::begin` menolak operasi baru dari `Origin::Request` dengan `daemon.shutting_down`. Hanya operasi yang dimulai lewat method `OperationEngine::begin_bypassing_quiesce`, yang hanya dipanggil handler `agent_off` untuk operasinya sendiri, yang lolos (method, bukan field `bypass_quiesce` di `BeginRequest`); `replay` tidak diperiksa gerbang. Operasi latar tetap boleh, karena start sistem memeriksa desired di bawah lock (*Antrean start*) dan instance yang dihentikan sudah desired `stopped`.
   - Daemon menjalankan satu operasi `stop_all_instances` (`source` `agent_off`) dengan langkah untuk himpunan `stop` dan juga `already_stopping` dari *Pilihan stop all*. Langkah `already_stopping` menunggu lock nya lalu berjalan seperti langkah lain, jadi daemon tidak keluar selagi stop lain masih berjalan, dan instance yang macet di `stopping` berakhir di `failed`.
   - Kalau `failed` dan `skipped` hasil operasi kosong, daemon menjalankan `select` sekali lagi. Kalau `stop`, `already_stopping`, dan `skipped` dari `select` terakhir itu juga kosong (`apps/daemon/src/ipc/agent_off.rs`), daemon membalas `ShutdownAccepted` (spec 0030) dengan `exiting: true` dan `stopAll`, lalu keluar 0 seperti `stop`. Selain itu, daemon membalas `exiting: false` dengan `stopAll`, mencabut gerbang, dan tetap jalan. Replay dengan key yang sama selama daemon masih jalan mengembalikan balasan yang sama.
3. Dengan `exiting: true`, client menunggu socket hilang (spec 0015 AC-10), lalu memanggil `unregister` (CLI lewat port `BackgroundAgent`, GUI lewat `SMAppService` di Swift). Status sesudahnya `not_registered`, dan banner Turn On spec 0015 muncul. Dengan `exiting: false`, client berhenti dan menyebut setiap instance yang menghalangi.
4. Daemon tidak terjangkau:
   - Dengan state agent `requires_approval` atau `disabled`, launchd tidak menjalankan daemon, jadi `unregister` langsung boleh.
   - Dengan state `enabled`, client berhenti dengan error tidak terjangkau yang ada dan hint `stackly daemon start`.
   - Dengan root override aktif, `daemon uninstall` ditolak dengan aturan yang sama dengan `daemon install` (spec 0015).

### Batas disruption

| Kejadian | Perilaku |
| --- | --- |
| GUI ditutup | instance tetap jalan |
| `stackly daemon stop` atau `restart` | instance tetap jalan dan diadopsi (spec 0010); kalau logout terjadi saat daemon mati, OS bisa mengakhiri instance tanpa stop bersih |
| `kickstart -k`, `bootout`, `relaunch` di sesi yang sama | instance di stop dengan bersih, lalu menyala lagi saat daemon kembali, kecuali yang kamu stop |
| Logout | stop bersih paling lama 15 detik; sisanya `interrupted` |
| Login berikutnya | `at_login` menyala; `manual` tetap mati |
| Reboot | sama dengan logout lalu login; boot tanpa login tidak menyalakan apa pun (PRD 19.5) |
| Crash daemon | tidak ada handler; proses diadopsi di start berikutnya (spec 0010 AC-21) |
| Sleep dan wake | bukan sesi baru; audit session tetap sama |
| Rilis pertama spec ini | rotasi `first`, tidak ada yang berubah |

### Spike native

Dua spike manual (setelah persetujuan kamu, karena keduanya butuh logout sungguhan), dengan hasil dicatat di `rationale.md`:

- **Sebelum journey 1, audit session**: nilai `ai_asid` LaunchAgent di dua login berturut turut, sesudah sleep, dan dari terminal. Asumsinya: berbeda per login, sama selama sesi, dan sama dengan sesi Aqua.
- **Sebelum journey 3, logout**: apakah agent menerima `SIGTERM` saat logout dan shutdown, berapa lama sebelum `SIGKILL`, dan apakah proses instance (di session POSIX sendiri) juga menerima sinyal langsung dari OS. Kalau ya, catat urutannya terhadap stop provider.

**Urutan yang kamu pilih (2026-11-02)**: langkah 2 sampai 9 dibangun sebelum spike audit session (langkah 1) selesai, di atas asumsi di atas. Sleep dan dua logout kamu lakukan nanti, sebelum journey 3 (langkah 10). Kalau hasilnya bertentangan dengan asumsi, spec ini di update lagi lewat `/architect` sebelum journey 3. Ini menunda urutan "sebelum journey 1" untuk spike audit session di AC-21; teks AC-21 tidak berubah. Hasil sementara ada di [rationale.md](rationale.md) (*Spike native: hasil sementara*): LaunchAgent spike `com.stackly.spike.asid` masih aktif, dan baris pertamanya mencatat asid 100014, sama untuk LaunchAgent dan Terminal (`~/Library/Logs/stackly-asid-spike.log`).

### Model data

Migration `NNNN_daemon_sessions.sql` (nomor bebas berikutnya saat build; 0007 sampai 0009 sudah dipakai spec 0028, 0029, dan 0030, jadi paling kecil 0010; spec 0032 juga merencanakan migrasi, jadi siapa pun yang build lebih dulu memakai nomor itu):

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `id` | `TEXT` PK | `SessionId` |
| `boot_id`* | `uuid` | `kern.bootsessionuuid` |
| `audit_session`* | `INTEGER` | `>= 1`, muat di `u32` |
| `started_at`* | `ts` | |
| `ended_at` | `ts` | |
| `end_reason` | `TEXT` | set tertutup `clean`, `interrupted`, `unknown` |

CHECK `(ended_at IS NULL) = (end_reason IS NULL)`, unique partial index `((1)) WHERE ended_at IS NULL` (paling banyak satu sesi terbuka), dan index `started_at`. Migration yang sama menambah unique partial index `instance_demands (instance_id, session_id) WHERE source_kind = 'startup' AND state IN ('active', 'suppressed')`. `instance_demands.session_id` tetap tanpa FK.

Operasi registry baru: `InsertSession`, `EndSession { id, at, reason }`, `ReopenSession { id }`, dan `PruneSessions { keep: 50 }` (yang juga menghapus lease `released` sesuai AC-14). Domain mendapat `DaemonSession` dengan `restore`, plus setter `Instance::set_startup`. Daemon autostart tidak disimpan: registrasi OS satu satunya sumber kebenaran.

### Method IPC

| Method | Params | Reply | Recovery | Error utama |
| --- | --- | --- | --- | --- |
| `instance.set_startup` | `instanceId`, `startup` (`manual`, `at_login`), `expectedRevision` opsional, `idempotencyKey` | `InstanceSummary` | refused | `resource.not_found`, `supervisor.instance_retained`, `resource.revision_mismatch` |
| `instance.stop_all` | `idempotencyKey` (spec 0032 menambah `expectedInstanceIds`) | `StopAllResult { stopped: [Uuid], failed: [{ instanceId, code }], skipped: [{ instanceId, code }] }`, setelah operasi selesai (*Operasi `stop_all_instances`*) | refused | `operation.queue_full`; per instance di `failed` dan `skipped` |
| `instance.preview_stop_all` | none | `StopAllPreview` seperti *Method IPC* spec 0032, dari `select` | refused | `ipc.*` |
| `instance.create` (diperluas) | `startup` opsional, default `manual` (spec 0032 AC-18 menggantinya dengan `default_startup`) | sama | sama | sama |
| `daemon.shutdown` (diperluas) | `mode` `agent_off` baru; `idempotencyKey` wajib untuk `agent_off` dan `restart` | `ShutdownAccepted` spec 0030 diperluas dengan `exiting: bool` dan `stopAll: StopAllResult \| null`; `stopAll` null untuk `stop` dan `restart` | sama | sama, plus `daemon.restart_pending` (spec 0030), `daemon.shutting_down`, dan `registry.recovery_mode` untuk `agent_off` (*Matikan agent*) |

`ShutdownAccepted` tidak lagi `Copy` setelah `stopAll` ditambah. Kontrak berubah di langkah 5, 9, dan 12. Setiap langkah itu menaikkan minor `ProtocolVersion::CURRENT` dari versi di `main` saat itu (2.1 pada 2026-11-02, dan spec 0028 sedang membuat 2.2), kecuali versi itu belum ada di `main` (`cargo xtask codegen --amend`). Hasil di `main`: 3.1, karena spec 0032 lebih dulu menaikkan mayor ke 3.0 dan langkah 12 menambah satu minor.

Nilai DTO baru:

- `InstanceAutostart { policy: StartupPolicyKind, session: AutostartSession }`, opsional `#[serde(default)]` di `InstanceSummary`. `session` bernilai `active` (lease startup sesi ini `active`), `suppressed` (lease itu `suppressed`), `none` (tidak ada lease startup sesi ini, termasuk di konteks manual), atau `Unrecognized`.
- `DaemonSessionInfo`, opsional di `DaemonInfo`, null di konteks manual, di recovery mode (rotasi dilewati), saat sesi gagal dibaca atau rotasi gagal ditulis, dan selama daemon launchd belum selesai rotasi (`starting`); hanya `SessionStatus::Rotated` yang mengisinya (`session_info` di `apps/daemon/src/session.rs`):
  - `id`;
  - `startedAt`: `started_at` baris sesi, yaitu awal sesi, bukan awal run daemon;
  - `rotation`: `first`, `new`, atau `continued`, hasil run daemon ini yang disimpan di memori;
  - `previousEnd`: `end_reason` baris tepat sebelum baris sesi sekarang, atau null kalau tidak ada.
- Kode baru: `platform.session_unavailable` dan `operation.cancelled` (kategori `cancelled`, alasan `skipped` untuk langkah yang tidak dimulai karena cancel). Operation kind baru: `set_instance_startup` dan `stop_all_instances`. Pemicu start baru: `login` dan `secret_retry`; pemicu stop baru: `session_end` dan `session_rotation`.

### CLI

- `stackly instance create … --autostart`: `startup` `at_login`. Spec 0032 menambah `--no-autostart`, dan tanpa keduanya `startup` tidak dikirim.
- `stackly instance autostart <target> on|off`: target lewat `TargetArg` dan `InstanceTarget::resolve`, lalu `instance.set_startup`.
  - Output teks: "Autostart for “{name}”: at login" atau "off". `--json` berisi `InstanceSummary`.
  - Saat agent tidak `enabled` di root default, `on` menulis peringatan ke stderr: "The background agent is off, so “{name}” won’t start at login until you run stackly daemon install."
- `stackly instance inspect`: baris `autostart` bernilai `off`, `at login`, `at login (stopped for this session)`, atau `at login (won’t start: background agent {state})`. Bagian terakhir hanya untuk root default saat `DaemonInfo.session` tidak null.
- `stackly daemon status`: baris `session` dengan id, `startedAt`, `rotation`, dan `previousEnd`.
- `stackly daemon uninstall [--yes]`: urutan *Matikan agent*, dengan konfirmasi lewat `Confirm` spec 0008. Tanpa TTY dan tanpa `--yes`, perintah gagal seperti `Confirm` lain.
  - `--json` berisi `{ exiting, stopAll, wasRegistered, agent }`.
  - Exit 0 kalau unregister berhasil. Daemon yang membalas `exiting: false` memberi exit dari kategori `conflict`, dengan daftar instance yang menghalangi.
  - Help menyebut bahwa perintah ini hanya unregister background agent, dan app, data, serta link CLI tetap.

### GUI

- Sheet New Instance: toggle "Start at login", default off, dikirim sebagai `startup`; perintah setara menambah `--autostart`.
- Tab Summary, baris Autostart: toggle dengan teks sekunder di *Copy*, lewat `instance.set_startup` dengan revision baris yang tampil; gagal memakai umpan balik `FeedbackCenter`.
- Diagnostics Background Agent: Turn Off… saat state `enabled`, dengan dialog konfirmasi destruktif (Cancel default, spec 0012) yang mendaftar dampak; `exiting: false` menampilkan daftar instance yang menghalangi.
- Blocker `NeedsSecret` dengan `store_locked`: teks di *Copy* dan tombol Start.

### Copy

| Tempat | Kondisi | Teks |
| --- | --- | --- |
| Summary | `policy` `at_login`, `session` `suppressed`, desired `stopped` | "Stopped for this session. Starts at your next login." |
| Summary | `policy` `at_login`, `session` `none`, `rotation` bukan `new` di sesi ini | "Starts at your next login." |
| Summary | `policy` `at_login`, agent tidak enabled (AC-19) | "Won’t start at login: the background agent is off." atau "Won’t start at login: the background agent needs approval in Login Items." |
| Summary | lainnya | tanpa teks sekunder |
| Blocker | `NeedsSecret` dengan `store_locked` atau `store_busy` | "The Keychain is locked. Stackly tries again every 30 seconds." |
| Konfirmasi Turn Off | | judul "Turn Off Background Agent?"; isi "{n} running instances will stop. {m} instances set to start at login won’t start until you turn it on again."; tombol "Turn Off" (`n` = `items` ditambah `alreadyStopping` preview) |
| Konfirmasi Turn Off | `skipped` preview tidak kosong | ditambah "Turn Off can’t finish while these instances run: {nama · alasan}."; tombol Turn Off nonaktif |
| Turn Off ditolak | `exiting: false` | "Background agent is still on. Stopped {a} instances. These couldn’t be stopped: {daftar}." |
| Alasan per instance di konfirmasi Turn Off, Turn Off ditolak, dan `stackly daemon uninstall` | kode `skipped` atau `failed` | `supervisor.stop_requires_override` "an environment uses it", `supervisor.identity_unproven` "it can’t be confirmed as running", `operation.recovery_required` "an operation needs recovery", `supervisor.stop_timeout` "it didn’t stop in time", `operation.cancelled` "the stop was cancelled"; kode lain ditulis apa adanya. Setiap item ditulis "{nama} · {alasan}" (`TurnOffPlan.reason(of:)` di GUI, `reason` di `apps/cli/src/commands/daemon/uninstall.rs`) |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| rotasi | `SessionId`, `boot_id`, `audit_session` | `LoginSessionInfo::current()` (AC-1) |
| rotasi | konteks | `LaunchContext` (spec 0015) |
| rotasi | sesi terakhir | baris terbaru `daemon_sessions` |
| rotasi | instance yang mendapat desired `running` | kolom `startup`, lifecycle tersimpan, `data_initialized_at`, lease aktif |
| antrean start | eligible | desired, lifecycle, blocker, `last_failure` lewat `LOGIN_ELIGIBLE_FAILURES` |
| stop manual | sesi untuk `evaluate_stop` | `LoginSession` yang disimpan daemon saat start |
| akhir sesi | apakah ini logout | `LaunchContext` plus `shutdown_requested` di memori |
| akhir sesi | tenggat | `SESSION_END_STOP_BUDGET` 15 detik di `SupervisorConfig`, dihitung dari `t0` |
| retry secret | interval | `LOGIN_SECRET_RETRY_INTERVAL` 30 detik di `SupervisorConfig` |
| `autostart.session` | `active`, `suppressed`, `none` | lease `startup` sesi sekarang untuk instance itu |
| "Won’t start at login" | state agent | `BackgroundAgentModel` (GUI) dan probe `BackgroundAgent` (CLI), hanya saat `DaemonInfo.session` tidak null dan root default |
| `DaemonSessionInfo` | `startedAt`, `rotation`, `previousEnd` | lihat *Method IPC* |
| konfirmasi Turn Off | instance yang berhenti dan yang menghalangi | `instance.preview_stop_all` (`items`, `alreadyStopping`, `skipped`) |
| konfirmasi Turn Off | jumlah `at_login` | `instance.list`, `autostart.policy` `at_login` |
| start sistem | boleh jalan atau tidak | desired yang dibaca lagi setelah lock instance dipegang |
| `stop_all::select` | instance, lease, operasi terbuka | satu baca registry: instance, lease `active`, dan journal operasi (*Pilihan stop all*) |
| `stop_all_instances` | key yang ditunggu | `LockKey::Instance` setiap instance yang dapat langkah, `BusyPolicy::Queue` |
| `StopAllResult` | daftar | `result.resources` dan `result.partial` operasi `stop_all_instances`, juga untuk replay |
| `agent_off` | penolakan mutasi | gerbang quiesce spec 0030 di `OperationEngine::begin`, dengan kode `daemon.shutting_down` |

### Key invariants

- Paling banyak satu baris `daemon_sessions` terbuka, dan paling banyak satu lease `startup` tidak `released` per instance per sesi (unique index).
- Rotasi tidak pernah menyalakan atau menghentikan proses sendiri; ia hanya menulis desired, lease, dan baris sesi dalam satu transaksi.
- Rotasi `first` dan rotasi di konteks manual tidak mengubah desired.
- Daemon tidak pernah menyalakan instance yang desired nya `stopped`, dan tidak pernah menyalakan apa pun di boot tanpa login.
- Akhir sesi tidak mengubah desired dan tidak suppress; hanya stop pengguna, `stop_all`, dan `agent_off` yang suppress.
- `instance.stop_all`, `agent_off`, `instance.preview_stop_all`, dan konfirmasi Turn Off memilih instance hanya lewat `stop_all::select`.
- Start sistem tidak pernah mengubah desired; ia hanya menjalankan instance yang desired nya masih `running` di bawah lock.
- Daemon `agent_off` hanya keluar setelah setiap instance pilihan, termasuk `already_stopping`, tidak lagi punya proses.
- Start pemicu `login`, `reconcile`, dan `secret_retry` tidak pernah menampilkan dialog Keychain.
- Stackly tidak pernah memanggil `register` atau `unregister` tanpa aksi pengguna.

### Security model

Semua method lewat handshake dan peer user yang sama (spec 0006), dan mutasi wajib `idempotencyKey`. `SessionId` bukan rahasia; tidak ada secret yang masuk ke lease, baris sesi, log, atau event. Interaksi secret mengikuti spec 0011.

### Configuration required

Tidak ada env var baru. `SESSION_END_STOP_BUDGET` (15 detik), `LOGIN_SECRET_RETRY_INTERVAL` (30 detik), dan `LOGIN_ELIGIBLE_FAILURES` ada di `SupervisorConfig` dan core, dan bisa diperpendek di test. Plist `ExitTimeOut` tetap 30.

### Critical test scenarios

- Tabel `plan_session_rotation`: tanpa baris (`first`), id beda dengan baris terbuka, id sama terbuka, id sama tertutup, kombinasi `at_login` dan `manual`, lease environment, instance retained, `unknown`, dan belum initialized. Memverifikasi **AC-2**, **AC-3**, **AC-5**, **AC-14**.
- Daemon in process dengan `FakeLoginSessionInfo` dan konteks launchd: sesi A, instance `at_login` menyala; stop; restart daemon di sesi A tidak menyalakan; ganti ke sesi B, instance menyala sekali. Memverifikasi **AC-4**, **AC-8**.
- Crash disimulasikan tepat sesudah commit rotasi `new`: start berikutnya (`continued`) tetap menyalakan instance `at_login` dan tidak membuat lease kedua. Memverifikasi **AC-7**.
- Upgrade: registry dengan instance yang jalan dan tanpa `daemon_sessions`; tidak ada yang di stop. Memverifikasi **AC-3**.
- Konteks manual: `SpawnedDaemon` dengan sesi lain tidak mengubah desired, dan `SIGTERM` membiarkan instance hidup. Memverifikasi **AC-12**.
- Logout simulasi di konteks launchd: `SIGTERM` tanpa shutdown, dua instance fake, satu dengan stop lambat di atas budget yang diperpendek, satu start yang sedang berjalan; baris `interrupted`, tidak ada `SIGKILL`, start dibatalkan. Memverifikasi **AC-11**.
- Boot sama, sesi baru, proses `Gone`: tidak ada respawn dan tidak ada hitungan crash; proses `Proven` dengan policy `manual` di stop dengan pemicu `session_rotation`. Memverifikasi **AC-6**.
- `SIGTERM` lalu start di sesi yang sama: instance yang dihentikan akhir sesi menyala lagi, yang di stop pengguna tidak. Memverifikasi **AC-13**.
- `FakeSecretStore` terkunci: blocker `NeedsSecret`, pemeriksaan setiap interval tanpa operasi, start `secret_retry` saat terbuka, berhenti saat stop pengguna (perubahan registry saja). Memverifikasi **AC-8**, **AC-15**.
- Tabel `stop_all::select`: setiap baris tabel *Pilihan stop all*, termasuk restart di separuh stop dan di antara stop dan start, start pengguna yang antre, stop pengguna yang sedang berjalan, `stopping` yang macet, operasi `needs_recovery`, dan urutan nama. Memverifikasi **AC-10**.
- `stop_all` dengan instance `unknown`, instance berlease environment, instance menunggu Keychain, start yang sedang berjalan (dibatalkan lalu instance berhenti), instance yang dihapus selagi antre, cancel saat antre dan di tengah, pilihan kosong, dan replay selagi berjalan, sesudah `succeeded`, sesudah `cancelled`, dan sesudah `failed`. Memverifikasi **AC-10**.
- Start sistem (`login`, `reconcile`, `secret_retry`) yang antre di belakang `stop_all` tidak menyalakan instance yang sudah desired `stopped`. Memverifikasi **AC-10**, **AC-20**.
- `daemon uninstall` dengan `FakeBackgroundAgent`: `exiting: true` lalu unregister; `exiting: false` tanpa unregister; start yang dikirim di antara keduanya ditolak `daemon.shutting_down`; kiriman ulang dengan key yang sama menunggu operasi yang sama; instance `already_stopping` ditunggu sebelum exit; `skipped` di preview menahan CLI tanpa request; `registry.recovery_mode` dalam recovery mode. Memverifikasi **AC-20**.
- `SIGTERM` di konteks launchd selama quiesce restart dan selama `agent_off` masih menghentikan instance: *Akhir sesi* berjalan, gerbang tidak dicabut, dan tidak ada unregister. Memverifikasi **AC-11**, **AC-20**.
- Fixture dan codegen. Memverifikasi **AC-17**.

### Usulan update untuk spec lain

- **Spec 0010**: urutan start mendapat rotasi sebelum rekonsiliasi; rekonsiliasi menerima `session_changed` (AC-6); daftar "jalankan lagi" digabung ke *Antrean start* di konteks launchd (AC-7); langkah `SIGTERM` diganti tabel *Sinyal per konteks* (AC-11, AC-12); stop pengguna mengirim sesi ke `evaluate_stop`, dan stop pada desired `running` tanpa proses menjadi perubahan registry dengan efek lease (AC-8); kalimat "Boot baru tidak pernah menjalankan instance" ditambah "kecuali lewat rotasi `new` spec 0031".
- **Spec 0015**: `ShutdownMode::AgentOff` dan dua field baru di `ShutdownAccepted` spec 0030; record survival `kickstart -k` dan `bootout` kini membuktikan stop bersih plus start kembali (AC-13), bukan instance yang tetap hidup; baris *Batas dengan fitur lain* untuk fitur 31 menunjuk spec ini.
- **Spec 0004**: sumber `session` di tabel parameter adalah `LoginSessionInfo` (AC-1); setter `startup` dibangun di sini.
- **Spec 0005**: tabel `daemon_sessions` dan unique index lease startup.
- **Spec 0020 dan 0021**: `startup` di create (AC-16), dan baris Autostart menjadi toggle (AC-19).
- **Spec 0030**: Follow-up baris 1 terjawab di sini (`ShutdownAccepted` diperluas, bukan diganti; `agent_off` ditolak selama quiesce dan rencana `stopping`; restore menang atas *Antrean start* selama rencana `restoring`), jadi barisnya bisa dicentang.
- **Spec 0007**: kode `operation.cancelled` masuk katalog `operation`; bypass quiesce sebagai method `OperationEngine::begin_bypassing_quiesce` (rencana awalnya flag `bypass_quiesce` di `BeginRequest`).
- **Spec 0010**: start sistem (`reconcile`, dan pemicu baru di spec ini) memeriksa desired di bawah lock dan selesai tanpa start kalau bukan `running` (*Antrean start*).
- **Spec 0030**: restore untuk instance `stop_restore` yang desired nya kini `stopped` (Stop All, Turn Off, atau rotasi sesi menang lebih dulu) mendapat outcome `skipped`, supaya rencana tetap bisa `finished`.
- **Spec 0030** *(ditambah 2026-11-03, disetujui user)*: AC-8 membuktikan survival PostgreSQL dan NATS lewat `stackly daemon restart` dan crash daemon (`SIGKILL`), bukan `launchctl kickstart -k` dan `bootout`, yang di konteks launchd kini akhir sesi (AC-11, AC-13); `HANDOVER_PROVEN` tetap didasarkan pada test itu. Tabel test, *Edge cases*, dan *Consequences* ikut disesuaikan. Spec 0015 AC-16 mendapat penyesuaian yang sama untuk instance fake.
- **Spec 0032**: *Method IPC* menulis balasan `instance.stop_all` sebagai `StopAllResult` (bukan `OperationView`) dan `instance.preview_stop_all` sebagai milik spec 0031; langkah 9 tinggal `expectedInstanceIds`, `instance.stop_all_changed`, popover, dan `stackly instance stop --all`; *Copy Stop All* mendapat kalimat untuk `operation.recovery_required` dan `operation.cancelled`.

## Build plan

Approach: Journey. Setiap journey dibangun lengkap, termasuk state, error, dan teks, sebelum journey berikutnya.

Catatan urutan: langkah 1 boleh selesai setelah langkah 2 sampai 9, paling lambat sebelum langkah 10 (*Spike native*, keputusan kamu 2026-11-02).

**Journey 1: login menyalakan instance terpilih**

1. Spike native audit session (*Spike native*), lalu catat hasilnya di `rationale.md`, `launchctl bootout` LaunchAgent spike, dan hapus plist nya. Memenuhi **AC-21**.
2. Port `LoginSessionInfo` di `stackly-platform` dengan `with_login_session`, adapter macOS (`getaudit_addr` dengan `// SAFETY:`), fake di testkit, dan cek Windows. Memenuhi **AC-1**.
3. Migration `NNNN_daemon_sessions.sql` (nomor bebas berikutnya, paling kecil 0010; tabel dan index lease), operasi registry sesi, `DaemonSession` di domain, dan `Instance::set_startup`. Memenuhi **AC-2**, **AC-14**.
4. `plan_session_rotation` di core beserta test tabel, *Urutan daemon start*, `session_changed` di `plan_for`, dan *Antrean start* dengan `LOGIN_ELIGIBLE_FAILURES` dan pemeriksaan desired di bawah lock untuk start sistem. Memenuhi **AC-2** sampai **AC-7**, **AC-12** (bagian rotasi).
5. Protocol: `startup` di create, `instance.set_startup`, `InstanceAutostart`, `DaemonSessionInfo`, kode dan fixture, bump minor, codegen. Memenuhi **AC-16**, **AC-17**.
6. CLI `--autostart`, `instance autostart`, baris `inspect` dan `daemon status`. Memenuhi **AC-18**.
7. GUI toggle di sheet dan Summary, beserta teks. Memenuhi **AC-19**.

Di antara journey 1 dan 5, start sistem dengan Keychain terkunci sudah berakhir dengan blocker `NeedsSecret` spec 0011 tanpa fallback; journey 5 hanya menambah timer.

**Journey 2: stop menahan autostart**

8. Lease manual di start pengguna, sesi di jalur stop, stop registry saja untuk desired `running` tanpa proses, dan `autostart.session` di summary. Memenuhi **AC-8**, **AC-9**.
9. `stop_all::select` dengan test tabel (*Pilihan stop all*), `instance.preview_stop_all`, `instance.stop_all`, `OperationKind::StopAllInstances` (key, cancel start dan restart, langkah `stop`, cancel, `result`, balasan dari keadaan akhir, replay), kode `operation.cancelled`, DTO, fixture, dan bump minor. Memenuhi **AC-10**, **AC-17**. `expectedInstanceIds` dan `instance.stop_all_changed` tetap milik spec 0032 langkah 9.

**Journey 3: logout dan kembali**

10. Spike native logout (*Spike native*), lalu catat hasilnya. Memenuhi **AC-21**.
11. Tabel *Sinyal per konteks*, `shutdown_requested`, urutan *Akhir sesi* dengan budget dan test relasi konstanta, `EndSession`, dan `ReopenSession` di rotasi `continued`. Memenuhi **AC-11**, **AC-12**, **AC-13**.

**Journey 4: agent dimatikan atau dicabut**

12. `ShutdownMode::AgentOff` di atas `ShutdownAccepted` spec 0030 (replay dulu, gerbang quiesce dengan `begin_bypassing_quiesce`, langkah `already_stopping`, `select` terakhir sebelum exit, penolakan di recovery mode), konfirmasi dari `instance.preview_stop_all`, `stackly daemon uninstall`, Turn Off… di Diagnostics, dan teks "Won’t start at login". Memenuhi **AC-19**, **AC-20**.

**Journey 5: Keychain terkunci**

13. Timer pemeriksaan secret di actor supervisor, pemicu `secret_retry`, dan teks blocker. Memenuhi **AC-15**.

**Penutup**

14. Record native setelah persetujuan kamu, lalu update spec 0004, 0005, 0010, 0015, 0020, dan 0021 sesuai *Usulan update*. Memenuhi **AC-21**.

## Consequences

**Positive**:

- Perilaku login, stop, logout, dan reboot punya satu aturan yang bisa dijelaskan di Settings.
- Start yang level triggered membuat crash di titik mana pun pulih ke keadaan yang sama, tanpa kasus `resumed` terpisah.
- Fondasi lease dari spec 0004 akhirnya dipakai di produksi, jadi fitur 47 tinggal menambah sumber environment.
- Logout tidak lagi membuat PostgreSQL crash recovery selama stop selesai dalam 15 detik.

**Negative / tradeoffs**:

- Di konteks launchd, `kickstart -k`, `bootout`, dan `relaunch` kini menghentikan lalu menyalakan instance, bukan membiarkannya hidup. Ini menambah disruption singkat dan mengubah bukti spec 0015.
- Cluster yang butuh lebih dari 15 detik untuk stop berakhir `interrupted` saat logout.
- `stackly daemon stop` lalu logout membuat instance diakhiri OS tanpa stop bersih.
- Daemon manual dan test tidak pernah menjalankan aturan sesi, jadi aturan itu hanya terbukti lewat konteks launchd yang disimulasikan dan record native.
- Pemeriksaan secret setiap 30 detik membaca Keychain diam diam selama terkunci.

**Neutral**:

- Satu tabel baru, satu index lease, dan satu migration; lease dan kolom `startup` sudah ada.
- Contract minor bump, dua operation kind baru, dan satu `ShutdownMode` baru.

## Follow-up

- [ ] Fitur 32: Stop Services memakai `instance.stop_all`, dan Settings menjelaskan perilaku autostart dengan *Batas disruption*.
- [x] `/architect` update spec 0032 dengan baris **Spec 0032** di *Usulan update untuk spec lain*: `stop_all::select`, cancel, dan `instance.preview_stop_all` sudah dibangun fitur 31 langkah 9, jadi langkah 9 spec 0032 tinggal `expectedInstanceIds`, `instance.stop_all_changed`, popover, dan `stackly instance stop --all`. Aturan pilihan yang berlaku ada di *Pilihan stop all* (lebih tajam dari teks *Tambahan* spec 0032 untuk start dan restart yang terbuka, dan untuk operasi `needs_recovery`). Diterapkan di spec 0032 (2026-11-02): *Method IPC*, langkah 9, dan *Copy Stop All*; dicek 2026-11-03.
- [x] `/architect` update spec 0030 dengan outcome `skipped` untuk restore yang desired nya kini `stopped`. Diterapkan di spec 0030 (*Edge cases*, `faafa832`), bersama baris Follow-up 1 spec 0030 yang sudah dicentang; dicek 2026-11-03.
- [x] Langkah 14, bagian update spec: *Usulan update untuk spec lain* diterapkan ke spec 0004, 0005, 0007, 0010, 0015, 0020, 0021, dan 0030 (2026-11-03), mengikuti kode yang sudah dibangun dan `verify.md` (misalnya stop akhir sesi lewat `Origin::Background` tanpa key, baris sesi `interrupted` juga saat stop gagal, dan bypass quiesce sebagai method `begin_bypassing_quiesce`, bukan field `bypass_quiesce`). Teks AC yang ikut berubah, semuanya disetujui user 2026-11-03: spec 0030 AC-8 dan spec 0015 AC-16 (survival lewat restart terencana dan `SIGKILL`; `kickstart -k` dan `bootout` menjadi akhir sesi lalu start lagi), spec 0004 AC-13 dan AC-15 (capability `login_session`), spec 0010 AC-21 (dipersempit ke konteks manual dan sinyal yang bukan logout), spec 0015 AC-11 (pemanggil `unregister`) dan AC-17 (izin yang dimatikan), spec 0020 AC-12 (`startup` dari params atau default), dan spec 0021 AC-7 (Autostart dikecualikan dari paritas teks). Record native langkah 14 tetap terbuka.
- [ ] Hasil spike audit session dan logout dicatat di `rationale.md`; hasil yang bertentangan dengan asumsi membawa spec ini kembali ke `/architect` sebelum journey 3.
- [ ] Fitur 46 dan 47: aturan lease environment di rotasi sesi (sekarang lease environment hanya menahan desired `stopped` untuk instance `manual`) dan override di `stop_all` dan `agent_off`.
- [ ] Fitur 26: restart tidak memanggil suppression.
- [ ] Fitur 30: restart daemon terencana memakai `daemon.shutdown` supaya tidak masuk jalur *Akhir sesi*.
- [ ] Fitur 66: runbook memuat tabel *Batas disruption*.
- [ ] Cek mockup baris Autostart di Summary; kalau mockup menggambar teks biasa, toggle dicatat sebagai selisih yang disengaja di `docs/scope/ui-macos.md`.
- [ ] `LAUNCHD_EXIT_TIMEOUT` (`apps/daemon/src/shutdown.rs`, dipakai test relasi budget akhir sesi) dan `EXIT_TIMEOUT_SECS` (`xtask/src/build_app/plist.rs`, nilai `ExitTimeOut` di plist) sama sama 30 detik tetapi tidak terhubung. Satukan di satu konstanta, misalnya di `crates/platform/src/bundle.rs` seperti `LAUNCHD_THROTTLE_INTERVAL_SECS`, atau tambah test yang membandingkan keduanya, supaya relasi AC-11 tetap benar kalau salah satunya berubah.
- [ ] Blocker `store_locked` yang basi saat resolve secret menjawab `Backend`, dan timer `secret_retry` yang tidak berhenti dalam keadaan itu (spec 0011 *Needs secret*, AC-15 spec ini). Kalau perbaikannya mengubah teks AC, perubahan itu butuh persetujuan user. Tambah juga log level debug dengan kode status asli Keychain di adapter `crates/platform-macos/src/secret.rs`.

## Rationale

Alasan dan opsi: lihat [rationale.md](rationale.md).
