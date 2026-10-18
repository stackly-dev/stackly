# 0007. Operation engine

**Date**: 2026-10-18
**Status**: Proposed

## Summary

Spec ini menetapkan mesin operasi yang dipakai setiap mutasi Stackly. Setiap perintah yang mengubah sesuatu (install, create, start, stop, backup, dan lainnya) menjadi satu operasi yang langsung dibalas dengan ID nya dalam waktu kurang dari 500 ms, lalu dikerjakan daemon di latar dengan tujuh state PRD, lock per resource yang hanya ada di memori (bukan mutex SQLite), antrian FIFO berbatas (8 berjalan, 64 antri), dan journal langkah di registry. Idempotency key wajib untuk setiap mutasi, sehingga GUI dan CLI yang mengirim perintah yang sama dengan key yang sama hanya menghasilkan satu efek samping. Cancel, kompensasi (membersihkan apa yang dibuat operasi itu sendiri dan tidak pernah yang lain, walau lewat symlink), resume setelah daemon terputus, serta aksi pemulihan resume dan abandon berjalan lewat satu API engine yang dipakai fitur 10, 18, 20, 49, 51, dan seterusnya.

## Requirements

**User stories**:

- Sebagai pengguna yang memakai GUI dan CLI bersamaan, saya ingin perintah yang sama tidak pernah menghasilkan runtime atau instance ganda.
- Sebagai pengguna, saya ingin menutup GUI atau memutus CLI tanpa membatalkan pekerjaan yang sedang berjalan, lalu melihat progress nya lagi dari client mana pun.
- Sebagai pengguna yang membatalkan operasi, saya ingin tahu persis hasilnya: dibatalkan bersih, atau terlambat karena sudah lewat langkah yang tidak bisa dibalik.
- Sebagai pengguna yang Mac nya mati di tengah operasi, saya ingin operasi yang aman diulang lanjut sendiri, dan yang tidak aman berhenti dengan jelas tanpa menghapus data milik saya.
- Sebagai developer fitur 10, 18, 20, 49, dan 51, saya ingin satu API untuk memulai operasi, menandai langkah, melapor progress, dan mencatat apa yang saya buat, supaya cancel, kompensasi, dan resume tidak saya tulis ulang di setiap fitur.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-048):

- **AC-1**: `OperationEngine::begin` dengan request valid menulis dalam satu batch registry baris `operations` (state `queued`), `operation_journal`, dan `operation_locks`, menerbitkan `operation.state_changed` setelah commit, lalu mengembalikan `BeginOutcome::Created` berisi ringkasan operasi. Dari 100 panggilan `begin` berurutan di `SqliteRegistry` di `TestRoot` di Mac ini, persentil 99 waktu dari panggilan sampai kembali paling lama 500 ms, dan jalur IPC `start_operation` menambah kurang dari 50 ms di atasnya (NFR-006).
- **AC-2**: 16 task yang memanggil `begin` bersamaan dengan idempotency key dan sidik jari yang sama menghasilkan tepat satu baris operasi, pekerjaan berjalan tepat sekali (penghitung efek samping bernilai 1), satu task menerima `Created`, 15 menerima `Replayed`, dan semuanya membawa `operationId` yang sama. Key yang sama setelah operasinya berstate akhir mengembalikan operasi itu sebagai `Replayed` tanpa menjalankan apa pun, sampai prune spec 0005 menghapus barisnya; setelah itu key yang sama membuat operasi baru (REQ-048, AC-048, NFR-005).
- **AC-3**: Request IPC ke method yang memulai operasi tanpa `idempotencyKey` gagal dengan `operation.idempotency_key_required` tanpa menulis baris apa pun. Key yang sama dengan sidik jari berbeda (method lain atau params lain) gagal dengan `operation.idempotency_key_reused` yang membawa `operationId` operasi pertama. Dua request yang hanya berbeda di nilai `SecretText` menghasilkan sidik jari yang sama. Setelah `begin` dengan params berisi `FixtureCredential`, pindaian byte file registry dan log daemon tidak menemukan nilai sentinel. Operasi latar dari daemon (origin `background`) tidak membawa key.
- **AC-4**: Lock key sebuah operasi diambil sekaligus atau tidak sama sekali. Operasi yang key nya dipegang menunggu berstate `queued` dengan `waitingOn` berisi penghalang tertua menurut `(created_at, id)`; operasi yang berbagi key berjalan urut `(created_at, id)` tanpa saling mendahului, dan operasi dengan key yang tidak bersinggungan berjalan paralel. `BusyPolicy::Reject` pada key yang dipegang atau ditunggu menghasilkan `resource.busy` dengan `operationId` pemegangnya (tanpa detail itu kalau pemegangnya internal) tanpa menulis baris. Key yang dipegang operasi `needs_recovery` menghasilkan `operation.recovery_required` dengan `operationId` operasi itu, dan operasi antri yang sudah menunggu key itu saat pemegangnya masuk `needs_recovery` ditutup `failed` dengan error yang sama. `LockTable::holder` melaporkan pemegang key, dan `try_lock_internal` gagal seketika saat key dipegang, serta menahan operasi antri selama dipegang (REQ-048).
- **AC-5**: Paling banyak 8 operasi berjalan; yang kesembilan menunggu `queued`. Operasi antri ke 65 ditolak dengan `operation.queue_full` (`max` 64) tanpa menulis baris. Menunggu lock tidak memakai slot berjalan. `operation.get_stats` melaporkan jumlah berjalan, antri, dan `needs_recovery`, kedua batas, penghitung penolakan antrian penuh, serta penghitung pemulihan saat start, dan subsystem `operations` di `daemon.get_health` bernilai `ok`, atau `degraded` dengan detail jumlah operasi `needs_recovery` (NFR-004).
- **AC-6**: Saat task request yang memanggil `start_operation` dibatalkan tepat setelah `begin` berhasil (simulasi client putus), operasi tetap berjalan sampai `succeeded`, event tetap terbit, dan tidak ada cancel yang tercatat. Task request yang dibatalkan di titik mana pun selama `begin`, `cancel`, `resume`, atau `abandon` tidak pernah meninggalkan baris `queued` yang tidak dijalankan atau kompensasi yang setengah jalan, karena bagian kritisnya berjalan di task milik engine (PRD bagian 18.2).
- **AC-7**: Cancel: (a) operasi `queued` yang belum pernah mulai langsung menjadi `cancelled` dengan disposisi `cancelled_before_start` dan pekerjaannya tidak pernah berjalan; (b) operasi berjalan sebelum titik tak bisa dibalik mendapat disposisi `requested`, berhenti di checkpoint berikutnya, menjalankan kompensasi urut terbalik, lalu `cancelled`; (c) operasi di atau setelah titik tak bisa dibalik mendapat disposisi `continuing_past_point_of_no_return`, menyelesaikan langkah sisanya, lalu berakhir `succeeded` dengan `cancelRequestedAt` tetap tercatat, atau `needs_recovery` kalau langkah berikutnya gagal, dan tidak pernah `cancelled` saat efek yang tidak bisa dibalik sudah terjadi; (d) operasi berstate akhir mendapat `already_final` tanpa perubahan; (e) operasi `needs_recovery` mendapat `recovery_required` tanpa perubahan; (f) operasi yang sudah `cancel_requested` mendapat lagi `requested` atau `continuing_past_point_of_no_return` tanpa commit baru; (g) operasi `queued` hasil resume yang sudah melewati titik tak bisa dibalik mendapat `continuing_past_point_of_no_return` dan pekerjaannya dilanjutkan, tidak dikompensasi. Cancel yang datang setelah batch akhir ter commit mendapat `already_final`, karena cancel, titik tak bisa dibalik, dan finalize berjalan di bawah satu mutex penulis per operasi. Skenario fixture restore palsu (langkah staging lalu langkah commit yang tidak bisa dibalik): cancel saat staging membuat file sentinel target tetap identik dan staging hilang; cancel setelah commit mulai menunjukkan titik tak bisa dibalik di status dan tidak pernah melaporkan sukses palsu (REQ-054, AC-054).
- **AC-8**: Kompensasi hanya menghapus folder scratch operasi itu, pohon instance atau environment yang ID nya dibuat atau diklaim operasi itu (dihitung ulang dari `DirectoryLayout`, tidak pernah dari path tersimpan), reference secret turunan ID milik operasi itu yang belum ada sebelum operasi mencatat niatnya, dan item kustom lewat kompensator terdaftar yang membuktikan kepemilikannya. Symlink yang ditanam di dalam pohon instance milik operasi dan menunjuk ke folder sentinel di luar owned root terhapus sebagai symlink, dan isi folder sentinel tetap identik. Root instance yang manifestnya menyebut `createdByOperation` operasi lain dibiarkan, dicatat `not_owned`, dan dilaporkan. Folder run `i/<short-key>` dilewati kalau instance lain yang dikenal registry memakai short key yang sama. Catatan niat yang pembuatannya gagal karena sudah ada (`AlreadyExists`, `ShortKeyTaken`) langsung menjadi `not_owned`. Kompensasi yang dijalankan dua kali memberi hasil yang sama, path yang sudah hilang dihitung `compensated`, kompensasi berlanjut ke semua catatan walau satu gagal, dan folder scratch disimpan kalau ada catatan yang gagal. Kepemilikan dan path diperiksa ulang tepat sebelum setiap hapus (BR-023, VAL-007, VAL-015).
- **AC-9**: `Filesystem::remove_tree(root, relative)` di `FakeFilesystem` dan `MacosFilesystem` (lewat `fs_conformance`) tidak pernah mengikuti symlink di path target, di komponen perantara, atau di dalam pohon (entri symlink dihapus sebagai dirinya sendiri). Symlink atau bukan folder di komponen perantara menghasilkan error tanpa menghapus apa pun. Entri di device lain menghentikan hapus dengan `CrossesDevice` (di macOS diuji dengan disk image yang di attach lewat `hdiutil attach -mountpoint` di dalam `TestRoot`; kalau attach gagal, test itu dicatat `BLOCKED`), dan kedalaman lebih dari 128 menghasilkan `TooDeep`. Test balapan di macOS: satu thread menukar folder perantara dengan symlink ke folder sentinel di luar root secara terus menerus selama 2 detik sementara `remove_tree` berulang kali menghapus pohon baru; semua file sentinel tetap identik (VAL-007).
- **AC-10**: Saat daemon start (slot start, setelah registry dibuka dan sebelum rekonsiliasi supervisor): operasi `running` dari jenis resumable yang langkah terputusnya (kalau ada) `repeatable` menjadi `queued` lalu dilanjutkan setelah daemon siap, melompati langkah `done` dan mengulang langkah terputus dengan `attempts` bertambah 1; langkah yang tidak `repeatable` tidak pernah diulang. Operasi yang tidak bisa dilanjutkan dan belum melewati titik tak bisa dibalik menjadi `needs_recovery` dengan hint `auto_compensate`, lalu setelah daemon siap dikompensasi dan ditutup `failed` dengan alasan `interrupted` (atau `cancelled` kalau cancel sudah diminta). Yang sudah melewati titik itu menjadi `needs_recovery` dengan hint `resume` atau `manual` sesuai aturan hint. Operasi yang `compensation_started_at` nya terisi atau punya langkah `failed` tidak pernah dilanjutkan, melainkan dikompensasi (sebelum titik tak bisa dibalik) atau `needs_recovery`. Operasi `queued` yang belum pernah mulai dibangun ulang lewat `WorkFactory` jenisnya dari `input` dan dijalankan setelah siap, urut waktu dibuat, dengan `expected_revision` dari journal diperiksa ulang; kalau factory tidak ada atau `rebuild` menolak (misalnya params nya memuat secret), operasi itu ditutup `failed` dengan alasan `interrupted` tanpa kompensasi. Lock dibangun ulang dari `operation_locks` sebelum rekonsiliasi, dan operasi antri yang menunggu key milik operasi `needs_recovery` ditutup seperti di AC-4 (REQ-054). Pengecualian jenis `CloseInterrupted` (start, stop, restart, dan reload instance): ditutup `failed` alasan `interrupted` (atau `cancelled` kalau cancel sudah diminta) tanpa kompensasi dan tanpa dilanjutkan, sebelum rekonsiliasi supervisor (spec 0010).
- **AC-11**: `operation.resume` pada operasi `needs_recovery` yang aksinya mencakup `resume` mengembalikannya ke `queued` (`expectedRevision` opsional; tidak cocok menghasilkan `resource.revision_mismatch`); selain itu `operation.not_resumable` dengan `reason`. `operation.abandon` wajib membawa `expectedRevision`, mengompensasi item dari langkah yang bisa dibalik (dan folder scratch kalau titik tak bisa dibalik belum dilewati), melepas lock, menutup operasi `failed` dengan alasan `abandoned`, dan membalas daftar sisa yang tidak dibalik, yang juga muncul sebagai `result.partial` (`compensated` menjadi `done`, `compensation_failed` menjadi `failed`, `not_owned` menjadi `skipped`, dan catatan yang masih `created` atau `intended` menjadi `left_over`). Abandon pada state selain `needs_recovery` menghasilkan `operation.not_abandonable` dengan `reason` `wrong_state`, dan selama kompensasi otomatis sedang berjalan dengan `reason` `auto_compensating`.
- **AC-12**: Registry tidak pernah memegang transaksi selama langkah menunggu. Saat sebuah langkah tertahan 5 detik (simulasi download atau startup proses), task lain menjalankan 50 commit dan 50 baca di `SqliteRegistry`, dan setiap panggilan selesai di bawah 50 ms (REQ-048).
- **AC-13**: Perubahan yang hanya menyangkut progress di commit paling banyak sekali per detik per operasi, dengan nilai terakhir yang menang (commit pertama seketika, lalu satu commit penutup di akhir jendela): 100 pembaruan dalam satu detik (jam tokio dijeda) menghasilkan paling banyak 2 commit, dan nilai terakhir tercatat dalam 1 detik. Perubahan stage dan state di commit seketika. Setiap commit yang mengubah tampilan menerbitkan `operation.progress` atau `operation.state_changed` berisi `OperationSummary` lengkap dengan `revision` sama dengan revision yang di commit, dan revision itu selalu naik. Progress `counted` hanya muncul kalau pekerjaan memberi `done` dan `total` (REQ-054).
- **AC-14**: Shutdown dengan grace 10 detik: tidak ada operasi baru yang dimulai; operasi resumable yang `running` berhenti di checkpoint atau batas langkah berikutnya dan di commit `queued`, lalu dilanjutkan pada start berikutnya; operasi `cancel_requested` menyelesaikan kompensasinya dalam grace; operasi tidak resumable yang belum melewati titik tak bisa dibalik dikompensasi lalu `failed` dengan alasan `daemon_stopping` (retryable); yang sudah melewatinya diberi sisa grace, dan kalau belum selesai, start berikutnya mencatatnya terputus. `shutdown` tidak pernah lebih lama dari grace ditambah 1 detik. Operasi berjenis `CloseInterrupted` tidak dihentikan, tidak dikompensasi, dan tidak ditunggu shutdown; ia ditutup `recover` pada start berikutnya.
- **AC-15**: `begin` dengan `expected_revision` memeriksanya saat dibuat (tidak cocok menghasilkan `resource.revision_mismatch` tanpa baris), menyimpannya di journal, dan memeriksanya sekali lagi setelah lock diambil dan sebelum commit `start`, selama `started_at` masih kosong (juga setelah daemon restart); kalau resource sudah berubah selama antri, operasi `failed` langsung dari `queued` dengan alasan `precondition_failed` dan error `resource.revision_mismatch` (`expected`, `actual`) tanpa efek samping. `creates_resource` dengan `expected_revision` terisi ditolak sebagai bug pemanggil (VAL-015).
- **AC-16**: Pekerjaan yang panic membuat operasi `failed` dengan error `internal.unexpected` setelah kompensasi, dan daemon tetap berjalan. Pekerjaan yang selesai sukses dengan catatan dibuat yang masih `intended` adalah bug: operasi dikompensasi lalu `failed` dengan `internal.unexpected`. Commit `start` yang gagal melepas lock dan slot, operasi tetap `queued`, dan dispatcher mencoba lagi setiap 5 detik. Tulisan journal yang gagal (`fail_next_commit(NoSpace)`) menghentikan pekerjaan di checkpoint berikutnya, menjalankan kompensasi, lalu mencoba commit state akhir setiap 5 detik sampai berhasil. DTO operasi `failed` membawa `failure.error` berupa `ErrorBody` dengan `operationId`, kode dari katalog, dan kategori dari katalog; test memastikan setiap `ErrorCodeName` yang bisa dihasilkan modul operasi ada di katalog. Payload journal di atas batas ditolak `operation.payload_too_large` tanpa baris.
- **AC-17**: Migrasi `NNNN_operation_journal.sql` (versi bebas berikutnya saat build) membuat empat tabel journal, mengosongkan `idempotency_key` pada baris yang key nya dipakai baris lain yang lebih baru, mengganti index `idempotency_key` menjadi unique index parsial, dan mengisi satu baris `operation_journal` untuk setiap operasi yang sudah ada (origin `background`, `input` `{}`, `resumable` dari jenisnya). Upsert langkah dan catatan memakai `INSERT ... ON CONFLICT DO UPDATE`, tidak pernah `INSERT OR REPLACE`. Migrasi maju dari fixture versi sebelumnya mempertahankan data, fixture beku `registry-v<N>.sql` ditambahkan, dan `registry_conformance` mencakup `WriteOp` dan baca baru di `FakeRegistry` dan `SqliteRegistry` (termasuk urutan cek dengan `IdempotencyKeyTaken`). Prune operasi berstate akhir ikut menghapus baris journal nya lewat cascade, dan operasi `needs_recovery` tidak pernah di prune.
- **AC-18**: DTO, method `operation.get`, `operation.list`, `operation.cancel`, `operation.resume`, `operation.abandon`, `operation.get_stats`, event `operation.state_changed` dan `operation.progress`, reply `OperationAccepted`, dan kode di `codes::operation` punya fixture di `protocols/fixtures/ipc/` (termasuk nilai asing untuk enum terbuka dan setiap varian `Reply` baru), test kontrak Swift mendecode semuanya, `cargo xtask codegen --check` bersih, dan minor protokol naik.
- **AC-19**: Setiap baris log yang dibuat di dalam task sebuah operasi membawa `operation_id` dan `kind`; transisi state dicatat di level info; baris log request untuk method yang memulai operasi membawa `operation_id`; params, payload journal, dan nilai secret tidak pernah tercatat (NFR-011).
- **AC-20**: `operation.list` menerima `limit` 1 sampai 200 (default 50), `activeOnly`, `resource`, dan cursor opaque `before`, mengurutkan `(created_at, id)` turun, membaca journal halaman itu dalam satu baca batch, dan berhenti mengisi halaman sebelum JSON nya melewati 900 KiB, dengan `next` menunjuk baris terakhir yang masuk, sehingga tidak pernah melewati batas frame. `operation.get` mengembalikan detail dengan langkah dan item yang dibuat; ID yang tidak ada menghasilkan `resource.not_found`.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): engine operasi di `stackly-core::operation` dengan lock per resource di memori dan antrian FIFO berbatas, idempotency key wajib yang terikat selama baris operasi ada dan ditolak kalau isinya berbeda, journal di empat tabel baru yang ditulis niat dulu, langkah dengan flag `repeatable` dan `reversible`, kompensasi hanya atas apa yang dimiliki operasi karena konstruksi, primitive hapus `remove_tree` berbasis file descriptor, dan pemulihan per jenis operasi dengan resume dan abandon.

Mesin operasi adalah satu satunya jalan untuk mengubah resource: semua mutasi masuk sebagai operasi yang dicatat dulu di registry, dijalankan di bawah lock di memori, dan ditutup dengan satu commit akhir yang atomik, sehingga crash, cancel, dan client yang putus selalu meninggalkan jejak yang bisa dipulihkan.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan untuk runner, antrian, `watch` channel cancel, `spawn_blocking` ke registry, dan penanganan panic lewat `JoinError`.

## Feature design

### Batas fitur

Fitur ini membangun: engine (begin, antrian, lock, runner, langkah, progress, cancel, kompensasi, pemulihan saat start, resume, abandon, shutdown), journal di registry, primitive hapus aman `Filesystem::remove_tree` beserta adapter macOS dan pembungkus `remove_owned_tree` di core, sidik jari request, kontrak wire area `operation`, handler dan wiring di `stacklyd`, serta `testing::ScriptedWork` untuk test.

Bukan bagian fitur ini: pekerjaan nyata tiap jenis operasi (fitur 10, 14, 18, 20, dan seterusnya), command CLI `stackly operation ...` (fitur 8), tampilan GUI (fitur 13 dan layar pemiliknya), baris audit (fitur 35), operasi anak untuk environment (fitur 46), aksi pemulihan khusus per jenis di doctor (fitur 34), dan aturan purge (fitur 54 memakai `remove_owned_tree`).

### Peta modul dan dependency

| Crate | Tambahan dependency | Isi baru |
| --- | --- | --- |
| `stackly-domain` | tidak ada | modul `operation::journal` (lihat *Model domain*), `LockKey`, `RecoveryHint`, field `recovery_hint` di `Operation`, `OperationKind::resumable()`, konstanta alasan kegagalan; tambahan port `Registry` (Assumed: spec 0005) |
| `stackly-platform` | tidak ada | `Filesystem::remove_tree`, `RemoveReport`, varian error hapus |
| `stackly-platform-macos` | tidak ada (`libc` sudah ada) | `remove_tree` lewat `openat`, `fstatat`, `unlinkat` relatif ke file descriptor folder, dengan blok `unsafe` ber komentar `// SAFETY:` |
| `stackly-core` | `tokio` (fitur `rt`, `sync`, `time`, `macros`), `tracing` | modul `operation` (engine, `lock`, `scheduler`, `runner`, `context`, `compensation`, `recovery`, `limits`, `#[doc(hidden)] pub mod testing`), `remove_owned_tree` di modul `ownership` |
| `stackly-storage` | tidak ada | migrasi `NNNN_operation_journal.sql`, pemetaan, baca, dan `WriteOp` baru |
| `stackly-testkit` | tidak ada | `FakeRegistry` untuk tabel journal, `registry_conformance` baru, `FakeFilesystem::remove_tree`, pemeriksaan `fs_conformance` |
| `stackly-protocol` | `sha2` | modul `operation` (DTO dan params), `codes::operation`, `fingerprint`, varian `Call`, `Reply`, dan `EventKind` |
| `stacklyd` | tidak ada | `ipc::operation` (handler, peta DTO, `start_operation`), sink event, subsystem `operations`, panggilan engine di slot start dan shutdown |

Arah dependency tidak berubah: core tidak mengenal protocol atau storage, dan pemetaan ke DTO tetap di `stacklyd`.

### Model domain (Follow-up spec 0004)

Semua tipe baru ada di `stackly-domain`, dengan field privat, konstruktor yang memvalidasi, `as_code()` dan `FromStr` untuk enum berkode.

| Tipe | Aturan |
| --- | --- |
| `MethodName` | `^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*$`, 3 sampai 64 byte, nama method wire |
| `ErrorCodeName` | pola sama dengan `MethodName`, 3 sampai 96 byte, kode katalog `area.alasan` |
| `CompensatorName` | pola `ReasonCode`, maksimal 64 |
| `JournalPayload` | teks JSON object, 2 sampai `max` byte (`max` dipilih pemanggil dari *Batas*), tidak pernah berisi nilai secret; core yang menjamin JSON valid, SQL memeriksa ulang |
| `StepSeq` | 1 sampai 256 |
| `CreatedSeq` | 1 sampai 1024 |
| `OperationTrigger` | `origin: TriggerOrigin` (`user`, `background`), `client: Option<ClientCode>` (pola `ReasonCode`, misalnya `cli`, `gui`), `interactive: bool`; `interaction()` bernilai izinkan prompt hanya kalau `origin` `user` dan `interactive` true |
| `FailureError` | `code: ErrorCodeName`, `message` 1 sampai 200 karakter, `hint: Option<String>` paling banyak 200 karakter, `details: Vec<(DetailKey, String)>` dengan `DetailKey` camelCase 1 sampai 32 byte dan nilai paling banyak 512 byte, dan `items: Vec<FailureItem { kind: DetailKey, fields: Vec<(DetailKey, String)> }>` paling banyak 100 (bentuk `ErrorItem` spec 0006, supaya error seperti `secret.needs_secret` tidak kehilangan daftarnya) |
| `OperationJournal` | `operation_id`, `method: Option<MethodName>`, `fingerprint: Option<Sha256Digest>`, `trigger`, `resumable: bool`, `input: JournalPayload`, `expected_revision: Option<ResourceRevision>`, `waiting_on: Option<OperationId>`, `point_of_no_return_at`, `cancel_requested_at`, `compensation_started_at` (ketiganya opsional), `recovery_cause: Option<RecoveryCause>`, `failure_error: Option<FailureError>`, `result: Option<JournalPayload>`; `method` dan `fingerprint` terisi bersama |
| `RecoveryCause` | `interrupted`, `step_failed`, `compensation_failed`, `panicked`; diisi saat masuk `needs_recovery`, dikosongkan saat `resume` |
| `JournalStep` | `seq`, `name: StageName`, `repeatable`, `reversible`, `state: StepState` (`running`, `done`, `failed`, `interrupted`), `attempts: u32` (minimal 1), `checkpoint: Option<JournalPayload>`, `error_code: Option<ErrorCodeName>` (hanya `failed`), `started_at`, `finished_at` (kosong tepat saat `running`) |
| `CreatedItem` (`#[non_exhaustive]`) | `InstanceTree(InstanceId)`, `EnvironmentTree(EnvironmentId)`, `SecretRef(SecretRef)` (tipe spec 0011), `Custom { compensator: CompensatorName, key }` dengan `key` 1 sampai 256 byte `[!-~]`; teks `item_ref` berturut turut ID, ID, teks reference, dan `<compensator>:<key>` |
| `CreatedRecord` | `seq`, `step_seq`, `item`, `payload: Option<JournalPayload>` (hanya `Custom`), `state: CreatedState` (`intended`, `created`, `compensated`, `kept`, `compensation_failed`, `not_owned`), `detail: Option<ReasonCode>`, `recorded_at`, `settled_at` (kosong tepat saat `intended` atau `created`) |
| `LockKey` (`#[non_exhaustive]`) | `Runtime(RuntimeId)`, `Instance(InstanceId)`, `Environment(EnvironmentId)`, `Manager`, `Artifact(Sha256Digest)`, `InstanceName(InstanceName)`, `EnvironmentName(EnvironmentName)`, `ExternalRoot(PathBuf)` (path canonical, maksimal 1024 byte UTF 8); teks `runtime:<id>`, `instance:<id>`, `environment:<id>`, `manager:self`, `artifact:<sha256>`, `instance_name:<nama>`, `environment_name:<nama>`, `external_root:<path>`; `LockKey::for_resource(OperationResource)` |
| `RecoveryHint` | `auto_compensate`, `resume`, `abandon`, `manual` |
| `JournalView` | `journal`, `steps: Vec<JournalStep>`, `created: Vec<CreatedRecord>`, `locks: Vec<LockKey>` (baca saja) |

Perubahan pada tipe yang sudah ada:

- `Operation` mendapat `recovery_hint: Option<RecoveryHint>`, diisi lewat `set_recovery_hint(hint, at)` yang hanya diterima saat `needs_recovery`, dan dikosongkan otomatis oleh `apply` saat operasi meninggalkan `needs_recovery`. `Operation::restore` (spec 0005) menerima field ini.
- Revision `Operation` naik lewat `Operation::touch(at)`, tepat sekali per batch yang menyentuh operasi itu atau baris journal nya; `apply`, `set_stage`, progress, dan `set_recovery_hint` mengubah field dan `updated_at` tanpa menaikkan revision. Engine memanggil `touch` sekali sebelum membangun setiap batch, sehingga aturan spec 0005 (revision baru sama dengan expected ditambah 1) selalu terpenuhi.
- `Outcome::Failed` membawa `OperationFailure`, dan `apply(Resolve(..))` mencatatnya, supaya operasi yang ditutup `failed` lewat `resolve` memenuhi CHECK spec 0005 (`failed` wajib `failure_code`). `Outcome` tidak lagi `Copy`.
- `OperationKind::resumable() -> bool` mengikuti *Tabel resume per jenis*. `OperationKind::interrupt_policy() -> InterruptPolicy` (`Compensate` atau `CloseInterrupted`) mengikuti tabel yang sama.
- Konstanta `FailureCode` alasan engine: `STEP_FAILED`, `INTERRUPTED` (sudah ada), `ABANDONED`, `DAEMON_STOPPING`, `PRECONDITION_FAILED`, `PANICKED`.
- Doc comment `OperationEvent::CompensationFailed` diperluas menjadi "undoing finished steps failed, or is impossible because an irreversible step already ran". Engine memakai event ini untuk kegagalan setelah titik tak bisa dibalik, sehingga tabel state machine spec 0004 tidak berubah.

### Model data (migrasi `operation_journal`)

Satu migrasi `crates/storage/migrations/NNNN_operation_journal.sql` dengan versi bebas berikutnya saat build. Semua tabel `STRICT`, memakai jenis kolom bersama spec 0005 (`uuid`, `ts`, `sha256`, `code`, `bool`, `json`), ditambah jenis `errcode`: `TEXT`, `length(c) BETWEEN 3 AND 96 AND c GLOB '[a-z]*.[a-z]*' AND c NOT GLOB '*[^a-z0-9_.]*' AND c NOT GLOB '*.*.*'`. `*` berarti wajib.

**`operations`** (tabel spec 0005, kolomnya tidak berubah): migrasi lebih dulu mengosongkan `idempotency_key` pada setiap baris yang key nya juga dipakai baris lain dengan `(created_at, id)` lebih besar, lalu menghapus index biasa pada `idempotency_key` dan membuat `CREATE UNIQUE INDEX operations_idempotency_key ON operations(idempotency_key) WHERE idempotency_key IS NOT NULL`. Kolom `recovery_hint` yang sudah ada mulai diisi dari `Operation::recovery_hint`; set nilainya dicek di Rust (kolom tetap `code`). Setelah tabel baru dibuat, migrasi mengisi satu baris `operation_journal` untuk setiap baris `operations` yang sudah ada: `trigger_origin` `background`, `trigger_interactive` 0, `resumable` dari kode `kind` (tabel resume di bawah, ditulis sebagai `CASE` di SQL), `input` `'{}'`, kolom lain NULL.

**`operation_journal`** (1:1 dengan `operations`)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `operation_id` | `uuid` PK | FK `operations(id)` `ON DELETE CASCADE` |
| `method` | `TEXT` | 3 sampai 64, pola `MethodName` lewat GLOB yang sama dengan `errcode` |
| `request_fingerprint` | `sha256` | |
| `trigger_origin`* | `TEXT` | set tertutup `user`, `background` |
| `trigger_client` | `code` | set terbuka |
| `trigger_interactive`* | `bool` | |
| `resumable`* | `bool` | salinan `OperationKind::resumable()` saat dibuat, dibekukan per operasi |
| `input`* | `json` object | `length(input) <= 16384`; cukup untuk membangun ulang pekerjaan lewat `WorkFactory` |
| `expected_revision` | `INTEGER` | `>= 1`; revision resource yang diharapkan saat dibuat |
| `waiting_on` | `uuid` | tanpa FK |
| `point_of_no_return_at` | `ts` | |
| `cancel_requested_at` | `ts` | |
| `compensation_started_at` | `ts` | ditulis di commit yang memulai kompensasi |
| `recovery_cause` | `code` | set terbuka `RecoveryCause`; terisi selama `needs_recovery` |
| `failure_error_code` | `errcode` | |
| `failure_message` | `TEXT` | 1 sampai 200 karakter |
| `failure_details`* | `json` array | default `'[]'` |
| `result` | `json` object | `length(result) <= 16384` |

CHECK: `(method IS NULL) = (request_fingerprint IS NULL)`; `(failure_error_code IS NULL) = (failure_message IS NULL)`.

**`operation_steps`** (N:1 ke `operations`)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `operation_id`* | `uuid` | FK `operations(id)` `ON DELETE CASCADE` |
| `seq`* | `INTEGER` | 1 sampai 256 |
| `name`* | `code` | maksimal 32 |
| `repeatable`*, `reversible`* | `bool` | |
| `state`* | `TEXT` | set tertutup `running`, `done`, `failed`, `interrupted` |
| `attempts`* | `INTEGER` | `>= 1` |
| `checkpoint` | `json` | `length(checkpoint) <= 4096` |
| `error_code` | `errcode` | |
| `started_at`* | `ts` | |
| `finished_at` | `ts` | |

PK `(operation_id, seq)`; unik `(operation_id, name)`. CHECK: `(state = 'running') = (finished_at IS NULL)`; `error_code IS NULL OR state = 'failed'`; `repeatable = 1 OR attempts = 1`.

**`operation_created`** (N:1 ke `operations` dan ke langkah pembuatnya)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `operation_id`* | `uuid` | FK `operations(id)` `ON DELETE CASCADE` |
| `seq`* | `INTEGER` | 1 sampai 1024 |
| `step_seq`* | `INTEGER` | FK komposit `(operation_id, step_seq)` ke `operation_steps(operation_id, seq)` `ON DELETE CASCADE` |
| `item_kind`* | `code` | set terbuka: `instance_tree`, `environment_tree`, `secret_ref`, `custom` |
| `item_ref`* | `TEXT` | 1 sampai 512, hanya `[!-~]` |
| `payload` | `json` | `length(payload) <= 4096` |
| `state`* | `TEXT` | set tertutup `intended`, `created`, `compensated`, `kept`, `compensation_failed`, `not_owned` |
| `detail` | `code` | |
| `recorded_at`* | `ts` | |
| `settled_at` | `ts` | |

PK `(operation_id, seq)`; unik `(operation_id, item_kind, item_ref)`. CHECK: `(state IN ('intended', 'created')) = (settled_at IS NULL)`; `payload IS NULL OR item_kind = 'custom'`.

**`operation_locks`** (N:1 ke `operations`)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `operation_id`* | `uuid` | FK `operations(id)` `ON DELETE CASCADE` |
| `lock_key`* | `TEXT` | 3 sampai 1100, `GLOB '[a-z]*:?*'` |

PK `(operation_id, lock_key)`; index `lock_key`. Baris ditulis saat operasi dibuat dan dihapus di commit akhir. Baris ini hanya catatan key yang dibutuhkan operasi, bukan tanda sedang memegang lock: yang memegang ada di `LockTable` di memori.

Upsert `operation_steps` dan `operation_created` ditulis `INSERT ... ON CONFLICT (operation_id, seq) DO UPDATE`, tidak pernah `INSERT OR REPLACE` (yang menghapus lalu menyisipkan dan memicu cascade ke `operation_created`).

Seluruh tabel ikut retensi spec 0005: prune menghapus baris `operations` berstate akhir, dan baris anaknya hilang lewat cascade tanpa dihitung di `PruneReport`. Operasi aktif dan `needs_recovery` tidak pernah di prune.

### Port registry (Assumed: spec 0005)

Tambahan pada trait `Registry` di `stackly-domain::registry`, diimplementasikan `FakeRegistry` dan `SqliteRegistry`:

- `WriteOp::InsertJournal(OperationJournal)` dan `UpdateJournal(OperationJournal)`; `UpsertStep { operation, step: JournalStep }`; `UpsertCreated { operation, record: CreatedRecord }`; `InsertLocks { operation, keys: Vec<LockKey> }`; `DeleteLocks { operation }`. Cek khususnya: operasi ada (`MissingReference`), `step_seq` ada untuk catatan baru.
- `RegistryError::IdempotencyKeyTaken { key: IdempotencyKey, existing: OperationId }`, dicek di tahap aturan unik `InsertOperation`.
- Baca: `operation_journal(id) -> Versioned<Option<JournalView>>`; `operation_journals(&[OperationId]) -> Versioned<Vec<OperationJournal>>` (satu transaksi baca, tanpa langkah dan catatan, untuk halaman list); `operation_by_idempotency_key(&IdempotencyKey) -> Versioned<Option<(Operation, OperationJournal)>>`; `unfinished_operations() -> Versioned<Vec<(Operation, JournalView)>>` untuk state `queued`, `running`, `cancel_requested`, dan `needs_recovery`, urut `(created_at, id)` naik.
- Kolom `operations.recovery_hint` dipetakan ke `Operation::recovery_hint`.
- Prune operasi berstate akhir menghapus paling banyak 500 baris `operations` per transaksi (bukan 5.000), karena setiap baris membawa cascade ke tabel journal.

Engine selalu menulis baris journal di batch yang sama dengan `InsertOperation` atau `UpdateOperation` operasi itu, dengan `Operation::touch` dipanggil sekali per batch, sehingga revision operasi naik tepat satu per batch (pola anak berubah bersama revision induk dari spec 0005). Satu batch boleh berisi `UpdateOperation` untuk beberapa operasi (misalnya perubahan `waiting_on` satu pass dispatcher).

### State operasi di engine

Engine hanya memakai event state machine spec 0004:

| Situasi | Event | Hasil |
| --- | --- | --- |
| `begin` | `Operation::new` | `queued` |
| dispatcher memberi slot dan semua lock, lalu cek ulang `expected_revision` lolos | `start` | `running` |
| `expected_revision` basi saat dispatcher mengambil lock (operasi belum pernah mulai) | `fail` (`precondition_failed`) | `failed` |
| operasi antri menunggu key yang kini dipegang operasi `needs_recovery` | `fail` (`precondition_failed`, error `operation.recovery_required`) | `failed` |
| operasi antri yang belum pernah mulai tidak bisa dibangun ulang setelah restart | `fail` (`interrupted`) | `failed` |
| commit akhir berhasil | `succeed` | `succeeded` |
| pekerjaan gagal sebelum titik tak bisa dibalik, kompensasi berhasil | `fail` (`step_failed` atau `panicked`) | `failed` |
| cancel pada `queued` yang belum pernah mulai | `request_cancel` | `cancelled` |
| cancel pada `running` | `request_cancel` | `cancel_requested` |
| kompensasi setelah cancel berhasil | `cancelled` | `cancelled` |
| kompensasi gagal, atau pekerjaan gagal setelah titik tak bisa dibalik (kecuali jenis `CloseInterrupted`, yang selesai `failed`) | `compensation_failed` | `needs_recovery` |
| shutdown memarkir operasi resumable | `interrupted { resumable: true }` | `queued` |
| start menemukan `running` atau `cancel_requested` | `interrupted { resumable }` | `queued` atau `needs_recovery` |
| `operation.resume` | `resume` | `queued` |
| kompensasi otomatis atau abandon selesai | `resolve(Failed(failure))` atau `resolve(Cancelled)` | state akhir |

Commit `start` yang gagal (misalnya `NoSpace`) melepas lock dan slot; operasi tetap `queued`, dan dispatcher mencoba lagi setelah `JOURNAL_RETRY_INTERVAL`.

Aturan hint saat masuk `needs_recovery`: `auto_compensate` kalau titik tak bisa dibalik belum dilewati dan kompensasi belum pernah gagal; `resume` kalau sudah dilewati, jenisnya resumable dengan `WorkFactory` terdaftar, `compensation_started_at` kosong, dan langkah yang gagal atau terputus `repeatable`; `abandon` kalau kompensasi gagal sebelum titik itu; selain itu `manual`. `recovery_cause` diisi bersamaan (`interrupted`, `step_failed`, `compensation_failed`, atau `panicked`) beserta `failure_error` penyebabnya, dan keduanya dikosongkan saat `resume`. Aksi di DTO: `resume` hanya untuk hint `resume`; `abandon` untuk semua `needs_recovery` kecuali selama kompensasi otomatis sedang berjalan.

### API engine (dipakai fitur 10, 18, 20, dan seterusnya)

Semua di `stackly-core::operation`. API ini stabil; fitur lain menambah jenis operasi, kompensator, dan factory, bukan bentuk baru.

```rust
#[derive(Clone)]
pub struct OperationEngine { /* Arc<Inner> */ }

pub struct EngineDeps {
    pub registry: Arc<dyn Registry>,
    pub platform: Arc<Platform>,
    pub layout: Arc<PreparedLayout>,
    pub events: Arc<dyn OperationEvents>,
}

impl OperationEngine {
    pub fn new(deps: EngineDeps, limits: EngineLimits) -> Self;
    pub fn register_factory(&self, kind: OperationKind, factory: Arc<dyn WorkFactory>);
    pub fn register_compensator(&self, name: CompensatorName, compensator: Arc<dyn Compensator>);
    pub async fn recover(&self) -> Result<RecoveryReport, EngineError>; // slot start
    pub fn start(&self);                                                // setelah daemon siap
    pub async fn begin(&self, request: BeginRequest) -> Result<BeginOutcome, BeginError>;
    pub async fn get(&self, id: OperationId) -> Result<Option<OperationDetailView>, EngineError>;
    pub async fn list(&self, query: OperationQuery) -> Result<OperationListPage, EngineError>;
    pub async fn cancel(&self, id: OperationId) -> Result<CancelOutcome, EngineError>;
    pub async fn resume(&self, id: OperationId, expected: Option<ResourceRevision>) -> Result<OperationView, EngineError>;
    pub async fn abandon(&self, id: OperationId, expected: ResourceRevision) -> Result<AbandonOutcome, EngineError>;
    pub fn stats(&self) -> EngineStats;
    pub fn locks(&self) -> LockTable;
    pub async fn shutdown(&self, grace: Duration) -> ShutdownReport;
}

pub struct BeginRequest {
    pub kind: OperationKind,
    pub resource: OperationResource,
    pub creates_resource: bool,                       // true: resource berisi ID baru yang dibuat handler
    pub extra_locks: Vec<LockKey>,                    // engine selalu menambah LockKey::for_resource(resource)
    pub busy: BusyPolicy,                             // Queue (default) atau Reject
    pub expected_revision: Option<ResourceRevision>,  // revision resource; None untuk Manager dan creates_resource
    pub origin: Origin,                               // Request(RequestOrigin) atau Background
    pub input: JournalPayload,                        // cukup untuk WorkFactory::rebuild, tanpa secret
    pub work: Box<dyn OperationWork>,
    pub precheck: Option<Box<dyn Precheck>>,          // dijalankan di gerbang begin setelah pencarian key
}

pub trait Precheck: Send {
    fn check(self: Box<Self>) -> PrecheckOutcome;     // Reject(OperationError), NoOp(NoOpReply), atau Proceed
}

pub struct RequestOrigin {
    pub method: MethodName,
    pub key: IdempotencyKey,
    pub fingerprint: Sha256Digest,
    pub client: ClientCode,
    pub interactive: bool,
}

pub enum BeginOutcome { Created(OperationView), Replayed(OperationView) }

pub enum BeginError {
    KeyReused { existing: OperationId },
    Busy { holder: LockHolder },
    RecoveryRequired { operation: OperationId, key: LockKey },
    QueueFull { max: u32 },
    RevisionMismatch { expected: ResourceRevision, actual: ResourceRevision },
    ResourceNotFound,
    PayloadTooLarge { field: &'static str, max: usize },
    InvalidRequest { reason: &'static str },          // bug pemanggil, misalnya creates_resource dengan expected_revision
    NotReady,
    ShuttingDown,
    Registry(RegistryError),
}

pub trait OperationWork: Send + 'static {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult>;
}
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type WorkResult = Result<WorkOutcome, WorkError>;

pub struct WorkOutcome { pub result: OperationOutcome, pub finalize: Option<Finalize> }
pub type Finalize = Box<dyn FnMut(&dyn Registry) -> Result<FinalBatch, WorkError> + Send>;
pub struct FinalBatch { pub expected_generation: Option<Generation>, pub ops: Vec<WriteOp> }
pub struct OperationOutcome { pub resources: Vec<ResourceRef>, pub partial: Vec<PartialItem> }

pub enum WorkError { Cancelled, Parked, Failed(Box<dyn OperationError>), Engine(EngineError) }

pub trait OperationError: std::error::Error + Send + Sync + 'static {
    fn code(&self) -> ErrorCodeName;                 // kode katalog, misalnya supervisor.spawn_failed
    fn retryable(&self) -> bool;                     // operasi baru mungkin berhasil
    fn details(&self) -> Vec<(DetailKey, String)>;   // kunci harus ada di entri katalog kode itu
    fn rule(&self) -> Option<&'static str> { None }
}

impl OperationContext {
    pub fn id(&self) -> OperationId;
    pub fn kind(&self) -> OperationKind;
    pub fn resource(&self) -> OperationResource;
    pub fn trigger(&self) -> &OperationTrigger;
    pub fn input(&self) -> Option<&JournalPayload>;
    pub fn registry(&self) -> &Arc<dyn Registry>;     // baca; tulis hanya lewat commit atau finalize
    pub fn platform(&self) -> &Arc<Platform>;
    pub fn layout(&self) -> &Arc<PreparedLayout>;
    pub async fn begin_step(&mut self, spec: StepSpec) -> Result<StepStart, WorkError>;
    pub async fn finish_step(&mut self, checkpoint: Option<JournalPayload>) -> Result<(), WorkError>;
    pub fn check_cancel(&self) -> Result<(), WorkError>;
    pub async fn stopped(&self);                       // selesai saat cancel atau parkir berlaku untuk langkah ini
    pub async fn set_stage(&mut self, stage: StageName) -> Result<(), WorkError>;
    pub fn set_progress(&mut self, progress: Progress);
    pub async fn claim_resource(&mut self) -> Result<CreatedHandle, WorkError>;   // niat pohon untuk resource creates_resource
    pub async fn new_instance_id(&mut self) -> Result<(InstanceId, CreatedHandle), WorkError>;
    pub async fn new_environment_id(&mut self) -> Result<(EnvironmentId, CreatedHandle), WorkError>;
    pub async fn record_secret_intent(&mut self, reference: SecretRef) -> Result<CreatedHandle, WorkError>;
    pub async fn record_custom_intent(&mut self, compensator: CompensatorName, key: CustomKey, payload: Option<JournalPayload>) -> Result<CreatedHandle, WorkError>;
    pub async fn confirm_created(&mut self, item: CreatedHandle) -> Result<(), WorkError>;
    pub async fn not_created(&mut self, item: CreatedHandle, reason: ReasonCode) -> Result<(), WorkError>;
    pub async fn scratch_dir(&mut self) -> Result<PathBuf, WorkError>;
    pub async fn commit(&mut self, batch: FinalBatch) -> Result<Committed, WorkError>;
}

pub struct StepSpec { pub name: StageName, pub repeatable: bool, pub reversible: bool }
pub enum StepStart { Run { attempt: u32 }, AlreadyDone { checkpoint: Option<JournalPayload> } }
pub struct CreatedHandle { /* CreatedSeq, opaque */ }

pub trait Compensator: Send + Sync + 'static {
    fn compensate(&self, scope: CompensationScope, record: &CreatedRecord)
        -> BoxFuture<'static, Result<CompensationResult, CompensationError>>;
}
pub struct CompensationScope {
    pub operation: OperationId,
    pub registry: Arc<dyn Registry>,
    pub platform: Arc<Platform>,
    pub layout: Arc<PreparedLayout>,
}
pub enum CompensationResult { Compensated, NotOwned { reason: ReasonCode } }

pub trait WorkFactory: Send + Sync + 'static {
    fn rebuild(&self, operation: &Operation, journal: &JournalView) -> Result<Box<dyn OperationWork>, RebuildError>;
}
pub enum RebuildError { NotRebuildable { reason: ReasonCode }, InvalidInput }

pub struct OperationView {
    pub operation: Operation,
    pub journal: OperationJournal,
    pub recovery_actions: Vec<RecoveryAction>,       // dihitung dari aturan hint dan factory terdaftar
    pub auto_compensating: bool,
}
pub struct OperationDetailView { pub view: OperationView, pub steps: Vec<JournalStep>, pub created: Vec<CreatedRecord> }
pub struct OperationListPage { pub items: Vec<OperationView>, pub next: Option<OperationCursor> }
pub struct EngineLimits {                             // Default::default() memakai konstanta di Batas
    pub max_running: u32, pub max_queued: u32,
    pub progress_interval: Duration, pub journal_retry: Duration, pub finalize_retries: u32,
}

pub trait OperationEvents: Send + Sync + 'static {
    fn state_changed(&self, view: &OperationView);
    fn progress(&self, view: &OperationView);
}

#[derive(Clone)]
pub struct LockTable { /* Arc<Mutex<..>> */ }
impl LockTable {
    pub fn holder(&self, key: &LockKey) -> Option<LockHolder>;
    pub fn try_lock_internal(&self, keys: &[LockKey], name: &'static str) -> Result<InternalLock, LockBusy>;
}
pub enum LockHolder { Operation { id: OperationId, state: OperationState }, Internal(&'static str) }
```

Aturan pakai yang mengikat:

- **Commit registry terakhir.** Pekerjaan menaruh semua efek filesystem di path miliknya dulu (scratch atau pohon ber ID baru), lalu mengembalikan `finalize`. Engine menjalankan `finalize` di `spawn_blocking`, menambahkan `UpdateOperation(succeed)`, `UpdateJournal` (hasil), semua catatan `created` menjadi `kept`, dan `DeleteLocks`, lalu meng commit nya sebagai satu batch. `GenerationConflict` memanggil `finalize` lagi, paling banyak lima kali (aturan spec 0005). Konflik revision menjadi kegagalan langkah dengan error `resource.revision_mismatch`. `ctx.commit` di tengah operasi hanya untuk keadaan yang memang harus terlihat sebelum selesai, dan apa pun yang ia sisipkan wajib punya kompensator kustom.
- **Resource baru.** Untuk jenis yang membuat resource (`create_instance`, `install_runtime`, restore sebagai baru, dan sejenisnya), handler membuat ID baru lebih dulu (untuk instance: `InstanceId::generate()` diulang sampai short key nya tidak ada di `known_instance_ids`), mengisinya di `resource`, dan memberi `creates_resource: true`. Engine melewati cek keberadaan dan revisi resource itu, tetap mengunci `LockKey::for_resource`, dan pekerjaan memanggil `claim_resource` di langkah pertama yang menyentuh disk untuk mencatat niat `InstanceTree` atau `EnvironmentTree` atas ID itu. `ShortKeyTaken` yang masih terjadi karena balapan menggagalkan operasi (kode milik fitur 20), bukan membuat ID kedua. `result.resources` melaporkan resource akhirnya.
- **Langkah.** `begin_step` dijalankan di bawah mutex penulis operasi: ia memeriksa cancel (sebelum titik tak bisa dibalik berarti `Cancelled`), parkir shutdown (operasi resumable berarti `Parked`), dan berhenti internal shutdown (operasi tidak resumable sebelum titik itu berarti `Cancelled` internal) lebih dulu, apa pun jenis langkah berikutnya; lalu mencatat langkah `running` (atau mengembalikan `AlreadyDone` dengan checkpoint saat resume), dan untuk langkah dengan `reversible: false` meng commit `point_of_no_return_at` sebelum mengembalikan `Run`. Saat langkah diulang, `started_at` dan `attempts` diperbarui per percobaan. Langkah yang terbuka saat pekerjaan mengembalikan error dicatat `failed`. Nama langkah unik per operasi; paling banyak 256 langkah.
- **Checkpoint di dalam langkah.** `check_cancel` dan `stopped` di tengah langkah hanya memicu untuk cancel kalau langkah itu `reversible`, dan untuk parkir kalau langkah itu `repeatable` di operasi resumable; di langkah lain keduanya tidak pernah memicu sampai langkah berikutnya. Pekerjaan wajib memanggil `check_cancel` di setiap titik aman dan memakai `stopped` dalam `tokio::select!` untuk penantian panjang. Engine tidak pernah mematikan pekerjaan secara paksa.
- **Catatan dibuat.** `claim_resource`, `new_instance_id`, `new_environment_id`, `record_secret_intent`, dan `record_custom_intent` hanya bisa dipanggil di dalam langkah, meng commit catatan `intended` sebelum mengembalikan ID atau handle, dan efek sampingnya baru boleh terjadi setelah itu. `confirm_created` menandai `created`; `not_created(item, reason)` menandai `not_owned` dengan `detail` `already_exists` atau `short_key_taken` saat pembuatan gagal karena sudah ada.
- **Secret dan kustom.** `record_secret_intent` hanya menerima reference turunan ID yang dibuat atau diklaim operasi itu (bentuk daemon spec 0011, `local:instance/<id>/<purpose>` atau padanannya untuk environment), dan sebelum mencatat niat ia mem probe store lewat spec 0011; item yang sudah ada menghasilkan error `secret.conflict` tanpa catatan. Kompensator kustom wajib membuktikan kepemilikan sebelum menghapus (misalnya identitas proses yang dicatat di payload saat spawn) dan mengembalikan `NotOwned` kalau tidak bisa; `stackly_core::operation::testing::compensator_conformance` memeriksa idempotensi dan penolakan item yang bukan miliknya (di core, karena testkit tidak boleh bergantung pada core).
- **Bagian kritis di task engine.** `begin`, `cancel`, `resume`, dan `abandon` menjalankan bagian kritisnya (commit sampai antrian dan memori sinkron) di task milik engine dan pemanggil hanya menunggu handle nya, sehingga request yang dibatalkan tidak pernah memotongnya di tengah.
- **Lock internal.** `try_lock_internal` untuk bagian kritis pendek tanpa operasi (misalnya supervisor menulis lifecycle setelah crash); dipegang paling lama satu commit registry dan tidak pernah selama IO eksternal. Operasi antri tidak diberi key yang dipegang lock internal.
- **Kompensator kustom** wajib idempoten dan menganggap resource yang sudah hilang sebagai `Compensated`.
- **`ScriptedWork`** (`stackly_core::operation::testing`, `#[doc(hidden)]`): builder langkah (`step(name).repeatable().irreversible()`) dengan perilaku per langkah `succeed`, `fail(code, retryable)`, `panic`, `hold` (ditahan sampai test melepas lewat `HoldHandle`, dengan sinyal saat mulai), `check_cancel_every(n)`, `creates_instance_tree`, dan `progress(done, total)`; penghitung efek samping bersama (`EffectCounter`) dan `ScriptedFactory` untuk resume dari `input`.

### Langkah, titik tak bisa dibalik, dan resume

- Titik tak bisa dibalik adalah saat langkah pertama dengan `reversible: false` dimulai. Sebelum titik itu, semua efek bisa dibersihkan lewat catatan dibuat; sesudahnya engine tidak pernah mengompensasi otomatis.
- `repeatable: true` berarti langkah aman dijalankan ulang dari awal setelah terputus di tengah (misalnya download yang melanjutkan file parsial di scratch, verifikasi digest, ekstrak ulang ke folder staging yang dikosongkan dulu). Langkah dengan `repeatable: false` dijalankan paling banyak sekali per operasi; constraint SQL `repeatable = 1 OR attempts = 1` menjadi penjaga terakhir.
- Setiap jenis operasi yang dibangun fitur pemiliknya mendaftarkan `WorkFactory`. Resume, dan operasi `queued` yang belum pernah mulai saat daemon restart, membangun ulang pekerjaan lewat factory itu dari `input` dan `JournalView`. `begin_step` untuk langkah `done` mengembalikan `AlreadyDone` dengan checkpoint nya, sehingga pekerjaan melompat ke langkah berikutnya. Params yang tidak boleh masuk `input` (nilai secret dari `SecretInput`) membuat `rebuild` mengembalikan `NotRebuildable`; operasi seperti itu hanya bisa berjalan di run daemon tempat ia dibuat.

### Tabel resume per jenis operasi

Default yang diwarisi fitur pemilik; perubahan baris lewat update spec ini.

| Jenis | Resumable | Catatan untuk fitur pemilik |
| --- | --- | --- |
| `install_runtime` | ya | download, verifikasi, dan ekstrak di scratch; commit lewat rename atomik lalu `finalize` (fitur 18) |
| `start_instance`, `stop_instance`, `restart_instance`, `reload_instance` | tidak, kebijakan `CloseInterrupted` | proses instance selamat dari restart daemon dan diadopsi rekonsiliasi spec 0010 (AC-9, AC-21), jadi operasi yang terputus ditutup `failed` alasan `interrupted` tanpa kompensasi; diubah 2026-10-18 atas keputusan engineer di pass rekonsiliasi (fitur 10, 26) |
| `environment_up`, `environment_down` | ya | fitur 46 |
| `backup_instance` | ya | menulis ke scratch; mulai ulang staging aman (fitur 49) |
| `create_instance`, `restore_instance`, `migrate_data`, `purge_instance`, `remove_instance`, `remove_runtime`, `register_external_runtime`, `rename_instance`, `rebind_runtime`, `update_provider_revision`, `update_manager` | tidak | terputus sebelum titik tak bisa dibalik berarti kompensasi otomatis; sesudahnya `needs_recovery` (fitur 20, 51, 54, 55, 56, 58, 59, 61) |

`OperationKind::interrupt_policy() -> InterruptPolicy` (domain, Follow-up spec 0004) memberi `Compensate` (default, perilaku di bawah) atau `CloseInterrupted` (empat jenis operasi instance di atas). Untuk jenis `CloseInterrupted`, titik tak bisa dibalik hanya menahan kompensasi: kegagalan pekerjaan setelah titik itu tetap selesai `failed` dengan kode dari `WorkError` (tidak pernah `needs_recovery`), `check_cancel` dan `stopped` tetap menyampaikan cancel di langkah mana pun, dan pekerjaan menanggapinya sendiri (spec 0010: stop proses, lalu `cancelled`). Operasi `CloseInterrupted` yang terputus, saat shutdown maupun crash, tidak pernah dikompensasi dan tidak pernah dilanjutkan: `recover` menutupnya `failed` alasan `interrupted` (`retryable` true, error `operation.interrupted`) dan melepas key nya sebelum rekonsiliasi supervisor, yang lalu mengadopsi atau menyelesaikan proses nya.

### Idempotency

- Method yang memulai operasi (reply nya `OperationAccepted`) terdaftar di `stackly_protocol::operation::STARTING_METHODS`. `start_operation` di `stacklyd` menolak request tanpa `idempotencyKey` dengan `operation.idempotency_key_required` sebelum apa pun dibaca dari registry.
- Sidik jari: `stackly_protocol::fingerprint::request_fingerprint(&Call) -> [u8; 32]` adalah SHA-256 atas byte `<method>\n<params kanonis>`. Params kanonis adalah `Call` yang diserialize ke `serde_json::Value`, dengan semua key object diurutkan rekursif, lalu ditulis dengan `serde_json::to_vec`. Selama fungsi itu berjalan, flag thread local `fingerprint::redacting()` bernilai true dan `SecretText` diserialize sebagai string tetap `"[secret]"` (Assumed: spec 0011), sehingga nilai secret tidak pernah masuk sidik jari.
- `begin` mencari key lewat `operation_by_idempotency_key` (tanpa peta di memori, yang bisa basi setelah prune). Sidik jari sama menghasilkan `Replayed` dengan state saat ini (aktif atau akhir); berbeda menghasilkan `KeyReused`. Unique index parsial adalah penjaga terakhir; `IdempotencyKeyTaken` saat insert diperlakukan sebagai pencarian ulang.
- Semua `begin` lewat satu mutex async ("gerbang begin"), sehingga pencarian key dan insert atomik terhadap `begin` lain. Di dalam gerbang, cek lock, policy, dan antrian dilakukan di bawah mutex scheduler (sync, dipegang singkat) sekaligus mereservasi tempat di antrian sebagai entri tertunda; lalu batch awal di commit; lalu entri ditandai siap (atau dibuang kalau commit gagal). Dispatcher tidak pernah memulai entri tertunda, tetapi menghitung key nya sebagai ditunggu, sehingga keputusan `Reject` dan urutan FIFO tidak bisa dilangkahi di antara cek dan commit.
- Urutan cek `begin`: (1) `ShuttingDown` atau `NotReady` (`NotReady` berlaku sejak daemon start sampai `start()` dipanggil, sehingga start yang dipicu rekonsiliasi dikirim setelah `start()`); (2) `InvalidRequest` dan `PayloadTooLarge`; (3) pencarian key (`Replayed` atau `KeyReused`), sebelum apa pun yang bergantung pada state; (3a) `precheck` milik fitur (kalau ada): `Reject` mengembalikan error fitur tanpa operasi, `NoOp(reply)` menjawab tanpa operasi (kalau `NoOpReply` membawa batch yang menulis sesuatu, misalnya desired state, engine meng commit batch itu bersama baris operasi `succeeded` tanpa langkah sehingga key terikat dan replay mengembalikan jawaban yang sama), `Proceed` melanjutkan; supaya replay tetap berhasil saat antrian penuh; (4) `ResourceNotFound`; (5) `RevisionMismatch`; (6) `RecoveryRequired`; (7) `Busy`; (8) `QueueFull`. Key wajib sudah diperiksa `start_operation` sebelum `begin`.
- Key terikat selama baris operasinya ada. `cancel`, `resume`, dan `abandon` tidak butuh key karena idempoten terhadap state.
- Method pemula operasi boleh membungkus `OperationAccepted` dalam reply milik fitur pemiliknya, dan boleh menjawab tanpa membuat operasi kalau permintaannya tidak mengubah apa pun (misalnya `alreadyInState` spec 0010), lewat `precheck`. Urutannya tetap: key wajib, pencarian key (replay mengembalikan reply aslinya, tanpa cek `expectedRevision` ulang), baru `precheck`. Jawaban tanpa operasi yang tidak menulis apa pun tidak mengikat key; yang menulis sesuatu mengikatnya lewat baris operasi `succeeded` tanpa langkah.

### Lock dan antrian

- `LockTable` di memori memegang key untuk operasi yang sedang dikerjakan runner (`running`, atau `cancel_requested` yang sedang dikompensasi) dan operasi `needs_recovery`, serta pemegang internal. Operasi `queued` (termasuk `cancel_requested` yang belum dikerjakan dispatcher) tidak memegang key. Tidak ada mutex SQLite: lock tidak pernah ditulis sebagai kunci di registry; `operation_locks` hanya catatan untuk membangun ulang setelah restart.
- Dispatcher dijalankan setiap kali operasi dibuat, lock atau slot dilepas, lock internal dilepas, dan saat `start()`. Ia menelusuri antrian urut `(created_at, id)`. Sebuah operasi dimulai kalau slot berjalan tersedia, semua key nya bebas, dan tidak ada key nya yang ditunggu operasi antri yang lebih tua. Kalau tidak, `waiting_on` diisi penghalang tertua menurut `(created_at, id)` (pemegang key atau penunggu yang lebih tua; kosong untuk pemegang internal). Semua perubahan `waiting_on` dari satu pass dispatcher di commit dalam satu batch, lalu masing masing menerbitkan `operation.progress`.
- Saat dispatcher memulai operasi: ambil semua key, cek ulang `expected_revision` (selama `started_at` kosong), lalu commit `start` atau `fail`. Kalau commit gagal, key dan slot dilepas dan operasi tetap antri.
- `BusyPolicy::Reject` menolak kalau ada key yang dipegang atau ditunggu operasi antri yang lebih tua; `resource.busy` membawa `operationId` pemegang kecuali pemegangnya internal. Key yang dipegang operasi `needs_recovery` selalu ditolak `RecoveryRequired`, apa pun policy nya. Saat sebuah operasi masuk `needs_recovery`, setiap operasi antri yang membutuhkan salah satu key nya ditutup `failed` (alasan `precondition_failed`, error `operation.recovery_required` dengan `operationId` operasi itu).
- Batas di `operation::limits`: `MAX_RUNNING_OPERATIONS` 8, `MAX_QUEUED_OPERATIONS` 64. Benchmark fitur 64 boleh menyetel ulang lewat update spec ini.

### Progress dan event

- `set_progress` menyimpan nilai terakhir di memori. Kalau commit progress terakhir lebih dari `PROGRESS_COMMIT_INTERVAL` (1 detik) yang lalu, nilai itu langsung di commit; kalau belum, satu commit penutup dijadwalkan di akhir jendela dengan nilai terakhir saat itu. `set_stage`, transisi state, perubahan `waiting_on`, titik tak bisa dibalik, dan cancel di commit seketika bersama progress terakhir, dan membatalkan commit penutup yang tertunda.
- Setiap tulisan ke baris operasi lewat satu penulis per operasi (mutex async milik operasi itu), jadi CAS revision operasi tidak pernah bentrok dalam jalan normal; bentrok berarti bug dan menjadi `internal.unexpected`. Tiga keputusan berjalan utuh di bawah mutex ini: `begin_step` (cek cancel sampai commit titik tak bisa dibalik), `cancel` (baca titik tak bisa dibalik sampai commit `cancel_requested`), dan finalize (cek cancel sampai commit batch akhir).
- Setelah commit, engine memanggil `OperationEvents::state_changed` untuk perubahan state (termasuk saat dibuat) dan `OperationEvents::progress` untuk perubahan lain yang terlihat di `OperationSummary` tanpa mengubah state (stage, progress, `waiting_on`, titik tak bisa dibalik, cancel). Commit langkah atau catatan dibuat yang tidak mengubah tampilan tidak menerbitkan event. `stacklyd` memetakan ke DTO dan memanggil `publish_resource` dengan resource `operation` dan revision hasil commit.

### Cancel

- `cancel(id)`: berstate akhir menghasilkan `already_final`; `needs_recovery` menghasilkan `recovery_required`; `cancel_requested` menghasilkan lagi `requested` atau `continuing_past_point_of_no_return` tanpa commit; `queued` tanpa `started_at` langsung `cancelled` (`cancelled_before_start`); `queued` dengan `started_at` (operasi yang dilanjutkan) menjadi `cancel_requested`, lalu dispatcher menjalankan kompensasi tanpa melanjutkan pekerjaan kalau titik tak bisa dibalik belum dilewati, atau melanjutkan pekerjaan sampai selesai kalau sudah (`continuing_past_point_of_no_return`); `running` menjadi `cancel_requested` dengan `cancel_requested_at`, sinyal `watch` dikirim, dan disposisinya `requested` atau `continuing_past_point_of_no_return`.
- Cancel sebelum titik tak bisa dibalik juga diperiksa tepat sebelum `finalize`: kalau sudah diminta, engine mengompensasi lalu `cancelled`, bukan `succeeded`.
- Setelah titik tak bisa dibalik, cancel tidak menghentikan pekerjaan. Operasi selesai sebagai `succeeded` dengan `cancelRequestedAt` dan `pointOfNoReturnAt` di DTO, atau `needs_recovery` kalau gagal.

### Kompensasi dan primitive hapus

- Memulai kompensasi selalu diawali commit `compensation_started_at` (bersama `cancel_requested_at` atau kegagalan langkah bila ada), dan engine memegang flag "kompensasi diklaim" per operasi di memori selama kompensasi berjalan; abandon pada operasi yang sedang diklaim menghasilkan `not_abandonable` (`auto_compensating`).
- Urutan: catatan `intended` dan `created` urut `seq` turun, lalu folder scratch. Setiap catatan menjadi `compensated`, `not_owned`, atau `compensation_failed` dan di commit begitu selesai; kalau tulisan itu gagal, hasilnya disimpan di memori dan ditulis di batch akhir yang diulang. Kompensasi berlanjut ke semua catatan walau ada yang gagal. Folder scratch hanya dihapus kalau tidak ada catatan yang gagal. Satu saja `compensation_failed` membuat operasi `needs_recovery` dengan hint `abandon` dan `recovery_cause` `compensation_failed`.
- Abandon memakai urutan yang sama tetapi hanya untuk catatan yang `step_seq` nya langkah `reversible`, dan untuk folder scratch hanya kalau titik tak bisa dibalik belum dilewati; catatan dari langkah yang tidak bisa dibalik dibiarkan dan masuk daftar sisa.
- Kompensator bawaan engine:
  - `instance_tree(id)`: path dihitung dari `DirectoryLayout` (`instance_paths(id).root`, `instance_log_dir(id)`, dan `instance_run_dir(id.short_key())`). Kalau `instance-manifest.json` terbaca dan `createdByOperation` bukan operasi ini, seluruh item `not_owned` (alasan `created_by_other_operation`). Folder run dilewati kalau `known_instance_ids` memuat instance lain dengan short key yang sama. Setiap path lewat `remove_owned_tree`.
  - `environment_tree(id)`: `environment_dir(id)` lewat `remove_owned_tree`.
  - Folder scratch `operations_dir()/<operation-id>/`: dibuat malas oleh `scratch_dir()` dengan `create_dir_private`, dihapus lewat `remove_owned_tree` sebagai langkah terakhir kompensasi, dan juga tepat setelah batch akhir `succeeded` ter commit (usaha terbaik; kegagalan hanya dicatat warning, pembersih saat start menjadi jaring pengaman).
- `secret_ref` adalah kompensator yang didaftarkan `stacklyd` di atas use case hapus spec 0011 (Assumed). Kompensator lain didaftarkan fitur pemiliknya dengan nama `<area>_<objek>` (misalnya kompensator kustom milik fitur yang membuat item di luar layout; supervisor fitur 10 tidak memakai kompensator untuk proses, karena operasi instance berkebijakan `CloseInterrupted`).
- `remove_owned_tree(platform, layout, path) -> Result<RemoveReport, OwnershipError>` di `stackly-core::ownership`: `check_owned_path` lebih dulu (menolak path di luar root, root itu sendiri, dan path tidak normal), lalu `Filesystem::remove_tree(root canonical, path relatif)`. Ini juga primitive yang dipakai purge fitur 54.
- `Filesystem::remove_tree(root, relative) -> Result<RemoveReport, PlatformError>` (Follow-up spec 0004): `root` adalah owned root canonical, `relative` path normal dengan minimal satu komponen. Adapter macOS membuka `root` dengan `O_DIRECTORY | O_NOFOLLOW`, turun per komponen perantara dengan `openat(O_DIRECTORY | O_NOFOLLOW)` (symlink atau bukan folder menghasilkan `SymlinkInPath` atau `NotADirectory` tanpa menghapus apa pun), membaca target dengan `fstatat(AT_SYMLINK_NOFOLLOW)` (tidak ada berarti `RemoveReport { missing: true }`), menghapus file, symlink, dan entri lain dengan `unlinkat`, menelusuri folder dengan `openat(O_DIRECTORY | O_NOFOLLOW)` lalu `fdopendir` secara iteratif dengan tumpukan paling dalam 128 (`TooDeep`), menolak entri yang `st_dev` nya berbeda dari root (`CrossesDevice`), lalu menghapus folder kosong dengan `unlinkat(AT_REMOVEDIR)`. `RemoveReport { files, dirs, missing }`. `FakeFilesystem` (std::fs di dalam root test) mengikuti semantik yang sama lewat penelusuran portabel `symlink_metadata`, `read_dir`, `remove_file`, dan `remove_dir` yang tidak pernah mengikuti symlink, dengan cek device lewat `MetadataExt::dev`; ia tidak tahan race, jadi test balapan hanya untuk `MacosFilesystem`.
- Engine saat start menghapus folder `operations/<uuid>/` hanya kalau operasi itu ada di registry, berstate akhir, dan (`succeeded` atau `point_of_no_return_at` kosong). Folder tanpa baris operasi atau dengan nama lain dibiarkan dan dicatat warning `scratch_leftover`.

### Start daemon dan pemulihan

Urutan di slot start spec 0006: buka registry (spec 0005), lalu `OperationEngine::recover`, lalu rekonsiliasi supervisor (spec 0010, diperkuat fitur 30), lalu `ready`, lalu `OperationEngine::start`.

`recover`:

1. Baca `unfinished_operations()`.
2. `running` dan `cancel_requested` dengan kebijakan `CloseInterrupted`: langkah `running` menjadi `interrupted`, lalu `interrupted { resumable: false }` dan langsung `resolve(Failed(..))` alasan `interrupted` tanpa kompensasi (atau `resolve(Cancelled)` kalau `cancel_requested_at` terisi), dalam satu batch yang juga mengosongkan `waiting_on` operasi antri yang menunggu operasi itu; key nya tidak masuk `LockTable`. `running` lain: langkah `running` menjadi `interrupted`. Kalau `journal.resumable`, `compensation_started_at` kosong, tidak ada langkah `failed`, factory jenisnya terdaftar, dan langkah terputus (kalau ada) `repeatable`: `interrupted { resumable: true }` ke `queued`. Selain itu `interrupted { resumable: false }` ke `needs_recovery` dengan hint dan `recovery_cause` `interrupted` dari *Aturan hint*.
3. `cancel_requested`: `interrupted` ke `needs_recovery`, hint `auto_compensate` kalau titik tak bisa dibalik belum dilewati, selain itu menurut *Aturan hint*.
4. `needs_recovery` dengan hint `auto_compensate` (crash di tengah kompensasi otomatis): tetap, diulang.
5. `queued` yang belum pernah mulai: bangun ulang lewat `WorkFactory`; factory tidak ada atau `RebuildError` berarti `fail` dengan alasan `interrupted` (`retryable` true) dan error `operation.interrupted`. `queued` hasil resume dibangun ulang dengan cara yang sama; yang gagal dibangun ulang masuk `needs_recovery` hint `manual` lewat `start` lalu `interrupted { resumable: false }` yang diterapkan di memori dan di commit dalam satu batch (state machine tidak punya jalan langsung dari `queued` ke `needs_recovery`).
6. Bangun ulang `LockTable` dari `operation_locks` untuk semua `needs_recovery`, lalu tutup operasi antri yang membutuhkan key itu seperti di *Lock dan antrian*.
7. Commit satu batch per operasi, bersihkan folder scratch menurut aturan di atas, dan catat `RecoveryReport` (jumlah dilanjutkan, dikompensasi otomatis, `needs_recovery`, antri, gagal dibangun ulang) di log info dan penghitung `get_stats`.

Dalam recovery mode registry (spec 0006 AC-18), `recover` dan `start()` tidak dijalankan: subsystem `operations` bernilai `unavailable` dengan detail `registry in recovery`, dan setiap method operasi dijawab `registry.recovery_mode`.

`start()` membuka dispatcher: pekerjaan kompensasi otomatis dan operasi antri dijalankan urut `(created_at, id)`, masing masing memakai satu slot. Kompensasi otomatis ditutup `resolve(Failed(failure))` dengan alasan `interrupted` (`retryable` true) dan error `operation.interrupted`, atau `resolve(Cancelled)` kalau `cancel_requested_at` terisi; kalau kompensasinya gagal, operasi tetap `needs_recovery` dengan hint `abandon`. Selama `recover` berjalan, `begin`, `cancel`, `resume`, dan `abandon` mendapat `daemon.starting` dengan subsystem `operations`; `get` dan `list` dilayani begitu registry siap.

### Shutdown

`OperationEngine::shutdown(OPERATION_SHUTDOWN_GRACE)` (10 detik) dipanggil spec 0006 setelah request baru ditolak, bersamaan dengan penantian request berjalan:

1. `begin` berikutnya mendapat `ShuttingDown` (`daemon.shutting_down`); dispatcher berhenti.
2. Operasi resumable yang `running`: tanda parkir; `begin_step` berikutnya, atau `check_cancel` dan `stopped` di dalam langkah `repeatable`, mengembalikan `Parked`, lalu engine meng commit `interrupted { resumable: true }` ke `queued`.
3. Operasi tidak resumable sebelum titik tak bisa dibalik: berhenti seperti cancel internal, dikompensasi, lalu `fail` dengan alasan `daemon_stopping` (`retryable` true) dan error `operation.daemon_stopping`. Pengecualian: operasi berkebijakan `CloseInterrupted` dibiarkan apa adanya (tanpa cancel, kompensasi, atau sinyal ke proses) dan ditutup `recover` pada start berikutnya. Runner nya diberi tanda berhenti (`begin_step`, `check_cancel`, dan `stopped` mengembalikan `Abandoned` tanpa menulis journal) dan shutdown tidak menunggunya, jadi grace tidak terbuang menunggu readiness dan restart policy tidak me respawn selama shutdown.
4. Operasi `cancel_requested` menyelesaikan kompensasinya, dan operasi setelah titik tak bisa dibalik dibiarkan selesai, keduanya dalam sisa grace.
5. Kembali saat semua runner selesai atau grace habis, dengan `ShutdownReport`. Yang masih berjalan dicatat terputus pada start berikutnya.

Sinyal kedua selama shutdown (spec 0006 langsung melompat ke pelepasan lock) tidak menunggu engine: operasi yang masih berjalan dibiarkan `running` di registry dan dipulihkan `recover` pada start berikutnya.

### Kegagalan dan error

- Pekerjaan yang panic ditangkap lewat `JoinError::is_panic`, dikompensasi (sebelum titik tak bisa dibalik), lalu `failed` dengan alasan `panicked` dan error `internal.unexpected`; setelah titik itu, `needs_recovery` dengan `recovery_cause` `panicked`.
- Pekerjaan yang kembali sukses sementara masih ada catatan `intended` diperlakukan sebagai bug: kompensasi lalu `failed` dengan alasan `step_failed` dan error `internal.unexpected`.
- Di `needs_recovery`, DTO tidak membawa `failure` (yang hanya untuk `failed`), melainkan `recovery.cause` dan `recovery.error` dari `recovery_cause` dan `failure_error` journal.
- Tulisan journal yang gagal (`NoSpace`, `Busy`, `Io`) membuat `begin_step`, `check_cancel`, dan pemanggilan context berikutnya mengembalikan `WorkError::Engine`; engine mengompensasi lalu mencoba commit state akhir setiap `JOURNAL_RETRY_INTERVAL` (5 detik) sampai berhasil atau daemon berhenti. `Unavailable` tetap fatal untuk daemon (spec 0005).
- Error akhir operasi disimpan sebagai `FailureError` dari `OperationError::code()`, pesan dan hint yang sudah lewat `Redactor` spec 0011, `details`, dan `items`. `stacklyd` membangun `ErrorBody` dengan mencari `ErrorSpec` kode itu di `catalog()`, mengisi `operationId`, dan `rule` bila ada. Kode yang tidak ada di katalog adalah bug: test katalog menjaganya, dan saat berjalan ia dipetakan ke `internal.unexpected` dengan baris log error.
- Dua arti retryable: `ErrorBody.retryable` mengikuti katalog (request yang sama tidak berefek), sedangkan `failure.retryable` di DTO operasi berarti operasi baru dengan key baru mungkin berhasil.

### Batas

| Konstanta (`stackly-core::operation::limits`) | Nilai |
| --- | --- |
| `MAX_RUNNING_OPERATIONS` | 8 |
| `MAX_QUEUED_OPERATIONS` | 64 |
| `PROGRESS_COMMIT_INTERVAL` | 1 detik |
| `OPERATION_SHUTDOWN_GRACE` | 10 detik |
| `JOURNAL_RETRY_INTERVAL` | 5 detik |
| `FINALIZE_RETRIES` | 5 |
| `MAX_STEPS`, `MAX_CREATED` | 256, 1024 |
| `INPUT_MAX`, `RESULT_MAX` | 16 384 byte |
| `CHECKPOINT_MAX`, `CUSTOM_PAYLOAD_MAX` | 4 096 byte |
| `REMOVE_TREE_MAX_DEPTH` (platform) | 128 |
| `OPERATION_LIST_LIMIT_DEFAULT`, `OPERATION_LIST_LIMIT_MAX` (protocol) | 50, 200 |

### Asumsi lintas spec

- **Assumed (spec 0004)**, diselaraskan 2026-10-18: `Filesystem::remove_tree` dari spec ini adalah satu satunya primitive hapus di port (tanpa `remove_file` dan `remove_empty_dir`), dipanggil hanya lewat `remove_owned_tree`; `check_owned_path`, `DirectoryLayout::instance_paths`, `instance_log_dir`, `instance_run_dir`, `environment_dir`, dan `operations_dir` tetap seperti spec 0004; `OperationKind::interrupt_policy()` ditambah ke domain bersama `resumable()`.
- **Assumed (spec 0005)**: migrasi `operation_journal` memakai versi bebas berikutnya dengan runner, sidik jari, dan fixture beku spec 0005, termasuk pengosongan key ganda dan backfill journal di *Model data*; `WriteOp`, baca (termasuk `operation_journals` batch), dan `RegistryError::IdempotencyKeyTaken` di *Port registry* ditambahkan ke trait `Registry` dengan urutan cek yang sama di fake dan SQLite; upsert memakai `ON CONFLICT DO UPDATE`; kolom `recovery_hint` dipetakan ke field domain baru; revision `Operation` naik lewat `touch` sekali per batch; prune menghapus baris anak lewat cascade, tetap menghitung hanya baris `operations`, dan memotong operasi per 500 baris per transaksi.
- **Assumed (spec 0006)**: area `operation` dengan kode di *Kode error*; `resource.busy` (detail `operationId`, kosong untuk pemegang internal), `resource.revision_mismatch`, dan `resource.not_found` dengan `ResourceKind::Operation` dipakai sesuai pemesanannya; event lewat `publish_resource` dengan resource operasi; `ClientInfo` di handshake mendapat field opsional `interactive: bool` (default false, minor naik) yang diisi CLI dari TTY dan mode noninteractive fitur 8 dan selalu true di GUI, dan context request yang diterima handler membawa `ClientKind` dan `interactive` dari handshake koneksi itu (juga diasumsikan spec 0011); slot start menjalankan `recover` setelah registry dibuka dan sebelum rekonsiliasi, lalu `start()` setelah `ready`; langkah berhenti memanggil `shutdown(OPERATION_SHUTDOWN_GRACE)` setelah langkah 3 bersamaan dengan langkah 4, dan langkah 5 menunggu keduanya; subsystem `operations` diambil alih fitur ini.
- **Assumed (spec 0011)**: `SecretText::serialize` menulis `"[secret]"` selama `stackly_protocol::fingerprint::redacting()` bernilai true; `SecretService` menyediakan probe keberadaan dan hapus reference yang dibuat operasi dengan aturan pemilik spec 0011, dan menganggap reference yang tidak ada sebagai sukses; `resolve_for_start` memakai `OperationTrigger::interaction()` dari journal, juga setelah restart; operasi yang berhenti dengan needs secret sebelum efek samping berakhir `failed` biasa lewat `OperationError` dengan kode spec 0011 yang mengikuti pola `area.alasan` spec 0006 (misalnya `secret.needs_secret`, sehingga lolos CHECK `errcode`); `Redactor` dipakai untuk `FailureError.message` dan nilai `details` sebelum disimpan.
- **Assumed (spec 0012)**: label Partial muncul saat `result.partial` tidak kosong; `MutationAvailability` untuk resource yang sedang dikerjakan membaca `operation.list` (`activeOnly`, `resource`) ditambah event operasi; abandon memakai pola konfirmasi spec 0012.
- **Untuk spec 0010**, diselaraskan 2026-10-18: supervisor memulai operasi lewat `begin` dengan `BusyPolicy::Queue` untuk start, create, dan reload pengguna, `BusyPolicy::Reject` untuk stop pengguna selama start berjalan (spec 0010 AC-7) dan untuk restart policy (`Origin::Background`); pemicu `restart_policy` dan `reconcile` dicatat di input journal; langkah spawn dan sesudahnya `reversible: false` tanpa kompensator yang menghentikan proses (cancel setelah spawn adalah stop biasa di dalam pekerjaan); operasi instance berkebijakan `CloseInterrupted`; supervisor memakai `LockTable::holder` untuk tahu apakah keluarnya proses milik operasi yang sedang berjalan dan `try_lock_internal` untuk menulis lifecycle setelah crash; rekonsiliasi berjalan di antara `recover` dan `start()`.
- **Untuk fitur 8 dan 13**: CLI membuat idempotency key UUID v7 lowercase per pemanggilan (bisa diganti `--idempotency-key`), GUI satu key per aksi pengguna; `--wait` memakai resep subscribe lalu snapshot spec 0006 dengan filter `resource` operasi; exit code operasi `failed` mengikuti `failure.error.category`, dan operasi `cancelled` keluar 9.

### Permukaan API

Semua method butuh handshake di koneksi milik uid yang sama (spec 0006), dan semuanya bisa mendapat kode `request.*`, `internal.unexpected`, `ipc.reply_too_large`, `daemon.starting` (subsystem `operations`), dan `daemon.shutting_down`.

| Method | Input | Output | Error utama |
| --- | --- | --- | --- |
| `operation.get` | `GetOperationParams { operationId: Uuid }` | `OperationDetail { operation: OperationSummary, steps: Vec<OperationStepInfo>, created: Vec<CreatedItemInfo> }` | `resource.not_found` |
| `operation.list` | `ListOperationsParams { activeOnly: bool, resource: Option<ResourceRef>, limit: Option<u32>, before: Option<String> }` (`resource` boleh `{ kind: manager, id: "self" }` untuk operasi `update_manager`; `ResourceKind` `manager` ditambah spec 0006) | `OperationPage { items: Vec<OperationSummary>, next: Option<String> }`; halaman berhenti sebelum JSON nya melewati 900 KiB | `request.invalid_params` (`limit` di luar 1 sampai 200, `before` tidak valid, `resource.kind` bukan runtime, instance, environment, atau manager) |
| `operation.cancel` | `CancelOperationParams { operationId }` | `CancelReply { operation: OperationSummary, disposition: CancelDisposition }` | `resource.not_found` |
| `operation.resume` | `ResumeOperationParams { operationId, expectedRevision: Option<Revision> }` | `OperationSummary` | `resource.not_found`, `resource.revision_mismatch`, `operation.not_resumable` |
| `operation.abandon` | `AbandonOperationParams { operationId, expectedRevision: Revision }` | `AbandonReply { operation: OperationSummary, leftovers: Vec<CreatedItemInfo> }` | `resource.not_found`, `resource.revision_mismatch`, `operation.not_abandonable` |
| `operation.get_stats` | tidak ada | `OperationStats { running, queued, needsRecovery, maxRunning, maxQueued, interruptedAtStart, resumedAtStart, autoCompensatedAtStart: u32, rejectedQueueFull: Count }` | tidak ada selain di atas |
| (method fitur lain yang memulai operasi) | params fitur, `idempotencyKey` wajib | `OperationAccepted { operation: OperationSummary, replayed: bool }` | `operation.idempotency_key_required`, `operation.idempotency_key_reused`, `operation.queue_full`, `operation.recovery_required`, `resource.busy`, `resource.revision_mismatch` |

Event (spec 0006 *Event dan cursor*): `operation.state_changed(OperationSummary)` dan `operation.progress(OperationSummary)`, keduanya lewat `publish_resource` dengan `ResourceRef { kind: operation, id }` dan revision operasi.

DTO di `stackly-protocol::operation` (JSON camelCase):

- `OperationSummary { id: Uuid, kind: OperationKind, resource: Option<ResourceRef>, state: OperationState, stage: Option<String>, progress: OperationProgress, waitingOn: Option<Uuid>, cancelRequestedAt: Option<Timestamp>, pointOfNoReturnAt: Option<Timestamp>, failure: Option<OperationFailureInfo>, recovery: Option<RecoveryInfo>, result: Option<OperationResult>, trigger: OperationTriggerInfo, createdAt: Timestamp, startedAt: Option<Timestamp>, updatedAt: Timestamp, revision: Revision }`. `resource` null untuk jenis `update_manager`.
- `OperationKind`: enum terbuka, kode spec 0004 plus fallback `unrecognized` (spec 0006). `OperationState`: enum tertutup tujuh nilai PRD 18.2. `OperationProgress`: enum berdata tertutup `indeterminate` atau `counted { done: Count, total: Count }`.
- `OperationFailureInfo { reason: String, retryable: bool, error: ErrorBody }`, hanya untuk state `failed`.
- `RecoveryInfo { hint: RecoveryHint, cause: RecoveryCause, error: Option<ErrorBody>, actions: Vec<RecoveryAction> }`, hanya untuk `needs_recovery`; `RecoveryHint` terbuka (`auto_compensate`, `resume`, `abandon`, `manual`, `unrecognized`); `RecoveryCause` terbuka (`interrupted`, `step_failed`, `compensation_failed`, `panicked`, `unrecognized`); `RecoveryAction` terbuka (`resume`, `abandon`, `unrecognized`).
- `OperationResult { resources: Vec<ResourceRef>, partial: Vec<PartialItem> }`; `PartialItem { item: String, resource: Option<ResourceRef>, outcome: PartialOutcome, code: Option<String> }`; `PartialOutcome` terbuka (`done`, `failed`, `skipped`, `left_over`, `unrecognized`).
- `OperationTriggerInfo { origin: TriggerOrigin, client: Option<ClientKind> }`; `TriggerOrigin` terbuka (`user`, `background`, `unrecognized`).
- `OperationStepInfo { seq: u32, name: String, state: StepState, repeatable: bool, reversible: bool, attempts: u32, startedAt: Timestamp, finishedAt: Option<Timestamp>, errorCode: Option<String> }`; `StepState` terbuka. Checkpoint tidak pernah dikirim.
- `CreatedItemInfo { kind: String, reference: String, state: CreatedState, detail: Option<String> }`; `CreatedState` terbuka. Payload kustom tidak pernah dikirim.
- `CancelDisposition` terbuka: `cancelled_before_start`, `requested`, `continuing_past_point_of_no_return`, `already_final`, `recovery_required`, `unrecognized`.
- Cursor `before` dan `next`: teks opaque `<created_at 30 karakter>_<uuid>` (waktu dengan 9 digit pecahan dari registry), paling banyak 80 karakter; client mengembalikannya apa adanya.

### Kode error

Area `operation` di `stackly-protocol::codes::operation` (semua `retryable` false kecuali disebut):

| Kode | Kategori | Kapan | Kunci `details` |
| --- | --- | --- | --- |
| `operation.idempotency_key_required` | `invalid_input` | method yang memulai operasi tanpa key | `method` |
| `operation.idempotency_key_reused` | `invalid_input` | key sama, sidik jari berbeda | `operationId`, `method` |
| `operation.queue_full` | `unavailable`, retryable | antrian penuh | `max` |
| `operation.recovery_required` | `conflict` | key dipegang operasi `needs_recovery` | `operationId`, `lockKey` |
| `operation.not_resumable` | `conflict` | resume ditolak | `operationId`, `reason` (`wrong_state`, `kind_not_resumable`, `step_not_repeatable`, `no_factory`, `compensation_failed`, `auto_compensating`) |
| `operation.not_abandonable` | `conflict` | abandon ditolak | `operationId`, `state`, `reason` (`wrong_state`, `auto_compensating`) |
| `operation.payload_too_large` | `internal` | payload journal melewati batas (bug pemanggil) | `field`, `max` |
| `operation.journal_full` | `internal` | lebih dari 256 langkah atau 1024 catatan | `kind`, `max` |
| `operation.interrupted` | `failed` | error akhir operasi yang ditutup setelah terputus | |
| `operation.daemon_stopping` | `unavailable` | error akhir operasi yang dihentikan shutdown | |
| `operation.abandoned` | `failed` | error akhir operasi yang di abandon | `leftovers` |
| `operation.compensation_failed` | `failed` | error yang dicatat saat kompensasi gagal | `item`, `detail` |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| `begin` | ID operasi | `OperationId::generate()` di engine |
| `begin` | `created_at`, `updated_at`, semua waktu journal | `Clock::now` dari `Platform` |
| `begin` | `kind`, `resource`, `creates_resource`, `extra_locks`, `busy`, `expected_revision`, `input`, pekerjaan | `BeginRequest` dari handler fitur pemilik; `input` diserialize handler dari params tanpa field secret |
| `begin` | ID resource baru (`creates_resource`) | handler: `InstanceId::generate()` diulang sampai short key nya tidak ada di `Registry::known_instance_ids`, atau `RuntimeId::generate()`, `EnvironmentId::generate()` |
| `begin` | `expected_revision` di journal | `BeginRequest.expected_revision` |
| `begin` | `idempotency_key` | `Request.idempotencyKey` (spec 0006) lewat `start_operation` |
| `begin` | `method` | tag `Call` request (`Call::METHODS`) |
| `begin` | `fingerprint` | `request_fingerprint(&Call)` |
| `begin` | `trigger.client` | `ClientInfo.kind` dari handshake koneksi (spec 0006) |
| `begin` | `trigger.interactive` | `ClientInfo.interactive` dari handshake koneksi (Assumed spec 0006); `false` untuk latar |
| `begin` | `trigger.origin` | `Origin::Request` berarti `user`, `Origin::Background` berarti `background` |
| `begin` | `resumable` | `OperationKind::resumable()` saat dibuat |
| `begin` | revision resource saat ini | `Registry::runtime`, `instance`, atau `environment` sesuai `OperationResource` |
| `begin` | lock key resource | `LockKey::for_resource(resource)` |
| `begin` | `Replayed` atau `KeyReused` | `operation_by_idempotency_key` dan perbandingan sidik jari |
| dispatcher | `waiting_on` | penghalang tertua menurut `(created_at, id)` di antara pemegang key di `LockTable` dan penunggu antri yang lebih tua |
| dispatcher | cek ulang revisi | `expected_revision` journal dibanding revision resource yang dibaca setelah lock diambil |
| restart | pekerjaan operasi antri | `WorkFactory::rebuild` dari `input` dan `JournalView` |
| runner | `started_at` | `at` event `start` pertama (domain) |
| langkah | `seq`, `attempts` | urutan `begin_step` per operasi; `attempts` dari baris langkah yang sama ditambah 1 |
| langkah | `point_of_no_return_at` | `Clock::now` saat `begin_step` langkah `reversible: false` pertama |
| langkah | `checkpoint` | argumen `finish_step` dari pekerjaan |
| progress | `stage`, `progress` | `set_stage`, `set_progress` dari pekerjaan |
| catatan dibuat | ID instance atau environment | `InstanceId::generate()` atau `EnvironmentId::generate()` di `new_instance_id` dan `new_environment_id` |
| catatan dibuat | path yang dihapus kompensasi | `DirectoryLayout` dari ID di catatan, tidak pernah dari path tersimpan |
| catatan dibuat | pemilik instance root | `createdByOperation` di `instance-manifest.json` (spec 0004) |
| catatan dibuat | short key yang dipakai instance lain | `Registry::known_instance_ids` |
| manifest instance | `createdByOperation`, `byOperation` | `OperationContext::id()` |
| cancel | `cancel_requested_at` | `Clock::now` saat `cancel` |
| cancel | disposisi | state operasi dan `point_of_no_return_at` saat itu |
| akhir | `failure.reason` | konstanta alasan engine |
| akhir | `failure.error` (kode, pesan, details) | `OperationError` pekerjaan atau kode `operation.*` engine; pesan lewat `Redactor` |
| akhir | `failure.retryable` | `OperationError::retryable()`, atau aturan engine (`daemon_stopping` true, `interrupted` true, lainnya false) |
| akhir | `ErrorBody.category`, `ErrorBody.retryable` | `catalog()` untuk kode itu (spec 0006) |
| akhir | `result` | `WorkOutcome.result`, atau sisa abandon |
| kompensasi | `compensation_started_at` | `Clock::now` di commit yang memulai kompensasi |
| pemulihan | hint dan aksi | *Aturan hint* atas journal dan registry factory |
| pemulihan | `recovery_cause`, `recovery.error` | sebab masuk `needs_recovery` (terputus, langkah gagal, kompensasi gagal, panic) dan `failure_error` penyebabnya |
| abandon | `PartialOutcome` tiap sisa | `compensated` ke `done`, `compensation_failed` ke `failed`, `not_owned` ke `skipped`, `created` atau `intended` ke `left_over` |
| `get_stats` | angka berjalan, antri, `needs_recovery` | state engine di memori |
| `get_stats` | batas | `EngineLimits` (konstanta di atas) |
| `get_stats` | penghitung start | `RecoveryReport` |
| `list` | halaman | `Registry::operations(OperationQuery)` (spec 0005) dari `activeOnly`, `resource`, `limit`, dan cursor yang didecode, lalu `operation_journals` untuk ID halaman itu |
| `list` | batas byte halaman | ukuran JSON hasil encode tiap ringkasan, dijumlah sampai 900 KiB |
| DTO | `revision` | `Operation::revision` setelah commit |
| DTO | waktu | `jiff::Timestamp` dipotong ke 3 digit pecahan (spec 0006) |
| event | `resource`, `revision` | `publish_resource(kind, ResourceRef { operation, id }, revision)` |
| log | `operation_id`, `kind` | span tracing runner dan field di baris request |

### Key invariants

- Hanya engine yang menulis baris `operations` dan tabel journal; setiap baris operasi punya satu penulis, dan setiap batch yang menyentuh operasi atau baris anaknya menaikkan revision operasi tepat 1 lewat `Operation::touch`.
- Cancel, titik tak bisa dibalik, dan finalize sebuah operasi diputuskan berurutan di bawah mutex penulis operasi itu, sehingga tidak ada urutan kejadian yang menghasilkan `cancelled` setelah langkah tidak bisa dibalik dimulai, atau `succeeded` setelah cancel yang diterima sebelum titik itu.
- Tidak ada operasi antri yang menunggu tanpa batas: key yang dipegang operasi `needs_recovery` menggagalkan penunggunya, dan operasi antri yang tidak bisa dibangun ulang setelah restart ditutup.
- Operasi yang kompensasinya sudah dimulai tidak pernah dilanjutkan (resume).
- Sebuah lock key dipegang paling banyak satu operasi atau satu pemegang internal. Operasi `queued` tidak memegang key; operasi `needs_recovery` tetap memegang key nya sampai di resume atau di abandon.
- Tidak ada transaksi registry yang hidup selama `await`, dan lock tidak pernah disimpan sebagai mutex di SQLite (REQ-048).
- `succeeded` hanya setelah batch akhir ter commit; `cancelled` hanya saat tidak ada langkah tidak bisa dibalik yang sudah dimulai.
- Langkah dengan `repeatable: false` dijalankan paling banyak sekali.
- Kompensasi tidak pernah menghapus path selain folder scratch operasi itu dan path layout dari ID yang dibuat operasi itu, selalu lewat `check_owned_path` dan `remove_tree` yang tidak mengikuti symlink, dan tidak pernah berjalan otomatis setelah titik tak bisa dibalik (BR-023, VAL-007).
- Satu idempotency key menunjuk paling banyak satu baris operasi.
- Payload journal, sidik jari, log, dan event tidak pernah memuat nilai secret.
- Event terbit hanya setelah commit, dengan revision hasil commit itu.
- Paling banyak 8 operasi berjalan dan 64 antri (NFR-004).

### Security model

- Batas kepercayaan adalah user lokal yang sama, yang sudah ditegakkan spec 0006 lewat uid peer. Semua client (CLI, GUI) setara: siapa pun boleh membaca, membatalkan, melanjutkan, dan meng abandon operasi apa pun, termasuk operasi latar. Trigger dan jenis client tercatat untuk audit fitur 35.
- Abandon wajib membawa `expectedRevision` supaya hanya berlaku atas keadaan yang benar benar dilihat pengguna; GUI memakai konfirmasi spec 0012.
- Kompensasi dan purge hanya menghapus lewat `remove_owned_tree`, yang menolak path di luar owned root dan tidak mengikuti symlink di komponen mana pun, sehingga registry atau folder yang dirusak tidak bisa mengarahkan hapus ke luar. Reference secret dan item kustom hanya dikompensasi kalau terbukti milik operasi itu (turunan ID nya dan belum ada sebelum niat dicatat, atau dibuktikan kompensatornya).
- Nilai secret tidak pernah masuk journal: sidik jari mengganti `SecretText` dengan penanda tetap, pekerjaan membangun payload dari tipe tanpa field secret, dan pesan error lewat `Redactor` spec 0011.

### Configuration required

Tidak ada env var atau credential baru. Semua batas adalah konstanta di *Batas*.

### Menjalankan test di Mac ini

- Unit test engine di `stackly-core` memakai `FakeRegistry`, `FakePlatform`, `testing::ScriptedWork`, dan jam tokio yang dijeda (`tokio::time::pause`).
- Test storage, `remove_tree` macOS, dan daemon memakai `TestRoot` dan `SqliteRegistry` nyata; daemon in process lewat `Daemon::start(deps)` dengan client IPC nyata.
- Test balapan `remove_tree` dan pengukuran NFR-006 berjalan di Mac ini dan dicatat sebagai evidence `fixture`; angka waktu diambil dari run itu, bukan dari target lain.

### Critical test scenarios

- Happy path: `begin` lalu `ScriptedWork` tiga langkah dengan stage dan progress lalu `finalize`, event `queued`, `running`, progress, dan `succeeded` urut revision, memverifikasi **AC-1**, **AC-13**.
- Idempotency: 16 task dengan key sama, satu efek samping; key sama dengan params lain ditolak; dua request yang hanya berbeda nilai secret memberi sidik jari sama; replay tetap berhasil saat antrian penuh; key bebas lagi setelah prune, memverifikasi **AC-2**, **AC-3** (evidence REQ-048, AC-048, NFR-005).
- Lock dan antrian: start lalu stop pada instance yang sama berurutan dengan `waitingOn`; dua instance berbeda paralel; `Reject` mendapat `resource.busy`; operasi kesembilan antri dan ke 65 ditolak, memverifikasi **AC-4**, **AC-5**.
- Client putus: task request dibatalkan, operasi tetap selesai, memverifikasi **AC-6**.
- Cancel: lima disposisi plus skenario restore palsu dengan sentinel target, memverifikasi **AC-7** (evidence REQ-054, AC-054).
- Kompensasi: symlink keluar di pohon instance, manifest operasi lain, short key bersama, kompensasi ganda, memverifikasi **AC-8** (evidence BR-023, VAL-007, VAL-015).
- Hapus aman: conformance `remove_tree` dan test balapan 2 detik di macOS, memverifikasi **AC-9** (evidence VAL-007).
- Restart: daemon dihentikan paksa saat langkah repeatable, langkah tidak repeatable, sebelum dan sesudah titik tak bisa dibalik, di tengah kompensasi, dan dengan operasi antri yang belum mulai, lalu start ulang dengan factory terdaftar dan tidak, memverifikasi **AC-10**, **AC-11**, **AC-15**.
- `CloseInterrupted`: operasi `start_instance` fake yang terputus setelah spawn (daemon dihentikan paksa, lalu lewat shutdown biasa) ditutup `failed interrupted` oleh `recover` tanpa kompensasi dan tanpa sinyal ke proses, waiter antri nya dilepas, dan versi `cancel_requested` nya ditutup `cancelled`; kegagalan pekerjaan setelah titik tak bisa dibalik selesai `failed`, bukan `needs_recovery`; `precheck` `NoOp` dengan batch mengikat key dan replay mengembalikan jawaban yang sama. Memverifikasi **AC-10**, **AC-14**.
- Tanpa mutex SQLite: langkah tertahan 5 detik sementara 50 commit dan baca lain selesai cepat, memverifikasi **AC-12** (evidence REQ-048).
- Shutdown: tiga kelompok operasi di bawah grace, memverifikasi **AC-14**.
- Revisi basi: `expected_revision` berubah selama antri, memverifikasi **AC-15** (evidence VAL-015).
- Gagal keras: panic dan `fail_next_commit(NoSpace)`, kelengkapan katalog, memverifikasi **AC-16**.
- Registry: migrasi, fixture, conformance, cascade prune, memverifikasi **AC-17**.
- Kontrak: fixture dan test Swift, memverifikasi **AC-18**.
- Observability dan paging: span log, pindaian sentinel log, `list` 200 operasi, memverifikasi **AC-19**, **AC-20** (evidence NFR-011).
- Otorisasi: tidak ada peran; peer uid lain sudah ditolak spec 0006 sebelum method apa pun, jadi tidak ada test izin tambahan di fitur ini.

## Build plan

Urutan mengikuti pendekatan Journey: satu jalur dibangun lengkap dulu, yaitu "mulai operasi, jalankan langkah, lihat progress, batalkan dan bersihkan, lalu bertahan saat daemon restart", di atas fake, baru kemudian disambungkan ke registry SQLite dan wire. Primitive hapus aman berdiri sendiri dan bisa dibangun paling awal.

**Milestone 1: Journal di registry** (butuh spec 0005 milestone 2 untuk langkah 1 dan 2, dan milestone 3 untuk langkah 3)

1. Domain: modul `operation::journal` (semua tipe di *Model domain*, termasuk `RecoveryCause`), `LockKey`, `RecoveryHint` dan field `recovery_hint` di `Operation` (termasuk `restore`), `Operation::touch`, `Outcome::Failed(OperationFailure)`, `OperationKind::resumable()` dan `OperationKind::interrupt_policy()`, konstanta alasan kegagalan, doc `CompensationFailed`, dan tambahan trait `Registry` (`WriteOp`, baca termasuk `operation_journals`, `IdempotencyKeyTaken`). Memenuhi **AC-2**, **AC-10**, **AC-11**, **AC-17**.
2. Testkit: `FakeRegistry` untuk tabel journal, unique key, upsert, dan cascade prune; kasus baru `registry_conformance`. Memenuhi **AC-2**, **AC-17**.
3. Storage: migrasi `NNNN_operation_journal.sql` (pengosongan key ganda, unique index, empat tabel, backfill journal), pemetaan baris, baca, `WriteOp` dengan `ON CONFLICT DO UPDATE`, prune per 500 operasi, fixture `registry-v<N>.sql`, test migrasi maju dari fixture yang memuat key ganda dan operasi lama, dan conformance di `SqliteRegistry`. Memenuhi **AC-17**.

**Milestone 2: Primitive hapus aman** (bisa mulai sekarang)

4. Platform: `Filesystem::remove_tree`, `RemoveReport`, varian `PlatformError`, `FakeFilesystem::remove_tree` (penelusuran portabel tanpa mengikuti symlink), dan pemeriksaan di `fs_conformance`. Memenuhi **AC-9**.
5. `stackly-platform-macos`: `MacosFilesystem::remove_tree` berbasis file descriptor dengan blok `unsafe` ber `// SAFETY:`, conformance, test `CrossesDevice` lewat disk image `hdiutil` (`BLOCKED` kalau attach gagal), dan test balapan 2 detik dengan evidence VAL-007. Memenuhi **AC-9**.
6. Core: `remove_owned_tree` di modul `ownership` dengan test path di luar root, root itu sendiri, dan symlink. Memenuhi **AC-8**, **AC-9**.

**Milestone 3: Engine dari ujung ke ujung di atas fake** (butuh langkah 1 dan 2; kompensasi butuh langkah 4)

7. `LockTable`, dispatcher FIFO dengan entri tertunda, batas, lock internal, `holder`, penghalang tertua, batch `waiting_on` per pass, cek ulang revisi saat mulai, ulang commit `start`, dan penutupan penunggu key `needs_recovery`. Memenuhi **AC-4**, **AC-5**, **AC-15**, **AC-16**.
8. `begin`: gerbang begin, urutan cek, idempotency lewat registry, batas payload, `creates_resource`, `expected_revision`, batch awal, bagian kritis di task engine, `OperationView`, dan `stats`. Memenuhi **AC-1**, **AC-2**, **AC-3**, **AC-6**, **AC-15**.
9. Runner dan `OperationContext`: langkah, titik tak bisa dibalik, stage, penulis progress leading dan trailing, mutex penulis per operasi, catatan niat dan `claim_resource`, scratch, `finalize` dengan ulang generation, sink event, panic, catatan `intended` saat sukses, dan ulang tulisan journal. Memenuhi **AC-1**, **AC-7**, **AC-12**, **AC-13**, **AC-16**.
10. Cancel dan kompensasi: tujuh disposisi, `compensation_started_at` dan flag klaim, kompensator bawaan, registry kompensator, aturan secret dan kustom, urutan, commit per catatan, idempotensi, aturan `not_owned`, dan `compensator_conformance`. Memenuhi **AC-7**, **AC-8**.
11. `recover`, pembangunan ulang operasi antri lewat `WorkFactory`, kompensasi otomatis, `resume`, `abandon` dengan aturan sisa, `shutdown` (termasuk `cancel_requested` dan sinyal kedua), dan pembersih scratch. Memenuhi **AC-10**, **AC-11**, **AC-14**.
12. `testing::ScriptedWork`, `ScriptedFactory`, dan test evidence `fixture`: REQ-048, AC-048, NFR-005, REQ-054, AC-054 (restore palsu), BR-023, VAL-015. Memenuhi **AC-2**, **AC-7**, **AC-8**, **AC-15**.

**Milestone 4: Kontrak wire dan daemon**

13. Protocol: DTO dan params `operation`, `EventKind` baru, `OperationAccepted`, `STARTING_METHODS`, `codes::operation`, `fingerprint` (penanda `SecretText` diberi `// provisional: F11` kalau spec 0011 milestone 1 belum masuk), `ClientInfo.interactive` kalau build spec 0006 belum menambahkannya, `Validate` untuk `ListOperationsParams`, `cargo xtask codegen`, fixture, test kontrak Swift, dan minor protokol naik. Memenuhi **AC-3**, **AC-18**, **AC-20**.
14. `stacklyd`: engine di slot start dan `start()` setelah siap, enam handler, peta DTO, `ErrorBody` dari `FailureError` lewat katalog, sink event ke `publish_resource`, subsystem `operations`, hook shutdown, `start_operation` dengan cek key dan sidik jari, kompensator `secret_ref` (setelah spec 0011 milestone 2), dan span log. Memenuhi **AC-1**, **AC-3**, **AC-5**, **AC-6**, **AC-11**, **AC-14**, **AC-19**, **AC-20**.
15. Test integrasi daemon di `apps/daemon/tests/operations.rs`: event dan revision lewat client IPC nyata, cancel, resume, dan abandon lewat wire, restart di tengah operasi, pengukuran NFR-006, pindaian sentinel registry dan log, kelengkapan katalog, dengan evidence NFR-006 dan NFR-011. Memenuhi **AC-1**, **AC-6**, **AC-10**, **AC-11**, **AC-13**, **AC-14**, **AC-16**, **AC-19**.

Sesi paralel: milestone 2 tidak menyentuh file milestone lain dan bisa menjadi sesi sendiri sekarang juga. Langkah 13 hanya menyentuh `stackly-protocol`, fixture, dan Swift, jadi bisa berjalan paralel dengan milestone 3 setelah spec 0006 milestone 1 masuk. Langkah 3 (SQLite) bisa paralel dengan milestone 3 karena engine diuji di atas fake. Langkah 14 dan 15 menunggu milestone 3, langkah 13, dan spec 0006 milestone 4.

## Consequences

**Positive**:

- Satu jalan untuk semua mutasi: fitur 10, 18, 20, 46, 49, 51, dan 54 tinggal menulis `OperationWork` dengan langkah bertanda, tanpa menulis ulang lock, antrian, cancel, kompensasi, atau resume.
- GUI dan CLI yang bersamaan tidak bisa menghasilkan efek ganda: key wajib, lock di memori, CAS, dan commit akhir atomik saling menjaga.
- Crash di mana pun meninggalkan journal yang menjelaskan apa yang sudah terjadi, apa yang bisa dibersihkan, dan apa yang perlu tangan pengguna.
- Hapus tidak pernah bisa diarahkan ke luar owned root lewat symlink, termasuk saat race, dan purge fitur 54 memakai primitive yang sama.

**Negative / tradeoffs**:

- Setiap langkah, catatan niat, dan perubahan tampilan adalah satu commit SQLite dengan `fullfsync`, jadi operasi dengan banyak langkah kecil lebih lambat; fitur sebaiknya memakai langkah besar yang bermakna.
- Semua `begin` lewat satu gerbang, jadi throughput pembuatan operasi dibatasi satu commit per `begin` (cukup untuk alat lokal, tidak untuk ribuan per detik).
- Kompensasi hanya untuk resource yang dimiliki karena konstruksi; fitur yang menyentuh path lain harus memakai pola commit atomik, dan itu menuntut disiplin desain di fitur 18, 51, dan 59.
- Pekerjaan yang tidak pernah memanggil `check_cancel` tidak bisa dibatalkan sampai selesai; engine tidak mematikan paksa.
- Operasi `needs_recovery` mengunci resource nya sampai pengguna bertindak, jadi pesan dan aksi pemulihan di CLI dan GUI harus jelas.
- `remove_tree` menambah satu blok `unsafe` baru di `stackly-platform-macos` dan perlu review keamanan fitur 63.
- Progress paling cepat sekali per detik, sehingga progress bar terlihat melompat.
- Setiap jenis operasi wajib menulis `input` yang cukup untuk membangun ulang pekerjaannya dan mendaftarkan `WorkFactory`; operasi yang params nya memuat nilai secret tidak bisa bertahan melewati restart selagi masih antri, dan ditutup `failed` (retryable) kalau daemon mati sebelum ia mulai.

**Neutral**:

- Empat tabel baru dan satu migrasi; trait `Registry` dan `Filesystem` bertambah method.
- `stackly-core` mulai memakai tokio dan tracing; `stackly-protocol` mendapat `sha2`.
- Daemon mendapat subsystem `operations` yang bisa `degraded`, dan `stackly daemon status` akan menampilkan operasi yang perlu dipulihkan.

## Follow-up

Perubahan untuk spec 0004 (dicatat di sini, tidak ditulis langsung karena sesi paralel; diterapkan lewat `/architect` update spec 0004 pada pass rekonsiliasi):

- [x] *Port platform*: tambah `Filesystem::remove_tree(root, relative) -> RemoveReport` dengan semantik di *Kompensasi dan primitive hapus* (tidak pernah mengikuti symlink di komponen mana pun, tidak menyeberang device, kedalaman paling banyak 128), varian `PlatformError` `SymlinkInPath`, `NotADirectory`, `CrossesDevice`, `TooDeep`, implementasi `FakeFilesystem` (penelusuran portabel, tidak tahan race) dan `MacosFilesystem` (berbasis file descriptor), dan pemeriksaan `fs_conformance`. Perbarui key invariant "tidak ada operasi yang menghapus file": hapus hanya lewat `remove_file`, `remove_empty_dir`, dan `remove_tree`, dan hapus rekursif hanya lewat `remove_owned_tree` di core. Diterapkan di spec 0004 (2026-10-18). Ronde 2: `remove_file` dan `remove_empty_dir` dibuang; `remove_tree` lewat `remove_owned_tree` satu satunya jalur hapus.
- [x] *Fungsi core*: `remove_owned_tree(platform, layout, path)` di modul `ownership`, dipakai kompensasi fitur 7 dan purge fitur 54. Diterapkan di spec 0004 (2026-10-18).
- [x] *Model domain*: modul `operation::journal` (`OperationJournal`, `JournalStep`, `StepState`, `StepSeq`, `CreatedItem`, `CreatedRecord`, `CreatedState`, `CreatedSeq`, `OperationTrigger`, `TriggerOrigin`, `FailureError`, `JournalPayload`, `JournalView`, `RecoveryCause`), value type `MethodName`, `ErrorCodeName`, `CompensatorName`, `DetailKey`, `ClientCode`, `LockKey`, dan `RecoveryHint`. Diterapkan di spec 0004 (2026-10-18).
- [x] *Model domain*: field `recovery_hint` di `Operation` dengan `set_recovery_hint` dan pengosongan otomatis, parameter baru di `Operation::restore`, `Operation::touch(at)` sebagai satu satunya jalan menaikkan revision operasi (sekali per batch; mutator lain tidak menaikkannya), `OperationKind::resumable()` dengan tabel spec 0007, dan konstanta `FailureCode` `STEP_FAILED`, `ABANDONED`, `DAEMON_STOPPING`, `PRECONDITION_FAILED`, `PANICKED`. Diterapkan di spec 0004 (2026-10-18).
- [x] *State operation*: `Outcome::Failed` membawa `OperationFailure` dan `apply(Resolve(..))` mencatatnya (supaya CHECK spec 0005 untuk `failed` terpenuhi); doc `compensation_failed` mencakup "tidak mungkin karena langkah tidak bisa dibalik sudah berjalan"; kalimat "`resume` hanya dipakai engine untuk operasi yang menyatakan bisa dilanjutkan (fitur 7)" menunjuk tabel resume spec 0007. Diterapkan di spec 0004 (2026-10-18).
- [x] *Invariant BR*: baris BR-023 menunjuk spec 0007 sebagai pemilik alur penuh (AC-8), dan *Value sourcing* manifest `createdByOperation` bersumber dari `OperationContext::id()`. Diterapkan di spec 0004 (2026-10-18).

Perubahan untuk spec 0001 (lewat `/architect` update spec 0001 pada pass rekonsiliasi):

- [x] *Batas keputusan*: baris "Journal operasi, lock, dan idempotency" sudah diputuskan spec 0007. Diterapkan di spec 0001 (2026-10-18).
- [x] *Peta crate*: `stackly-core` mulai memakai `tokio` (fitur `rt`, `sync`, `time`, `macros`) dan `tracing` untuk modul `operation`; `stackly-protocol` mendapat `sha2` untuk sidik jari request; `stackly-platform-macos` memakai `libc` untuk `openat` dan `unlinkat`. Diterapkan di spec 0001 (2026-10-18).

Untuk pemilik fitur lain:

- [x] Spec 0005 (pemilik fitur 5): catat tambahan port, migrasi, `IdempotencyKeyTaken`, aturan `touch` untuk revision operasi, dan potongan prune 500 operasi dari *Port registry* saat milestone 1 fitur ini masuk. Diterapkan di spec 0005 (2026-10-18).
- [x] Spec 0006 (pemilik fitur 6): catat `ClientInfo.interactive` di handshake (juga dibutuhkan spec 0011) dan urutan slot start serta shutdown dengan engine operasi. Diterapkan di spec 0006 (2026-10-18): `ClientInfo.interactive` (AC-19) dan urutan slot start serta shutdown.
- [ ] Fitur 8: command `stackly operation list`, `inspect`, `cancel`, `resume`, dan `abandon`, `--wait`, `--idempotency-key`, dan exit code dari `failure.error.category`. Spec 0008 menambah `resume` dan `abandon` (2026-10-18).
- [ ] Fitur 34: doctor menampilkan operasi `needs_recovery` beserta hint nya, dan folder scratch tanpa baris operasi (`scratch_leftover`).
- [ ] Fitur 35: baris audit untuk cancel, abandon, dan jenis operasi destruktif memakai `trigger` dari journal.
- [ ] Fitur 46: operasi environment tidak boleh menunggu operasi lain sambil memegang slot berjalan (risiko deadlock di batas 8); desain operasi anak diputuskan di sana.
- [ ] Fitur 63: review keamanan blok `unsafe` `remove_tree` dan test balapan.
- [ ] Fitur 64: benchmark boleh menyetel ulang `MAX_RUNNING_OPERATIONS`, `MAX_QUEUED_OPERATIONS`, dan `PROGRESS_COMMIT_INTERVAL` lewat update spec ini.

## Rationale

Alasan dan opsi yang dipertimbangkan ada di [rationale.md](rationale.md).
