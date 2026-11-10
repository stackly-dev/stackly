# 0033. Crash loop dan error start yang aman

**Date**: 2026-11-10
**Status**: In Progress

## Summary

Spec ini menetapkan apa yang kamu lihat saat sebuah instance gagal start atau terus crash, di GUI dan di CLI. Supervisor (spec 0010) sudah membatasi crash loop di 5 percobaan per 5 menit dengan backoff; fitur ini tidak mengubah aturan itu, tetapi membuatnya terlihat: jeda backoff menjadi stage operasi yang bisa dibaca, daemon mulai mengisi waktu, exit code, dan sinyal kegagalan terakhir, error port membawa saran port bebas, dan setiap error start membawa operation ID sebagai correlation ID. Di GUI, banner kegagalan di tab Summary dan inspector menyebut sebabnya (crash loop, port dipakai, data dipakai, dan lainnya), kode error, cuplikan output, serta tombol Use Port, Show Logs, Retry, dan Diagnose. Di CLI, error dan `stackly instance inspect` menampilkan hal yang sama plus perintah lanjutan yang bisa langsung kamu salin. Stackly tidak pernah mengganti port, mereset data, atau menghapus lock tanpa aksi eksplisit kamu, dan kontrak protokol tidak berubah.

## Requirements

**User stories**:

- Sebagai developer, saya ingin tahu kenapa instance saya gagal start (port dipakai, data dikunci, terus crash) dalam satu kalimat dan satu kode, supaya saya tidak perlu menebak.
- Sebagai developer, saya ingin melihat bahwa Stackly sedang menunggu untuk restart (percobaan ke berapa, kapan) dan bahwa ia berhenti sendiri setelah budget habis, supaya saya yakin Mac saya tidak terjebak loop.
- Sebagai developer, saya ingin satu klik untuk memakai port bebas yang disarankan, tetapi tetap lewat alur ganti port yang saya konfirmasi, supaya port fixed saya tidak pernah berubah diam diam.
- Sebagai developer, saya ingin langsung ke log, ke doctor, atau mencoba lagi dari pesan errornya, dan menyalin operation ID untuk dilaporkan, supaya pemulihan tidak butuh menelusuri layar lain.
- Sebagai pengguna CLI, saya ingin error start dan `inspect` menampilkan sebab, kode, operation ID, dan perintah lanjutan, supaya hasilnya setara dengan GUI.

**Acceptance criteria**:

- **AC-1** (retry budget, AC-068, PRD 19.4 dan 35.2): Fake provider dengan `crashAfterMs` kecil yang crash sebelum ready, dijalankan lewat `instance.start`, menghasilkan tepat 6 spawn (spawn pertama ditambah 5 respawn), lalu berhenti dengan lifecycle `failed`, `lastFailure.code` `crash_loop`, `restart.exhausted` true, dan operasi start `failed` dengan `supervisor.retry_budget_exhausted` (detail `attempts` 6 dan `lastExitCode` dari crash keenam, lewat `with_last_exit` yang sudah ada). Penanda `started` dan `exited` di store log spec 0027 membuktikan tidak pernah ada dua proses utama bersamaan (setiap `exited` mendahului `started` berikutnya) dan tidak ada spawn selama jeda: jarak spawn ke n+1 setelah exit ke n paling sedikit `RESTART_BASE_DELAY × 2^(n−1) × 0,5` dari konfigurasi test. Fake yang crash setelah ready juga berhenti di 6 spawn dengan `failed`, `crash_loop`, dan `exhausted`; di jalur ini jeda pertama dijalankan timer actor tanpa operasi, setiap respawn adalah operasi sistem `start_instance` (pemicu `restart_policy`), dan crash keenam diputuskan actor tanpa operasi bila proses terakhir sempat ready. Konstanta tetap milik spec 0010; spec ini tidak mengubahnya.
- **AC-2** (backoff terlihat): Setiap jeda di dalam operasi `start_instance` (start pengguna, atau operasi respawn sistem untuk crash sebelum ready) membuat stage operasi itu `backoff` setelah `show_pause` menerbitkan `instance.health_changed` dengan `restart.nextAttemptAt` di masa depan, sehingga client yang melihat stage `backoff` lalu membaca `instance.get` selalu mendapat `nextAttemptAt` yang terisi. Percobaan berikutnya kembali ke stage `lock`. `restart.attempts` adalah jumlah crash di jendela, termasuk crash yang memicu jeda, jadi respawn yang ditunggu adalah respawn ke `attempts` dari `budget`. Jeda timer actor di luar operasi tidak punya stage, tetapi field `restart` yang sama berlaku.
- **AC-3** (fakta kegagalan terakhir): Untuk setiap kegagalan yang dicatat daemon run ini (`crashed` dengan atau tanpa retry, `start_failed`, `init_failed`), `lastFailure.at` adalah waktu commit yang mencatatnya, dan `lastFailure.exitCode` atau `lastFailure.signal` (nomor sinyal) adalah exit proses yang memicunya, bila ada. Setiap commit seperti itu menimpa fakta sebelumnya, jadi `crash_loop` membawa waktu dan exit crash keenam. Setelah daemon restart ketiganya null, seperti yang sudah dijanjikan DTO. `excerpt` tidak berubah. Tidak ada kolom registry baru.
- **AC-4** (saran port, AC-010, AC-055): Kegagalan start atau restart di tahap `endpoints` dengan `supervisor.endpoint_in_use`, `supervisor.endpoint_conflict`, atau `supervisor.endpoint_unavailable`, dan kegagalan `supervisor.endpoint_in_use` dari exit dini setelah spawn, membawa detail `suggestedPort` saat ada port bebas, dicari dengan pencarian yang sama dengan spec 0020 dan 0029 (port pertama sesudah port itu yang lolos pemeriksaan sendiri, instance lain, dan bind percobaan). Detail itu tidak ada saat tidak ada kandidat atau saat daftar instance lain gagal dibaca. Port, revision config, dan registry tidak berubah karena pencarian ini.
- **AC-5** (envelope dan correlation ID, NFR-011, VAL-020): Setiap operasi start atau restart yang gagal punya `failure.error` dengan `code`, `category`, `message`, `retryable`, `details` dari katalog, dan `operationId` yang sama dengan ID operasi. `supervisor.retry_budget_exhausted`, `endpoint_in_use`, `endpoint_conflict`, `endpoint_unavailable`, dan `data_in_use` mendapat hint statis dari *Hint statis* lewat fallback hint spec 0006. Baris log JSON daemon yang mencatat kegagalan operasi itu membawa field operation ID yang sama (span spec 0001).
- **AC-6** (error start di CLI): `stackly instance start` dan `stackly instance restart` yang gagal (dengan atau tanpa `--wait`) menulis blok error spec 0008 dengan pesan, `code:`, `hint:` dari daemon, baris baru `operation: <uuid>` setiap kali `operationId` ada, lalu hint CLI dari *Perintah lanjutan CLI* dengan nama instance yang sudah di resolve. Exit code tetap dari kategori. `--json` tidak berubah (ID sudah ada di `operationId`).
- **AC-7** (inspect di CLI): `stackly instance inspect` dan `get` membaca `operation.list { resource, limit: 10 }` dan memilih operasi start terkait menurut *Operasi terkait*. Untuk instance `failed` atau `recovering`, atau `stopped` yang operasi terkaitnya start atau restart yang gagal, output menambah baris `Last operation` (`<kind> failed, <code>, <uuid>`, atau `-` tanpa operasi terkait, atau `unavailable` saat bacaan itu gagal, tanpa membuat command gagal) dan, setelah Failure output, blok `Next steps:` dengan perintah *Perintah lanjutan CLI*. Baris Restarts dan Last failure yang sudah ada kini terisi waktu dan exit code (AC-3). JSON `inspect` mendapat field `lastOperation` (summary operasi terkait, atau null).
- **AC-8** (backoff di `--wait`): Setiap kali operasi yang ditunggu `stackly instance start --wait` atau `restart --wait` masuk stage `backoff` (sekali per jeda), CLI membaca `instance.get` sekali dan menulis "crashed, restart {attempts} of {budget} at {time}" lewat `Progress` spec 0008 (satu baris di mode baris, baris hidup di terminal), dengan `{time}` dari helper waktu yang sama dengan baris Restarts `inspect`. Kalau `nextAttemptAt` null atau bacaan gagal, ia menulis "crashed, restarting".
- **AC-9** (banner crash loop, mockup UI): Tab Summary instance `failed` dengan `crash_loop` menampilkan banner `critical` berjudul "Crash loop. Automatic restarts paused.", isi dari *Copy banner* ("It crashed {attempts} times in {window}, so Stackly stopped restarting it and is waiting for you."), blok kode (baris pertama kode error dan operation ID bila ada operasi terkait yang gagal, lalu satu baris per baris `lastFailure.excerpt`), dan tombol Retry (utama), Show Logs, dan Diagnose…. Teks blok kode bisa dipilih untuk disalin.
- **AC-10** (banner port dipakai, alur ganti port spec 0029): Kegagalan karena port (AC-4) menampilkan banner berjudul "Couldn’t start: port in use" atau "Couldn’t start: port unavailable" dengan isi dari *Copy banner*. Saat error operasi terkait membawa `endpoint` dan `suggestedPort`, tombol utamanya "Use Port {n}" yang membuka sheet Ports spec 0029 dengan endpoint itu terisi port {n} dan verdict dry run langsung berjalan; tanpa saran atau tanpa nama endpoint (misalnya hanya `lastFailure.code` setelah daemon restart), tombolnya "Change Port…" yang membuka sheet yang sama tanpa isian. Port hanya berubah saat kamu menekan tombol konfirmasi sheet itu (`instance.change_endpoints`), tidak pernah dari banner; saran yang sudah basi ditolak verdict sheet, yang menampilkan saran barunya sendiri (spec 0029 AC-14). Setelah ganti port berhasil pada instance `failed`, banner menyembunyikan tombol port dan menambah "Its ports changed. Retry to start with them."
- **AC-11** (banner kegagalan lain dan start yang ditolak): Instance `failed` dengan sebab lain menampilkan judul dan isi dari *Copy banner* (kode yang tidak dikenal memakai `InstanceFailureCopy` spec 0022). Instance `stopped` yang operasi terkaitnya (*Operasi terkait*) `start_instance` atau `restart_instance` `failed` (kegagalan sebelum `journal`, misalnya port fixed yang dipakai proses luar) menampilkan banner yang sama dari `failure.error` operasi itu. Banner hilang saat operasi terkait yang lebih baru dimulai (start, restart, stop, atau ganti port) atau lifecycle meninggalkan `stopped` atau `failed`; operasi lain (rename, edit config yang hanya menyimpan) tidak menyembunyikannya.
- **AC-12** (banner restart dan inspector): Instance `recovering` menampilkan banner `caution` "Restarting after a crash" dengan "Restart {attempts} of {budget} in {seconds} s." yang menghitung mundur setiap detik dari `nextAttemptAt` (atau "Stackly is starting it again." saat null), cuplikan excerpt, dan Show Logs. Inspector Instances menampilkan judul dan baris error yang sama dalam bentuk ringkas (judul, blok kode tanpa operation ID, dan tombol Use Port {n} bila ada), mengikuti mockup.
- **AC-13** (Retry): Retry di banner sama dengan aksi utama Retry spec 0022: satu `instance.start` baru dengan idempotency key baru, nonaktif dengan alasan yang sama, label sibuk "Starting…", dan kartu umpan balik spec 0067 saat gagal. Retry pada `failed` memulai budget baru (spec 0010); itulah satu satunya jalan keluar dari crash loop selain stop.
- **AC-14** (tidak ada perbaikan otomatis, JOURNEY-05, PRD 35.3): Daemon, CLI, dan GUI tidak pernah mengganti port, menulis revision config, menghapus atau mereset data, menghapus `instance.lock` atau file di folder run instance, atau menjalankan ulang instance di luar budget tanpa aksi eksplisit. Test mencatat baris registry instance dan endpoint, isi folder data, `instance.lock`, daftar file folder run, dan revision config sebelum dan sesudah crash loop dan kegagalan port, lalu membandingkannya (berkas log dan spool yang memang ditulis selama run dikecualikan).
- **AC-15** (aman dari secret): Credential fixture yang dicetak fake ke stderr sebelum crash tidak pernah muncul di `lastFailure.excerpt`, pesan atau hint error, output CLI teks dan JSON, maupun teks banner dan blok kode.
- **AC-16** (tautan Logs, Operations, dan doctor): Show Logs membuka tab Logs instance itu (spec 0027). Operation ID di banner Summary punya tautan "Show in Operations" yang membuka operasi itu di halaman Operations (spec 0035). Diagnose… membuka halaman Diagnostics dengan fokus instance itu (spec 0034), dan CLI menambah hint `stackly doctor --instance <name>`. Selama fitur 34 atau 35 belum ada di `main`, tautan jatuh ke `.diagnostics` atau `.operations` tanpa fokus, dan hint doctor CLI tidak ditulis (`// provisional: F34`, `// provisional: F35`).
- **AC-17** (fixture dan snapshot, spec 0068): Setiap state di *State banner* punya kasus fixture dan snapshot di `InstanceDetailSnapshotTests` (Summary) dan `InstancesSnapshotTests` (inspector), dengan tiga varian spec 0068 (light, dark, narrow); label tombol dan blok kode tidak terpotong di lebar sempit. *Diperbarui 2026-11-10 (koordinasi scope 05):* "lima varian" menjadi tiga, karena Interaction policy `AGENTS.md` (2026-10-24) menghapus varian aksesibilitas.

## Decision

**Chosen option**: pilihan yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): tanpa field atau method baru, banner dibangun dari `InstanceSummary` ditambah operasi terbaru instance itu (`operation.list` dengan `resource` plus event operasi), daemon mengisi field kegagalan yang sudah ada dan detail `suggestedPort` yang sudah ada di katalog, jeda backoff menjadi stage `backoff`, dan saran port selalu lewat sheet Ports spec 0029.

Spec ini menampilkan keputusan supervisor spec 0010, bukan mendesain ulang: correlation ID adalah operation ID yang sudah ada di envelope spec 0006, dan operasi menjadi sumber kebenaran untuk error yang terjadi sebelum instance berubah.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` dan `swift-concurrency` (lihat [apps/macos/AGENTS.md](../../../apps/macos/AGENTS.md))

## Feature design

### Trace PRD

| ID | Dipenuhi oleh |
| --- | --- |
| AC-068 | AC-1, AC-2, AC-3, AC-8, AC-9, AC-12 |
| JOURNEY-05 | AC-6, AC-7, AC-9, AC-10, AC-11, AC-13, AC-14, AC-16 |
| NFR-011 | AC-5, AC-6, AC-7, AC-9 |
| VAL-020 (tampilan) | AC-5, AC-6, AC-9, AC-10, AC-11 |
| PRD 19.4 | AC-1, AC-2, AC-14 |
| PRD 35.2 (restart retry) | AC-1, konstanta spec 0010 |
| PRD 35.3 (port conflict, crash loop, database start gagal) | AC-4, AC-10, AC-14, *Perintah lanjutan CLI* |

### Batas dengan spec lain

- **Spec 0010**: restart policy, konstanta, budget, jitter, dan tabel *Kode error* tetap. Fitur ini menambah tiga perilaku daemon: stage `backoff`, fakta kegagalan di `InstanceStatus`, dan `suggestedPort` di error endpoint start. Follow-up spec 0010 "Fitur 33: banner crash loop dari `restart` dan `lastFailure`" dijawab AC-9 dan AC-12.
- **Spec 0006**: envelope `ErrorBody` dan `operationId` tetap; fitur ini memakai fallback hint `FailureError` untuk hint statis baru, seperti spec 0025.
- **Spec 0021**: banner I1 di tab Summary diganti `InstanceFailureBanner` spec ini (Follow-up spec 0021 "banner crash loop yang lebih kaya" dijawab AC-9 sampai AC-12). Banner I2, I3, I4, "Still stopping.", dan gagal muat detail tetap di `InstanceDetailBanner`.
- **Spec 0022**: aturan Needs Attention I1 sampai I10 dan teks `InstanceFailureCopy` tetap; banner memakai `InstanceFailureCopy` untuk kode tanpa baris sendiri di *Copy banner* (Follow-up spec 0022 dijawab AC-11). Banner inspector I1 dan I7 memakai view ringkas spec ini.
- **Spec 0012**: state `failed` tetap tanpa detail di `StatusLabel`; alasannya tampil di banner spec ini (catatan "alasan kegagalan ditampilkan banner layar, fitur 33" di tabel status instance spec 0012 dijawab).
- **Spec 0027**: Show Logs membuka tab Logs; test AC-1 membaca penanda `started` dan `exited` dari store log.
- **Spec 0029**: sheet Ports (`EndpointsModel`) mendapat isian awal opsional; `instance.change_endpoints` tidak berubah.
- **Spec 0034** (di `main` sejak 2026-11-10): isi doctor, `ShellNavigator.openDiagnostics(focus: .instance(id))`, dan `stackly doctor --instance <name>` milik spec 0034. Spec ini hanya menautkannya. Doctor membaca `restart` dan `lastFailure` apa adanya.
- **Spec 0035** (di `main` sejak 2026-11-10): halaman Operations, `ShellNavigator.openOperation(_:)`, definisi stage `start_instance` (termasuk `backoff` di grup Launch dengan judul "Waiting to restart"), dan Retry dari halaman Operations milik spec 0035. Retry di banner dan di Operations sama sama mengirim `instance.start` baru.

### Kontrak

- Tidak ada DTO, method, event kind, kode error, atau migrasi baru. `lastFailure.at`, `exitCode`, dan `signal` sudah ada di DTO (hari ini selalu null, `apps/daemon/src/ipc/instance/dto.rs`); detail `suggestedPort` sudah ada di katalog ketiga kode endpoint; nama stage adalah teks bebas di `OperationSummary.stage`. Hint statis tidak masuk `protocols/error-codes.json` (aturan spec 0025).
- Fixture baru di `protocols/fixtures/` (instance `failed` dengan fakta kegagalan terisi, operasi gagal dengan `suggestedPort`, operasi dengan stage `backoff`) tidak mengubah schema, jadi `ProtocolVersion::CURRENT` tidak naik. Kalau build menemukan bahwa sebuah perubahan ternyata mengubah digest kontrak, ambil minor bebas berikutnya dari lead (sementara 3.3) di commit yang sama.

### Daemon dan core

**Stage `backoff`.** `StageName::BACKOFF` (`"backoff"`) di `crates/domain/src/supervisor.rs`. Di `wait_to_respawn` (`crates/core/src/supervisor/startup.rs`), urutannya: `actor.update(show_pause)` (yang menerbitkan `instance.health_changed` dengan `nextAttemptAt`), lalu `ctx.set_stage(StageName::BACKOFF)`, lalu tidur. Tanpa langkah journal (percobaan kedua dan seterusnya tidak menulis langkah, aturan `enter`). Jalur `on_lock_held` yang juga memanggil `wait_to_respawn` ikut mendapat stage ini. Jeda pertama timer actor (`respawn::pause_then_respawn`) tidak punya operasi dan tidak punya stage. Cancel saat `backoff` mengikuti AC-8 spec 0010 tanpa perubahan.

**Fakta kegagalan.** `ActorState` dan `InstanceStatus` mendapat `last_failure: Option<FailureFacts>` dengan `FailureFacts { code: FailureCode, at: Timestamp, exit_code: Option<i32>, signal: Option<i32> }` (`signal` adalah nomor sinyal, sama dengan DTO). Setiap commit yang menulis `crashed` (dengan atau tanpa retry), `start_failed`, atau `init_failed` menimpanya (waktu = `now` commit itu, exit dari `ExitSeen` atau status exit yang sedang diputuskan, keduanya null bila tidak ada proses yang keluar), lalu `actor.update` menerbitkan perubahannya (ikut `differs_visibly`). Fakta dikosongkan bersama `last_failure` registry (`spawned`, `adopted`), jadi instance yang di stop selama jeda tetap membawanya. `dto::summary` memakai fakta itu hanya bila `code` nya sama dengan `instance.last_failure()`; kalau tidak (misalnya setelah daemon restart), ketiga field null seperti sekarang.

**`suggestedPort` di start.** `crates/core/src/instance/create/port.rs` membuka satu fungsi `pub(crate) fn suggest_for_start(probe, endpoint: &IpEndpoint, port: Port, siblings: &[Endpoint], others: &[Instance]) -> Option<Port>` di atas `suggest` dan aturan `judge_change` yang sama. `EndpointFault` mendapat `suggested_port: Option<u16>`, `SupervisorError::EndpointUnavailable` dan `EndpointConflict` juga, dan `endpoint_details` menulis `suggestedPort` bila ada. Tahap `endpoints` mengisinya dari daftar instance lain yang sudah dibaca tahap itu untuk pemeriksaan konflik. Jalur exit dini (`after_unrequested_exit`, `Next::Final(EndpointInUse)`) hanya membaca instance itu sendiri, jadi ia membaca daftar instance lain sekali lewat `Registry` sebelum mencari; bila bacaan itu gagal, error tetap dikirim tanpa `suggestedPort` dan satu log `warn` ditulis.

**Hint statis.** `codes::supervisor::hint` (blok `@functions`, pola spec 0025) mendapat teks di *Hint statis*. `data_incompatible` tetap.

### Hint statis

| Kode | Hint (bahasa Inggris, dikirim daemon) |
| --- | --- |
| `supervisor.retry_budget_exhausted` | "It kept crashing, so Stackly stopped restarting it. Read its logs, fix the cause, then start it again." |
| `supervisor.endpoint_in_use` | "Stackly never moves a fixed port on its own. Pick another port for this endpoint, or stop the process that holds it." |
| `supervisor.endpoint_conflict` | "Stackly never moves a fixed port on its own. Pick another port, or stop that instance." |
| `supervisor.endpoint_unavailable` | "Pick another port for this endpoint." |
| `supervisor.data_in_use` | "Stackly never removes the lock on its own. Stop that process, then start it again." |

Isi banner (*Copy banner*) menyebut faktanya dan hint menyebut langkahnya, jadi tidak ada kalimat yang muncul dua kali di banner.

### Perintah lanjutan CLI

`apps/cli/src/commands/instance/next_steps.rs` (baru) membuat daftar dari `(name, error code, details, lifecycle)`, dipakai blok error start dan restart (sebagai hint) dan blok `Next steps:` `inspect`:

1. Port (ketiga kode endpoint, atau `lastFailure.code` `endpoint_in_use`): "use a free port: stackly instance endpoint set {name} --port {endpoint}={suggestedPort}" bila ada saran, selain itu "pick another port: stackly instance endpoint set {name} --port {endpoint}=<port>".
2. Selalu: "read its logs: stackly instance logs {name}".
3. Selalu: "check this Mac: stackly doctor --instance {name}". *Diperbarui 2026-11-10 (koordinasi scope 05):* sakelar sementara `DOCTOR_COMMAND` dan tanda `// provisional: F34` dihapus setelah fitur 34 mendarat (F34 langkah 11), jadi langkah ini selalu ditulis.
4. Selalu terakhir: "start it again: stackly instance start {name}".

Menjalankan `stackly instance endpoint set` adalah aksi eksplisit kamu, setara dengan tombol konfirmasi sheet Ports; tambahkan `--dry-run` untuk melihat verdict lebih dulu. Ia hanya menyimpan port untuk instance `stopped` atau `failed` (spec 0029 AC-6), karena itu langkah 4 selalu ada. Nama yang mengandung karakter shell ditulis dengan quoting yang sama dengan hint CLI lain (`escape_display`). Blok error juga mendapat baris `operation: <uuid>` di `StderrSink::error` setiap kali `body.operation_id()` ada, untuk semua command, sebelum baris `request:`; golden error yang membawa `operationId` ikut diperbarui.

### GUI

**Operasi terkait.** Dari operasi instance itu, urut `(createdAt, id)` turun seperti `operation.list`, ambil yang pertama berjenis `start_instance`, `restart_instance`, `stop_instance`, atau `change_endpoints`; jenis lain (rename, apply config, reload, create) dilewati. Itulah `latest`. Untuk GUI dan CLI aturannya sama.

- `latest` dipakai sebagai **error** hanya bila ia start atau restart yang `failed`, dan untuk lifecycle `failed` juga cocok dengan kegagalan instance: `latest.updatedAt` ≥ `lastFailure.at` bila `at` ada, atau kode errornya ada di baris yang sama di *Pemetaan sebab* dengan `lastFailure.code` bila `at` null (daemon restart). Selain itu banner hanya memakai `summary` (misalnya crash loop yang diputuskan actor setelah operasi respawn terakhir `succeeded`).
- `latest` berjenis `change_endpoints` yang `succeeded` dan lebih baru dari `lastFailure.at` (atau dengan `at` null) memberi kalimat "Its ports changed. Retry to start with them." dan menyembunyikan tombol port (AC-10); error yang dipakai kemudian adalah operasi start atau restart sebelumnya dengan aturan yang sama.

**Sumber data.** `InstanceFailureModel` (`@Observable`, `StacklyUI/InstanceDetail/Failure/`) per instance memegang operasi terkaitnya: saat banner pertama tampil, satu `operation.list { resource: { kind: instance, id }, limit: 10 }` (tanpa operasi terkait di 10 itu, `latest` nil). Ia membaca ulang saat `revision` atau `statusSeq` summary berubah, tetapi hanya selama instance bisa punya banner (lifecycle `failed`, `stopped`, atau `recovering`; instance `running` sering berubah status dan tidak pernah punya banner), dan saat `OperationCenter` memegang operasi instance itu yang sudah berakhir dan belum ada di bacaan terakhir dengan revision itu; di antara itu, entri `OperationCenter.active` dan `recent` dengan resource yang sama dan `(createdAt, id)` lebih baru menggantikannya. Sebelum bacaan pertama berhasil (masih berjalan, gagal, atau daemon offline), model memberi `latest` nil dan banner tampil dari `summary` saja (judul dari sebab `lastFailure`, tanpa operation ID dan tanpa Use Port), lalu naik ke isi lengkap begitu operasinya tiba; urutan aturan dan *Pemetaan sebab* menjamin judulnya tidak berubah saat naik. Bacaan berikutnya yang gagal, termasuk saat daemon offline, mempertahankan hasil bacaan terakhir yang berhasil. Tab Summary dan inspector berbagi satu model per instance; inspector membuatnya untuk baris terpilih saja. *Diperbarui 2026-11-10 (koordinasi scope 05):* hasil terakhir dipertahankan saat offline, dan baca ulang karena summary hanya untuk `failed`, `stopped`, dan `recovering`.

**Konten.** `FailureBannerContent.resolve(summary, related: [OperationSummary], instanceNames: (UUID) -> String?, now: Date) -> FailureBannerContent?` adalah fungsi murni (tanpa view) yang menerapkan *Operasi terkait*, lalu memilih state di *State banner*, judul, isi, baris blok kode, operation ID, `PortFix?` (`endpoint`, `suggestedPort`), dan aksi.

**View.** `InstanceFailureBanner` (gaya `.toneSurface(tone, on: .window, strength: .banner)` yang sama dengan `StateBanner`, karena perlu tombol utama, tautan, dan teks yang bisa dipilih) dengan mode `.summary` dan `.inspector`. `InstanceDetailBanner` memanggilnya untuk I1 dan I7 di Summary; `InstanceInspector` untuk I1 dan I7 di inspector. Tombol utama `.borderedProminent`, lainnya `.bordered`, gate `MutationAvailability` untuk Retry dan Use Port; deretan tombol pindah ke dua baris lewat `ViewThatFits` bila tidak muat, tanpa memotong label. Hitung mundur memakai `TimelineView(.periodic(from:by: 1))` dengan `fixtureNow` di fixture.

**Use Port.** Tombol membuka sheet Ports spec 0029 lewat `InstanceEditAction` yang sama dengan Edit di kartu Endpoints; halaman Instances mengisi action itu dan memasang sheetnya, jadi tab Summary dan inspector memakai jalur yang sama. Saran hanya ada untuk endpoint `Fixed`, karena endpoint `Auto` tidak pernah gagal `endpoint_in_use` (spec 0010 AC-17). `InstanceEdit.editPorts` mendapat nilai kedua `prefill: [String: UInt16]` (kosong untuk Edit dan Change Port…, identitas sheet tetap per instance), yang diteruskan ke `EndpointsModel(source:service:prefill:)`: draft endpoint itu langsung bernilai port saran dan verdict dry run berjalan tanpa jeda debounce. Prefill yang tidak cocok dengan endpoint IP mana pun diabaikan.

**Navigasi.** Show Logs memilih tab `.logs` di halaman detail yang sedang terbuka. "Show in Operations" memanggil `ShellNavigator.openOperation(id)` (spec 0035). Diagnose… memanggil `ShellNavigator.openDiagnostics(focus: .instance(id))` (spec 0034).

### State banner

**Pemetaan sebab.** `lastFailure.code` memakai `FailureCode` tanpa area; error operasi memakai kode katalog. Sebab dibaca dari error operasi terkait bila *Operasi terkait* memakainya, selain itu dari `lastFailure.code`:

| Sebab | `lastFailure.code` | Kode error operasi |
| --- | --- | --- |
| crash loop | `crash_loop` | `supervisor.retry_budget_exhausted` |
| port dipakai | `endpoint_in_use` | `supervisor.endpoint_in_use` |
| port instance lain | (tidak ada; hanya sebelum `journal`) | `supervisor.endpoint_conflict` |
| port tidak tersedia | (tidak ada; hanya sebelum `journal`) | `supervisor.endpoint_unavailable` |
| data dipakai | `data_in_use` | `supervisor.data_in_use` |
| lain | kode lain | kode lain |

**Aturan.** Aturan pertama yang cocok:

| No | Kondisi | Tone | State |
| --- | --- | --- | --- |
| F1 | lifecycle `recovering` | `caution` | restart (AC-12) |
| F2 | lifecycle `failed`, `initialized` false (start ditolak BR-008 sebelum init selesai, jadi lifecycle ini hanya datang dari init) | `critical` | init gagal |
| F3 | lifecycle `failed`, sebab crash loop | `critical` | crash loop (AC-9) |
| F4 | lifecycle `failed`, atau `stopped` dengan error dari *Operasi terkait*; sebab salah satu port | `critical` | port (AC-10) |
| F5 | kondisi lifecycle yang sama dengan F4; sebab data dipakai | `critical` | data dipakai |
| F6 | lifecycle `failed` | `critical` | lain (AC-11) |
| F7 | lifecycle `stopped` dengan error dari *Operasi terkait* | `critical` | start ditolak (AC-11) |

Instance `unknown`, retained, atau dengan blocker tetap memakai banner I2 sampai I4 dan tidak mendapat banner ini. Stop selama jeda timer actor membuat lifecycle `stopped` dengan `latest` stop yang `succeeded`, dan cancel operasi start selama `backoff` membuat `latest` start yang `cancelled`; keduanya tidak memberi banner, walau `lastFailure` tetap ada.

### Copy banner

| State | Judul | Isi |
| --- | --- | --- |
| restart | "Restarting after a crash" | "Restart {attempts} of {budget} in {s} s." atau "Stackly is starting it again." |
| init gagal | "Initialization failed" | `InstanceFailureCopy` |
| crash loop | "Crash loop. Automatic restarts paused." | "It crashed {attempts} times in {window}, so Stackly stopped restarting it and is waiting for you." saat `restart.attempts` > 0, selain itu (setelah daemon restart) "It kept crashing, so Stackly stopped restarting it and is waiting for you." |
| port, `in_use` | "Couldn’t start: port in use" | "Port {port} of {endpoint} is held by another process." |
| port, `conflict` | "Couldn’t start: port in use" | "Port {port} of {endpoint} belongs to {otherName}." |
| port, `unavailable` | "Couldn’t start: port unavailable" | "macOS refused port {port} of {endpoint}." |
| data dipakai | "Couldn’t start: data in use" | "Another process holds its data lock." |
| lain | "{name} failed" | `InstanceFailureCopy` |
| start ditolak | "Couldn’t start {name}" | `DaemonText.sentence(error.message)` |

- `{window}` dari `restart.windowSeconds`: "5 minutes" (menit dibulatkan ke atas, "1 minute" untuk satu). `{otherName}` dari `InstanceStore` untuk ID `otherInstance`, atau "another instance" bila instance itu tidak ada lagi. `{port}` dan `{endpoint}` dari details; tanpa details (sebab dari `lastFailure.code` saja), isi memakai `InstanceFailureCopy`. Isi crash loop memakai `restart.attempts` (6 di akhir crash loop), sedangkan `InstanceFailureCopy` menyebut budget restart ("after 5 restarts"); keduanya benar karena 6 crash berarti 5 respawn.
- Di bawah isi: hint daemon (`error.hint`) sebagai kalimat kedua bila ada, lalu "Its ports changed. Retry to start with them." menurut *Operasi terkait* (AC-10).
- Blok kode: baris pertama "{code} · operation {uuid}" (atau hanya kode, `error.code` lebih dulu, lalu `lastFailure.code`; tanpa operasi tidak ada bagian operation), lalu excerpt per baris (`InstanceInspector.excerpt`). Inspector hanya menampilkan excerpt.
- Tombol Summary: F3 dan F6: Retry, Show Logs, Diagnose…; F4: Use Port {n} atau Change Port…, Show Logs, Retry, Diagnose…; F5 dan F7: Retry, Show Logs, Diagnose…; F2: Show Logs, Diagnose… (Retry nonaktif seperti spec 0022); F1: Show Logs. Tautan "Show in Operations" di samping blok kode saat ada operation ID. Inspector: hanya Use Port {n} untuk F4.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| Stage backoff | `"backoff"` | `StageName::BACKOFF`, diset `wait_to_respawn` setelah `show_pause` |
| `lastFailure.at` | waktu kegagalan | `now` commit yang menulis kegagalan, `InstanceStatus.last_failure` |
| `lastFailure.exitCode`, `signal` | exit terakhir | `ExitSeen` atau status exit yang diputuskan commit itu |
| `suggestedPort` | port saran | `suggest_for_start` atas daftar instance lain (tahap `endpoints`, atau satu bacaan `Registry` di exit dini) dan `PortProbe` |
| Hint daemon | teks | *Hint statis*, `codes::supervisor::hint` |
| Correlation ID | operation ID | `failure.error.operationId` (= `OperationSummary.id`) |
| Judul, isi banner | teks | *Copy banner*, dari sebab (*Pemetaan sebab*) |
| `{budget}`, `{window}` | angka | `restart.budget`, `restart.windowSeconds` |
| `{attempts}`, `{s}` | angka | `restart.attempts`; `restart.nextAttemptAt` dikurangi `fixtureNow ?? Date()`, dibulatkan ke atas, minimal 0 |
| `{port}`, `{endpoint}`, `{n}` | port | details `port`, `endpoint`, `suggestedPort` |
| `{otherName}` | nama | `InstanceStore` untuk `otherInstance` |
| Operasi terkait | `OperationSummary` | *Operasi terkait* atas `operation.list { resource, limit: 10 }`, plus `OperationCenter.active` dan `recent` |
| Kalimat "Its ports changed" | teks | `change_endpoints` `succeeded` di *Operasi terkait* |
| Nama di perintah CLI | nama | `InstanceSummary.name` dari target yang di resolve |
| `Last operation` CLI | summary | *Operasi terkait* atas `operation.list { resource, limit: 10 }` |
| Baris backoff `--wait` | angka, waktu | `instance.get` (`restart.attempts`, `budget`, `nextAttemptAt`) sekali per jeda; waktu lewat helper waktu `inspect` |

### Key invariants

- Tidak ada jalur yang menulis port, revision config, data, lock, atau file folder run sebagai efek sebuah kegagalan atau saran; satu satunya penulis port tetap `instance.change_endpoints` yang kamu konfirmasi.
- Budget, jeda, dan jitter tidak berubah; tidak ada spawn baru karena fitur ini (stage dan fakta hanya membaca dan mencatat).
- Banner tidak pernah menampilkan sebab yang tidak didukung data: tanpa operasi dan tanpa `lastFailure`, isi jatuh ke "It failed." spec 0022.
- Teks yang tampil adalah teks tetap, details katalog, dan excerpt yang sudah disamarkan daemon; GUI dan CLI tidak membuat kalimat dari output proses.

### Security model

Satu user lokal lewat socket privat (spec 0006). Excerpt dan log sudah disamarkan sebelum keluar daemon (spec 0010, 0027). Hint dan judul adalah teks tetap tanpa nilai. Operation ID bukan rahasia. Saran port tidak membuka port apa pun di luar bind percobaan yang langsung ditutup (spec 0010).

### Configuration required

Tidak ada env var atau setting baru. Konstanta restart tetap di spec 0010.

### Critical test scenarios

- Crash loop di daemon (`apps/daemon/tests/supervisor/crash_loop.rs`, `DaemonConfig` dengan `restart_base_delay` 20 ms): fake `crashAfterMs` 50 dan `crashExitCode` 3; 6 penanda `started`, urutan `exited` sebelum `started`, jarak minimum, `failed`, `crash_loop`, `exhausted`, operasi `retry_budget_exhausted` dengan `operationId`, `lastFailure.at` terisi dan `exitCode` 3, stage `backoff` teramati di event `operation.progress` di antara percobaan, dan setiap kali stage itu teramati `instance.get` memberi `nextAttemptAt` terisi dan `attempts` = nomor respawn; baris log JSON daemon untuk kegagalan operasi membawa operation ID yang sama. Verifies **AC-1**, **AC-2**, **AC-3**, **AC-5**.
- Crash setelah ready: fake `readyDelayMs` 0 dan `crashAfterMs` di atas waktu ready; 6 spawn, `failed`, `crash_loop`, `exhausted`; jeda pertama tanpa operasi dan tanpa stage, setiap respawn adalah operasi `start_instance` sistem (pemicu `restart_policy`) yang `succeeded`, dan crash keenam tanpa operasi yang gagal. Verifies **AC-1**, **AC-2**.
- Stop dan cancel selama jeda: stop di jeda timer actor membuat `stopped` tanpa respawn dan `lastFailure` tetap; cancel operasi start di stage `backoff` membuat operasi `cancelled`. Verifies **AC-2**, **AC-14**.
- Daemon restart setelah crash loop: `lastFailure.code` tetap, `at` dan `exitCode` null. Verifies **AC-3**.
- Port: listener luar di port fixed (`allocate_port`), start gagal di tahap `endpoints` dengan `suggestedPort` = port bebas berikutnya; instance lain memegang port, `endpoint_conflict` dengan saran; tanpa kandidat (probe tiruan yang selalu `InUse`) tanpa detail; exit dini dengan `endpoint_in_use` setelah spawn membawa saran; baris registry, isi folder data, `instance.lock`, file folder run, dan revision sama sebelum dan sesudah. Verifies **AC-4**, **AC-14**.
- Hint: setiap kode di *Hint statis* membawa hintnya, `data_incompatible` tidak berubah. Verifies **AC-5**.
- Secret: fake dengan setting `leakToken` mencetak credential fixture ke stdout dan stderr lalu crash; credential tidak ada di excerpt, error, output CLI, dan teks banner fixture. Verifies **AC-15**.
- CLI golden: `instance_start_crash_loop.txt`, `instance_start_port_in_use.txt` (dengan saran dan tanpa), `instance_inspect_crash_loop.txt` dan `.json` dengan `lastOperation` dan `Next steps:`, progress `--wait` di stage `backoff` dari daemon skrip; baris `operation:` di golden error yang ada. Verifies **AC-6**, **AC-7**, **AC-8**.
- GUI model: test `FailureBannerContent.resolve` untuk setiap baris *State banner*, *Pemetaan sebab*, dan *Copy banner*, urutan aturan, *Operasi terkait* (rename lebih baru dilewati, stop lebih baru menyembunyikan F7, `change_endpoints` sukses, operasi yang lebih tua dari `lastFailure.at`, `at` null setelah daemon restart), banner yang naik dari isi `summary` ke isi lengkap tanpa judul berubah, bacaan gagal dan offline, kode tidak dikenal; `InstanceFailureModel` dengan `ScriptedDaemon`; `EndpointsModel` dengan prefill dan prefill yang ditolak verdict. Retry: request `instance.start` dengan key baru setiap klik, nonaktif dengan alasan spec 0022 saat daemon offline atau `initialized` false, label "Starting…", kartu gagal spec 0067. Navigasi: Show Logs memilih tab `.logs`, Show in Operations dan Diagnose… memanggil navigator dengan ID itu (dan jatuh ke `.operations` dan `.diagnostics` di build provisional). Verifies **AC-9** sampai **AC-13**, **AC-16**.
- Snapshot: Summary dan inspector untuk F1 sampai F7, Use Port dan Change Port, setelah ganti port, tanpa operasi, lebar sempit. Verifies **AC-17**, **AC-9** sampai **AC-12**.
- Native: PostgreSQL 18 dengan port fixed yang dipegang listener luar, start gagal dengan saran dan registry tidak berubah; evidence run crash loop fake dengan konstanta asli (31 detik backoff, budget nyata) di bawah `cargo xtask evidence run`. Verifies **AC-1**, **AC-4**, **AC-14**.

### Usulan update untuk spec lain

Diterapkan lead lewat `/sync` atau pemilik spec; spec ini tidak mengeditnya.

- **Spec 0010**: tabel *Startup transaction* menyebut stage `backoff` di antara percobaan; *Model data* status menyebut `InstanceStatus.last_failure`; tabel *Kode error* menyebut `suggestedPort` di ketiga kode endpoint dari start.
- **Spec 0021**: *Tab Summary* butir Banner menyebut bahwa I1 dan I7 memakai `InstanceFailureBanner` spec 0033.
- **Spec 0022**: *Inspector* butir banner menyebut view ringkas spec 0033 untuk I1 dan I7. Ditemukan saat menulis spec ini: isi I7 "Restart {attempts + 1} of {budget}" meleset satu, karena `restart.attempts` sudah menghitung crash yang memicu jeda (`RestartHistory::record_crash` di `crates/core/src/supervisor/restart.rs`); respawn pertama tampil sebagai "Restart 2 of 5" dan respawn kelima jatuh ke teks umum. Usulannya "{attempts}" dan syarat `attempts` ≤ `budget`; diterima lead 2026-11-10, dikerjakan langkah 4 spec ini di `AttentionRules`, dan teks spec 0022 diselaraskan lead lewat `/sync`.
- **Spec 0029**: `InstanceEdit.editPorts` dan `EndpointsModel` menerima `prefill` dari banner spec 0033.
- **Spec 0008**: bentuk JSON `stackly instance inspect` mendapat field `lastOperation`, dan blok error teks mendapat baris `operation:`.

### Selisih mockup yang disengaja

- Kode error memakai kode wire (`supervisor.endpoint_in_use`), bukan `E_PORT_IN_USE`; isi memakai kalimat dari *Copy banner*, bukan teks bebas.
- Ada tombol Diagnose… dan tautan Show in Operations yang tidak ada di mockup, karena scope meminta tautan ke doctor dan correlation ID.
- Banner restart (F1) dan banner start ditolak saat lifecycle `stopped` (F7) tidak ada di mockup; mockup menandai instance yang ditolak port sebagai Failed, sedangkan spec 0010 membiarkannya `stopped`.

## Build plan

Pendekatan Journey: jalur "start gagal, lihat sebabnya, perbaiki, coba lagi" dibangun utuh dari daemon sampai banner, termasuk semua state, sebelum fitur berikutnya. Urutan mengikuti arah dependency (core dan daemon, CLI, GUI), lalu bukti.

**Milestone 1: daemon terlihat** (needs fitur 10, 27, 29 di `main`)

1. Domain dan core: `StageName::BACKOFF` di `wait_to_respawn` setelah `show_pause`, `FailureFacts` di `ActorState` dan `InstanceStatus` yang ditimpa setiap commit kegagalan, `suggest_for_start` dan `suggested_port` di error endpoint (tahap `endpoints` dan exit dini dengan satu bacaan `Registry`), hint statis di `codes::supervisor::hint`; unit test core dengan `FakeProcessSupervision`, `PortProbe` tiruan, dan waktu dijeda. Satisfies **AC-2**, **AC-3**, **AC-4**, **AC-5**.
2. Daemon: `dto::summary` memakai fakta kegagalan; fixture di `protocols/fixtures/` dan `cargo xtask codegen --check` bersih; integration test `apps/daemon/tests/supervisor/crash_loop.rs` dan `ports.rs` (crash loop dua jalur, daemon restart, port, tanpa perbaikan otomatis, secret). Satisfies **AC-1** sampai **AC-5**, **AC-14**, **AC-15**.

**Milestone 2: CLI** (after milestone 1)

3. `StderrSink::error` dengan baris `operation:`, `next_steps.rs`, aturan *Operasi terkait* di CLI, hint di `start` dan `restart`, `Last operation` dan `Next steps:` di `inspect` dan `get` serta `lastOperation` JSON, baris backoff di `--wait`; golden baru dan golden error yang diperbarui. Satisfies **AC-6**, **AC-7**, **AC-8**, **AC-15**, **AC-16** (hint doctor provisional saat itu; final sejak F34 langkah 11).

**Milestone 3: model GUI** (after milestone 1; bisa paralel dengan milestone 2, file berbeda)

4. `FailureBannerContent` dengan *Operasi terkait* dan *Pemetaan sebab*, `InstanceFailureModel` (bacaan, event, offline), copy, prefill `InstanceEdit.editPorts` dan `EndpointsModel`, isi I7 `AttentionRules` menjadi "Restart {attempts} of {budget}" dengan syarat `attempts` ≤ `budget` (usulan spec 0022 yang diterima), dan kasus fixture spec 0068 untuk F1 sampai F7 (perluas `FixtureStartConflict`); unit test model. Satisfies **AC-9** sampai **AC-13**, **AC-16**.

**Milestone 4: banner GUI** (after milestone 3; needs fitur 34 dan 35 hanya untuk tautan final)

5. `InstanceFailureBanner` mode Summary dan inspector, pemasangan di `InstanceDetailBanner` dan `InstanceInspector`, Use Port ke sheet Ports, Show Logs, Show in Operations, Diagnose… (provisional sampai spec 0034 dan 0035 di `main`; kini final: `openOperation(id)` dan `openDiagnostics(focus: .instance(id))`), snapshot di `InstanceDetailSnapshotTests` dan `InstancesSnapshotTests`. Satisfies **AC-9** sampai **AC-13**, **AC-16**, **AC-17**.

**Milestone 5: bukti** (after milestone 2 dan 4)

6. Test native PostgreSQL 18 port dipakai, dan evidence run crash loop fake dengan konstanta asli (`BLOCKED` tanpa artifact runtime). Satisfies **AC-1**, **AC-4**, **AC-14**.
7. **Manual (butuh mata dan tangan kamu)**: satu sesi GUI di app Debug di atas daemon nyata: buat fake yang terus crash, lihat banner restart menghitung mundur lalu banner crash loop, tekan Show Logs dan Retry; pegang port fixed PostgreSQL dengan `nc -l`, lihat banner port, tekan Use Port, konfirmasi di sheet, lalu Retry; bandingkan dengan `stackly instance inspect` untuk instance yang sama. Satisfies **AC-9**, **AC-10**, **AC-12**, **AC-13**.

## Consequences

**Positive**:

- Tidak ada perubahan kontrak, jadi client lama tetap jalan dan F34 serta F35 bisa merge dalam urutan apa pun.
- Operasi menjadi satu sumber kebenaran untuk error start, sehingga GUI, CLI, Operations, dan doctor melihat kode dan ID yang sama.
- Kegagalan sebelum `journal` (port fixed yang dipakai proses luar, kasus paling umum) kini terlihat di halaman instance, bukan hanya di kartu umpan balik yang hilang.
- Saran port selalu melewati verdict dry run dan konfirmasi spec 0029, jadi saran yang basi tidak pernah diterapkan.

**Negative / tradeoffs**:

- Crash loop yang crash keenamnya diputuskan actor setelah proses sempat ready tidak punya operasi yang gagal, jadi tidak punya operation ID; correlation nya hanya ID instance di log daemon.
- Fakta kegagalan hanya di memori, jadi setelah daemon restart waktu dan exit code hilang dan banner jatuh ke teks tanpa detail.
- Banner bergantung pada satu `operation.list` tambahan per instance yang dibuka, dan aturan *Operasi terkait* harus dijaga sama di GUI dan CLI; saat daemon offline sebelum bacaan pertama berhasil, banner F7 tidak ada dan banner lain tampil tanpa operation ID dan tanpa Use Port; setelah satu bacaan berhasil, hasil itu tetap dipakai selama offline.
- Untuk sesaat sesudah kegagalan, banner bisa tampil dari `summary` saja lalu mendapat kode, operation ID, dan Use Port begitu event operasi tiba.
- `InstanceFailureBanner` adalah view baru di samping `StateBanner`, sehingga ada dua tempat yang memakai gaya banner yang sama.
- Baris `operation:` baru dan field `lastOperation` mengubah golden CLI yang sudah ada.

**Neutral**:

- Tidak ada migrasi data. Instance yang sudah gagal sebelum versi ini tampil dengan banner tanpa waktu dan exit code sampai kegagalan berikutnya.
- Stage `backoff` muncul di Operations (spec 0035) dan `stackly operation inspect` sebagai stage biasa.

## Follow-up

- [x] Lead: bawa *Usulan update untuk spec lain* ke spec 0010, 0021, 0022, dan 0029 lewat `/sync` setelah build. Diterapkan 2026-11-10 (koordinasi scope 05, `arc-sync`) di spec 0010, 0021, 0022, dan 0029, juga usulan untuk spec 0008.
- [ ] Lead: catat *Selisih mockup yang disengaja* di `ui-macos.md` lewat `/scope`. *(2026-11-10) Masih terbuka: `docs/scope/` di luar `docs/specs/`; teks usulannya ada di laporan `arc-sync` ke lead.*
- [x] Milestone GUI fitur ini (langkah 4): isi I7 Needs Attention menjadi "Restart {attempts} of {budget} at {time}." dengan syarat `attempts` ≤ `budget` (diterima lead 2026-11-10); teks spec 0022 diselaraskan lead lewat `/sync`. Dibangun di `AttentionRules` (langkah 4) dan teks tabel I7 spec 0022 diselaraskan 2026-11-10.
- [x] Fitur 34 dan 35: setelah keduanya di `main`, hapus `// provisional: F34` dan `// provisional: F35` (Diagnose… dengan fokus, hint `stackly doctor`, `openOperation`). Selesai: tidak ada lagi `// provisional: F34` atau `F35` di kode (`DOCTOR_COMMAND` dihapus F34 langkah 11; Diagnose… memanggil `openDiagnostics(focus: .instance(id))` dan Show in Operations memanggil `openOperation(id)`), dicek 2026-11-10.
- [ ] `/sync` setelah build: catat stage `backoff`, `InstanceFailureBanner`, `next_steps.rs`, dan aturan "saran port hanya lewat sheet Ports" di root `AGENTS.md` dan `apps/macos/AGENTS.md`. *(2026-11-10) Masih terbuka: `AGENTS.md` di luar `docs/specs/` (milik `/sync` lead).*

## Rationale

Pilihan dan alasannya: lihat [rationale.md](rationale.md).
