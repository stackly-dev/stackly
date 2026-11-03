# 0028. Metrics dan health

**Date**: 2026-11-02
**Status**: Accepted

## Summary

Spec ini menambah pemantauan sumber daya ke instance yang sedang berjalan: CPU, memori, disk I/O, ukuran data, jumlah restart sejak dibuat, dan latensi probe, di samping readiness dan health yang sudah ada dari spec 0010. Daemon mengambil sampel setiap 5 detik untuk proses utama dan seluruh pohon prosesnya (proses utama beserta semua turunannya), menyimpan 60 sampel terakhir di memori, dan menandai setiap angka yang tidak bisa diukur sebagai unavailable dengan alasan, tidak pernah nol. Client membaca lewat satu method baru, `instance.get_metrics`, dengan cursor (penanda sampel terakhir yang sudah dipegang), sehingga sampel tidak pernah masuk ke event bus. Setelah Mac bangun dari sleep, daemon langsung memeriksa ulang setiap instance di port yang sama dan memberi masa tenggang 30 detik supaya sleep tidak dibaca sebagai crash.

## Requirements

**User stories**:

- Sebagai developer, saya ingin melihat berapa CPU, memori, dan disk yang dipakai instance saya, untuk proses utama saja atau seluruh pohon prosesnya, supaya saya tahu instance mana yang membebani Mac.
- Sebagai developer, saya ingin angka yang tidak bisa diukur tampil sebagai unavailable beserta alasannya, supaya saya tidak tertipu oleh nol.
- Sebagai developer, saya ingin tahu berapa kali instance di restart otomatis sejak dibuat dan kapan terakhir gagal, supaya crash yang berulang terlihat walau sekarang instance sedang sehat.
- Sebagai developer, saya ingin readiness dan health tetap benar setelah Mac bangun dari sleep, tanpa instance berpindah port atau dianggap crash.
- Sebagai pemakai CLI, saya ingin `stackly instance metrics --json` yang isinya sama dengan layar Metrics, supaya script dan GUI melihat fakta yang sama.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-018):

- **AC-1** (REQ-018 sampling): Untuk setiap instance dengan lifecycle `running` dan proses utama yang identitasnya terverifikasi (termasuk proses yang diadopsi setelah daemon restart), daemon mengambil satu `MetricsSample` pada setiap tick global `METRICS_SAMPLE_INTERVAL` (5 detik) tanpa ada client yang meminta, dan menyimpan paling banyak `METRICS_HISTORY_CAPACITY` (60) sampel di memori. Sampel ke 61 membuang yang paling tua. Setiap history punya `historyId` (PID dan waktu start proses utama), dan setiap sampel membawa `seq` yang naik satu per sampel di history itu (mulai 1), `sampledAt` (UTC), pemakaian `main` dan `tree`. Proses yang baru running mendapat sampel pertamanya di tick berikutnya (paling lama 5 detik). Instance yang tidak `running` tidak punya history; history dibuang saat proses utama keluar, saat proses utama berganti, dan saat daemon berhenti.
- **AC-2** (REQ-018 nilai): CPU adalah persen dari satu core logis (100 berarti satu core penuh, pohon yang sibuk bisa lebih dari 100), dihitung dari selisih waktu CPU user dan system dibagi selisih `tokio::time::Instant` yang diambil tepat di sekitar pembacaan OS (di dalam `spawn_blocking`, bukan waktu tick), dibulatkan ke satu desimal. Memori adalah physical footprint (`ri_phys_footprint`, angka yang Activity Monitor sebut Memory) dalam byte. Disk I/O adalah byte dibaca dan byte ditulis per detik, dari selisih `ri_diskio_bytesread` dan `ri_diskio_byteswritten`, dibulatkan ke bawah. Sebuah proses hanya ikut dalam CPU dan Disk I/O kalau identitasnya (PID dan waktu start) ada di sampel ini dan sampel sebelumnya; proses yang hidup kurang dari satu interval tidak terhitung, dan itu disebut di label (AC-4).
- **AC-3** (REQ-018, AC-018 unavailable): Setiap nilai CPU, memori, dan Disk I/O adalah `Metric` yang berisi angka atau `unavailable` dengan `reason` dari himpunan terbuka `warming_up` (sampel pertama, belum ada selisih), `not_permitted` (OS menolak dengan `EPERM`), `process_exited` (proses keluar di tengah sampel), dan `not_supported` (platform tanpa adapter). Untuk `tree`, nilai adalah jumlah proses yang terukur; nilai `tree` `unavailable` hanya kalau tidak ada satu pun proses yang terukur, dengan reason milik proses utama. Tidak ada jalur kode yang menulis 0 untuk nilai yang tidak diukur: test core memberi `FakeProcessSupervision` yang menjawab `EPERM`, proses yang hilang, dan sampel pertama, lalu memastikan hasilnya `unavailable` dengan reason yang benar di DTO, JSON CLI, dan model Swift. Ukuran data yang belum pernah dipindai tampil `unavailable` reason `not_scanned`.
- **AC-4** (AC-018 label cakupan): Setiap `ResourceUsage` membawa `scope` (`main` atau `tree`), `processCount` (proses yang ditemukan), dan `measuredCount` (proses yang ikut di nilai itu). Layar Metrics menulis subjudul kartu "Main process" atau "Process tree · {n} processes", dan "Process tree · {m} of {n} measured" kalau `measuredCount` lebih kecil. Teks CLI menulis "main process (pid {pid})" atau "process tree ({n} processes, summed per process)", ditambah "{m} measured" kalau lebih kecil. Pada PostgreSQL 17 native (postmaster beserta proses latar belakangnya) `tree.processCount` lebih dari 1 dan memori `tree` paling sedikit memori `main`; pada NATS native `tree.processCount` 1 dan nilainya sama dengan `main`.
- **AC-5** (pohon proses): Pohon adalah proses utama ditambah semua turunannya lewat link parent, dibangun ulang setiap tick dari satu daftar semua PID (`list_pids`) dan parent dari `inspect`, dengan pengaman siklus (PID yang sudah dikunjungi dilewati). Hanya proses dengan uid efektif sama dengan uid daemon (dibaca sekali saat sampler mulai dari capability `UserIdentity`) yang dihitung; turunan dengan user lain dilewati. Fakta `inspect` (uid, parent, executable) disimpan per identitas proses sehingga `inspect` dipanggil sekali per proses baru, bukan setiap tick. Daftar proses hanya ada di sampel terbaru, paling banyak `METRICS_PROCESS_ROWS` (200) baris dengan `processesTruncated` true kalau lebih, urut sebagai pohon (anak di bawah parent, saudara urut PID). Setiap baris berisi `pid`, `parentPid`, `depth`, `name` (nama file executable dari path yang diverifikasi, tidak pernah argv), `isMain`, CPU, dan memori. Proses yang keluar di antara pembuatan pohon dan pembacaan pemakaiannya dibuang dari daftar tanpa membuat sampel gagal; kalau yang keluar proses utama, `main` `unavailable` reason `process_exited`.
- **AC-6** (restart sejak dibuat): `instances.restart_total` naik satu di setiap commit spawn yang berhasil dan mengikuti crash instance itu: spawn dari `StartRequest::respawn` (`respawn: true`, spec 0010 *Restart policy*), dan spawn ulang di dalam operasi start mana pun setelah percobaan sebelumnya crash sebelum ready. Spawn yang gagal tidak dihitung. Spawn pertama dari start atau restart pengguna (spec 0026), start rekonsiliasi, dan reload tidak dihitung. Nilainya bertahan setelah daemon restart, terlihat sebagai `restartTotal` (`Count`) di `InstanceSummary` (list, get, event `instance.state_changed`) dan di balasan `instance.get_metrics`, dan instance dari registry lama mulai dari 0. Kartu Restarts di layar Metrics dan baris Restarts di tab Summary dan inspector menampilkan angka ini dengan teks "Since created".
- **AC-7** (health dengan timestamp, alasan, dan latensi): `InstanceReadiness` dan `InstanceHealth` mendapat `latencyMs` opsional: waktu dari mulai sampai hasil satu percobaan probe yang menghasilkan observasi itu, untuk hasil sukses maupun gagal, dibulatkan ke atas dan dibatasi pada timeout probe; null untuk observasi tanpa probe, misalnya `daemon_restarted`, `not_running`, atau `waking`. Perubahan latensi saja tidak mengirim `instance.health_changed`; nilai terbaru terbaca lewat `instance.get` dan `instance.get_metrics`. Baris Readiness di tab Summary menampilkan `detail` dan latensi ("{detail} · {n} ms", atau "{n} ms" tanpa detail), dan header layar Metrics menampilkan state health (atau readiness kalau provider tidak punya health plan), alasan, jam `observedAt`, dan latensi.
- **AC-8** (cadence dan konstanta): `READINESS_REFRESH_INTERVAL` tetap 10 detik dan `READINESS_MAX_AGE` tetap 30 detik, dan penanda `// provisional: F28` di `crates/core/src/supervisor/config.rs` dan `crates/core/src/observation.rs` dihapus. Sampling (5 detik) berjalan terpisah dari probe dan tidak pernah mengirim probe ke provider.
- **AC-9** (wake dari sleep, PRD 19.5): `WakeWatch` di core membandingkan selisih `Clock::now` dengan selisih waktu monotonic tokio setiap `WAKE_CHECK_INTERVAL` (2 detik); selisih yang lebih besar dari `WAKE_GAP_THRESHOLD` (5 detik) dianggap wake, dan lama sleep dicatat di log daemon level info. Saat wake, untuk setiap instance `running`, dalam paling lama satu `WAKE_CHECK_INTERVAL`: readiness dan health menjadi `unknown` reason `waking` dengan `observedAt` waktu wake terdeteksi (satu `instance.health_changed`), penghitung kegagalan berturut turut di reset, dan probe readiness dan health langsung dijalankan ke endpoint dan port yang tersimpan. Readiness dan health masing masing punya `grace_until = wake + WAKE_GRACE` (30 detik): kegagalan probe sebelum batas itu tidak mengubah observasi dan tidak menaikkan penghitung; sukses pertama mengembalikan state dari probe dengan reason `probe_ok` dan menghapus batas itu untuk observasi tersebut saja. Setelah batas lewat tanpa sukses, kegagalan berikutnya dihitung dengan ambang biasa. Wake saat instance `starting` memperpanjang deadline readiness start dengan lama sleep; wake saat `stopping` atau `recovering` tidak mengubah apa pun. Port `Fixed` dan port `Auto` yang tersimpan tidak pernah berubah karena wake, dan restart policy tidak dipicu oleh wake.
- **AC-10** (ukuran data terjadwal): Selain permintaan on demand yang sudah ada (`instance.get_data_usage`, dengan cache dan TTL yang tidak berubah), `DataUsageService` menyimpan hasil pindai terakhir per instance di slot `last_scan` tanpa TTL (ukuran, `complete`, `scannedAt`), yang diisi oleh setiap pemindaian, on demand maupun terjadwal. Daemon memindai instance yang baru menjadi `running` sekali, lalu setiap `DATA_USAGE_SCHEDULE_INTERVAL` (15 menit) selama running, satu pemindaian dalam satu waktu, dengan batas `DataUsageConfig` yang sudah ada. Hasil parsial dihitung sebagai pindaian dengan `complete` false. `instance.get_metrics` hanya membaca `last_scan` (tidak pernah memulai pemindaian); kartu Disk menulis "Scanned {jam}", atau "at least {ukuran}" untuk hasil parsial.
- **AC-11** (`instance.get_metrics`): Method baru `instance.get_metrics { instanceId, historyId?, afterSeq? }` (recovery refused) menjawab `state` `live` (lifecycle `running` dengan proses terverifikasi) atau `not_running` (semua kasus lain, dengan `lifecycle` instance), `intervalSeconds`, `capacity`, `historyId` dan proses utama (`pid`, `startedAt`) saat live, `samples`, `historyReset`, `restartTotal`, `lastFailure`, readiness, health, dan `dataUsage`. Aturan cursor: tanpa `historyId` atau dengan `historyId` lain, semua sampel dikirim dan `historyReset` true; dengan `historyId` yang sama, sampel dengan `seq` lebih besar dari `afterSeq` dikirim, `historyReset` true kalau `afterSeq + 1` lebih kecil dari `seq` tertua, dan `afterSeq` yang lebih besar dari `seq` terbaru menghasilkan daftar kosong tanpa reset. Hanya sampel terbaru yang membawa `processes`. Balasan terbesar (60 sampel dan 200 baris proses) muat dalam frame control 1 MiB, dibuktikan oleh test ukuran. Instance yang tidak ada menjawab `instance.not_found`. Fixture `protocols/fixtures/` untuk `live`, `not_running`, `historyReset`, dan nilai unavailable lolos test kontrak Rust dan Swift, dan `ProtocolVersion::CURRENT` naik satu minor di commit yang sama.
- **AC-12** (CLI): `stackly instance metrics <target>` menerima nama atau UUID lewat `TargetArg` (nama yang tidak dikenal keluar 10 dengan `cli.instance_not_found`), mencetak sampel terbaru untuk `main` dan `tree` beserta label cakupan, uptime, restart sejak dibuat, kegagalan terakhir, readiness, health dengan latensi, ukuran data dengan waktu pindai, dan daftar proses; nilai unavailable dicetak `unavailable ({reason})`. `--json` mencetak balasan `instance.get_metrics` apa adanya. `--watch` mencetak satu baris per sampel baru memakai `historyId` dan `afterSeq` sampai Ctrl C, dengan perilaku keluar yang sama dengan `stackly instance logs --follow` (spec 0027), dan satu baris "history reset" saat `historyReset`. Instance yang tidak running mencetak "not running ({lifecycle})" dan keluar 0.
- **AC-13** (layar Metrics): Tab Metrics di detail instance mengikuti mockup UI dengan selisih yang dicatat di spec ini: kontrol segmented Main Process dan Process Tree (default Main Process), lima kartu (CPU, Memory, Disk, Disk I/O, Restarts) untuk scope terpilih, grafik CPU 60 sampel terakhir untuk scope terpilih dengan teks "sampled every {intervalSeconds} s", daftar Processes yang selalu menampilkan seluruh pohon (pid, nama diindentasi menurut `depth`, memori, CPU) dengan baris "{n} more processes not shown" kalau `processesTruncated`, dan header health dari AC-7. Memori tampil dalam MB atau GB dengan satu desimal, CPU dengan satu desimal dan tanda persen. Kartu dengan nilai unavailable menampilkan "Unavailable" dan teks tetap per reason: `warming_up` "Measuring…", `not_permitted` "macOS denied access", `process_exited` "Process exited", `not_supported` "Not supported on this Mac", `not_scanned` "Not scanned yet", dan nilai lain "Not available". Saat `not_running`, tab menampilkan state "No Live Metrics" dengan tombol Start yang aktif hanya kalau `InstanceActionModel` mengizinkan start. Tab meminta `instance.get_metrics` setiap `intervalSeconds` hanya selama tab terlihat, dengan `historyId` dan `afterSeq`, dan mengganti seluruh history kalau `historyReset`.
- **AC-14** (NFR-003): Dengan 10 instance fake idle yang running (readiness, health, sampling, dan pemindaian terjadwal aktif, tanpa client dan tanpa spam log) selama 5 menit di bawah `cargo xtask evidence run`, RSS daemon paling banyak 150 MiB dan CPU rata rata paling banyak 1 persen satu core logis, diukur dari luar daemon oleh harness test (bukan oleh sampler daemon), dan dicatat sebagai evidence native dengan `covers` NFR-003 dan REQ-018. Run kedua yang sama dengan satu client yang memanggil `instance.get_metrics` setiap 5 detik untuk kesepuluh instance juga dicatat. Run ketiga mengganti satu instance fake dengan PostgreSQL 17 native (pohon multiprocess), atau `BLOCKED` kalau artefak tidak ada di cache.
- **AC-15** (evidence native): Test native PostgreSQL 17 dan NATS (cache runtime xtask) membuktikan AC-2, AC-4, dan AC-5 pada proses asli, dan test wake memakai clock palsu di daemon in process untuk AC-9 tanpa sleep sungguhan. Artefak yang tidak ada di cache membuat test `BLOCKED` dengan alasan `artifact <id> is not in the runtime cache` dan owner `@ojiepermana`. Cek manual di aplikasi (AC-13, termasuk Mac yang benar benar sleep lalu bangun dengan instance PostgreSQL ready) dicatat sebagai evidence manual `native`, atau `BLOCKED` kalau tidak dijalankan.

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native, dan artefak, sesi GUI, atau Mac yang bisa sleep yang tidak tersedia dicatat `BLOCKED`.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md), dipilih user pada 2026-11-02: CPU dalam persen satu core, memori physical footprint, Disk I/O diukur untuk semua provider lewat `proc_pid_rusage`, 60 sampel di memori, pohon proses dari turunan proses utama, sampling selalu jalan setiap 5 detik selama running, ukuran data on demand ditambah pindai saat mulai running dan setiap 15 menit, wake dideteksi lewat selisih jam dinding dan jam monotonic, probe langsung setelah wake dengan masa tenggang 30 detik, `restart_total` sebagai kolom registry yang naik di commit spawn, hanya restart setelah crash yang dihitung, sampel dibaca lewat method dengan cursor (bukan event), CLI `metrics` dengan `--json` dan `--watch`, konstanta probe 10 dan 30 detik dipertahankan, baris proses berisi PID, nama executable, memori, dan CPU, dan latensi probe di baris Readiness serta header Metrics. Cross check 2026-11-02 menambah 12 perbaikan yang diterima user (rationale.md *Cross check*).

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · skill Swift di [apps/macos/AGENTS.md](../../../apps/macos/AGENTS.md) untuk tab Metrics

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Feature design

### Batas fitur

- **Di dalam**: tipe metrik di domain, port daftar PID dan penghitung proses beserta adapter macOS, sampler di core, `WakeWatch` dan masa tenggang probe, latensi probe, kolom `restart_total`, slot pindai terakhir dan pemindaian terjadwal, method `instance.get_metrics`, field `restartTotal` dan `latencyMs`, CLI `stackly instance metrics`, tab Metrics, baris Restarts dan latensi di Summary dan inspector, evidence NFR-003, dan test native PostgreSQL dan NATS.
- **Di luar**: history metrik yang bertahan setelah daemon restart atau lebih dari 60 sampel (PRD 35.2 meminta in memory bounded); alert atau notifikasi metrik (fitur menu bar 32 dan fitur gagal 33 sampai 35); metrik khusus provider seperti jumlah koneksi PostgreSQL; timestamp kegagalan terakhir yang bertahan setelah daemon restart (tetap null seperti hari ini, `InstanceLastFailure.at`); metrik untuk Windows (`not_supported`); restart karena wake (tidak pernah).

### Model data

Satu migrasi registry, nomor bebas berikutnya di `crates/storage/migrations/` (hari ini `0007_instance_restart_total.sql`):

| Tabel | Kolom | Tipe | Null | Aturan | Penulis |
| --- | --- | --- | --- | --- | --- |
| `instances` | `restart_total` | `INTEGER` | tidak | `NOT NULL DEFAULT 0 CHECK (restart_total >= 0)` | commit spawn setelah crash (AC-6) |

- Satu `ALTER TABLE ... ADD COLUMN`, tanpa membangun ulang tabel; baris lama membaca 0 dan migrasi tidak menebak restart yang dulu terjadi. Test migrasi membuka registry dengan versi skema sebelum migrasi ini, sesuai cara `crates/storage/src/migration.rs` menomori versi.
- Domain: field `restart_total: u64` di aggregate `Instance` dan di struct pemuat `stored` di `crates/domain/src/instance.rs` (di wire menjadi newtype `Count` dari `wire.rs`, dibatasi `MAX_SAFE_INTEGER`), naik lewat satu metode domain yang dipanggil di commit spawn. Penanda "spawn ini mengikuti crash" ada di `StartRequest` (true untuk `respawn` dan untuk percobaan ulang setelah crash di dalam operasi start), sehingga satu spawn yang berhasil menaikkan tepat satu. Commit itu sudah menaikkan revision karena lifecycle berubah, jadi tidak ada commit tambahan.

Tipe di memori daemon (domain, tanpa tokio dan tanpa API OS, supaya lolos `cargo check` Windows):

| Tipe | Isi | Aturan |
| --- | --- | --- |
| `MetricsHistory` | `history_id` (PID dan waktu start proses utama), ring `VecDeque<MetricsSample>` kapasitas 60, `next_seq`, baseline penghitung per identitas proses, cache fakta `inspect` per identitas, `latest_processes` | satu per instance running; dibuat baru saat identitas proses utama berubah |
| `MetricsSample` | `seq: u64` (di wire `Sequence`), `sampled_at: Timestamp`, `main: ResourceUsage`, `tree: ResourceUsage` | `seq` naik satu, mulai 1 per history |
| `ResourceUsage` | `scope: UsageScope { Main, Tree }`, `process_count: u64`, `measured_count: u64`, `cpu: Metric<f64>`, `memory: Metric<u64>`, `disk_read: Metric<u64>`, `disk_write: Metric<u64>` | `process_count` dan `measured_count` paling banyak 1 untuk `Main` |
| `ProcessUsage` | `pid`, `parent: Option<Pid>`, `depth: u32`, `name: String`, `is_main: bool`, `cpu: Metric<f64>`, `memory: Metric<u64>` | `name` adalah nama file dari `ProcessFacts.executable`; hanya untuk sampel terbaru, paling banyak 200 |
| `Metric<T>` | `Value(T)` atau `Unavailable(UnavailableReason)` | tidak ada nilai default; konstruktor nol tidak ada |
| `UnavailableReason` | `WarmingUp`, `NotPermitted`, `ProcessExited`, `NotSupported`, `NotScanned` | terbuka di wire (`Unrecognized`) |

Perhitungan selisih (CPU, Disk I/O), penjumlahan pohon, urutan pohon, dan `depth` adalah fungsi murni di domain, dengan unit test tanpa fake.

### Port platform

Dua method baru di `ProcessSupervision` (`crates/platform/src/process.rs`), dengan jawaban `not_supported` di implementasi non macOS:

| Method | Input | Output | macOS |
| --- | --- | --- | --- |
| `list_pids()` | | `Vec<Pid>` semua proses | `proc_listallpids`, buffer diperbesar sampai muat |
| `counters(pid, expected: ProcessStart)` | `Pid`, waktu start yang diharapkan | `ProcessCounters { cpu_user, cpu_system, footprint, disk_read, disk_write }` | `proc_pid_rusage` dengan `RUSAGE_INFO_V4`, waktu CPU dikonversi dari mach time lewat `mach_timebase_info` |

- `counters` menjawab `ProcessError::NotFound` kalau proses hilang, `ProcessError::IdentityMismatch` kalau waktu start tidak cocok (PID dipakai ulang), dan `ProcessError::PermissionDenied` untuk `EPERM`. Core memetakan dua yang pertama ke `process_exited` dan yang ketiga ke `not_permitted`.
- Parent, uid, dan executable datang dari `inspect` yang sudah ada, dipanggil sekali per identitas proses baru.
- Kode adapter ada di modul `process` `stackly-platform-macos`, setiap blok `unsafe` dengan `// SAFETY:`. Tidak ada API spawn baru, jadi aturan `check-deps` tidak berubah.
- `FakeProcessSupervision` di testkit mendapat daftar proses, parent, uid, dan penghitung yang bisa diatur test, dan conformance di `crates/testkit/src/conformance/` menguji kedua implementasi.

### Sampler

`stackly_core::metrics::Sampler`, satu task untuk seluruh daemon, dengan uid daemon dari capability `UserIdentity` yang dibaca sekali saat mulai:

1. Setiap `METRICS_SAMPLE_INTERVAL` (tokio interval, `MissedTickBehavior::Skip`), sampler membaca identitas proses utama setiap instance `running` dari status supervisor.
2. Satu `spawn_blocking` per tick: `list_pids` sekali, `inspect` untuk PID yang identitasnya belum ada di cache, pohon setiap instance dari parent, lalu `counters` untuk setiap anggota dengan `Instant` diambil tepat sebelum dan sesudah pembacaan.
3. History dibuat untuk identitas baru, dibuang untuk instance yang tidak lagi running atau yang proses utamanya berganti; cache `inspect` dibersihkan dari identitas yang tidak lagi terlihat.
4. Sampler tidak berlangganan `WakeWatch`: waktu CPU dan jam monotonic sama sama berhenti selama sleep, jadi selisih tetap benar (dibuktikan di spike langkah 0).

History disimpan di `Arc<Mutex<..>>` yang dibaca handler `instance.get_metrics` tanpa menunggu tick.

### Wake dan masa tenggang

`stackly_core::supervisor::wake::WakeWatch`: satu task dengan interval `WAKE_CHECK_INTERVAL`, menyimpan pasangan (`Clock::now`, `tokio::time::Instant`) dari tick sebelumnya. Kalau `(wall delta) - (monotonic delta) > WAKE_GAP_THRESHOLD`, ia mengirim `Wake { slept_for }` lewat `watch::Sender` dan menulis log info. Jam monotonic tokio berhenti selama Mac sleep (komentar di `restart.rs`, dibuktikan ulang di spike langkah 0), jadi selisih itu sama dengan lama sleep. Jam dinding yang mundur diabaikan; jam dinding yang maju karena diubah pengguna terbaca sebagai wake, dan akibatnya hanya satu pemeriksaan ulang.

Loop readiness dan health (`readiness.rs`) memakai `select!` atas tick probe dan `Wake`, dengan `grace_until` terpisah untuk readiness dan health:

| Kejadian | Tindakan |
| --- | --- |
| `Wake` saat `running` | observasi `unknown` reason `waking`, detail `rechecking after sleep`, `observed_at` sekarang, tanpa latensi; penghitung kegagalan 0; `grace_until = now + WAKE_GRACE`; probe sekarang |
| `Wake` saat `starting` | deadline readiness start diperpanjang `slept_for`; tidak ada observasi `waking` |
| `Wake` saat `stopping` atau `recovering` | tidak ada tindakan |
| probe gagal sebelum `grace_until` | observasi dan penghitung tidak berubah |
| probe sukses | observasi dari probe seperti biasa (`probe_ok`); `grace_until` observasi itu dihapus |
| `grace_until` lewat | aturan biasa; kegagalan berikutnya dihitung dengan ambang `READINESS_FAILURE_THRESHOLD` dan `failure_threshold` health |

Observasi `waking` berumur paling lama `WAKE_GRACE`, sama dengan `READINESS_MAX_AGE`, jadi kalau tidak ada probe yang berhasil, `effective` membuatnya stale (`unknown` reason `stale`) tepat saat masa tenggang habis.

### Latensi probe

`ProbeOutcome::{Ok, Failed}` (spec 0010) mendapat durasi satu percobaan dari mulai sampai hasil, diukur dengan jam monotonic. Observasi domain mendapat `latency: Option<Duration>`, yang tidak ikut `differs_visibly`, jadi tidak memicu event. Saat probe gagal di bawah ambang (observasi tidak berubah), latensi observasi tetap dari probe yang membuatnya. DTO menulisnya sebagai `latencyMs` (`u32`, dibulatkan ke atas, dibatasi pada timeout probe).

### Ukuran data terjadwal

`DataUsageService` di `apps/daemon/src/detail.rs` mendapat slot `last_scan` per instance (tanpa TTL, dibuang saat instance dihapus) yang ditulis oleh setiap `scan`, dan task penjadwal: pindai sekali saat instance menjadi `running`, lalu setiap `DATA_USAGE_SCHEDULE_INTERVAL` selama running, satu per satu lewat `scan` yang sudah ada (batas entri, deadline 2 detik, concurrency yang sama). Cache on demand dan TTL nya tidak berubah. Wake tidak memicu pemindaian.

### API surface

| Method atau field | Arah | Input | Output | Error utama |
| --- | --- | --- | --- | --- |
| `instance.get_metrics` (recovery refused) | client ke daemon | `instanceId: Uuid`, `historyId: String` opsional, `afterSeq: Sequence` opsional | `MetricsReply` (di bawah) | `instance.not_found`, `daemon.starting` (seperti method instance lain spec 0010) |
| `InstanceSummary.restartTotal` | snapshot dan event | | `Count` | |
| `InstanceReadiness.latencyMs`, `InstanceHealth.latencyMs` | snapshot dan event | | `u32` opsional | |
| CLI `stackly instance metrics <target> [--json] [--watch]` | pengguna ke CLI | target lewat `TargetArg` | teks atau JSON | exit 10 `cli.instance_not_found`, kode kategori biasa |

`MetricsReply` (camelCase di wire, field opsional `#[serde(default)]`, params `Validate`: `afterSeq` tanpa `historyId` ditolak, `historyId` paling panjang 64 byte):

```text
instanceId, state: live | not_running (terbuka), lifecycle,
intervalSeconds: u32, capacity: Count,
historyId?, process?: { pid, startedAt },
samples: [ { seq, sampledAt, main, tree } ],
latestProcesses: [ { pid, parentPid?, depth, name, isMain, cpu, memory } ],
processesTruncated: bool,
historyReset: bool,
restartTotal: Count, lastFailure?: InstanceLastFailure,
readiness: InstanceReadiness, health: InstanceHealth,
dataUsage: { type: "scanned", content: { bytes: Count, complete, scannedAt } } | { type: "unavailable", content: { reason } }
usage = { scope, processCount: Count, measuredCount: Count, cpu, memory, diskRead, diskWrite }
metric = { type: "value", content: number } | { type: "unavailable", content: { reason } }
```

Byte dan byte per detik dikirim sebagai `Count`; CPU sebagai number satu desimal. `historyId` dibentuk `"{pid}-{startedAt}"` dan diperlakukan client sebagai teks buram.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| sampel | CPU persen satu core | selisih `cpu_user + cpu_system` dua `counters` dibagi selisih `Instant` di sekitar pembacaan |
| sampel | memori | `ri_phys_footprint` dari `counters` |
| sampel | Disk I/O per detik | selisih `ri_diskio_bytesread` dan `ri_diskio_byteswritten` dibagi selisih `Instant` |
| sampel | `sampledAt` | `Clock::now()` |
| sampel | anggota pohon | `list_pids` dan parent dari `inspect`, mulai dari `ProcessStatus.pid`, disaring uid dari `UserIdentity` |
| sampel | `processCount`, `measuredCount` | jumlah anggota pohon, jumlah yang `counters` nya terbaca dan punya baseline |
| sampel | nama proses, `depth` | nama file dari `ProcessFacts.executable`, jarak ke proses utama di pohon |
| `instance.get_metrics` | `state`, `lifecycle`, `process` | status supervisor (lifecycle dan `ProcessStatus`) |
| `instance.get_metrics` | `historyId`, `historyReset` | identitas proses utama history dibanding params (AC-11) |
| `instance.get_metrics` | `restartTotal` | `instances.restart_total` |
| `instance.get_metrics` | `lastFailure` | `Instance::last_failure` dan detail actor, seperti `instance.get` |
| `instance.get_metrics` | readiness, health, `latencyMs` | observasi actor lewat `effective(now, READINESS_MAX_AGE)` |
| `instance.get_metrics` | `dataUsage`, `scannedAt` | slot `last_scan` `DataUsageService`, tanpa memindai |
| CLI | uptime | `sampledAt` sampel terbaru dikurangi `process.startedAt`, jadi umur pada sampel itu, bukan pada saat perintah berjalan; sebelum sampel pertama tidak ada baris Uptime (`apps/cli/src/commands/instance/metrics.rs`) |
| GUI | uptime | `now - process.startedAt` di client (fungsi `uptime` yang sudah ada di `DetailFormat.swift`) |
| GUI | "sampled every {n} s" | `intervalSeconds` balasan |
| GUI | header health | `health`; selama health `unknown` dan readiness tidak, `readiness` yang tampil (`MetricsFormat.swift`). `MetricsReply` tidak membawa tanda "provider tanpa health plan", jadi ini cara client membaca bagian "atau readiness" di AC-7; diterima lead tanpa perubahan kontrak (2026-11-02) |
| GUI | teks unavailable | tabel teks tetap di AC-13 |
| wake | `slept_for` | selisih `Clock::now` dikurangi selisih jam monotonic di `WakeWatch` |

### Key invariants

- Tidak ada metrik yang ditulis 0 karena tidak terukur; `Metric` tidak punya nilai default.
- Sampling dan pemindaian terjadwal tidak pernah mengirim probe, sinyal, atau menulis registry.
- `restart_total` naik tepat satu per spawn berhasil yang mengikuti crash, di commit spawn itu.
- Wake tidak pernah mengganti port, memicu restart, atau mengubah lifecycle dan desired state.
- Readiness dan health tetap tidak disimpan di registry, dan tetap tidak pernah memicu restart atau stop (spec 0010).
- History metrik tidak pernah melebihi 60 sampel per instance, daftar proses tidak pernah melebihi 200 baris, dan balasan `instance.get_metrics` selalu muat dalam 1 MiB.

### Security model

Socket privat milik user yang sama (spec 0006) adalah satu satunya akses; tidak ada aturan baru. Pohon hanya berisi proses dengan uid efektif yang sama dengan daemon. Nama proses diambil dari path executable yang terverifikasi, tidak pernah dari argv atau judul proses, jadi nilai yang diketik di command line tidak pernah tampil. Sampel tidak memuat secret dan tidak ditulis ke log.

### Configuration required

Tidak ada env var baru. Konstanta:

| Konstanta | Nilai | Tempat |
| --- | --- | --- |
| `METRICS_SAMPLE_INTERVAL` | 5 detik | `stackly_core::metrics` |
| `METRICS_HISTORY_CAPACITY` | 60 | `stackly_core::metrics` |
| `METRICS_PROCESS_ROWS` | 200 | `stackly_core::metrics` |
| `WAKE_CHECK_INTERVAL` | 2 detik | `supervisor::wake` |
| `WAKE_GAP_THRESHOLD` | 5 detik | `supervisor::wake` |
| `WAKE_GRACE` | 30 detik | `supervisor::wake` |
| `DATA_USAGE_SCHEDULE_INTERVAL` | 15 menit | `apps/daemon/src/detail.rs` |
| `READINESS_REFRESH_INTERVAL`, `READINESS_MAX_AGE` | 10 dan 30 detik, tidak lagi provisional | tempat yang sudah ada |

Semua bisa diperpendek lewat `SupervisorConfig` dan `DataUsageConfig` untuk test, seperti konstanta spec 0010.

### Edge cases

- Proses utama keluar di tengah tick: `main` `process_exited`, history dibuang saat lifecycle berubah.
- PID anak dipakai ulang di antara dua sampel: identitas berbeda, jadi proses itu dianggap baru, `inspect` dipanggil lagi, dan ia tidak ikut selisih CPU di sampel itu.
- Proses diadopsi setelah daemon restart: history mulai kosong, sampel pertama `warming_up`.
- Client memegang cursor dari proses sebelumnya: `historyId` berbeda, `historyReset` true.
- PostgreSQL dengan lebih dari 200 backend: nilai `tree` tetap menjumlah semuanya; hanya daftar yang dipotong.
- Daemon offline saat tab Metrics terbuka: tab menampilkan data terakhir tanpa diredupkan (spec 0012) dan banner offline shell; polling berhenti dan mulai lagi saat tersambung.
- Pemindaian yang melewati deadline: hasil parsial dengan `complete` false.
- Mac sleep lebih lama dari `restart_window`: tidak berpengaruh, restart policy memakai jam monotonic.
- Sampler terlambat karena beban: tick yang terlewat dibuang (`Skip`), selisih dihitung dari `Instant` di sekitar pembacaan.

### Critical test scenarios

| Test | Tempat | Membuktikan |
| --- | --- | --- |
| selisih CPU dan Disk I/O, pembulatan, ring 60, `seq`, jumlah pohon dan `measuredCount`, urutan dan `depth`, `Metric` tanpa default | unit domain | **AC-1**, **AC-2**, **AC-3**, **AC-4** |
| sampler atas `FakeProcessSupervision`: pohon, siklus parent, uid lain, `EPERM`, proses hilang, PID dipakai ulang, cache `inspect`, history baru saat proses berganti, potongan 200 baris | `crates/core/tests/metrics/` | **AC-1**, **AC-3**, **AC-5** |
| conformance `list_pids` dan `counters` pada child proses test asli (fork tiga level) | `crates/testkit` conformance, `stackly-platform-macos` tests | **AC-2**, **AC-5** |
| `restart_total`: respawn dan percobaan ulang setelah crash naik, spawn gagal, start dan restart pengguna tidak; migrasi registry | core supervisor tests, storage tests | **AC-6** |
| wake dengan clock palsu dan tokio paused: `unknown` `waking`, probe langsung, kegagalan dalam grace tidak dihitung, grace terpisah readiness dan health, wake saat `starting`, port tetap | `crates/core/tests/supervisor_start/wake.rs` dan unit test `crates/core/src/supervisor/wake.rs` | **AC-9**, bagian wake **AC-15** |
| latensi sukses dan gagal di observasi, perubahan latensi saja tanpa event | core supervisor tests | **AC-7** |
| `instance.get_metrics`: live, not running per lifecycle, aturan cursor, `historyReset`, not found, ukuran balasan terbesar di bawah 1 MiB; `last_scan` setelah TTL cache lewat; fixture kontrak | `apps/daemon/tests/`, test kontrak Swift | **AC-10**, **AC-11** |
| CLI teks, `--json`, `--watch` dengan reset, target tidak dikenal | `apps/cli/tests/` | **AC-12** |
| snapshot tab Metrics: live main, live tree, sebagian terukur, unavailable, daftar terpotong, not running (spec 0068) | StacklyUI snapshot | **AC-13** |
| 10 instance fake idle 5 menit, dengan dan tanpa client, dan dengan satu PostgreSQL, pada `stacklyd` build release (`cargo test --release`) | evidence run, `apps/daemon/tests/metrics_nfr003.rs` | **AC-14** |
| PostgreSQL 17 dan NATS native | `apps/daemon/tests/` native | **AC-4**, **AC-15** |

Dua cara ukur yang dipakai saat build, tanpa mengubah teks AC:

- **AC-14** diukur pada `stacklyd` build release, karena build itu yang dikirim. `cargo test` tanpa `--release` memakai `target/debug/stacklyd` tanpa optimasi, dan run 2 di build itu (1,02 sampai 1,14 persen) gagal hanya karena profil build; build release lulus ketiga run. Harness menulis `profile=debug|release` di baris hasil dan catatan record (keputusan lead 2026-11-02).
- **AC-15**, bagian wake: test wake menjalankan supervisor core di dalam proses dengan clock palsu, bukan `stacklyd` in process, karena `DaemonConfig` belum punya seam clock. Logika wake ada di core, dan daemon hanya menyambungkan clock asli, jadi lead menilai bagian ini terpenuhi (2026-11-02). Seam clock ada di Follow-up.

### Usulan update untuk spec lain

- Spec 0005: kolom `instances.restart_total` dan migrasi 0007.
- Spec 0010: `READINESS_REFRESH_INTERVAL` dan `READINESS_MAX_AGE` final, `WakeWatch` dan masa tenggang di bagian *Probe, readiness, dan health*, latensi di `ProbeOutcome`, penanda crash di `StartRequest`.
- Spec 0021: Follow-up "Fitur 28: jumlah restart sejak dibuat dan latensi probe" selesai lewat AC-6 dan AC-7.
- `docs/scope/ui-macos.md` (milik `/scope`): selisih yang diketahui untuk Metrics: "sampled every 5 s", Disk I/O diukur untuk semua provider (bukan capability), Restarts hanya menghitung restart setelah crash, kolom CPU di daftar Processes, Memory adalah footprint (bukan Resident).

## Build plan

Urutan Journey: satu jalur "lihat pemakaian instance" dibangun lengkap dari OS sampai layar, lalu wake dan evidence.

0. Spike di Mac ini: `proc_pid_rusage` `RUSAGE_INFO_V4` pada pohon PostgreSQL 17 native dan NATS dari cache xtask; catat unit waktu CPU (mach time dan `mach_timebase_info`), apakah `shared_buffers` ikut terhitung di footprint setiap child (bukan hanya main dibanding tree), nilai disk I/O, dan bahwa `tokio::time::Instant` serta waktu CPU proses tidak maju selama Mac sleep, di `rationale.md` *Hasil spike*. Kalau shared buffers terhitung per child, keputusan memori kembali ke `/architect` sebelum langkah 1. Memenuhi **AC-2**, **AC-4**, **AC-9**.
1. Domain: `Metric`, `UnavailableReason`, `ResourceUsage`, `ProcessUsage`, `MetricsSample`, `MetricsHistory`, fungsi selisih, jumlah, dan urutan pohon, dan `restart_total` di `Instance` dan struct `stored`; unit test. Memenuhi **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-6**.
2. Migrasi `0007_instance_restart_total.sql`, mapping storage, test migrasi, penanda crash di `StartRequest`, dan kenaikan di commit spawn (`startup.rs`, dipakai oleh `respawn.rs`). Memenuhi **AC-6**.
3. Port `list_pids` dan `counters`, adapter macOS, fake testkit, conformance, dan integration test adapter. Memenuhi **AC-2**, **AC-3**, **AC-5**.
4. Sampler di core dengan test atas fake. Memenuhi **AC-1**, **AC-3**, **AC-5**.
5. Protocol: `instance.get_metrics`, `MetricsReply`, `restartTotal`, `latencyMs`, fixture, `cargo xtask codegen`, kenaikan minor `ProtocolVersion::CURRENT`, dan `cargo xtask fuzz seed`; handler daemon, start sampler di `stacklyd::server::start`, slot `last_scan` dan pemindaian terjadwal; test daemon termasuk ukuran balasan. Memenuhi **AC-10**, **AC-11**, **AC-6**.
6. CLI `stackly instance metrics` dan baris Restarts di `inspect`; test CLI. Memenuhi **AC-12**, **AC-6**.
7. Swift: client `getMetrics` dan test fixture di `clients/swift`; tab Metrics di `InstanceTab` sesudah `logs`, kartu, grafik Swift Charts, daftar proses, teks unavailable, state not running; Restarts dan latensi di Summary dan inspector; snapshot spec 0068. Memenuhi **AC-13**, **AC-6**, **AC-7**.
8. Latensi probe di `ProbeOutcome` dan observasi, lalu `WakeWatch`, masa tenggang, perpanjangan deadline saat `starting`, penghapusan penanda provisional, dan test wake. Memenuhi **AC-7**, **AC-8**, **AC-9**.
9. Evidence: tiga run NFR-003, test native PostgreSQL 17 dan NATS, cek manual GUI termasuk sleep sungguhan. Memenuhi **AC-14**, **AC-15**, **AC-4**.

Langkah 3 bisa dimulai bersamaan dengan langkah 1 dan 2 (file berbeda). Langkah 8 hanya bergantung pada `latencyMs` di DTO langkah 5, jadi bisa jalan sebagai sesi paralel setelah langkah 5.

## Consequences

**Positive**:

- Pemakaian sumber daya terlihat sama di GUI dan CLI, dengan unavailable yang jujur dan label cakupan yang jelas untuk PostgreSQL.
- Sampel tidak pernah memenuhi ring buffer event bus, jadi event status dan operasi tidak terganggu.
- Crash berulang tetap terlihat lewat `restartTotal` walau jendela restart policy sudah reset.
- Wake tidak membuat instance tampak gagal, tanpa kode run loop atau `unsafe` baru untuk notifikasi daya.

**Negative / tradeoffs**:

- Daemon bekerja setiap 5 detik walau tidak ada yang melihat; biayanya harus dibuktikan oleh AC-14, dan kalau gagal cadence perlu dirancang ulang.
- GUI polling (bukan push) selama tab terbuka: satu request per 5 detik per tab, dan jeda tampil sampai satu interval.
- Setiap wake membuat readiness dan health `unknown` sebentar sampai probe pertama menjawab, dan instance yang mati saat sleep baru terlihat gagal setelah masa tenggang ditambah ambang kegagalan.
- Deteksi wake bisa terlambat sampai 2 detik, dan perubahan jam dinding ke depan oleh pengguna terbaca sebagai wake.
- Footprint berbeda dari RSS yang dikenal sebagian developer; label harus menjelaskannya, dan mockup "Resident" diganti.
- CPU pohon sedikit di bawah nilai sebenarnya untuk proses yang hidup kurang dari satu interval.
- Restart pengguna tidak terhitung di `restartTotal`; siapa yang ingin jumlah semua restart harus membaca histori operasi.
- History hilang setiap daemon restart.

**Neutral**:

- Satu migrasi registry dan satu kenaikan minor protocol.
- `ProcessSupervision` tumbuh dua method; implementasi non macOS menjawab `not_supported`.

## Follow-up

- [x] Langkah 0 spike: catat hasilnya di `rationale.md` sebelum langkah 1.
- [ ] Terapkan *Usulan update untuk spec lain* di spec 0005, 0010, dan 0021 saat langkah yang terkait selesai.
- [ ] `/scope`: catat selisih Metrics di `docs/scope/ui-macos.md`.
- [ ] `/sync` setelah build: catat `instance.get_metrics`, `restart_total`, sampler, dan `WakeWatch` di root `AGENTS.md`.
- [ ] Seam clock di `DaemonConfig`, supaya test wake AC-15 bisa berjalan di `stacklyd` in process, bukan hanya di supervisor core.
- [ ] Usulan debugger NFR-003 (`dbg-f28-nfr003`, 2026-11-02), bukan syarat AC-14 yang sudah lulus di build release: (a) satu daftar registry per tick yang dipakai bersama sampler dan `start_data_scans` (`apps/daemon/src/ipc/instance/metrics.rs`, dimulai di `server.rs`), karena kini keduanya membaca `MainProcesses::running` sendiri sendiri; (b) cadence pompa spool (`SPOOL_DRAIN_INTERVAL`) di build debug. Fix kecil yang sudah masuk: daftar parent proses dibangun sekali per tick (`8122ef3f`).

## Rationale

Alasan dan opsi: lihat [rationale.md](rationale.md).
