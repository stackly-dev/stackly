# 0027. Log per instance

**Date**: 2026-11-01
**Status**: Accepted

## Summary

Spec ini menetapkan cara Stackly menyimpan dan menampilkan log tiap instance. Daemon sudah menangkap stdout dan stderr ke spool dan menyamarkan setiap baris (spec 0010); fitur ini menambah sink tahan lama (penyimpan di disk) yang menulis baris itu sebagai JSON Lines per stream, dengan nomor urut (`seq`) per instance, rotasi 5 file × 20 MiB per stream, penanda truncation, start, dan exit, serta buang duplikat yang tepat setelah daemon crash. Client membaca lewat dua method baru: `instance.logs_read` untuk halaman (Load Earlier, `--tail`) dan `instance.logs_follow` yang mendorong frame `log` dari disk, sehingga client yang lambat hanya tertinggal di disk dan tidak pernah menahan proses atau menumbuhkan memori. `stackly instance logs` dan layar Logs di GUI memakai dua method yang sama, jadi hasilnya setara.

## Requirements

**User stories**:

- Sebagai developer, saya ingin melihat log terbaru satu instance dan mengikutinya secara langsung dari GUI atau CLI, supaya saya tahu apa yang sedang dilakukan PostgreSQL atau NATS saya.
- Sebagai developer, saya ingin memisahkan stdout dan stderr dan mencari teks, supaya error cepat ketemu walau log ramai.
- Sebagai developer, saya ingin tahu kapan ada output yang hilang (dipotong, dirotasi, atau tidak terbaca), supaya saya tidak salah menyimpulkan dari log yang bolong.
- Sebagai developer, saya ingin log yang besar tidak membuat daemon lambat atau Mac kehabisan disk, supaya instance lain tetap jalan normal.
- Sebagai script, saya ingin `stackly instance logs --json --follow` dengan cursor yang bisa dilanjutkan, supaya saya bisa memproses log tanpa kehilangan atau mengulang baris.

**Acceptance criteria**:

- **AC-1** (penyimpanan): Setiap `LogBatch` yang diterima sink tahan lama ditulis ke segmen stream itu di `<logs>/instances/<id>/captured/` sebagai satu record JSON per baris (*Format record*). Setiap record punya `seq` yang unik untuk instance itu dan naik lintas kedua stream, lintas run proses, dan lintas restart daemon; `at` adalah `read_at` batch (waktu baca daemon, UTC, milidetik). Folder `captured/` bermode 0700 dan setiap file 0600.
- **AC-2** (rotasi): Segmen aktif ditutup di batas record begitu ukurannya mencapai `LOG_SEGMENT_MAX_BYTES` (20 MiB), lalu segmen baru dibuka dengan nama dari `seq` record pertamanya. Setiap stream menyimpan paling banyak `LOG_SEGMENTS_PER_STREAM` (5) segmen; yang tertua dihapus saat rotasi. stdout yang spam tidak pernah menghapus segmen stderr. Total per instance tidak pernah melebihi 2 × 5 × (20 MiB + satu record).
- **AC-3** (tidak menahan, memori terbatas): `accept` sink tahan lama tidak pernah memblokir. Antrian tulis dibatasi `LOG_WRITE_QUEUE_MAX` per instance dan `LOG_WRITE_QUEUE_TOTAL` untuk seluruh daemon; antrian penuh menjawab `Busy`, sehingga byte tetap di spool (spec 0010). Satu follower per frame tidak memegang lebih dari satu frame dan satu buffer baca di memori. Di bawah fake server yang menulis 5000 baris × 512 byte per detik selama 60 detik, dengan satu follower yang tidak membaca socket nya sama sekali, `daemon.get_info` tetap menjawab di bawah 100 ms (p99), memori fisik daemon tidak naik lebih dari 64 MiB dari baseline, dan fake tidak melambat (ukuran di *Critical test scenarios*, hanya mengikat di bawah `cargo xtask evidence run`).
- **AC-4** (tanpa duplikat setelah crash): Daemon yang dibunuh dengan `SIGKILL` saat fake menulis, lalu dijalankan lagi dan mengadopsi proses itu, tidak pernah menyimpan satu baris dua kali dan tidak menghilangkan baris yang masih ada di spool. Pembeda nya adalah `src { pid, startedAt, end }` di setiap record baris dari spool (*Buang duplikat*).
- **AC-5** (penanda): Spool yang membuang backlog (`truncated { bytes }`, spec 0010) menjadi record `truncated` di stream itu. Setiap spawn proses utama menulis `started { pid }`, setiap exit menulis `exited { code | signal }` setelah baris terakhir run itu, dan adopsi yang spool nya tidak bisa dibuka menulis `unavailable`. Pembacaan dengan cursor yang sudah dirotasi mendapat gap `rotated`; cursor dengan `storeId` lain mendapat gap `store_reset`.
- **AC-6** (`instance.logs_read`): Tanpa posisi, method mengembalikan halaman terbaru; dengan `before` atau `after`, halaman tepat sebelum atau sesudah cursor itu. Entri urut `seq` naik, gabungan kedua stream kecuali `streams` membatasi, dan record run yang ada di dua file muncul sekali. `contains` mencocokkan teks baris tanpa membedakan huruf besar kecil. `limit` 1 sampai 1000, default 200. Satu reply tidak melebihi `LOG_BATCH_MAX` kecuali isinya satu entri saja. Pembacaan berhenti setelah memindai `LOG_SCAN_BUDGET` byte, lalu reply membawa `scanLimited: true` dan cursor `earlier` atau `later` dari titik henti.
- **AC-7** (`instance.logs_follow`): Daemon menjawab `LogsFollowing { subscriptionId, storeId, head }` sebelum frame `log` pertama, lalu mendorong entri setelah cursor follower dengan filter yang sama dengan AC-6, dibaca dari disk setiap kali writer melaporkan baris baru. Follower yang lambat tertinggal tanpa kehilangan baris; kalau rotasi melewati cursor nya, ia menerima gap `rotated` lalu lanjut dari `seq` tertua yang masih ada. Follow tetap hidup melewati stop, crash, dan start instance; instance yang dihapus mengirim frame terakhir dengan `ended: instance_removed` lalu subscription selesai. Follow memakai penghitung `subscriptionId` dan batas `MAX_SUBSCRIPTIONS` (8) yang sama dengan `events.subscribe`; yang ke 9 ditolak `events.too_many_subscriptions`. Cursor `after` dengan `storeId` yang sama dengan store sekarang dan `seq` di atas `head` (atau di atas 0 untuk store tanpa record) ditolak `request.invalid_params` dengan field `after.seq` kode `out_of_range`, sama seperti cursor `events.subscribe` yang melewati head; `storeId` lain tetap mendapat gap `store_reset` *(keputusan 2026-11-05 dari audit keamanan, catatan 124, protokol 3.2)*. `instance.logs_unfollow` menghentikannya.
- **AC-8** (level): Setiap baris mendapat `level` saat ditulis, dari aturan kata kunci (*Aturan level*): baris PostgreSQL `ERROR:`, `FATAL:`, `PANIC:` dan baris NATS `[ERR]`, `[FTL]` menjadi `error`; `WARNING:` dan `[WRN]` menjadi `warning`; selainnya `normal`. stderr sendiri tidak pernah membuat baris jadi warning atau error.
- **AC-9** (secret): Fixture credential yang dicetak dummy server, termasuk yang terbelah tepat di batas potong `LOG_LINE_HARD_MAX` (64 KiB) baris tanpa newline, tidak pernah muncul di file segmen, reply `logs_read`, frame `log`, output CLI teks dan JSON, maupun teks yang disalin GUI.
- **AC-10** (CLI): `stackly instance logs <target>` menerima nama atau UUID lewat `TargetArg` dan mencetak `--tail` baris terakhir (default 100, 1 sampai 10 000). Ia juga menerima `--follow`/`-f`, `--stream all|stdout|stderr`, `--grep <teks>`, `--before <cursor>`, `--after <cursor>`, dan `--json` (record `item` per entri lalu satu `result`, spec 0008). Ctrl+C saat follow keluar 0 dan, dengan `--json`, menulis `result` berisi cursor terakhir. Instance yang tidak dikenal keluar 10 dengan `cli.instance_not_found`, sama seperti instance yang dihapus saat diikuti.
- **AC-11** (layar Logs): Tab Logs di detail instance mengikuti mockup: segmented All, stdout, dan stderr, field cari, toggle Follow, tombol Load Earlier, tombol Copy Shown Lines, dan footer `Cursor <seq> · Retained 5 files × 20 MiB per stream · Secrets are redacted before storage`. Kolom waktu memakai `HH:mm:ss.SSS` lokal, dengan baris pemisah hari saat tanggal berganti. Penanda tampil sebagai baris bersumber `stackly`. Layar menyimpan paling banyak 10 000 baris; state kosong, filter tanpa hasil, error baca, dan daemon offline masing masing punya tampilan (*Layar Logs*). Untuk instance, stream, dan filter yang sama, isi yang tampil sama dengan output `stackly instance logs`.
- **AC-12** (gagal tulis): Saat penulisan segmen gagal (disk penuh, izin, IO), sink menjawab `Busy` dan writer mencoba lagi setiap `LOG_WRITE_RETRY`; subsystem `instance_logs` di `daemon.get_health` menjadi `degraded` dengan alasan, lalu kembali `ok` 60 detik setelah tulis berhasil lagi. Proses instance tidak pernah tertahan. Kalau backlog spool melewati batasnya selama itu, record `truncated` mencatat kehilangannya.
- **AC-13** (pemulihan store): Pertama kali store satu instance disentuh setelah daemon start, baris terakhir yang terpotong di segmen terbaru dipangkas sampai newline terakhir sebelum menulis lagi, dan `seq` berikutnya diambil dari record terakhir yang utuh. Record rusak di tengah segmen dilewati dan dihitung di `skippedRecords`. File yang namanya tidak cocok dengan pola segmen diabaikan dan tidak pernah dihapus. Folder `captured/` yang hilang membuat store baru dengan `storeId` baru.
- **AC-14** (recovery mode): Ketiga method baru ditandai `recovery refused`.

## Decision

**Chosen option**: pilihan yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): sink tahan lama di `stackly-core` yang menulis JSON Lines per stream dengan satu `seq` per instance, rotasi konstanta 5 × 20 MiB, buang duplikat lewat offset spool, follow sebagai subscription yang membaca dari disk dengan frame `log` baru, filter substring di daemon, aturan level kata kunci, dan file log milik provider ditunda.

Sink tahan lama menjadi sumber tunggal untuk CLI dan GUI; memori daemon dibatasi oleh antrian tulis dan satu frame per follower, sedangkan backpressure jatuh ke spool yang sudah dibatasi spec 0010.

**Implementation skills**: `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `swiftui-expert-skill` dan `swift-concurrency` (lihat [apps/macos/AGENTS.md](../../../apps/macos/AGENTS.md))

## Feature design

### Trace PRD

| ID | Dipenuhi oleh |
| --- | --- |
| REQ-017 | AC-1 sampai AC-11 |
| AC-017 | AC-3, AC-2, AC-5 |
| NFR-004 | AC-3, AC-12, `skippedRecords`, subsystem `instance_logs` |
| PAGE-07 (bagian Logs) | AC-11 |
| PRD 35.2 (rotasi) | AC-2, *Konstanta* |

### Batas dengan spec lain

- **Spec 0010**: spool, pembaca, `LogSink`, `SinkSet`, dan `TailSink` tetap. Fitur ini menambah tiga hal ke sana (*Usulan update untuk spec lain*): `LogBatch.source` dan `LogLine.end` untuk buang duplikat, `LogSink::run_event` untuk penanda run, dan overlap redaksi di batas potong 64 KiB.
- **Spec 0006**: frame server baru `log`, penghitung subscription yang dipakai bersama, dan subsystem `instance_logs`.
- **Spec 0008**: perintah streaming pertama, dengan Ctrl+C saat follow keluar 0.
- **Spec 0009**: komponen pertama `logs.files` tidak boleh `captured`.
- **Spec 0011**: setiap baris sudah tersamar sebelum sampai sink; fitur ini menambah test dummy server dengan fixture credential (Follow-up spec 0011 untuk fitur 27).
- **Spec 0067**: `LogView` dan `LogLine` dipakai apa adanya; tidak ada perubahan API komponen.
- **Di luar**: file log yang ditulis provider sendiri (`RunPlan.log_files`), setting ukuran rotasi (fitur Settings), ekspor ke file, filter level atau regex, dan log daemon sendiri.

### Alur

1. Pembaca spool (spec 0010) menyerahkan `LogBatch` tersamar ke `SinkSet`. `DurableLogSink::accept` menghitung ukuran batch dan menjawab `Accepted` atau `Busy` menurut *Antrian tulis*.
2. Thread writer tunggal `stackly-logs-writer` mengambil batch per instance sesuai urutan masuk, membuka store instance itu (pemulihan AC-13 sekali per run daemon), membuang baris duplikat, memberi `seq`, menghitung `level`, menambah record ke segmen aktif, merotasi bila perlu, lalu menerbitkan `head` baru lewat `tokio::sync::watch` per instance.
3. `instance.logs_read` membaca segmen langsung dari disk di `spawn_blocking`.
4. `instance.logs_follow` memulai task yang menunggu `watch` instance, membaca entri setelah cursor nya dari disk, menyusun frame `log` sampai `LOG_BATCH_MAX`, lalu menunggu tempat di antrian keluar koneksi. Task itu satu satunya yang tertahan saat client lambat.
5. Supervisor memanggil `run_event(Started)` setelah spool baru dibuat dan sebelum proses dilanjutkan, `run_event(Exited)` di handler exit setelah pembaca spool menyerahkan semua batch run itu dan sink menerima semuanya (urutan drain yang sudah ada di spec 0010), dan `run_event(Unavailable)` saat adopsi menemukan spool tidak bisa dibuka. Run event masuk antrian yang sama dengan batasnya sendiri (*Antrian tulis*), jadi urutannya terhadap baris terjaga. Adopsi setelah daemon restart tidak menulis `started`, karena run nya sama; ini disengaja.

### Aturan rinci writer, pembaca, dan follow

#### Antrian tulis

- Setiap instance punya satu antrian untuk kedua stream. Antrian yang kosong selalu menerima satu batch walau batch itu lebih besar dari `LOG_WRITE_QUEUE_MAX` (satu chunk spool 1 MiB plus overhead record bisa melebihinya). Antrian yang tidak kosong menjawab `Busy` kalau batch baru membuat ukurannya melewati `LOG_WRITE_QUEUE_MAX`, atau total semua antrian melewati `LOG_WRITE_QUEUE_TOTAL`.
- Run event tidak dihitung ke batas byte, tetapi paling banyak `LOG_RUN_EVENTS_MAX` (64) per instance menunggu di antrian. Di atas itu, `Exited` yang datang tanpa `Started` di antaranya digabung dengan yang sebelumnya (yang terakhir menang), sehingga crash loop dengan disk mati tetap berbatas.
- `Busy` karena gagal tulis tidak diketahui saat `accept`; artinya selalu "antrian penuh". Saat tulis gagal, batch tetap di kepala antrian instance itu, jadi antrian mengisi lalu `accept` menjawab `Busy` dan byte tertahan di spool.

#### Thread writer

- Satu thread untuk semua instance, bergiliran per instance. Instance yang tulis atau buka store nya gagal di parkir dengan tenggat retry sendiri (`LOG_WRITE_RETRY`, dari `Clock`), dan thread lanjut melayani instance lain; tidak ada antrian yang tertahan oleh instance lain.
- Store yang gagal dibuka (izin, `captured/` bukan folder, header tidak bisa ditulis) diperlakukan sama: instance di parkir, antrian dipertahankan sampai batasnya, dicoba lagi; `logs_read` untuk instance itu menjawab `instance.logs_unreadable`. `store.json` yang tidak bisa diparse diperlakukan seperti hilang (header baru, `storeId` baru).
- Pemetaan `io::ErrorKind` ke `reason`: `StorageFull` dan `QuotaExceeded` → `no_space`; `PermissionDenied` → `permission_denied`; `NotADirectory` → `not_a_directory`; selainnya `io`. Reason yang sama dipakai di health dan di `instance.logs_unreadable`.
- `head` baru diterbitkan hanya setelah `write_all` record terakhir batch itu kembali. Tidak ada fsync per batch; pembaca melihat data lewat page cache. File segmen ditutup (dan di fsync) saat rotasi dan saat daemon berhenti.
- Hapus instance: daemon menyuruh writer membuang store instance itu (tutup handle, buang antrian, terbitkan `removed`) sebelum folder log instance dihapus.

#### Kehilangan saat crash

Saat adopsi, kalau `src.end` terakhir di store untuk run itu (pid dan `startedAt` sama) lebih kecil dari `first_data_offset()` spool, writer menulis `truncated { bytes: selisih }` di stream itu sebelum baris pertama yang dibaca ulang. Jadi baris yang sudah `Accepted` tetapi belum tertulis saat daemon mati tetap meninggalkan penanda.

#### Buang duplikat, kasus tepi

- Kalau segmen terbaru stream itu belum punya record ber `src` (baru saja dirotasi), pemulihan melihat satu segmen ke belakang.
- Tidak ada `src` tersimpan untuk run itu dan batch bertanda `resumed`: batch ditulis semua (kasus jarang, misalnya store yang baru dibuat saat proses diadopsi); tidak ada tebakan.
- Potongan `continued` dari satu baris logis memakai `end` yang sama (offset setelah baris logis itu); hanya potongan terakhir yang menggeser nilai tersimpan.
- Keluaran init tidak pernah dikirim ulang setelah crash, jadi ia tidak butuh `src`.

#### Pembaca

- Record terakhir segmen aktif yang belum diakhiri newline dianggap belum ada (tidak dihitung `skippedRecords`); hanya baris lengkap yang gagal diparse yang dihitung.
- Segmen yang hilang saat dibuka atau dibaca (`NotFound`) berarti rotasi atau penghapusan: pembaca lanjut dari segmen berikutnya yang ada dan melaporkan gap `rotated` kalau cursor jatuh di dalamnya, tidak pernah `instance.logs_unreadable`. Penghapusan file yang sedang terbuka aman di macOS (handle tetap bisa dibaca).
- Paling banyak `LOG_READ_CONCURRENCY` (4) pembacaan log berjalan bersamaan di seluruh daemon (semaphore di `ipc::logs`, di depan `spawn_blocking`), supaya spam log dengan banyak follower tidak menghabiskan pool blocking yang juga dipakai registry.

#### Merge dua stream

- Pembaca menjalankan dua cursor (stdout dan stderr) bersamaan dan selalu mengambil record dengan `seq` terkecil. Record yang `seq` nya sama dengan record terakhir yang dikeluarkan dilewati (penanda run yang ditulis ke dua file). Writer menjamin dua record berbeda tidak pernah berbagi `seq`.
- Segmen untuk sebuah `seq` dicari dari nama file (`firstSeq` terbesar yang tidak lebih dari `seq`) per stream, lalu pencarian biner di dalam segmen. `before` membaca mundur per chunk 64 KiB dan pindah ke segmen sebelumnya saat habis.
- `LOG_SCAN_BUDGET` adalah total untuk kedua stream. Saat habis, titik henti adalah frontier yang paling dekat ke posisi awal (yang lebih kecil untuk `after`, yang lebih besar untuk `before`), sehingga tidak ada record yang terlewat.
- `streams: [stderr]` juga menyembunyikan `truncated` milik stdout; ini disengaja.

#### Follow

- Setiap instance punya `watch` berisi `{ storeId, head, removed }`. Daemon menyetel `removed` saat instance dihapus, sehingga follow tidak bergantung pada event bus. Task follow memeriksa `removed` setiap bangun dan setelah setiap pembacaan.
- Task follow bangun paling cepat setiap `LOG_FOLLOW_MIN_INTERVAL` (250 ms), sehingga banyak batch kecil cukup satu pembacaan.
- Frame `log` menunggu tempat di antrian keluar koneksi yang sudah ada (`OUTBOUND_QUEUE_LEN` 64 frame). Client yang tidak membaca sama sekali membuat antrian penuh; setelah `WRITE_TIMEOUT` (30 detik) koneksinya ditutup dengan `ipc.write_timeout` (spec 0006). Reply lain di koneksi itu menunggu di belakang frame follow, sama seperti event. Daemon dan koneksi lain tidak terpengaruh.
- Frame tidak pernah dikirim tanpa `entries`, `gap`, maupun `ended`. Dalam satu frame, `gap` berlaku sebelum `entries` nya.
- Store kosong: `storeId` dan `head` null, dan follower mulai dari record pertama begitu store dibuat; `storeId` baru itu diadopsi tanpa gap `store_reset`. Gap `store_reset` hanya untuk cursor yang membawa `storeId` lain.
- `before` berarti `seq` lebih kecil dari cursor (tidak sama dengan), `after` berarti lebih besar; cursor boleh menunjuk `seq` yang sudah tidak ada.

### Lokasi kode

| Crate | Modul | Isi |
| --- | --- | --- |
| `stackly-core` | `logs` (`record`, `level`, `filter`, `store`, `writer`, `read`, `follow`, `sink`) | format record, aturan level, filter, store per instance (segmen, rotasi, pemulihan, buang duplikat), thread writer, pembaca halaman, sumber follow, `DurableLogSink`. Memakai `std::fs` seperti `instance_dir`, jadi tetap lolos `cargo check` untuk Windows |
| `stackly-core` | `supervisor` | `LogBatch.source`, `LogLine.end`, `LogSink::run_event` (default kosong), pemanggilan run event, overlap redaksi |
| `stackly-protocol` | `logs` (baru), `message`, `wire`, `daemon`, `codes`, `limits` | DTO, tiga method, `ServerFrame::Log`, `LogSeq`, subsystem `instance_logs`, kode `instance.logs_unreadable`, konstanta wire |
| `stacklyd` | `ipc::logs` | handler, task follow, pemetaan DTO, pemasangan `DurableLogSink` di `SinkSet`, health subsystem |
| `stackly-cli` | `commands/instance/logs.rs` | perintah, render teks dan JSON |
| `StacklyClient` | `Logs` | panggilan method, decode frame `log`, `AsyncStream` per follow |
| `StacklyUI` | `Instances/Logs/` | `InstanceLogsModel`, `InstanceLogsView`, pemetaan ke `LogLine` |

### Model data

Store per instance di `<logs>/instances/<id>/captured/` (`DirectoryLayout::instance_captured_logs_dir(id)`, baru di layout spec 0004):

| Entitas | Kunci | Field | Aturan |
| --- | --- | --- | --- |
| Header `store.json` | `storeId` (UUID v7) | `formatVersion` (1), `createdAt` (UTC) | Satu per instance, ditulis atomik saat store pertama kali dibuat (sebelum segmen pertama). Header hilang atau rusak sementara segmen ada: header baru dengan `storeId` baru, segmen lama tetap dibaca dan `seq` lanjut dari yang terbesar |
| Segmen `<stream>.<firstSeq 20 digit>.jsonl` | `(stream, firstSeq)` | record JSON Lines | `stream` adalah `stdout` atau `stderr`. Paling banyak 5 per stream; yang namanya terbesar adalah segmen aktif |
| Record baris | `seq` | `at`, `k: "line"`, `text`, `level`, `partial?`, `cont?`, `step?`, `src?` | Ada di tepat satu segmen |
| Record penanda | `seq` | `at`, `k`: `truncated` (`bytes`), `started` (`pid`), `exited` (`code` atau `signal`), `unavailable` | `truncated` ada di file stream nya. `started`, `exited`, dan `unavailable` ditulis ke kedua stream dengan `seq` yang sama |

#### Format record

Satu objek JSON per baris, diakhiri `\n`, kunci pendek supaya overhead kecil:

```json
{"seq":4812,"at":"2026-11-01T07:03:22.118Z","k":"line","text":"database system is ready to accept connections","level":"normal","src":{"pid":4123,"startedAt":"2026-11-01T07:03:21.904512Z","end":18432}}
{"seq":4813,"at":"2026-11-01T07:03:22.618Z","k":"truncated","bytes":1048576}
{"seq":4814,"at":"2026-11-01T07:05:00.002Z","k":"exited","code":0}
```

- `partial` dan `cont` hanya ditulis kalau benar (sama dengan `LogLine.partial` dan `continued` spec 0010). `step` hanya untuk keluaran langkah init yang gagal (nama langkah dari `InitPlan`). `src` hanya untuk baris dari spool.
- `startedAt` adalah waktu start proses dari identitas majemuk spec 0010 (presisi mikrodetik), `end` adalah offset byte spool setelah baris itu.
- Serde struct dengan `deny_unknown_fields` mati, supaya `formatVersion` 1 bisa menambah kunci opsional.

#### Buang duplikat

- Writer menyimpan per `(instance, stream)` nilai `src` terakhir yang tersimpan; nilai itu dipulihkan dari record baris terakhir yang ber `src` di segmen terbaru stream itu (AC-13).
- Baris dengan `pid` dan `startedAt` yang sama dengan nilai tersimpan dan `end` yang tidak lebih besar dibuang tanpa diberi `seq`. Run baru (pid atau `startedAt` berbeda) selalu ditulis.
- Keluaran init dan penanda tidak punya `src` dan tidak pernah dibuang.

#### Aturan level

Fungsi murni `level_of(text) -> LogLevel` di `logs::level`, dihitung pada teks yang sudah tersamar, hanya pada 160 karakter pertama:

- Token adalah rangkaian huruf A sampai Z besar yang dibatasi karakter bukan huruf dan bukan angka. Token pertama yang cocok menentukan: `ERROR`, `ERR`, `FATAL`, `FTL`, `PANIC`, `CRITICAL`, `CRIT`, `EMERG`, `ALERT` → `error`; `WARNING`, `WARN`, `WRN` → `warning`.
- Pola logfmt dan JSON tanpa membedakan huruf: `level=error`, `"level":"error"` (juga `fatal`, `panic`, `critical`) → `error`; `level=warn`, `level=warning` → `warning`.
- Tidak ada yang cocok → `normal`. Kata huruf kecil biasa seperti "0 errors" tidak pernah cocok.

### Konstanta

| Nama | Nilai | Crate |
| --- | --- | --- |
| `LOG_SEGMENT_MAX_BYTES` | 20 MiB | core, juga di reply sebagai `retention` |
| `LOG_SEGMENTS_PER_STREAM` | 5 | core, juga di reply |
| `LOG_WRITE_QUEUE_MAX` | 1 MiB per instance | core |
| `LOG_WRITE_QUEUE_TOTAL` | 16 MiB | core |
| `LOG_WRITE_RETRY` | 500 ms | core |
| `LOG_SCAN_BUDGET` | 8 MiB per request atau per bangun follow | core |
| `LOG_READ_LIMIT_DEFAULT`, `LOG_READ_LIMIT_MAX` | 200, 1000 | protocol |
| `LOG_FILTER_MAX_CHARS` | 256 | protocol |
| `LOG_LEVEL_SCAN_CHARS` | 160 | core |
| `LOG_RUN_EVENTS_MAX` | 64 per instance | core |
| `LOG_READ_CONCURRENCY` | 4 | daemon |
| `LOG_FOLLOW_MIN_INTERVAL` | 250 ms | daemon |
| `LOGS_GUI_LINE_CAP` | 10 000 | StacklyUI |
| `LOGS_CLI_TAIL_DEFAULT`, `LOGS_CLI_TAIL_MAX` | 100, 10 000 | cli |

### Method IPC

Protokol naik minor (minor bebas berikutnya saat build; `CURRENT` sekarang 1.17). Tiga method baru di tabel `methods!`, semuanya `recovery refused`:

| Method | Params | Reply | Error |
| --- | --- | --- | --- |
| `instance.logs_read` | `LogsReadParams { instance: Uuid, streams: [LogStream] (kosong = keduanya, tanpa duplikat), contains?: String (1 sampai 256 karakter, tanpa `\n` atau `\r`), position?: LogPosition (default `newest`), limit?: u32 (1 sampai 1000) }` | `LogPage` | `request.invalid_params`; instance tidak dikenal atau retained: kode yang sama dengan `instance.get`; `instance.logs_unreadable` |
| `instance.logs_follow` | `LogsFollowParams { instance, streams, contains?, after?: LogCursor }` (tanpa `after` = mulai setelah `head` saat ini) | `LogsFollowing { subscriptionId: u32, storeId?: Uuid, head?: LogCursor }` | sama dengan di atas, plus `events.too_many_subscriptions`; `request.invalid_params` (`after.seq`, `out_of_range`) untuk `after` di store sekarang yang melewati `head` (AC-7, catatan audit 124) |
| `instance.logs_unfollow` | `LogsUnfollowParams { subscriptionId }` | unit | `events.unknown_subscription`, termasuk untuk ID yang dipegang `events.subscribe` (subscription event itu tidak tersentuh) |

Aturan wire: setiap field opsional memakai `#[serde(default)]`; ketiga params mengimplementasikan `Validate`; `subscriptionId` tetap `u32` seperti `events.subscribe`; `pid`, `code`, dan `signal` adalah angka biasa (bukan hitungan); `LogPosition` sengaja tertutup, karena daemon lama menolak method yang tidak dikenalnya sebelum params dibaca.

DTO (`stackly-protocol::logs`, camelCase, aturan wire spec 0006):

- `LogSeq`: newtype `u64` baru di `wire.rs`, dibatasi `MAX_SAFE_INTEGER`.
- `LogCursor { storeId: Uuid, seq: LogSeq }`. Bentuk teks untuk CLI: `<storeId>:<seq>`.
- `LogPosition` (enum berdata, tertutup): `newest`, `before { cursor }`, `after { cursor }`.
- `LogStream` (terbuka): `stdout`, `stderr`, `Unrecognized`.
- `LogLevel` (terbuka): `normal`, `warning`, `error`, `Unrecognized`.
- `LogEntry { seq, at: Timestamp, stream?: LogStream (null untuk penanda run), kind: LogEntryKind }`.
- `LogEntryKind` (terbuka, `tag = "type"`, `content = "content"`): `line { text, level, partial, continued, step? }`, `truncated { bytes: ByteCount }`, `started { pid: u32 }`, `exited { code?: i32, signal?: i32 }`, `unavailable`, `Unrecognized`.
- `LogGap { reason: LogGapReason }` dengan `LogGapReason` terbuka: `rotated`, `store_reset`, `Unrecognized`.
- `LogPage { storeId?, entries: [LogEntry], earlier?: LogCursor, later?: LogCursor, head?: LogCursor, gap?: LogGap, scanLimited: bool, retention: LogRetention { segmentsPerStream: Count, segmentMaxBytes: ByteCount }, storedBytes: LogStreamBytes { stdout: ByteCount, stderr: ByteCount }, skippedRecords: Count }`.
- `ServerFrame::Log(LogFrame { subscriptionId, entries, gap?, ended?: LogFollowEnd { reason: LogFollowEndReason } })` dengan alasan terbuka `instance_removed`, `Unrecognized`. Frame ini hanya dikirim ke koneksi yang memanggil `instance.logs_follow`, jadi client lama tidak pernah menerimanya.

Aturan halaman:

- `newest`: entri terakhir sampai `limit`. `before`: entri dengan `seq` lebih kecil dari cursor, yang terdekat. `after`: entri dengan `seq` lebih besar, yang terdekat. Entri selalu urut naik.
- `earlier` null kalau tidak ada entri yang lebih tua di store; `later` null kalau halaman sampai ke `head`. Pada `scanLimited`, cursor dari titik henti tetap diisi walau `entries` kurang dari `limit` atau kosong.
- `storeId` cursor berbeda dari store: `gap store_reset`, lalu halaman dibaca seperti `newest` (untuk `before`) atau dari awal store (untuk `after`). Cursor lebih tua dari record tertua: `gap rotated`, lalu dibaca dari record tertua.
- Filter: `streams` menyaring record baris dan `truncated`; penanda run selalu lolos filter stream. `contains` hanya menyaring record baris (perbandingan `to_lowercase` pada kedua sisi); `truncated` dan `unavailable` selalu lolos supaya kehilangan tetap terlihat; `started` dan `exited` hanya muncul tanpa `contains`.
- Ukuran: entri ditambahkan selama JSON reply tetap di bawah `LOG_BATCH_MAX`; entri pertama selalu masuk walau sendirian melebihinya (baris 16 KiB dengan escape terburuk tetap jauh di bawah `CONTROL_FRAME_MAX`). Aturan yang sama berlaku untuk frame `log`.

Aturan follow:

- Daemon membuat receiver `watch` dan membaca `head` dalam satu langkah, memasukkan reply `LogsFollowing` ke antrian keluar lebih dulu, baru memulai task penerus.
- Setiap bangun, task membaca paling banyak `LOG_SCAN_BUDGET` dari disk; sisa dibaca di putaran berikutnya tanpa menunggu `watch`.
- Instance dihapus (event `instance.removed` dari registry): frame terakhir dengan `ended`, lalu subscription dilepas. Koneksi ditutup: semua follow koneksi itu selesai.
- Penghitung `subscriptionId` dan batas `MAX_SUBSCRIPTIONS` dipegang satu struktur per koneksi yang dipakai `events.subscribe` dan `instance.logs_follow` bersama.

### Kode error

| Kode | Kategori | Kapan | `details` |
| --- | --- | --- | --- |
| `instance.logs_unreadable` | `unavailable` | store tidak bisa dibaca (izin, IO, folder bukan folder biasa) | `reason` (`permission_denied`, `io`, `not_a_directory`) |

Kesalahan params memakai `request.invalid_params` dengan field: `contains` (panjang, newline), `limit`, `streams` (duplikat).

### Health

Subsystem baru `instance_logs` di `subsystem::ALL` (`daemon.get_health`): `ok`; `degraded` selama ada instance yang tulisnya gagal atau dalam 60 detik sesudah kegagalan terakhir, dengan detail `reason` (`no_space`, `permission_denied`, `io`) dan jumlah instance yang terdampak. Penghitung `instanceLogTruncations` (jumlah record `truncated`) dan `instanceLogBytesTruncated` (jumlah `bytes` nya) masuk ke penghitung health yang sudah ada di `daemon.get_health`, dihitung sejak daemon start. Waktu 60 detik diukur dengan `Clock`, supaya test memakai `FakeClock`.

### CLI

`stackly instance logs <target> [--tail N] [--follow|-f] [--stream all|stdout|stderr] [--grep TEXT] [--before CURSOR | --after CURSOR] [--json]`

- `--before` dan `--after` saling meniadakan; `--tail` menentukan jumlah entri, lewat beberapa halaman `logs_read` kalau lebih dari 1000. Dengan `--after`, `--tail N` berarti N entri pertama setelah cursor. `--follow` dengan `--before` ditolak `cli.usage` (exit 2). `--grep` tidak membedakan huruf besar kecil.
- Teks: satu entri per baris, `YYYY-MM-DD HH:MM:SS.mmm  <sumber>  <teks>` dengan waktu lokal, sumber `stdout`, `stderr`, atau `stackly` untuk penanda. Penanda: `process started (pid 4123)`, `process exited with code 1` atau `with signal 9`, `1.0 MiB of output was dropped`, `output while Stackly was not running could not be read`. Gap: `older lines were rotated out` atau `the log store was reset`. TTY memberi warna tone untuk `warning` dan `error`; pipe dan `NO_COLOR` tanpa warna (aturan warna spec 0008).
- JSON: record `{"schemaVersion":1,"type":"item","command":"instance logs","item":<LogsItem>}` dengan `LogsItem` `entry { LogEntry }` atau `gap { LogGap }`, lalu satu `result`: `{ instance: { id, name }, storeId, last: LogCursor | null, retention, storedBytes, skippedRecords }`. `last` null kalau tidak ada entri yang diterima.
- Ctrl+C saat `--follow`: `logs_unfollow` dikirim tanpa menunggu, `result` ditulis, exit 0. Ctrl+C tanpa `--follow` (saat paging panjang) tetap `cli.interrupted`, exit 130 (spec 0008).
- Frame `ended: instance_removed`: `cli.instance_not_found`, exit 10. Koneksi daemon putus: error koneksi yang sama dengan perintah lain, dan dengan `--json` envelope error menggantikan `result` (tidak ada `result` sukses).

### Layar Logs

`InstanceLogsModel` (`@Observable`, MainActor) memegang `lines` (paling banyak 10 000 `LogLine`), `source` (All, stdout, stderr), `query`, `follow`, `phase` (loading, ready, empty, unreadable, offline), cursor terlama dan terbaru, `retention`, dan subscription aktif.

- **Buka tab**: `logs_read newest` (limit 200) dengan filter aktif, lalu kalau Follow menyala, `logs_follow after: page.later ?? page.head`. Tidak ada celah dan tidak ada duplikat di antara keduanya.
- **Ganti sumber atau query** (query di debounce 300 ms): batalkan pembacaan yang sedang berjalan, unfollow, kosongkan, baca ulang seperti buka tab.
- **Cursor model**: model menyimpan `oldest` (untuk Load Earlier) dan `resumeAfter`, yaitu `later ?? head` dari reply terakhir atau `seq` dari frame terakhir. `resumeAfter` dipakai untuk follow dan reconnect, bukan baris terbaru yang dimuat, karena dengan filter aktif keduanya berbeda.
- **Follow**: menyala secara default. Mematikan Follow = unfollow, baris tetap. Menyalakan lagi = baca ulang `newest` lalu follow. `LogView` sendiri mematikan Follow saat pengguna menggulir ke atas (spec 0067).
- **Load Earlier**: `logs_read before: oldestLoaded` dengan filter aktif. Tombol nonaktif saat `earlier` null. Kalau total melewati 10 000, baris terbaru dibuang, Follow dimatikan, dan `resumeAfter` dikosongkan, sehingga menyalakan Follow lagi selalu membaca ulang `newest`. Saat follow menambah baris dan total melewati batas, baris tertua dibuang.
- **Pemetaan ke `LogLine`**: `id` = `seq` (penanda run dari dua stream sudah satu). `timeText` = `HH:mm:ss.SSS` lokal dari `at`. `source` = `stdout` atau `stderr` untuk baris, `stackly` untuk penanda dan gap. `level` dari entri; `truncated` dan `unavailable` = warning; `exited` dengan code bukan 0 atau signal = error. Gap memakai `id` `gap-<seq entri sesudahnya>`. Baris pemisah hari (`id` `day-<yyyy-MM-dd>`, tanpa waktu dan sumber, teks tanggal panjang lokal) disisipkan saat tanggal lokal berganti antara dua entri; zona waktu adalah `TimeZone.current` saat baris dipetakan, dan perubahan zona waktu sistem berlaku untuk baris yang dimuat sesudahnya.
- **Copy Shown Lines**: menyalin semua baris yang sedang dimuat (sudah sesuai sumber dan filter) ke pasteboard sebagai teks `yyyy-MM-dd HH:mm:ss.SSS <sumber> <teks>`, tanpa baris pemisah hari. Nonaktif saat kosong. Context menu per baris: Copy Line.
- **Footer**: `Cursor <seq terbaru yang dimuat>` (`Cursor none` kalau kosong) `· Retained 5 files × 20 MiB per stream · Secrets are redacted before storage`, angka dari `retention`.
- **State**: belum ada store atau entri: "No output yet" dengan "Lines appear here when the instance writes to stdout or stderr."; filter tanpa hasil: "No lines match this filter." (mockup); `scanLimited` tanpa hasil: "No matches in the last 8 MB searched." dengan Load Earlier aktif; `instance.logs_unreadable`: "Logs can't be read" dengan alasan dan Try Again; daemon offline: baris tetap, Follow berhenti, banner daemon yang ada (spec 0013) tampil, dan saat terhubung lagi follow dilanjutkan dari `resumeAfter` (gap `rotated` tampil sebagai baris penanda kalau ada).
- Kebijakan interaksi 2026-10-24 berlaku: kontrol pointer native, tanpa shortcut atau dukungan pembaca layar khusus.
- Fixture tampilan (spec 0068): skenario `logs-follow`, `logs-empty`, `logs-filter-empty`, `logs-markers` (truncated, started, exited, gap, pemisah hari), dan `logs-unreadable`, masing masing dengan snapshot light dan dark.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| tulis | `seq` | penghitung writer per instance, dipulihkan dari record terakhir (AC-13) |
| tulis | `at` | `LogBatch.read_at` (spec 0010, `Clock` daemon); run event membawa `at` dari `Clock` saat dipanggil |
| tulis | `text`, `partial`, `cont` | `LogLine` spec 0010, sudah tersamar |
| tulis | `level` | `level_of(text)` |
| tulis | `step` | nama langkah `InitPlan` di jalur keluaran init (`init.rs`) |
| tulis | `src.pid`, `src.startedAt` | identitas majemuk proses yang spool nya dibaca (spec 0010), lewat `LogBatch.source` baru |
| tulis | `src.end` | offset spool setelah baris, lewat `LogLine.end` baru |
| tulis | `truncated.bytes` | `LogBatch.truncated` |
| tulis | `started.pid`, `exited.code`, `exited.signal` | `run_event` dari supervisor (spawn dan handler exit) |
| tulis | `storeId`, `createdAt` | UUID v7 dan `Clock` saat header dibuat |
| baca | `entries`, `earlier`, `later`, `head`, `gap`, `scanLimited`, `skippedRecords` | dihitung pembaca dari segmen dan params |
| baca | `retention` | konstanta `LOG_SEGMENTS_PER_STREAM`, `LOG_SEGMENT_MAX_BYTES` |
| baca | `storedBytes` | jumlah ukuran segmen per stream (`metadata().len()`) |
| follow | `subscriptionId` | penghitung subscription per koneksi bersama |
| follow | `ended` | event `instance.removed` dari bus daemon |
| health | `instance_logs` | status writer per instance |
| CLI | waktu lokal | zona waktu sistem lewat `jiff::tz::TimeZone::system()` |
| CLI | `instance.name` di `result` | `InstanceTarget::resolve` |
| GUI | waktu dan tanggal lokal | `TimeZone.current` dan `Locale.current` |
| GUI | footer retention | `LogPage.retention` |

### Key invariants

- Hanya teks yang sudah lewat `Redactor` yang sampai ke store; store, reply, frame, CLI, dan GUI tidak pernah menyamarkan ulang dan tidak pernah membaca spool.
- `seq` unik dan naik per instance; dua record dengan `seq` sama hanya ada untuk penanda run yang ditulis ke kedua stream, dan isinya identik.
- Byte yang dijawab `Accepted` sudah ada di antrian writer; `Busy` berarti sink tidak menyimpan apa pun dari batch itu.
- Memori daemon untuk log dibatasi `LOG_WRITE_QUEUE_TOTAL` ditambah satu batch per antrian kosong, `LOG_RUN_EVENTS_MAX` run event per instance, satu frame dan satu buffer baca per follower (paling banyak 8 per koneksi), dan antrian keluar koneksi yang sudah dibatasi spec 0006.
- Writer tidak pernah menghapus file selain segmen yang dirotasi keluar dan tidak pernah menulis di luar `captured/`.
- Daemon tidak mencatat isi `contains` atau teks baris di log daemon nya sendiri.

### Security model

- Hanya peer dengan uid yang sama dengan daemon yang bisa terhubung (spec 0006), jadi siapa pun yang terhubung boleh membaca log instance mana pun milik user itu; tidak ada peran lain.
- File 0600 di folder 0700 di root log milik user. Proses lain milik user yang sama bisa membaca file itu, sama dengan spool (residual risk spec 0010).
- Secret: tersamar sebelum sink (spec 0010 dan 0011), plus overlap redaksi di batas potong 64 KiB (*Usulan update*). Secret yang belum dipelajari `Redactor` (misalnya password yang diketik pengguna ke psql lalu dicetak server) tidak dikenali; ini batas yang sama dengan spec 0011.
- Tidak ada data yang diatur regulasi.
- Catatan audit keamanan 2026-11 nomor 126 (stream subscription `StacklyClient` memakai buffer tak terbatas untuk frame log) adalah residual untuk fitur 71 (keputusan user 2026-11-05); alasan dan perubahan desain yang dibutuhkan di [spec 0019](../0019-provider-postgresql/index.md#residual-audit-keamanan-2026-11).
- Catatan audit keamanan 2026-11 nomor 127 (event dan follow log satu koneksi berbagi satu antrian keluar 64 frame) adalah residual untuk fitur 71 (keputusan user 2026-11-05); alasan dan perubahan desain yang dibutuhkan di [spec 0019](../0019-provider-postgresql/index.md#residual-audit-keamanan-2026-11).

### Configuration required

Tidak ada env var atau setting baru. Ukuran rotasi tetap konstanta sampai fitur Settings menambah pengaturannya.

### Critical test scenarios

- Happy path: fake server menulis ke stdout dan stderr; `logs_read newest` mengembalikan keduanya urut `seq`, `streams: [stderr]` hanya stderr, `contains` tanpa membedakan huruf. Memverifikasi **AC-1**, **AC-6**.
- Rotasi: konstanta segmen diperkecil lewat `DurableLogConfig` test; spam stdout membuat paling banyak 5 segmen stdout, segmen stderr utuh, dan cursor lama mendapat `gap rotated`. Memverifikasi **AC-2**, **AC-5**.
- Client lambat: evidence run dengan 5000 baris × 512 byte per detik selama 60 detik, satu koneksi follow yang tidak pernah membaca socket, sampling `daemon.get_info` setiap 100 ms dan `phys_footprint` daemon setiap detik; batas AC-3; koneksi follower yang diam ditutup `ipc.write_timeout` setelah 30 detik; follower kedua yang membaca pelan (satu frame per detik) menerima `gap rotated` lalu entri yang urut. Memverifikasi **AC-3**, **AC-7**.
- Crash: daemon dibunuh `SIGKILL` di tengah spam, dijalankan lagi, mengadopsi proses; `seq` di store berurutan tanpa teks ganda untuk satu run dan tanpa baris spool yang hilang. Varian dengan writer yang ditahan test (antrian berisi batch `Accepted` saat `SIGKILL`) menghasilkan `truncated` dengan jumlah byte yang hilang. Memverifikasi **AC-4**, **AC-5**.
- Antrian: batch 1 MiB ke antrian kosong diterima; batch kedua saat antrian belum tertulis dijawab `Busy`; satu instance yang tulisnya gagal tidak menunda tulis instance lain; crash loop dengan disk gagal tidak menumpuk lebih dari 64 run event. Memverifikasi **AC-3**, **AC-12**.
- Race baca: pembaca yang membaca segmen aktif di tengah `write_all` tidak menghitung baris setengah jadi sebagai rusak; segmen yang dirotasi keluar di tengah halaman menghasilkan gap `rotated`, bukan error. Memverifikasi **AC-6**, **AC-13**.
- Penanda: start, crash dengan exit 1, restart policy, stop; store berisi `started`, `exited code 1`, `started`, `exited` di posisi yang benar; spool yang dihapus sebelum adopsi menghasilkan `unavailable`; backlog spool yang dipaksa melewati batas test menghasilkan `truncated`. Memverifikasi **AC-5**.
- Follow: follow sebelum store ada, lalu start; frame datang; stop dan start lagi tetap di subscription yang sama; hapus instance mengirim `ended`; follow ke 9 ditolak; `logs_unfollow` dengan ID milik `events.subscribe` dijawab `events.unknown_subscription` dan subscription event itu tetap jalan. Memverifikasi **AC-7**.
- Level: tabel test `level_of` dengan baris PostgreSQL dan NATS nyata, logfmt, JSON, dan baris normal yang memuat "errors". Memverifikasi **AC-8**.
- Secret: dummy server mencetak fixture credential utuh, di tengah baris 100 KiB tanpa newline yang terbelah tepat di 64 KiB, dan di keluaran init yang gagal; grep di segmen, reply, frame, output CLI, dan teks Copy Shown Lines tidak menemukannya. Memverifikasi **AC-9**.
- CLI: `--tail 1500` lewat dua halaman, `--follow` lalu SIGINT keluar 0 dengan `result`, `--json` item dan result, nama tidak dikenal exit 10, `--follow --before` exit 2. Memverifikasi **AC-10**.
- GUI: test model dengan daemon skrip (`ScriptedDaemon`) untuk buka tab, ganti filter, Load Earlier sampai batas 10 000, Follow mati dan nyala, offline dan reconnect; snapshot lima skenario spec 0068. Memverifikasi **AC-11**.
- Gagal tulis: folder `captured/` diubah ke 0500 di `TestRoot`; `accept` menjawab `Busy`, health `degraded permission_denied`, fake tetap jalan; izin dipulihkan, tulis lanjut tanpa baris hilang, health kembali `ok`. Memverifikasi **AC-12**.
- Pemulihan: segmen dengan baris terakhir terpotong, record rusak di tengah, file asing di folder, header hilang. Memverifikasi **AC-13**.
- Recovery mode: ketiga method dijawab error recovery. Memverifikasi **AC-14**.
- Native: PostgreSQL 17 lewat daemon menulis `database system is ready` ke stderr yang muncul di `stackly instance logs`; baris `FATAL` dari login yang salah menjadi `error`. Tanpa artifact PostgreSQL di cache runtime xtask, test ini `BLOCKED`. Memverifikasi **AC-6**, **AC-8**.

### Usulan update untuk spec lain

- **Spec 0010**: `LogBatch` mendapat `source: Option<SpoolSource { pid, started_at }>` dan `LogLine` mendapat `end: Option<u64>` (offset spool setelah baris; `None` untuk keluaran init). `LogSink` mendapat `fn run_event(&self, instance: InstanceId, event: RunEvent)` dengan implementasi default kosong, `RunEvent` = `Started { pid, at }`, `Exited { code, signal, at }`, `Unavailable { at }`; `SinkSet` meneruskannya ke semua sink. Overlap redaksi: saat baris tanpa newline dipotong di `LOG_LINE_HARD_MAX`, pembaca menahan `Redactor::max_secret_len() - 1` byte (method baru: panjang byte secret terpanjang yang sudah dipelajari) terakhir potongan, menyamarkan gabungan tahanan dan potongan berikutnya, lalu baru mengirim; residual risk "secret yang terpotong batas" di spec 0010 dihapus. Satu test dengan fixture credential yang terbelah di batas itu.
- **Spec 0006**: `ServerFrame` mendapat varian `log`; penghitung `subscriptionId` dan `MAX_SUBSCRIPTIONS` dipakai bersama `instance.logs_follow`; `events.unknown_subscription` juga untuk `instance.logs_unfollow`; subsystem `instance_logs` di daftar subsystem; tabel area menyebut method log milik spec 0027.
- **Spec 0008**: perintah streaming pertama adalah `instance logs --follow`; Ctrl+C yang mengakhiri follow keluar 0 dengan `result`, bukan `cli.interrupted`.
- **Spec 0009**: `logs.files` menolak komponen pertama `captured` (issue di `logs.files[<i>]`), karena folder itu milik store Stackly.
- **Spec 0004**: `DirectoryLayout::instance_captured_logs_dir(id)` = `<logs>/instances/<id>/captured/`, dibuat writer dengan `create_dir_private`, bukan oleh `prepare_layout`; baris pohon `<logs>/instances/<instance-id>/` menyebut `captured/`. Port `FileSystem` mendapat `create_file_private(path) -> Result<std::fs::File, PlatformError>` (buat baru, 0600, `O_NOFOLLOW`, gagal kalau sudah ada) dan `open_append_private(path)` untuk segmen yang sudah ada, sehingga core memakai `File` biasa tanpa `OpenOptionsExt` dan tetap lolos `cargo check` untuk Windows; `UnsupportedPlatform` mengembalikan `NotSupported`.
- **Spec 0011**: Follow-up fitur 27 dicentang oleh langkah test secret di build plan ini.

## Build plan

Pendekatan Journey: satu jalur "lihat dan ikuti log satu instance" dibangun lengkap dari store sampai layar, termasuk semua state, sebelum fitur berikutnya. Urutan mengikuti arah dependency (core, protocol dan daemon, CLI, GUI), dengan bukti native di akhir.

**Milestone 1: store di core**

1. Spec 0010 di core: `LogBatch.source`, `LogLine.end`, `LogSink::run_event` dan `RunEvent`, pemanggilan run event dari spawn, handler exit, dan adopsi, serta overlap redaksi dengan test batas 64 KiB. Satisfies **AC-4**, **AC-5**, **AC-9**.
2. `logs::record`, `logs::level`, `logs::filter`: format record, `level_of` dengan tabel test, filter substring. Satisfies **AC-1**, **AC-8**, **AC-6**.
3. `logs::store` dan `logs::writer`: header, segmen, rotasi, pemulihan, buang duplikat, antrian berbatas, `Busy`, retry tulis, status gagal tulis, `watch` `{ storeId, head, removed }`, penanda kehilangan saat adopsi; `DirectoryLayout::instance_captured_logs_dir` dan `FileSystem::create_file_private` serta `open_append_private` (adapter macOS dan unsupported). Test core dengan `TestRoot` dan `FakeClock`. Satisfies **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-12**, **AC-13**.
4. `logs::read` dan `logs::follow`: halaman, posisi, gap, scan budget, ukuran batch, merge stream, sumber follow. Satisfies **AC-5**, **AC-6**, **AC-7**.

Langkah 1 dan langkah 2 bisa berjalan di dua sesi paralel (file berbeda); langkah 3 butuh 1 dan 2.

**Milestone 2: protokol dan daemon**

5. `stackly-protocol::logs`, `LogSeq`, tiga method, `ServerFrame::Log`, kode `instance.logs_unreadable`, subsystem `instance_logs`, konstanta; naik minor; `cargo xtask codegen` dan fixture di `protocols/fixtures/`; spec 0009 rule `captured` di validator manifest. Satisfies **AC-6**, **AC-7**, **AC-12**, **AC-14**.
6. `stacklyd::ipc::logs`: handler, task follow di antrian keluar koneksi, penghitung subscription bersama, `ended` dari `instance.removed`, `DurableLogSink` di `SinkSet`, health. Integration test di `apps/daemon/tests/logs.rs`: happy path, rotasi, follow, crash dengan `SpawnedDaemon`, gagal tulis, recovery mode, dan test secret dengan dummy server. Satisfies **AC-1** sampai **AC-9**, **AC-12** sampai **AC-14**.

**Milestone 3: CLI**

7. `stackly instance logs` dengan render teks dan JSON, paging, follow, Ctrl+C, dan test di `apps/cli/tests/instance_logs.rs`. Satisfies **AC-10**, **AC-9**.

**Milestone 4: GUI**

8. `StacklyClient`: method log, decode frame `log`, `AsyncStream` per follow, test fixture. Satisfies **AC-7**, **AC-11**.
9. `StacklyUI/Instances/Logs/`: model, view di tab Logs detail instance, pemetaan ke `LogLine`, Copy Shown Lines, state, skenario fixture spec 0068 dan snapshot, test model dengan `ScriptedDaemon`. Satisfies **AC-11**, **AC-9**.

**Milestone 5: bukti**

10. Evidence run client lambat (AC-3) dan test native PostgreSQL 17 (setelah persetujuanmu, `BLOCKED` tanpa artifact), lalu sesi GUI manual yang membandingkan layar Logs dengan `stackly instance logs --follow` untuk instance yang sama. Satisfies **AC-3**, **AC-6**, **AC-8**, **AC-11**.

## Consequences

**Positive**:

- Satu store menjadi sumber CLI dan GUI, jadi kesetaraan keduanya dijamin oleh desain, bukan oleh dua implementasi.
- Client lambat dan disk yang bermasalah hanya mendorong balik ke spool yang sudah dibatasi; proses dan daemon tidak pernah menunggu log.
- Cursor tahan restart daemon dan restart instance, sehingga script bisa melanjutkan tanpa kehilangan atau mengulang baris.
- Setiap kehilangan (truncated, rotated, unavailable, store reset) terlihat sebagai penanda, bukan bolong diam diam.

**Negative / tradeoffs**:

- Disk per instance bisa sampai sekitar 200 MiB, dan 10 instance aktif sekitar 2 GiB, tanpa pengaturan sampai fitur Settings.
- JSON Lines menambah sekitar 60 byte per baris, jadi baris pendek menghabiskan kuota rotasi lebih cepat dari teks mentah.
- Saat daemon mati tepat setelah menjawab `Accepted` tapi sebelum menulis, sampai sekitar satu antrian per instance (`LOG_WRITE_QUEUE_MAX` plus satu batch) hilang, karena spool sudah membebaskan byte itu; kehilangannya ditandai `truncated` saat adopsi, tetapi isinya tidak bisa dipulihkan.
- Secret yang baru dipelajari `Redactor` setelah sebuah baris tersimpan tidak disamarkan di baris lama itu; store tidak pernah ditulis ulang.
- Client yang berhenti membaca follow diputus setelah 30 detik, dan reply lain di koneksi yang sama ikut menunggu di belakang frame follow; client sebaiknya memakai koneksi sendiri untuk follow.
- Pencarian substring pada log yang besar dibatasi 8 MiB per request, jadi menemukan baris lama butuh beberapa kali Load Earlier.
- Aturan level berbasis kata kunci bisa salah menandai baris; dampaknya hanya simbol dan warna.
- Spec 0010 berubah lagi (`LogBatch`, `LogSink`, redaksi), sehingga test spool yang ada perlu ikut disesuaikan.

**Neutral**:

- Tidak ada migrasi data: store dibuat baru di folder yang belum dipakai. Instance yang sudah berjalan mulai tercatat sejak daemon versi ini mengadopsinya.
- Folder `captured/` ikut terhapus atau tersimpan bersama folder log instance oleh fitur hapus dan retained.

## Follow-up

- [ ] Fitur Settings (Storage): pengaturan ukuran rotasi per stream yang dibaca writer saat start, mengganti konstanta.
- [ ] File log milik provider (`RunPlan.log_files`) sebagai sumber tambahan di layar Logs, kalau ada provider yang membutuhkannya.
- [ ] Filter level (errors only) di atas `level` yang sudah tersimpan.
- [ ] Terapkan *Usulan update untuk spec lain* ke spec 0004, 0006, 0008, 0009, 0010, dan 0011 sebelum langkah yang memakainya.

## Rationale

Pilihan dan alasannya: lihat [rationale.md](rationale.md).
