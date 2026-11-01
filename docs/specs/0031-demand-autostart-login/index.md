# 0031. Demand, autostart, dan login

**Date**: 2026-11-02
**Status**: Proposed

## Summary

Spec ini menetapkan apa yang terjadi pada instance saat kamu login, stop, dan logout. Daemon mengenali sesi login lewat `SessionId` (UUID boot ditambah audit session macOS). Di setiap sesi baru, instance dengan autostart At login dinyalakan, dan instance dengan autostart Off tetap mati. Stop yang kamu lakukan menahan autostart sampai login berikutnya (suppression). Logout menghentikan instance dengan bersih di dalam jendela launchd. Autostart daemon tetap setting terpisah: mematikannya berarti unregister background agent, setelah daemon menghentikan instance dengan bersih. Semua aturan sesi hanya berlaku untuk daemon yang dijalankan launchd; daemon manual dan test tetap berperilaku seperti spec 0010.

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
- **AC-10** (`instance.stop_all`): method baru `instance.stop_all { idempotencyKey }` (`recovery refused`) menjalankan satu operasi `stop_all_instances` dengan satu langkah per instance yang desired nya `running` atau yang punya proses hidup. Setiap langkah memakai jalur AC-8, bersamaan sampai `RECONCILE_START_CONCURRENCY`. Instance `unknown` dan instance dengan lease environment `active` masuk `skipped` dengan kode `supervisor.identity_unproven` atau kode override lease spec 0010 (aturan override milik fitur 47). Instance yang lock nya dipegang operasi lain menunggu lock itu seperti `instance.stop`. Balasannya `StopAllResult { stopped, failed, skipped }`, dan replay mengembalikan balasan yang sama (spec 0007). Fitur 32 memakai method ini untuk Stop Services.
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
| Rencana restart daemon, quiesce, `ShutdownAccepted` | spec 0030 (dipakai; spec ini menyesuaikan diri) |
| `BackgroundAgent`, `LaunchContext`, banner agent, `daemon.shutdown` | spec 0015 (dipakai dan diperluas dengan `agent_off`) |
| Blocker `NeedsSecret`, interaksi Keychain | spec 0011 (dipakai) |
| Restart dan reload | fitur 26 (restart tidak suppress) |
| Restart daemon terencana dengan jendela disruption | fitur 30 |
| Menu bar, Stop Services, halaman Settings | fitur 32 (memakai `instance.stop_all` dan teks autostart dari sini) |
| Lease environment, override stop | fitur 46 dan 47 |

### Model demand

Desired state tetap field sendiri (spec 0004). Lease menjelaskan alasan:

| Sumber | Dibuat | Berakhir | Sesi |
| --- | --- | --- | --- |
| `startup` | rotasi `new`, untuk instance `at_login` yang bisa jalan | `suppressed` oleh stop manual atau `stop_all`; `released` di rotasi `new` berikutnya atau saat policy menjadi `manual` | wajib, sesi sekarang |
| `manual` | `instance.start` pengguna (AC-9) | `released` oleh stop manual, `stop_all`, atau rotasi `new` | sesi sekarang, atau tanpa sesi di konteks manual |
| `environment` | fitur 46 | fitur 47 | tanpa sesi, bertahan lintas sesi |

`stopped` dengan desired `running` adalah keadaan istirahat yang sah (misalnya sesudah *Akhir sesi*, atau saat menunggu Keychain). Exit proses yang dihentikan *Akhir sesi* tidak pernah dibaca sebagai crash, karena stop selalu lewat `stopping` dan exit saat `stopping` menjadi `exited` (spec 0010).

### Urutan daemon start

1. `recover` spec 0007 menutup operasi yang terputus.
2. *Rotasi sesi* (konteks launchd saja, dan dilewati di recovery mode).
3. Rekonsiliasi spec 0010, dengan `session_changed` true hanya untuk rotasi `new`.
4. `OperationEngine::start()`, lalu *Antrean start* (konteks launchd), atau daftar "jalankan lagi" spec 0010 apa adanya (konteks manual).

Gagal membaca `LoginSession` di konteks launchd membuat rotasi `skipped`, subsystem `session` `degraded` dengan kode `platform.session_unavailable`, dan antrean start hanya berisi daftar "jalankan lagi" spec 0010. Guard rotasi yang gagal (error registry) menghentikan start daemon dengan exit 1, seperti error registry lain di slot start.

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
- **Rencana restart**: instance yang ada di rencana restart spec 0030 berstatus `restoring` tidak masuk antrean, karena restore spec 0030 yang menyalakannya. Satu instance hanya dinyalakan satu jalur.
- **Pemicu**: `login` kalau rotasi `new`, `reconcile` untuk yang lain; instance yang ada di dua daftar memakai `login`. Recovering tetap lewat jalur respawn actor spec 0010.
- **Satu percobaan**: setiap instance dicoba sekali per daemon start dengan budget restart baru. Kegagalan selain `NeedsSecret` tidak dicoba lagi otomatis.

### Sinyal per konteks

| Konteks | Sinyal | Perilaku |
| --- | --- | --- |
| launchd | `SIGTERM` tanpa `daemon.shutdown` sebelumnya | *Akhir sesi* |
| launchd | `SIGTERM` sesudah `daemon.shutdown` `stop` | spec 0010: instance dibiarkan, baris sesi ditutup `clean` |
| launchd | `SIGTERM` sesudah `daemon.shutdown` `restart` | rencana restart spec 0030; baris sesi ditutup `clean` |
| launchd | `SIGINT`, `SIGHUP` | sama dengan `SIGTERM` sesudah `daemon.shutdown` `stop` (bukan logout) |
| manual | `SIGTERM`, `SIGINT`, `SIGHUP` | spec 0006 dan 0010 apa adanya, tanpa sesi |
| semua | crash, `SIGKILL` | tidak ada handler; adopsi di start berikutnya (spec 0010 AC-21) |

### Akhir sesi

Urutan bernomor sejak `SIGTERM` diterima (`t0`):

1. Server IPC menolak setiap request baru, termasuk `daemon.shutdown`, dengan `daemon.shutting_down`.
2. Operasi yang sedang berjalan dibatalkan lewat jalur cancel spec 0007 dan 0010 (start yang dibatalkan dengan proses hidup berakhir lewat alur *Stop* spec 0010).
3. Untuk setiap instance dengan proses hidup, mulai operasi `stop_instance` dengan pemicu `session_end` dan idempotency key `session_end:<SessionId>:<InstanceId>`, tanpa `evaluate_stop`. Instance yang lock nya masih dipegang operasi yang sedang dibatalkan menunggu lock itu di dalam tenggat yang sama.
4. Tunggu sampai semua stop commit, atau sampai `t0 + SESSION_END_STOP_BUDGET` (15 detik).
5. Tulis `EndSession { end_reason }`: `clean` atau `interrupted`. Gagal commit tidak memblokir exit; rotasi berikutnya menutup baris itu `unknown`.
6. Urutan berhenti spec 0006 (actor, probe, pembaca spool, dan watcher berhenti tanpa commit lifecycle, seperti spec 0010).

`SESSION_END_STOP_BUDGET` + batas urutan berhenti spec 0006 + margin 3 detik ≤ `ExitTimeOut` 30. Test konstanta memeriksa relasi ini, dan kalau batas spec 0006 ternyata lebih dari 12 detik, budget stop yang dikurangi. Instance yang masih `stopping` saat tenggat dibiarkan; rekonsiliasi berikutnya membaca `stopping` plus `Gone` sebagai `exited` (spec 0010).

### Matikan agent

1. Client membaca `instance.list`, lalu menampilkan konfirmasi:
   - instance yang akan berhenti (proses hidup);
   - instance `at_login` yang tidak akan menyala di login berikutnya;
   - instance `unknown` dan yang dipegang lease environment, yang membuat proses berhenti sebelum unregister.
2. `daemon.shutdown { mode: agent_off }`. Selama quiesce atau rencana restart `stopping` spec 0030, request ini ditolak `daemon.restart_pending`, dan sesudah `handed_over` ditolak `daemon.shutting_down`. Selain itu, daemon menolak mutasi baru sejak request itu diterima, lalu menjalankan langkah yang sama dengan `instance.stop_all` (AC-10). Kalau `failed` dan `skipped` kosong, daemon membalas `ShutdownAccepted` (spec 0030) dengan `exiting: true` dan `stopAll`, lalu keluar 0 seperti `stop`. Selain itu, daemon membalas `exiting: false`, membuka lagi mutasi, dan tetap jalan.
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

### Model data

Migration `NNNN_daemon_sessions.sql` (nomor bebas berikutnya saat build; spec 0030 juga merencanakan migrasi, jadi siapa pun yang build lebih dulu memakai nomor itu):

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
| `instance.stop_all` | `idempotencyKey` | `StopAllResult { stopped: [Uuid], failed: [{ instanceId, code }], skipped: [{ instanceId, code }] }` | refused | per instance di `failed` dan `skipped` |
| `instance.create` (diperluas) | `startup` opsional, default `manual` | sama | sama | sama |
| `daemon.shutdown` (diperluas) | `mode` `agent_off` baru | `ShutdownAccepted` spec 0030 diperluas dengan `exiting: bool` dan `stopAll: StopAllResult \| null`; `stopAll` null untuk `stop` dan `restart` | sama | sama, plus `daemon.restart_pending` (spec 0030) |

Nilai DTO baru:

- `InstanceAutostart { policy: StartupPolicyKind, session: AutostartSession }`, opsional `#[serde(default)]` di `InstanceSummary`. `session` bernilai `active` (lease startup sesi ini `active`), `suppressed` (lease itu `suppressed`), `none` (tidak ada lease startup sesi ini, termasuk di konteks manual), atau `Unrecognized`.
- `DaemonSessionInfo`, opsional di `DaemonInfo`, null di konteks manual atau saat sesi gagal dibaca:
  - `id`;
  - `startedAt`: `started_at` baris sesi, yaitu awal sesi, bukan awal run daemon;
  - `rotation`: `first`, `new`, atau `continued`, hasil run daemon ini yang disimpan di memori;
  - `previousEnd`: `end_reason` baris tepat sebelum baris sesi sekarang, atau null kalau tidak ada.
- Kode baru: `platform.session_unavailable`. Operation kind baru: `set_instance_startup` dan `stop_all_instances`. Pemicu start baru: `login` dan `secret_retry`; pemicu stop baru: `session_end` dan `session_rotation`.

### CLI

- `stackly instance create … --autostart`: `startup` `at_login`.
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
| Blocker | `NeedsSecret` dengan `store_locked` atau `busy` | "The Keychain is locked. Stackly tries again every 30 seconds." |
| Konfirmasi Turn Off | | judul "Turn Off Background Agent?"; isi "{n} running instances will stop. {m} instances set to start at login won’t start until you turn it on again."; tombol "Turn Off" |
| Turn Off ditolak | `exiting: false` | "Background agent is still on. These instances couldn’t be stopped: {daftar}." |

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
| konfirmasi Turn Off | jumlah instance | `instance.list`: `process` ada, `autostart.policy` `at_login`, lifecycle `unknown` |
| `StopAllResult` | daftar | langkah per instance operasi `stop_all_instances` |

### Key invariants

- Paling banyak satu baris `daemon_sessions` terbuka, dan paling banyak satu lease `startup` tidak `released` per instance per sesi (unique index).
- Rotasi tidak pernah menyalakan atau menghentikan proses sendiri; ia hanya menulis desired, lease, dan baris sesi dalam satu transaksi.
- Rotasi `first` dan rotasi di konteks manual tidak mengubah desired.
- Daemon tidak pernah menyalakan instance yang desired nya `stopped`, dan tidak pernah menyalakan apa pun di boot tanpa login.
- Akhir sesi tidak mengubah desired dan tidak suppress; hanya stop pengguna, `stop_all`, dan `agent_off` yang suppress.
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
- `stop_all` dengan instance `unknown`, instance berlease environment, instance menunggu Keychain, dan instance yang lock nya dipegang. Memverifikasi **AC-10**.
- `daemon uninstall` dengan `FakeBackgroundAgent`: `exiting: true` lalu unregister; `exiting: false` tanpa unregister; start yang dikirim di antara keduanya ditolak. Memverifikasi **AC-20**.
- Fixture dan codegen. Memverifikasi **AC-17**.

### Usulan update untuk spec lain

- **Spec 0010**: urutan start mendapat rotasi sebelum rekonsiliasi; rekonsiliasi menerima `session_changed` (AC-6); daftar "jalankan lagi" digabung ke *Antrean start* di konteks launchd (AC-7); langkah `SIGTERM` diganti tabel *Sinyal per konteks* (AC-11, AC-12); stop pengguna mengirim sesi ke `evaluate_stop`, dan stop pada desired `running` tanpa proses menjadi perubahan registry dengan efek lease (AC-8); kalimat "Boot baru tidak pernah menjalankan instance" ditambah "kecuali lewat rotasi `new` spec 0031".
- **Spec 0015**: `ShutdownMode::AgentOff` dan dua field baru di `ShutdownAccepted` spec 0030; record survival `kickstart -k` dan `bootout` kini membuktikan stop bersih plus start kembali (AC-13), bukan instance yang tetap hidup; baris *Batas dengan fitur lain* untuk fitur 31 menunjuk spec ini.
- **Spec 0004**: sumber `session` di tabel parameter adalah `LoginSessionInfo` (AC-1); setter `startup` dibangun di sini.
- **Spec 0005**: tabel `daemon_sessions` dan unique index lease startup.
- **Spec 0020 dan 0021**: `startup` di create (AC-16), dan baris Autostart menjadi toggle (AC-19).

## Build plan

Approach: Journey. Setiap journey dibangun lengkap, termasuk state, error, dan teks, sebelum journey berikutnya.

**Journey 1: login menyalakan instance terpilih**

1. Spike native audit session (*Spike native*), lalu catat hasilnya. Memenuhi **AC-21**.
2. Port `LoginSessionInfo` di `stackly-platform` dengan `with_login_session`, adapter macOS (`getaudit_addr` dengan `// SAFETY:`), fake di testkit, dan cek Windows. Memenuhi **AC-1**.
3. Migration `NNNN_daemon_sessions.sql` (tabel dan index lease), operasi registry sesi, `DaemonSession` di domain, dan `Instance::set_startup`. Memenuhi **AC-2**, **AC-14**.
4. `plan_session_rotation` di core beserta test tabel, *Urutan daemon start*, `session_changed` di `plan_for`, dan *Antrean start* dengan `LOGIN_ELIGIBLE_FAILURES`. Memenuhi **AC-2** sampai **AC-7**, **AC-12** (bagian rotasi).
5. Protocol: `startup` di create, `instance.set_startup`, `InstanceAutostart`, `DaemonSessionInfo`, kode dan fixture, bump minor, codegen. Memenuhi **AC-16**, **AC-17**.
6. CLI `--autostart`, `instance autostart`, baris `inspect` dan `daemon status`. Memenuhi **AC-18**.
7. GUI toggle di sheet dan Summary, beserta teks. Memenuhi **AC-19**.

Di antara journey 1 dan 5, start sistem dengan Keychain terkunci sudah berakhir dengan blocker `NeedsSecret` spec 0011 tanpa fallback; journey 5 hanya menambah timer.

**Journey 2: stop menahan autostart**

8. Lease manual di start pengguna, sesi di jalur stop, stop registry saja untuk desired `running` tanpa proses, dan `autostart.session` di summary. Memenuhi **AC-8**, **AC-9**.
9. `instance.stop_all` dan operasi `stop_all_instances`. Memenuhi **AC-10**.

**Journey 3: logout dan kembali**

10. Spike native logout (*Spike native*), lalu catat hasilnya. Memenuhi **AC-21**.
11. Tabel *Sinyal per konteks*, `shutdown_requested`, urutan *Akhir sesi* dengan budget dan test relasi konstanta, `EndSession`, dan `ReopenSession` di rotasi `continued`. Memenuhi **AC-11**, **AC-12**, **AC-13**.

**Journey 4: agent dimatikan atau dicabut**

12. `ShutdownMode::AgentOff`, `stackly daemon uninstall`, Turn Off… di Diagnostics, dan teks "Won’t start at login". Memenuhi **AC-19**, **AC-20**.

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
- [ ] Fitur 46 dan 47: aturan lease environment di rotasi sesi (sekarang lease environment hanya menahan desired `stopped` untuk instance `manual`) dan override di `stop_all` dan `agent_off`.
- [ ] Fitur 26: restart tidak memanggil suppression.
- [ ] Fitur 30: restart daemon terencana memakai `daemon.shutdown` supaya tidak masuk jalur *Akhir sesi*.
- [ ] Fitur 66: runbook memuat tabel *Batas disruption*.
- [ ] Cek mockup baris Autostart di Summary; kalau mockup menggambar teks biasa, toggle dicatat sebagai selisih yang disengaja di `docs/scope/ui-macos.md`.

## Rationale

Alasan dan opsi: lihat [rationale.md](rationale.md).
