# 0010. Process supervisor

**Date**: 2026-10-18
**Status**: Proposed

## Summary

Spec ini memutuskan cara `stacklyd` menjalankan, mengawasi, dan menghentikan proses provider. Proses instance dijalankan di session sendiri dalam keadaan tertahan sampai identitasnya tercatat, dengan argv, env, dan cwd persis dari plan spec 0009, menulis log ke file spool yang tetap hidup walau daemon mati, dan memegang lock file instance yang diwariskan (lock yang ikut dipegang proses itu). Setelah daemon restart, proses yang masih hidup hanya diadopsi kalau identitas majemuknya terbukti (PID, waktu start, executable, user, boot, session, dan lock data); yang tidak terbukti menjadi `unknown` dan tidak pernah dibunuh. Restart policy membatasi crash loop di 5 percobaan per 5 menit dengan backoff dan jitter, dan spec ini juga menyediakan method IPC minimal (create, start, stop, get, list) supaya walking skeleton (fitur 14) bisa dibangun dari spec Foundation saja.

## Requirements

**User stories**:

- Sebagai pengguna, saya ingin database lokal saya tetap berjalan walau daemon Stackly crash atau di restart, dan tidak pernah ada server kedua yang menyentuh data yang sama, supaya data saya aman dan pekerjaan saya tidak terputus.
- Sebagai pengguna, saya ingin melihat process, readiness, health, dan desired state secara terpisah, supaya proses yang hidup tetapi belum siap tidak tampil sebagai siap (AC-012).
- Sebagai pengguna, saya ingin provider yang terus crash berhenti dicoba setelah batasnya dan menunjukkan error terakhir, supaya Mac saya tidak sibuk menjalankan ulang proses yang pasti gagal (AC-068).
- Sebagai developer fitur 14, 20, 21, 26, 27, dan 30, saya ingin satu executor plan, satu port proses, dan satu set method dan event instance, supaya fitur saya cukup menambah di atasnya tanpa menafsirkan ulang PRD bagian 19.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-013):

- **AC-1**: `MacosProcessSupervision::spawn` memakai `posix_spawn` dengan `POSIX_SPAWN_SETSID`, `POSIX_SPAWN_SETSIGDEF` (semua sinyal default, termasuk `SIGPIPE`), mask sinyal kosong, dan `POSIX_SPAWN_CLOEXEC_DEFAULT`, ditambah `POSIX_SPAWN_START_SUSPENDED` untuk proses utama. argv[0] adalah path canonical executable, argv[1..] sama persis dengan `args` plan, env sama persis dengan env plan (tidak ada yang diwarisi dari daemon), dan cwd dari plan. Daftar fd proses utama yang masih tertahan hanya memuat fd 0 (`/dev/null`), fd 1 dan 2 (spool), dan fd 3 (lock instance). Dengan `TestRoot` di bawah folder bernama `Test Root Ünï cödé`, `argv-serve.json` dummy server sama persis dengan plan, termasuk spasi, Unicode, dan metakarakter shell, dan tidak ada file yang dibuat oleh tafsiran shell (AC-058, AC-031).
- **AC-2**: Proses utama dibuat tertahan (belum menjalankan kode apa pun). Selama tertahan, supervisor menangkap `ProcessIdentity` (PID, waktu start dengan mikrodetik, path executable, session ID, dan boot ID), menulis lifecycle `spawned`, port `Auto` yang diberikan, dan satu baris `process_observations` dalam satu `WriteBatch`, mendaftarkan pengawasan exit, lalu melanjutkan proses dengan `SIGCONT`. Kalau penangkapan atau commit gagal, child yang belum pernah berjalan dibuang lewat `SuspendedChild::discard` dan start gagal dengan `process.identity_capture_failed` (`start_failed`), tanpa pernah ada proses berjalan yang tidak tercatat.
- **AC-3**: Start mengikuti tahap startup transaction PRD 19.2 dalam urutan *Startup transaction*, masing masing terlihat sebagai `stage` operasi. Untuk fake provider: `instance.start` dijawab dalam 500 ms (NFR-006), lifecycle `starting` lalu `running`, readiness `not_ready` sampai probe `/ready` berhasil, lalu `ready`, dan operasi selesai `succeeded`. Port `Auto` tersimpan sejak `spawned` dan baru ditampilkan sebagai final (`bound: true`) setelah readiness berhasil (REQ-010). `run_plan` dan `ensure_not_in_argv` berjalan sebagai dry run sebelum `journal`, sehingga kegagalan validasi apa pun tidak mengubah instance.
- **AC-4**: `instance.get`, `instance.list`, dan event menampilkan lifecycle, desired state, readiness, health, dan proses sebagai field terpisah. Fake dengan `readyDelayMs` 5000 terlihat `running` dengan readiness `not_ready` selama delay itu, bukan `ready` (AC-012, BR-010). Setelah ready, readiness diprobe ulang setiap 10 detik dan turun ke `not_ready` setelah 2 kegagalan berturut turut; nilai yang lebih tua dari `READINESS_MAX_AGE` dikirim sebagai `unknown` dengan reason `stale`. Readiness atau health yang buruk tidak pernah memicu restart atau stop (REQ-012).
- **AC-5**: Stop normal hanya menjalankan strategi provider (`StopPlan::Signal` ke proses utama yang identitasnya diperiksa ulang tepat sebelum `kill`, atau `StopPlan::Command`), tidak pernah `SIGKILL`, tidak pernah kill berdasarkan nama, dan tidak pernah menyinyal proses yang pemeriksaan ulangnya gagal (`process.signal_refused`, instance menjadi `unknown`). Setelah `stopSeconds` lewat, operasi gagal dengan kategori `timeout`, lifecycle tetap `stopping`, proses terus diawasi, dan begitu keluar instance menjadi `stopped` lewat handler exit yang sama. Stop pada `stopping` mengirim ulang strategi dengan anggaran baru. Anggota process group yang tersisa setelah proses utama keluar dilaporkan, tidak disinyal. Setelah proses keluar, `remove_secret_material` dipanggil (AC-011 bagian fake, REQ-011).
- **AC-6**: Supervisor tidak pernah menjalankan proses kedua pada data yang sama. Start dan init mengambil `flock` eksklusif pada `instance.lock` lebih dulu dan gagal dengan `supervisor.data_in_use` tanpa spawn kalau lock itu dipegang proses lain (termasuk turunan proses lama), atau kalau proses dari observasi terakhir masih hidup dengan identitas cocok walau lock bisa diambil (misalnya `instance.lock` dihapus pengguna). Sepuluh `instance.start` bersamaan dari dua client menghasilkan tepat satu spawn; start pada instance yang `starting`, `running`, atau `recovering` selesai tanpa efek samping (RISK-03).
- **AC-7**: Proses yang keluar tanpa diminta saat desired `running` dijalankan ulang dengan jeda 1, 2, 4, 8, dan 16 detik dikali jitter acak 0,5 sampai 1,0. Crash keenam dalam jendela geser 5 menit tidak dijalankan ulang: lifecycle `failed`, `last_failure` `crash_loop`, dan status restart `exhausted` dengan error terakhir. Total spawn untuk satu crash loop adalah 6, tidak pernah ada dua proses utama bersamaan, dan tidak ada spawn di antara jeda (tanpa busy loop). Selama jeda di luar operasi pengguna, tidak ada operasi yang memegang lock resource, jadi `instance.stop` langsung berjalan dan membatalkan jeda. Riwayat percobaan dihapus setelah readiness `ready` terus menerus 60 detik (AC-068).
- **AC-8**: Operasi start dari pengguna menunggu lintas percobaan: `succeeded` saat ready, `failed` dengan `supervisor.retry_budget_exhausted` saat budget habis, dan `failed` dengan `supervisor.readiness_timeout` (kategori `timeout`) saat deadline readiness percobaan berjalan habis, dengan proses tetap hidup, readiness `not_ready`, dan probe tetap berjalan. Cancel sebelum `journal` tidak punya efek samping; cancel setelah `journal` tetapi tanpa proses hidup (sebelum spawn atau saat jeda backoff) menjadi `stop_begin` lalu `exited` dengan desired `stopped`; cancel saat proses hidup menjalankan stop graceful, dan operasi berakhir `cancelled` setelah proses keluar.
- **AC-9**: Daemon yang dibunuh dengan `SIGKILL` saat instance fake `running` lalu dijalankan lagi mengadopsi proses yang sama (PID dan waktu start sama, tanpa spawn baru) hanya kalau semua pemeriksaan di *Identitas majemuk* lolos, termasuk fd proses itu memuat inode `instance.lock`. Lifecycle tetap `running`, readiness kembali `ready` setelah probe ke port yang tersimpan, pembacaan spool berlanjut, dan tidak ada data yang terhapus (AC-013 bagian fake, REQ-013). Proses terbukti yang masih tertahan (daemon mati setelah commit tetapi sebelum `SIGCONT`) dilanjutkan lalu diadopsi.
- **AC-10**: Observasi yang PID nya kini milik executable lain, atau milik proses dengan waktu start berbeda, tidak pernah menyebabkan sinyal ke PID itu. Kalau `instance.lock` bebas, proses lama dianggap sudah tidak ada; kalau lock dipegang, instance menjadi `unknown` dengan blocker `dataInUse` yang memuat hasil setiap pemeriksaan. Proses yang identitasnya cocok tetapi tidak memegang lock menjadi `unknown` dengan blocker `identityUnproven`. Error apa pun saat mengumpulkan bukti menghasilkan `Unproven`, tidak pernah `Gone`. Instance `unknown` menolak start dan stop, diperiksa ulang setiap 30 detik, dan hanya keluar lewat `adopted` atau `confirmed_stopped` (AC-067, RISK-14).
- **AC-11**: Rekonsiliasi di slot start daemon menerapkan tabel *Rekonsiliasi* untuk setiap lifecycle dan setiap hasil klasifikasi, hanya dengan transisi yang ada di tabel lifecycle spec 0004, termasuk boot baru (instance menjadi `stopped` tanpa start otomatis) dan boot yang sama (instance desired `running` yang prosesnya terbukti hilang dijalankan lagi dengan budget baru setelah daemon `ready`, paling banyak 4 bersamaan). `initializing` selalu menjadi `failed` dengan reason `interrupted`. Operasi start, stop, restart, dan reload instance yang terputus sudah ditutup `failed interrupted` oleh `recover` spec 0007 (kebijakan `CloseInterrupted`) sebelum rekonsiliasi; create yang terputus dikompensasi spec 0007. Event `daemon.reconciling` terkirim di awal dan akhir. Dalam recovery mode registry, supervisor tidak menyentuh proses apa pun.
- **AC-12**: stdout dan stderr proses utama masuk ke spool di folder run dan terus ditulis walau daemon mati. Daemon membaca spool, menyamarkan setiap baris logis utuh (sampai `LOG_LINE_HARD_MAX`) lewat `Redactor` (spec 0011) sebelum dipotong, lalu menyerahkannya ke `LogSink`; sink yang `Busy` tidak pernah menahan proses. Di bawah fake dengan `logLinesPerSecond` 2000 selama 60 detik, blok yang dialokasikan spool tetap di bawah 2 MiB berkat punch hole. Backlog di atas batas (bisa disetel lewat `SupervisorConfig` di test) dibuang dari bagian tertua dengan penanda truncation. Setelah daemon restart, pembacaan berlanjut dari data pertama yang belum dibebaskan, byte sampai newline pertama dibuang, dan batch pertama ditandai `resumed`.
- **AC-13**: `execute_init` menjalankan `InitPlan` fake: semua segmen secret diselesaikan sebelum efek samping pertama, `EnsureDirectories` dan `WriteFile` mengikuti aturan spec 0009, file secret sekali pakai dibuat di `<run>/secrets/` lewat `SecretFile` dan hilang di akhir aksi, langkah `Run` menerima secret lewat stdin lalu stdin ditutup, dan `token.sha256` yang ditulis fake sama dengan SHA-256 fixture credential. Langkah yang melewati timeout nya dihentikan menurut aturan pembantu dan init berakhir `failed` dengan `init_step_timeout`; init yang melewati `initSeconds` berakhir `init_timeout`. Setelah sukses, `instance-manifest.json` ditulis ulang dengan `data.initializedAt`.
- **AC-14**: `execute_reload` pada fake merender ulang config, mengirim `SIGHUP` ke proses utama yang terverifikasi, lalu memverifikasi readiness dalam `reloadSeconds`; `GET /config` mengembalikan greeting baru dan PID tidak berubah. Paket tanpa capability `reload` menghasilkan `supervisor.capability_unsupported` tanpa sinyal apa pun (BR-016). Reload tidak pernah menjalankan ulang proses.
- **AC-15**: Proses pembantu (langkah `Run` init, probe `exec`, perintah stop dan reload) yang melewati timeout nya menerima `SIGTERM` ke process group nya, lalu `SIGKILL` ke group itu setelah 5 detik kalau masih hidup (dibuktikan dengan pembantu yang mengabaikan `SIGTERM`). `SIGKILL` hanya bisa dikirim lewat `HelperHandle` atau `SuspendedChild::discard`, tidak pernah ke proses utama yang sudah berjalan atau proses yang diadopsi.
- **AC-16**: Method `instance.create`, `instance.start`, `instance.stop`, `instance.get`, dan `instance.list`, event `instance.state_changed`, `instance.health_changed`, dan `daemon.reconciling`, serta kode error area `supervisor`, `process`, dan `instance` ada di `stackly-protocol` dengan fixture di `protocols/fixtures/` yang lolos test kontrak Rust dan Swift. Snapshot dan kedua event instance membawa `statusSeq`, dan fixture resep client membuktikan `instance.health_changed` diterapkan kalau `statusSeq` nya lebih besar dari yang dipegang, bukan berdasarkan revision.
- **AC-17**: Port `Fixed` yang sedang dipakai proses luar menggagalkan start sebelum spawn dengan `supervisor.endpoint_in_use` yang menyebut nama endpoint, alamat, dan port, tanpa menyentuh proses luar itu (AC-010 dasar). Proses yang keluar sebelum ready lalu port nya terbukti dipakai pihak lain (bind percobaan gagal) menggagalkan start dengan kode yang sama tanpa memakai retry budget (AC-055 dasar); test memakai `PortProbe` yang bisa diinjeksi. Endpoint `Auto` memakai ulang `bound` lama kalau masih bebas, kalau tidak memilih port baru lewat `resolve_port`, dan setiap respawn mengulang pemeriksaan ini. Konflik dengan port efektif instance Stackly lain ditolak lewat `endpoints_conflict` (BR-004, BR-005).
- **AC-18**: Sebelum efek samping apa pun, start memeriksa runtime dan dependency nya `installed`, paket provider pinned yang digest nya sama dengan provider revision instance, executable canonical di dalam root runtime, folder instance milik user dan privat, `preflight` spec 0009 `Compatible`, dan `resolve_for_start` spec 0011; setiap kegagalan punya kode sendiri, dan needs secret menjadi blocker tanpa perubahan lifecycle. `ensure_instance_run_dir` dipanggil sebelum setiap aksi yang memakai folder run, dan `ensure_not_in_argv` sebelum setiap spawn. Hanya percobaan pertama operasi pengguna yang boleh memunculkan dialog Keychain; setiap respawn memakai interaksi `Never`.
- **AC-19**: `MacosPlatform` melaporkan capability `process_supervision` `supported`; `UnsupportedPlatform` mengembalikan `NotSupported` dari setiap method `ProcessSupervision`; `stackly-domain`, `stackly-platform`, `stackly-protocol`, dan `stackly-core` tetap lolos `cargo check --target aarch64-pc-windows-msvc` (probe ke socket Unix di core berada di balik `cfg(unix)`).
- **AC-20**: Dengan 10 instance fake idle (readiness dan health aktif, tanpa spam log) selama 5 menit, RSS daemon paling banyak 150 MiB dan CPU rata rata paling banyak 1 persen satu core logis, diukur di Mac ini dan dicatat sebagai evidence (NFR-003 bagian supervisor).
- **AC-21**: `SIGTERM` ke daemon menghentikan actor, probe, pembacaan spool, dan pengawasan tanpa menyinyal proses instance dan tanpa mengubah lifecycle di registry; operasi instance yang sedang berjalan terputus dan ditutup `failed interrupted` oleh `recover` spec 0007 pada start berikutnya (kebijakan `CloseInterrupted`), lalu rekonsiliasi mengadopsi prosesnya; start daemon berikutnya mengadopsi proses itu menurut AC-9.
- **AC-22**: Commit yang dipicu actor (exit, crash, `exited`, `adopted`, rekonsiliasi) yang gagal karena konflik revision dibaca ulang dan dicoba lagi paling banyak 5 kali, `Busy` dicoba lagi dengan jeda, dan `NoSpace` atau `Unavailable` membuat subsystem `supervisor` `degraded` sambil dicoba lagi setiap 5 detik. Selama state di memori dan di registry berbeda, supervisor tidak men spawn dan tidak menyinyal proses utama instance itu.
- **AC-23**: Setelah pencarian idempotency key (replay mengembalikan reply aslinya, spec 0007), method instance menjawab penolakan yang bisa diputuskan dari state (instance tidak ada, retained, `not_initialized`, `initializing`, `stopping`, `unknown`, `expectedRevision` tidak cocok) langsung sebagai error, tanpa membuat operasi. Start atau stop yang tidak punya efek samping menjawab `operationId` null dengan `alreadyInState: true` dan tidak membuat operasi, kecuali menyimpan desired state yang berbeda. Semua kegagalan lain muncul sebagai kegagalan operasi.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): proses instance tetap hidup saat daemon mati dan diadopsi lewat identitas majemuk, stdout dan stderr ke spool di folder run yang dibaca daemon, deadline readiness yang habis membiarkan proses berjalan, stop timeout tanpa eskalasi, restart policy 1 detik kali 2 dengan budget 5 per 5 menit tanpa override, retry di dalam operasi start, rekonsiliasi yang menjalankan lagi instance hanya di boot yang sama, lock instance yang diwariskan sebagai bukti asosiasi data dan penjaga proses kedua, cancel setelah spawn sebagai stop graceful, timeout pembantu TERM lalu KILL setelah 5 detik, method IPC instance minimal termasuk create di spec ini, urutan event status lewat `statusSeq` per instance, file secret di folder `secrets/` spec 0011, client HTTP kecil buatan sendiri, probe readiness rutin setiap 10 detik dengan debounce, spawn proses utama dalam keadaan tertahan sampai identitasnya tercatat, dan port `Auto` yang tersimpan sejak spawn.

Hanya supervisor di dalam daemon yang menjalankan, menyinyal, dan mengadopsi proses, dan setiap sinyal didahului pemeriksaan identitas; ketidakpastian selalu berakhir di `unknown`, tidak pernah di kill.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan untuk actor per instance, timer backoff dengan waktu tokio yang bisa dijeda di test, dan jembatan dari thread watcher platform ke task tokio.

## Feature design

### Batas fitur

- **Di dalam**: port `ProcessSupervision` dan adapter macOS, lock instance, identitas majemuk dan klasifikasinya, executor `InitPlan`, `RunPlan`, `ReloadPlan`, dan `StopPlan`, startup transaction, stop, probe `tcp`, `http`, dan `exec`, readiness dan health rutin, restart policy, rekonsiliasi dasar setelah daemon start, spool log beserta pembacanya, `LogSink` dan `TailSink`, alokasi port `Auto`, method IPC instance minimal (create, start, stop, get, list), event instance, kode error area `supervisor`, `process`, dan `instance`, fake proses di testkit, dan helper pemasang fake runtime untuk test.
- **Di luar**: force stop dan restart serta method reload untuk pengguna (fitur 26), log tahan lama dengan rotasi, cursor, dan follow, serta file log yang dikelola provider sendiri (`RunPlan.log_files`) (fitur 27), metrics, tuning kesegaran, dan pemeriksaan listener setelah startup (fitur 28), config revision lengkap dan edit config (fitur 29), pengerasan rekonsiliasi serta restart daemon terencana dan handover (fitur 30), start saat login dan logout (fitur 31), tampilan crash loop (fitur 33), doctor (fitur 34), preflight dan wizard create (fitur 20), trust provider (fitur 37), dan adapter Windows.

### Peta modul dan dependency

Aturan arah dependency spec 0001 dan 0004 tetap berlaku. Spec ini menambah:

| Crate | Dependency langsung yang baru | Isi baru |
| --- | --- | --- |
| `stackly-domain` | tidak ada | modul `process` (`Pid`, `ProcessStart`, `BootId`, `ProcessIdentity`, `ProcessObservation`, `IdentityCheck`, `IdentityEvidence`); varian blocker `DataInUse` dan `IdentityUnproven`; setter port `bound`; konstanta `FailureCode` dan `StageName` supervisor (semuanya lewat Follow-up spec 0004) |
| `stackly-platform` | tidak ada | modul `process` (trait `ProcessSupervision` dan tipenya), `UnsupportedProcessSupervision`, port ketujuh di `Platform` |
| `stackly-platform-macos` | tidak ada (tetap `libc`) | modul `process` (`spawn`, `inspect`, `signal`, `kill`, `watch`, `lock`, `spool`, `boot`) |
| `stackly-core` | `tokio` (fitur `rt`, `time`, `net`, `sync`, `macros`, `io-util`), `tracing`; dev: `tokio` fitur `test-util` | modul `supervisor` (`actor`, `config`, `commit`, `executor`, `start`, `stop`, `reload`, `init`, `probe`, `http`, `ports`, `restart`, `classify`, `reconcile`, `spool`, `sink`, `status`, `error`), use case `instance::create`, dan fitur crate `fixture` (mati secara default) berisi `supervisor::fixture::install_fake` |
| `stackly-protocol` | tidak ada | modul `instance` (DTO, params, event) dan `codes::{supervisor, process, instance}` |
| `stackly-storage` | tidak ada | implementasi op dan baca registry baru dari update spec 0005 |
| `stackly-testkit` | tidak ada | modul `fake_process` (`FakeProcessSupervision`) dan pemasangannya di `FakePlatform` |
| `stacklyd` | tidak ada | `ipc::instance`, langkah supervisor di slot start dan saat berhenti, pemetaan DTO, dan pemasangan `LogSink` |
| `xtask` | tidak ada | aturan baru di `check_deps` (lihat *Key invariants*) |

`getrandom` untuk jitter sudah masuk core lewat spec 0011. Core memakai tokio untuk timer, probe jaringan, dan actor; use case sync spec 0004 tetap dipanggil lewat `spawn_blocking`. `SupervisorConfig` memegang semua konstanta di *Konstanta* dengan default produksi dan bisa diganti di test.

### Port `ProcessSupervision`

Di `stackly-platform::process`. Sync, object safe, `Send + Sync`, seperti port lain di spec 0004. Fitur ini menambahkannya sebagai port ketujuh `processes` lewat `Platform::with_processes` (pola `with_<port>` spec 0004; `Platform::new` tidak berubah).

```rust
pub trait ProcessSupervision: Send + Sync {
    fn spawn_main(&self, request: SpawnRequest<'_>) -> Result<SuspendedChild, ProcessError>;
    fn resume(&self, child: SuspendedChild) -> Result<MainHandle, ProcessError>;
    fn discard(&self, child: SuspendedChild) -> Result<(), ProcessError>;
    fn spawn_helper(&self, request: SpawnRequest<'_>) -> Result<HelperHandle, ProcessError>;
    fn inspect(&self, pid: Pid) -> Result<Option<ProcessFacts>, ProcessError>;
    fn holds_file(&self, pid: Pid, file: FileIdentity) -> Result<HoldsFile, ProcessError>;
    fn signal(&self, target: &SignalTarget, signal: ProviderSignal) -> Result<(), ProcessError>;
    fn continue_stopped(&self, target: &SignalTarget) -> Result<(), ProcessError>;
    fn terminate_helper_group(&self, helper: &HelperHandle, force: bool) -> Result<(), ProcessError>;
    fn group_members(&self, group: Pid) -> Result<Vec<Pid>, ProcessError>;
    fn boot_id(&self) -> Result<BootId, ProcessError>;
    fn watch_exit(&self, pid: Pid, listener: Arc<dyn ExitListener>) -> Result<(), ProcessError>;
    fn lock_instance(&self, path: &Path) -> Result<LockOutcome, ProcessError>;
    fn probe_lock(&self, path: &Path) -> Result<LockState, ProcessError>;
    fn create_spool(&self, path: &Path) -> Result<SpoolWriter, ProcessError>;
    fn open_spool(&self, path: &Path) -> Result<Option<Box<dyn SpoolReader>>, ProcessError>;
}
```

- `SpawnRequest { program: &Path, args: &[String], env: &[(String, String)], cwd: &Path, stdin: StdinSource, output: OutputTarget, lock: Option<&InstanceLock>, role: ChildRole }`. `StdinSource`: `Null` atau `Bytes(SecretValue)` (ditulis sekali lalu pipe ditutup). `OutputTarget`: `Spool { stdout: SpoolWriter, stderr: SpoolWriter }` (proses utama) atau `Capture` (pembantu; pipe yang dibaca thread platform ke buffer ekor `HELPER_CAPTURE_MAX` per stream). `ChildRole`: `Main`, `InitStep`, `Probe`, `StopCommand`, `ReloadCommand`. `lock` hanya diterima untuk `Main` dan `InitStep`, dan dipetakan ke fd 3.
- `spawn_main` membuat proses utama tertahan (`POSIX_SPAWN_START_SUSPENDED`) dan mengembalikan `SuspendedChild { pid, facts }`. `resume` mengirim `SIGCONT` dan mengembalikan `MainHandle`. `discard` mengirim `SIGKILL` lalu me reap child yang belum pernah berjalan; ia hanya menerima `SuspendedChild`, jadi tidak bisa diarahkan ke proses yang sudah berjalan.
- `spawn_helper` menjalankan pembantu langsung (tidak tertahan) dan mengembalikan `HelperHandle`. Hanya `HelperHandle` yang bisa diberikan ke `terminate_helper_group`.
- `ProcessFacts { pid, parent, group, session, uid, start: ProcessStart, executable: PathBuf, stopped: bool }` dari proses yang hidup (`stopped` benar kalau proses tertahan); `inspect` mengembalikan `None` untuk PID yang tidak ada atau zombie.
- `HoldsFile`: `Yes`, `No`, atau `Unreadable` (daftar fd tidak bisa dibaca).
- `SignalTarget { pid, expected: ProcessIdentity }`. `signal` dan `continue_stopped` membaca ulang `ProcessFacts` tepat sebelum `kill(2)` dan menolak dengan `IdentityMismatch` kalau waktu start, user, atau session berbeda. Executable tidak dibandingkan di sini, karena PID dan waktu start sudah unik; executable dibandingkan saat adopsi. `ProviderSignal`: `Term`, `Int`, `Quit`, `Hup`; tidak ada `Kill`.
- `terminate_helper_group(helper, force)`: `SIGTERM` (atau `SIGKILL` kalau `force`) ke process group helper itu, hanya selama helper belum di reap.
- `watch_exit` mendaftarkan PID ke watcher; `ExitListener::exited(ExitNotice { pid, status: Option<ExitStatus>, captured: Option<CapturedOutput> })` dipanggil sekali dari thread watcher. `status` hanya ada untuk child daemon (`Exited(code)` atau `Signaled(signal)`); proses yang diadopsi selalu `None`. `captured` hanya untuk pembantu.
- `lock_instance` membuka atau membuat file lock (0600, tanpa mengikuti symlink, harus file biasa milik user) lalu `flock` eksklusif tanpa menunggu: `Acquired(InstanceLock)` atau `Held`. `InstanceLock` membawa `FileIdentity` file nya dan melepas lock dengan `close`, tidak pernah `flock(LOCK_UN)`, supaya lock yang sudah diwariskan ke child tidak ikut terlepas. `probe_lock` membuka deskripsi baru, mencoba lock lalu menutupnya, hasilnya `Free` atau `Held`.
- `create_spool` membuat file baru (append, 0600, gagal kalau sudah ada). `open_spool` mengembalikan `None` kalau file tidak ada. `SpoolReader`: `len()`, `read_at(offset, buf)`, `first_data_offset()`, `release_below(offset)` (punch hole untuk blok penuh di bawah offset), dan `file_identity()`.
- `ProcessError` (`#[non_exhaustive]`): `NotSupported { capability, os }`, `SpawnFailed { kind, errno }`, `InvalidArgument { position }` (NUL di argv atau env), `IdentityMismatch { pid }`, `NotFound { pid }`, `PermissionDenied { pid }`, `LockFileInvalid { path, reason }`, `Io { op, path, kind, message }`.

`UnsupportedProcessSupervision` mengembalikan `NotSupported` dari setiap method. `FakeProcessSupervision` di testkit mensimulasikan proses di memori (PID buatan, fakta identitas yang bisa diubah test, exit yang dipicu test, error yang bisa diinjeksi per method, lock dan spool di `TestRoot`) dan mencatat setiap spawn, resume, discard, sinyal, dan terminate supaya test bisa memastikan tidak ada `SIGKILL` ke proses yang sudah berjalan dan tidak ada sinyal ke identitas yang tidak cocok.

### Adapter macOS

Di `stackly-platform-macos::process`. Semua `unsafe` di crate ini, masing masing dengan `// SAFETY:`.

| Kebutuhan | Cara |
| --- | --- |
| Spawn | `posix_spawn` dengan flag di AC-1. Fd sumber (spool, pipe, lock) lebih dulu diduplikasi ke nomor 10 ke atas supaya tidak bertabrakan dengan fd 0 sampai 3 daemon yang mungkin tertutup; file action `addopen` `/dev/null` atau `adddup2` pipe stdin ke fd 0, `adddup2` ke fd 1, 2, dan 3, `addchdir_np` ke cwd. Pipe stdin pembantu ditulis thread pembantu dengan batas waktu langkahnya, lalu ditutup |
| Tertahan dan lanjut | `POSIX_SPAWN_START_SUSPENDED`; `kill(pid, SIGCONT)` untuk melanjutkan; `kill(pid, SIGKILL)` lalu `waitpid` untuk membuang child yang belum pernah berjalan |
| Waktu start, user, group, parent, tertahan | `proc_pidinfo(PROC_PIDTBSDINFO)`: `pbi_start_tvsec`, `pbi_start_tvusec`, `pbi_uid`, `pbi_pgid`, `pbi_ppid`, `pbi_status` |
| Executable | `proc_pidpath` |
| Session | `getsid(pid)`; selalu sama dengan PID karena `SETSID` (bukti tambahan yang murah, bukan bukti yang berdiri sendiri) |
| Boot | `sysctlbyname("kern.bootsessionuuid")` |
| Fd yang dipegang | `proc_pidinfo(PROC_PIDLISTFDS)` lalu `proc_pidfdinfo(PROC_PIDFDVNODEPATHINFO)` per fd vnode, dibandingkan device dan inode |
| Anggota group | `proc_listpids(PROC_PGRP_ONLY, group)` |
| Exit | satu thread `stackly-proc-watch` dengan kqueue `EVFILT_PROC` `NOTE_EXIT` (ditambah `NOTE_EXITSTATUS` untuk child), lalu `waitpid(pid, WNOHANG)` untuk child. Pendaftaran yang gagal `ESRCH` langsung menghasilkan notice dan `waitpid`. Untuk proses yang diadopsi, waktu start dibaca ulang setelah pendaftaran supaya jendela antara klasifikasi dan pengawasan tertutup |
| Sinyal dan reap | satu mutex per PID: reap, `signal`, `continue_stopped`, dan `terminate_helper_group` saling mengunci, dan handle yang sudah di reap menolak sinyal |
| Lock | `open(O_RDWR, O_CREAT, O_NOFOLLOW, O_CLOEXEC, 0600)`, `fstat` (file biasa, milik user), `flock(LOCK_EX, LOCK_NB)` |
| Spool | tulis: `open(O_WRONLY, O_CREAT, O_EXCL, O_APPEND, O_NOFOLLOW, O_CLOEXEC, 0600)`; baca: `O_RDONLY`, `O_NOFOLLOW`; `fcntl(F_PUNCHHOLE)` dengan offset dan panjang yang diratakan ke `f_bsize` dari `fstatfs`; `lseek(SEEK_DATA)` untuk data pertama |

Daemon tidak memasang handler `SIGCHLD` dan tidak memakai `std::process::Command` atau `tokio::process` untuk proses instance atau pembantu, supaya hanya watcher yang me reap child. Build step pertama adalah spike di Mac ini yang membuktikan setiap baris tabel di atas pada macOS 27, termasuk untuk proses yang bukan child (setelah proses induknya keluar). Kalau satu baris gagal, build berhenti dan keputusan yang bergantung padanya kembali ke `/architect`. Fungsi `MacosProcessSupervision::fd_numbers(pid)` (`#[doc(hidden)]`, di luar trait) dipakai integration test untuk AC-1.

### Lock instance

- Path `<data>/instances/<id>/instance.lock` (`InstancePaths.lock`, Follow-up spec 0004), file kosong 0600 yang dibuat supervisor saat pertama kali dibutuhkan dan tidak pernah dihapus supervisor. Ia ikut berpindah ke `retained/<id>/` bersama folder instance.
- Init, start, dan setiap respawn mengambil lock (`lock_instance`) sebelum efek samping pertama; `Held` berarti `supervisor.data_in_use` tanpa spawn. Sebagai pengaman kedua, start juga menolak dengan kode yang sama kalau proses dari observasi terakhir masih hidup dengan waktu start, user, dan executable yang cocok, walau lock bisa diambil (misalnya file lock dihapus atau diganti).
- Proses utama dan setiap langkah `Run` init mewarisi lock di fd 3. Setelah `resume` berhasil, daemon menutup salinan fd nya; lock tetap dipegang selama proses utama atau turunannya yang mewarisi fd 3 hidup, termasuk setelah daemon mati.
- Probe, perintah stop, dan perintah reload tidak mewarisi lock.
- Saat readiness pertama berhasil, supervisor memanggil `holds_file` sekali. Hasil selain `Yes` membuat status `adoptable: false` dan baris log warning: instance itu berjalan normal, tetapi akan menjadi `unknown` setelah daemon restart.
- `probe_lock` dipakai rekonsiliasi dan pemeriksaan ulang; ia tidak pernah menyimpan lock.
- Retain, purge, cold backup, dan restore (fitur 54, 50, 51) wajib mengambil lock yang sama lebih dulu (Follow-up).

### Identitas majemuk

`ProcessIdentity { pid, start, executable, session, boot }` dicatat saat spawn dan saat adopsi. User tidak disimpan: setiap catatan ditulis daemon milik user itu, dan pemeriksaannya membandingkan uid proses dengan user yang sedang berjalan.

| Pemeriksaan | Lolos kalau |
| --- | --- |
| `boot_matches` | boot ID sekarang sama dengan yang tercatat |
| `pid_exists` | `inspect(pid)` mengembalikan fakta |
| `start_matches` | waktu start (detik dan mikrodetik) sama |
| `executable_matches` | path executable sama dengan path canonical yang tercatat (folder runtime content addressed, jadi path adalah identitas runtime) |
| `user_matches` | uid proses sama dengan user yang sedang berjalan |
| `session_matches` | session ID sama |
| `holds_lock` | `holds_file(pid, inode lock)` mengembalikan `Yes` |

Klasifikasi adalah fungsi murni `classify(recorded, boot, facts, uid, lock_state, holds) -> Classification` di core, dengan test tabel lengkap. Setiap input yang berupa error (dari `inspect`, `holds_file`, `probe_lock`, atau `boot_id`) langsung menghasilkan `Unproven`, tidak pernah `Gone`.

| Kondisi | Hasil | Blocker |
| --- | --- | --- |
| Tidak ada catatan, lock bebas | `Gone(no_record)` | |
| Tidak ada catatan, lock dipegang | `Unproven` | `dataInUse` |
| Boot berbeda, lock bebas | `Gone(boot_changed)` | |
| Boot berbeda, lock dipegang | `Unproven` | `dataInUse` |
| PID tidak ada, atau start, executable, user, atau session berbeda; lock bebas | `Gone(exited)` atau `Gone(pid_reused)` | |
| Sama seperti baris sebelumnya, tetapi lock dipegang | `Unproven` | `dataInUse` |
| Semua cocok dan `holds_lock` `Yes` | `Proven(identity)` | |
| Semua cocok, `holds_lock` `No` atau `Unreadable` | `Unproven` | `identityUnproven` |
| Ada error saat mengumpulkan bukti | `Unproven` | `identityUnproven` |

`Unproven` membawa `IdentityEvidence` (hasil setiap pemeriksaan, PID, state lock, `stopped`, dan error kalau ada), yang menjadi isi blocker dan baris log. Tidak ada jalur dari `Unproven` ke sinyal apa pun. Proses `Unproven` yang masih tertahan (daemon mati sebelum commit) ditandai `stopped: true` di bukti dan tidak dilanjutkan atau dibuang otomatis.

Format kolom `process_observations` (spec 0005): `pid` angka; `start_identity` `<detik>.<mikrodetik 6 digit>`; `executable_identity` path canonical UTF-8; `boot_id` UUID huruf kecil dari `kern.bootsessionuuid`; `session_id` angka desimal; `observed_at` waktu commit.

### Spool log

- Path `<run>/stdout.spool` dan `<run>/stderr.spool`, dengan `<run>` = `instance_run_dir(short key)` (Follow-up spec 0004). Spool berada di runtime root (folder temp privat milik user), sama seperti file secret spec 0011, sehingga output mentah tidak pernah masuk root data atau log tahan lama.
- Sebelum setiap spawn proses utama: spool lama dibaca sampai habis ke sink, dihapus, lalu dibuat baru. Spool tidak dihapus saat stop, supaya ekor log masih bisa dibaca setelah daemon restart.
- Pembaca per instance berjalan setiap `SPOOL_DRAIN_INTERVAL` selama proses utama hidup: membaca paling banyak `SPOOL_READ_CHUNK` dari offset terakhir, mengubah byte ke UTF-8 secara lossy (U+FFFD untuk byte yang tidak valid), memotong per newline dan membuang `\r` di akhir baris, menyamarkan setiap baris logis utuh lewat `Redactor::redact`, lalu memotong baris yang lebih panjang dari `LOG_LINE_MAX` menjadi beberapa `LogLine` bertanda `continued`. Baris yang belum diakhiri newline ditahan sampai `LOG_LINE_HARD_MAX`; di atas itu ia disamarkan dan dikirim sebagai baris `partial`. Setelah sink menerima, pembaca memanggil `release_below(offset)`.
- Saat proses keluar, pembaca membaca sekali sampai akhir file, mengirim sisa baris sebagai `partial`, lalu berhenti untuk spool itu.
- Kalau byte yang belum terbaca di satu stream melebihi `SPOOL_BACKLOG_MAX` (sink tertahan, atau daemon baru hidup lagi setelah lama mati), bagian tertua dibebaskan tanpa dikirim dan sink menerima `truncated { bytes }`.
- Setelah daemon restart, pembaca membuka spool proses yang diadopsi (spool yang tidak ada hanya dicatat sebagai warning dan status `logsUnavailable`, adopsi tetap berjalan), memastikan file biasa milik user, mulai dari `first_data_offset()`, membuang byte sampai newline pertama, dan menandai batch pertama `resumed`.
- `LogSink` (core): `fn accept(&self, instance: InstanceId, batch: LogBatch) -> SinkOutcome` dengan `SinkOutcome` `Accepted` atau `Busy`, dan `LogBatch { stream, read_at, lines: Vec<LogLine { text, partial, continued }>, resumed, truncated: Option<u64> }`; `read_at` adalah waktu baca daemon dari `Clock`, bukan waktu tulis proses. Sink tidak boleh memblokir; `Busy` membuat pembaca berhenti untuk instance itu sampai tick berikutnya. `SinkSet` meneruskan ke beberapa sink.
- `TailSink` (fitur ini): ekor `TAIL_LINES` baris per stream per instance di memori, sudah tersamar, sumber cuplikan kegagalan. Fitur 27 menambah sink tahan lama di `SinkSet`.
- Output pembantu tidak masuk spool: thread platform menyimpan ekornya, core menyamarkannya, lalu memakainya untuk detail kegagalan dan log daemon tingkat `debug`.

### Executor plan

Semua path tulis dan hapus diperiksa `check_owned_path` tepat sebelum IO. `ensure_instance_run_dir` dipanggil sebelum setiap aksi yang memakai folder run.

**Init** (`execute_init`, dipanggil create):

1. Lock instance diambil; `resolve_for_start` dengan interaksi operasi (spec 0011); semua `WriteFile` di materialize lewat `RenderedContent::materialize` dan argv setiap langkah `Run` dicek `ensure_not_in_argv` sebelum efek samping pertama.
2. Langkah berurutan di bawah `InitPlan.deadline`:
   - `EnsureDirectories`: aturan spec 0009 (parent dulu, buat 0700, terima yang ada hanya kalau folder privat milik user).
   - `WriteFile`: `write_file_atomic` (0600).
   - `MaterializeSecret`: nama file dari plan, ditulis lewat `SecretFile::create` di `<run>/secrets/` (spec 0011); guard dipegang sampai akhir aksi.
   - `Run`: spawn pembantu `InitStep` dengan lock di fd 3 dan stdin dari `stdin_payload` kalau `Stdin::Secret`, tunggu sampai `min(timeout langkah, sisa deadline)`; exit di luar `success_exit_codes` menghasilkan `init_step_failed` dengan exit code dan cuplikan; timeout menjalankan aturan pembantu lalu `init_step_timeout`.
3. Akhir aksi, berhasil atau gagal: guard `SecretFile` dilepas dan setiap path di `InitPlan.cleanup` dihapus lewat `remove_owned_tree` (spec 0007, satu satunya jalur hapus).
4. Sukses: lifecycle `init_succeeded` (mengisi `data_initialized_at`) lalu `write_instance_manifest` dengan `data.initializedAt`. Gagal: `init_failed { reason }` dengan reason `init_step_failed`, `init_step_timeout`, `init_timeout`, atau `interrupted`.

**Aturan pembantu**: pembantu yang melewati timeout nya menerima `terminate_helper_group(force: false)`, lalu `terminate_helper_group(force: true)` setelah `HELPER_TERM_GRACE` kalau belum keluar. Langkah atau probe itu dihitung gagal dengan reason timeout.

**Render sebelum spawn**: setiap spawn proses utama (start dan respawn) lebih dulu menjalankan langkah `EnsureDirectories` dan `WriteFile` untuk setiap `configFiles`, supaya config selalu sesuai settings dan port saat itu. Sumbernya `RunPlan.render` (Follow-up spec 0009). Selama field itu belum ada, executor memakai langkah `EnsureDirectories` dan `WriteFile` dari `init_plan(ctx)` dengan penanda `// provisional: F9` yang menyebut field itu.

**Reload** (`execute_reload`, dipakai fitur 26): `reload_plan(ctx)`; `Unsupported` menjadi `supervisor.capability_unsupported`. Materialize lalu jalankan `steps`; aksi `Hangup` lewat `signal` ke proses utama terverifikasi, `Run` lewat pembantu `ReloadCommand`, `None` tanpa aksi; lalu `verify` dengan probe readiness sampai ready atau `deadline` (satu anggaran bersama). Gagal menghasilkan `supervisor.reload_failed`; proses tidak pernah dijalankan ulang oleh reload.

### Commit

- Setiap perubahan aggregate instance ditulis lewat `WriteBatch` dengan satu `UpdateInstance` per mutasi domain, masing masing dengan `expected` berurutan, karena spec 0005 mewajibkan revision naik tepat 1 per op (misalnya `set_desired(running)` lalu `start_begin` menjadi dua op dalam satu batch). Satu event `instance.state_changed` dikirim setelah batch dengan revision akhir.
- Commit dari operasi memakai revision yang dibaca di `validate`. Konflik revision antara `verify` dan `journal` (misalnya config diubah di antaranya) mengulang `validate` dan `verify` sekali, lalu gagal dengan `resource.revision_mismatch`.
- Commit yang dipicu actor (exit, crash, `exited`, `adopted`, `confirmed_stopped`, rekonsiliasi, `holds_file`) mengikuti AC-22: konflik revision dibaca ulang dan event diterapkan lagi kalau masih sah, paling banyak `COMMIT_RETRY_LIMIT` kali; `Busy` dicoba lagi dengan jeda 100 ms yang berlipat sampai 5 detik; `NoSpace` atau `Unavailable` membuat subsystem `supervisor` `degraded` dengan detail, sambil dicoba lagi setiap `REGISTRY_RETRY_INTERVAL`. Selama sebuah commit actor tertunda, instance itu tidak di spawn dan proses utamanya tidak disinyal; state di memori tetap menjadi dasar tampilan dan diberi detail `pending_commit`.
- Error registry yang sampai ke client memakai kode area `registry` spec 0005.

### Startup transaction

Satu operasi `start_instance` (spec 0007) dengan tahap berikut. `StageName` sama dengan kolom pertama. Penolakan berdasarkan state dijawab method sebelum operasi dibuat (AC-23); tahap `validate` mengulanginya di dalam operasi untuk jaga jaga.

| Tahap | Isi | Gagal |
| --- | --- | --- |
| `validate` | instance ada dan tidak retained, `expectedRevision` (kalau dikirim), lifecycle boleh start, data sudah diinisialisasi (BR-008) | error tanpa perubahan |
| `lock` | lock resource operasi (spec 0007), lalu `lock_instance` dan pengaman observasi terakhir | `resource.busy`, `supervisor.data_in_use` |
| `verify` | runtime dan `depends_on` `installed`; paket pinned dibaca dari `provider_revision_dir(key)` dengan `NameCheck::Skip` dan digest sama; folder instance milik user dan privat; `preflight` `Compatible`; `ensure_instance_run_dir`; `resolve_for_start`; dry run `run_plan(ctx)` dengan port kandidat; `ensure_not_in_argv` atas argv hasil dry run | kode per kegagalan; needs secret menjadi blocker |
| `endpoints` | port `Fixed` dicek bebas lewat `PortProbe` (bind percobaan di alamat endpoint, `tcp` dengan listener dan `udp` dengan socket UDP, IPv4 atau IPv6 sesuai alamat, tanpa `SO_REUSEADDR`), `Auto` memakai `bound` lama kalau bebas atau kandidat baru, `endpoints_conflict` terhadap port efektif instance lain | `supervisor.endpoint_in_use`, `supervisor.endpoint_conflict`, `supervisor.auto_port_unavailable` |
| `journal` | commit desired `running` dan `start_begin` (lihat *Commit*) | `resource.revision_mismatch` |
| `render` | `run_plan(ctx)` final, render config, hapus socket Unix basi di folder run (aman karena lock dipegang) | `start_failed { config_render_failed }` |
| `spawn` | verifikasi `ExecutablePath` (canonical, di dalam root runtime, file biasa, bisa dieksekusi), siapkan spool, `spawn_main` dengan lock di fd 3 | `start_failed { spawn_failed }` atau `{ executable_invalid }` |
| `identity` | tangkap `ProcessIdentity` dari child yang tertahan, commit `spawned` plus port `Auto` plus observasi, `watch_exit`, `resume`, tutup salinan fd lock daemon | `discard` lalu `start_failed { identity_capture_failed }` |
| `readiness` | probe readiness sampai ready, crash (restart policy), cancel, atau deadline | lihat AC-7, AC-8, dan AC-17 |
| `publish` | observasi ready, `holds_file` sekali, port `Auto` ditandai final, event | |
| `complete` | operasi `succeeded`, lock resource dilepas | |

- Deadline readiness dihitung dari `resume`: `readiness.deadline` untuk spawn pertama, `recovery_deadline` untuk respawn setelah crash. Percobaan berikutnya mulai `interval` setelah percobaan sebelumnya selesai.
- Proses yang keluar sebelum ready: supervisor menjalankan `PortProbe` untuk setiap endpoint IP. Port yang kini dipakai pihak lain menghasilkan `crashed { will_retry: false, reason: endpoint_in_use }` dan operasi gagal `supervisor.endpoint_in_use` tanpa memakai budget. Selain itu restart policy berlaku: respawn mengulang tahap `lock` (hanya `lock_instance`), `verify`, `endpoints`, `render`, `spawn`, `identity`, dan `readiness` di bawah operasi yang sama, dengan interaksi Keychain `Never`, dan transisi `recovering` ke `spawned` tanpa `start_begin`. Lock yang masih `Held` saat respawn dihitung sebagai percobaan gagal dengan reason `data_in_use`, dan jeda berikutnya berlaku.
- Cancel: sebelum `journal` tanpa perubahan; setelah `journal` tanpa proses hidup (sebelum spawn atau saat jeda), `stop_begin` lalu `exited` dan desired `stopped`; dengan proses hidup, alur *Stop* lalu operasi `cancelled` (atau `failed` dengan `supervisor.stop_timeout`).
- Start saat lifecycle `starting`, `running`, atau `recovering` selesai tanpa operasi (AC-23); kalau desired tersimpan `stopped` (misalnya proses yang diadopsi saat stop terputus), desired diubah menjadi `running` lewat satu commit. Start saat `stopping` gagal `supervisor.instance_stopping`, saat `unknown` gagal `supervisor.identity_unproven`, saat `uninitialized` atau `initializing` gagal `supervisor.not_initialized`.
- Stop pengguna selama operasi start pengguna berjalan (termasuk saat jeda backoff nya) mendapat `resource.busy`; pengguna membatalkan operasi start itu lebih dulu.

### Stop

Operasi `stop_instance`:

1. Penolakan dan no op dijawab method (AC-23): `unknown` gagal `supervisor.identity_unproven`, `initializing` gagal `supervisor.instance_initializing`, retained gagal `supervisor.instance_retained`, `uninitialized` tanpa operasi; `stopped` atau `failed` tanpa operasi, kecuali desired tersimpan `running`, yang diubah menjadi `stopped` lewat satu commit. `evaluate_stop(Manual { session: None, override_leases })` spec 0004 untuk stop pengguna; `RequiresOverride` menjadi `supervisor.stop_requires_override`.
2. `lock`, lalu `journal`: commit desired `stopped` dan `stop_begin`. Instance `recovering` tanpa proses (sedang jeda backoff) membatalkan timer lalu langsung `exited`. Instance yang sudah `stopping` (stop sebelumnya timeout, atau rekonsiliasi) melewati `journal` dan lanjut ke langkah 3 dengan anggaran baru.
3. `stop`: `StopPlan::Signal` lewat `signal(SignalTarget, Term|Int|Quit)`, atau `StopPlan::Command` lewat pembantu `StopCommand`; perintah dan menunggu proses keluar berbagi satu anggaran `timeout`. Perintah yang keluar dengan kode di luar `success_exit_codes` membuat operasi gagal `supervisor.stop_command_failed`, lifecycle tetap `stopping`, proses tetap diawasi. `IdentityMismatch` membuat `identity_lost` (`unknown`) dan operasi gagal `process.signal_refused`.
4. Anggaran habis: operasi `failed` dengan `supervisor.stop_timeout`; lifecycle tetap `stopping`. Tidak ada eskalasi sinyal; force milik fitur 26.
5. Handler exit tunggal (di dalam atau di luar operasi): pembaca spool selesai, `exited`, `remove_secret_material` (spec 0011), lalu `group_members(pid)`; anggota tersisa dicatat di status dan log sebagai `leftoverProcesses`, tidak disinyal. Kalau operasi stop masih berjalan, ia `succeeded`.

`remove_secret_material` juga dipanggil setiap kali proses utama keluar tanpa respawn (`crash_loop`, `start_failed`, `endpoint_in_use`).

### Probe, readiness, dan health

- Runner di `core::supervisor::probe`: `tcp` (connect dengan batas `attempt_timeout`, ke IP; ke socket Unix hanya di bawah `cfg(unix)`, di OS lain hasilnya `Unsupported`), `http` (client kecil di `core::supervisor::http`: satu `GET` HTTP/1.1 dengan `Host: <host>:<port>` (IPv6 dalam kurung siku) dan `Connection: close`, tanpa redirect; connect, tulis, dan baca status line semuanya di dalam `attempt_timeout`; header dibaca paling banyak `HTTP_HEADER_MAX` sampai status line selesai, body tidak dibaca; sukses kalau status di `expect_status`), dan `exec` (pembantu `Probe` dengan argv dan env plan, tanpa stdin, sukses kalau exit code di `success_exit_codes`; timeout menjalankan aturan pembantu).
- Satu percobaan probe per jenis per instance pada satu waktu.
- Sebelum ready: probe readiness setiap `interval` sampai deadline (lihat *Startup transaction*).
- Setelah ready: probe readiness setiap `READINESS_REFRESH_INTERVAL`; sukses memperbarui `ready` (`observed_at` baru), kegagalan tidak memperbarui `observed_at`, dan readiness menjadi `not_ready` setelah `READINESS_FAILURE_THRESHOLD` kegagalan berturut turut, sebelum nilai lama menjadi stale. Probe health berjalan setiap `HealthPlan.interval`: gagal di bawah `failure_threshold` `degraded`, mencapai ambang `unhealthy`, sukses `healthy`. Kalau probe dan interval health sama dengan readiness, satu percobaan dipakai untuk keduanya.
- Readiness dan health hanya diobservasi lewat `observe_readiness` dan `observe_health` spec 0004, tidak disimpan di registry, dan tidak pernah memicu restart atau stop.
- Pada setiap tick probe, actor menghitung `effective(now, READINESS_MAX_AGE)` dengan jam dinding `Clock`; kalau hasilnya berubah (misalnya menjadi `unknown` reason `stale` setelah Mac bangun dari tidur), actor mengirim `instance.health_changed`. `instance.get` selalu menghitung `effective` saat dibaca.
- Waktu deadline, jeda, jendela restart, dan interval memakai jam monotonik tokio (berhenti saat Mac tidur, jadi tidur tidak dihitung sebagai crash); `observed_at` memakai `Clock` platform.

### Restart policy

| Konstanta | Nilai |
| --- | --- |
| `RESTART_BASE_DELAY` | 1 detik |
| `RESTART_MAX_DELAY` | 30 detik (penjaga; dengan budget 5, jeda terpanjang 16 detik) |
| `RESTART_JITTER` | faktor acak 0,5 sampai 1,0 (`getrandom`) |
| `RESTART_BUDGET` | 5 percobaan |
| `RESTART_WINDOW` | 5 menit, jendela geser, jam monotonik |
| `RESTART_HEALTHY_RESET` | 60 detik readiness `ready` terus menerus |

- Crash adalah exit proses utama tanpa permintaan saat desired `running` dan lifecycle `running` atau `recovering`. Supervisor mencatat waktu crash, membuang catatan di luar jendela, lalu: kalau jumlah respawn dalam jendela kurang dari budget, `crashed { will_retry: true, reason }` (lifecycle `recovering`) dan respawn setelah `min(MAX, BASE × 2^(n−1)) × jitter` dengan n = respawn ke berapa dalam jendela; kalau tidak, `crashed { will_retry: false, reason: crash_loop }` (lifecycle `failed`).
- Reason crash: `process_exited` (exit code di detail), `process_signaled` (sinyal di detail), atau `exit_status_unknown` (proses yang diadopsi).
- Di luar operasi pengguna, jeda dijalankan timer actor tanpa operasi dan tanpa lock resource. Saat timer jalan, actor membuat operasi sistem `start_instance` dengan pemicu `restart_policy` dan interaksi `Never`, lalu menjalankan jalur respawn. Kalau saat itu lock resource dipegang operasi lain, atau desired sudah `stopped`, respawn dibatalkan.
- Start pengguna dari `failed` menghapus riwayat. Riwayat ada di memori daemon; daemon restart memulai budget baru.
- Tidak ada override per provider atau per instance di versi ini; perbedaan provider lewat timeout spec 0009.

### Rekonsiliasi

Berjalan di slot start daemon (spec 0006) setelah registry `ready` (spec 0005) dan pemulihan operasi (spec 0007). Dalam recovery mode registry, langkah ini dilewati seluruhnya dan subsystem `supervisor` melaporkan `unavailable` dengan detail `registry in recovery`.

1. Publish `daemon.reconciling { phase: started }`.
2. Baca boot ID, semua instance yang tidak retained, dan observasi terakhir masing masing.
3. Untuk setiap instance: kumpulkan fakta, `probe_lock`, `holds_file`, lalu `classify`, dan terapkan tabel di bawah (lewat *Commit*). Operasi instance yang terputus sudah ditutup `failed` alasan `interrupted` oleh `recover` spec 0007 (kebijakan `CloseInterrupted`) sebelum langkah ini, tanpa kompensasi, jadi rekonsiliasi hanya menyelesaikan lifecycle dan proses nya. Readiness dan health instance yang tidak `running` diobservasi `unknown` dengan reason `not_running`.
4. `sweep_secret_material` untuk instance yang tidak berjalan, dengan lifecycle `starting`, `running`, `recovering`, `stopping`, dan `unknown` dihitung berjalan, lalu `learn_running` untuk semuanya (spec 0011).
5. Untuk setiap proses yang `Proven`: kalau `stopped` (tertahan), `continue_stopped`; lalu `watch_exit`, lanjutkan pembaca spool, dan mulai probe ke port tersimpan.
6. Publish `daemon.reconciling { phase: finished, counts }`; subsystem `supervisor` `ok` (detail menyebut jumlah `unknown` kalau ada).
7. Setelah `OperationEngine::start()` (spec 0007; sebelum itu `begin` menjawab `NotReady`): instance di daftar "jalankan lagi" dijalankan, paling banyak `RECONCILE_START_CONCURRENCY` bersamaan, urut nama. Instance `recovering` masuk jalur respawn actor (jeda pertama `RESTART_BASE_DELAY`, budget baru); instance `failed` atau `stopped` lewat operasi sistem `start_instance` dengan pemicu `reconcile` dan interaksi `Never`. Instance `running` atau `stopping` yang `Proven` dengan desired `stopped` mendapat operasi sistem `stop_instance` dengan pemicu `reconcile` (tanpa `evaluate_stop`).

Semua transisi di tabel ini ada di tabel lifecycle spec 0004.

| Lifecycle tersimpan | `Proven` | `Gone`, boot sama | `Gone`, boot baru | `Unproven` |
| --- | --- | --- | --- | --- |
| `initializing` | `init_failed { interrupted }` dan blocker `dataInUse` | `init_failed { interrupted }` | `init_failed { interrupted }` | `init_failed { interrupted }` dan blocker |
| `starting` | `spawned` | `start_failed { interrupted }`; jalankan lagi kalau desired `running` | `start_failed { interrupted }` | `identity_lost` (`unknown`) |
| `running` | tetap `running`, probe ulang; stop kalau desired `stopped` | desired `running`: `crashed { will_retry: true, reason: exit_status_unknown }` lalu respawn; desired `stopped`: `identity_lost` lalu `confirmed_stopped` | `identity_lost` lalu `confirmed_stopped` (`stopped`) | `identity_lost` |
| `recovering` | `spawned` | tetap `recovering`, respawn | `identity_lost` lalu `confirmed_stopped` | `identity_lost` |
| `stopping` | tetap `stopping`; stop dikirim ulang | `exited` | `exited` | `identity_lost` |
| `unknown` | `adopted` (`running`); stop kalau desired `stopped` | `confirmed_stopped`; jalankan lagi kalau desired `running` | `confirmed_stopped` | tetap `unknown` |
| `stopped`, `failed`, `uninitialized` | tidak berubah, blocker `dataInUse` | tidak berubah | tidak berubah | tidak berubah, blocker `dataInUse` kalau lock dipegang |

- Boot baru tidak pernah menjalankan instance; menyalakan instance saat login milik fitur 31.
- Setiap `UNKNOWN_RECHECK_INTERVAL`, actor memeriksa ulang instance `unknown` dengan `classify` yang sama (`Proven` menjadi `adopted`, `Gone` menjadi `confirmed_stopped` tanpa start otomatis, `Unproven` tetap) dan instance lain yang punya blocker `dataInUse` dengan `probe_lock` (blocker hilang saat lock bebas).
- `SIGTERM` ke daemon (langkah supervisor di urutan berhenti spec 0006, Follow-up): actor, probe, pembaca spool, dan watcher dihentikan tanpa menyinyal proses instance dan tanpa commit lifecycle; operasi instance yang sedang berjalan dibiarkan terputus; `recover` spec 0007 menutupnya pada start berikutnya dan rekonsiliasi mengadopsi prosesnya (AC-21).

### Status dan event

`InstanceSummary` (snapshot, dari registry ditambah state di memori actor):

| Field | Isi | Setelah daemon restart |
| --- | --- | --- |
| `id`, `name`, `displayName`, `revision` | aggregate instance | sama |
| `software`, `provider { name, revision, manifestSha256 }`, `runtimeId`, `runtimeVersion` | aggregate instance dan record runtime nya | sama |
| `lifecycle`, `desired` | kode spec 0004 | sama |
| `readiness`, `health` | `{ state, reason, detail, observedAt }` setelah `effective(now, READINESS_MAX_AGE)` | `unknown`, reason `daemon_restarted` atau `not_running`, sampai probe pertama |
| `statusSeq` | penghitung monotonik per instance per run daemon, mulai 1, naik setiap kali isi summary berubah | mulai lagi dari 1 (client tahu lewat gap `server_restarted`) |
| `process` | `{ pid, startedAt, adopted, adoptable }` atau null | dari observasi terakhir untuk proses yang diadopsi, `adopted: true` |
| `lastFailure` | `{ code, at, exitCode, signal, excerpt }` atau null; `excerpt` paling banyak `FAILURE_EXCERPT_LINES` baris dari `TailSink` (stderr dulu, lalu stdout), sudah tersamar | `code` dari `last_failure` registry; `at`, `exitCode`, `signal` null; `excerpt` dari ekor spool kalau masih ada |
| `restart` | `{ attempts, budget, windowSeconds, nextAttemptAt, exhausted }` | `attempts` 0; `exhausted` benar kalau lifecycle `failed` dengan `last_failure` `crash_loop` |
| `blocker` | satu nilai dengan prioritas `identityUnproven`, lalu `dataInUse`, lalu `needsSecret` (spec 0011), atau null | dihitung ulang oleh rekonsiliasi dan start berikutnya |
| `endpoints` | `{ name, transport, host, port, portMode, bound, socketPath }`; `port` adalah port efektif (`bound` kalau ada, kalau tidak port `Fixed`, null untuk `Auto` tanpa `bound`); `bound` benar hanya setelah readiness berhasil untuk proses yang sekarang; `socketPath` = folder run ditambah `socketName` | `bound` benar untuk proses yang diadopsi setelah ready pertama |
| `leftoverProcesses` | daftar PID dari stop terakhir | kosong |
| `logsUnavailable` | benar kalau spool proses yang berjalan tidak bisa dibaca | dihitung saat adopsi |

- Enum wire `lifecycle`, `desired`, `readiness.state`, dan `health.state` tertutup (tanpa varian fallback), karena ketiganya sudah punya nilai `unknown` yang nyata dan himpunannya ditetapkan PRD 19.1; menambah nilai berarti protocol major.
- `instance.state_changed { instance: InstanceSummary }` lewat `publish_resource(Instance(id), revision)` setelah setiap batch yang menaikkan revision (create, lifecycle, desired, port `Auto`). Instance yang menjadi `retained` diumumkan lewat event ini dengan lifecycle `retained`; client membuangnya dari daftar default, karena `instance.list` tidak memuat instance retained.
- `instance.health_changed { instanceId, statusSeq, readiness, health, blocker, restart, process, logsUnavailable }` lewat `publish_resource(Instance(id), revision saat ini)` setiap kali nilai `effective` readiness atau health, blocker, status restart, proses, atau ketersediaan log berubah. Client menerapkannya kalau `statusSeq` lebih besar dari yang dipegang untuk instance itu, tanpa melihat revision (field status yang sama di dalam `InstanceSummary` milik `instance.state_changed` juga mengikuti `statusSeq` nya), dan mengambil snapshot baru setelah gap `server_restarted` (pengecualian langkah 4 resep spec 0006, Follow-up). Pembaruan `observed_at` saja tanpa perubahan nilai tidak dikirim.
- `daemon.reconciling { phase: started | finished, instances, adopted, unknown, restarting }` lewat `publish_daemon`.

### Permukaan IPC

Semua method lewat handshake dan peer user yang sama (spec 0006). Method mutasi wajib `idempotencyKey`. Urutannya mengikuti spec 0007: key wajib, lalu pencarian key (replay mengembalikan reply aslinya), baru `precheck` milik fitur ini: penolakan berdasarkan state menjadi `Reject` (error langsung), jawaban `alreadyInState` menjadi `NoOp` (kalau ia menulis desired state, engine meng commit tulisan itu bersama baris operasi `succeeded` tanpa langkah supaya key terikat), dan selain itu `Proceed`. Selain itu jawabannya `InstanceActionAccepted { accepted: Option<OperationAccepted>, alreadyInState: bool }`, dengan `OperationAccepted` spec 0007 untuk operasi yang resource nya sudah memuat `InstanceId`; jawaban `alreadyInState` tidak membuat operasi dan tidak mengikat key.

| Method | Input | Output | Error sinkron | Kegagalan operasi |
| --- | --- | --- | --- | --- |
| `instance.create` | `{ name, displayName?, provider: { name, revision }, runtimeId, endpoints: [{ name, port: { type: fixed, content: u16 } \| { type: auto } }], settings: object JSON, secrets: [{ field, input: SecretInput }] }` | `InstanceActionAccepted` (operasi `create_instance`) | `instance.name_taken`, `instance.provider_not_found`, `instance.provider_ambiguous`, `instance.runtime_not_found`, `instance.runtime_mismatch`, `instance.settings_invalid` (`fields[]`), `instance.endpoint_invalid` | `supervisor.endpoint_conflict`, `secret.needs_secret`, `secret.*`, `supervisor.init_failed` |
| `instance.start` | `{ instanceId, expectedRevision? }` | `InstanceActionAccepted` (operasi `start_instance` atau no op) | `resource.not_found`, `resource.revision_mismatch`, `supervisor.instance_retained`, `supervisor.not_initialized`, `supervisor.instance_stopping`, `supervisor.identity_unproven` | `resource.busy` dan kode `supervisor.*` serta `process.*` lain di tabel kode |
| `instance.stop` | `{ instanceId, expectedRevision?, overrideLeases }` | `InstanceActionAccepted` (operasi `stop_instance` atau no op) | `resource.not_found`, `resource.revision_mismatch`, `supervisor.instance_retained`, `supervisor.instance_initializing`, `supervisor.identity_unproven`, `supervisor.stop_requires_override` | `resource.busy`, `supervisor.stop_timeout`, `supervisor.stop_command_failed`, `process.signal_refused` |
| `instance.get` | `{ instanceId }` | `InstanceSummary` (termasuk instance retained, dengan lifecycle `retained`) | `resource.not_found` | |
| `instance.list` | `{ limit? (1 sampai 200, default 100), before?: String }` | `{ items: [InstanceSummary], next: String? }`, urut `name` lalu `id`, tanpa instance retained; halaman berhenti sebelum JSON nya melewati 900 KiB; cursor teks opaque `<name>_<uuid>` yang dikembalikan client apa adanya | `request.invalid_params` | |

Create minimal: validasi di method (nama VAL-001, paket pinned ditemukan unik untuk `name` dan `revision` lalu dibaca dan digest nya cocok dengan registry, runtime ada dan `installed` dengan software sama, `SettingsSchema::validate`, setiap endpoint `tcp` atau `udp` manifest punya permintaan port dengan default `auto`, endpoint `unix` tidak menerima permintaan), lalu operasi `create_instance` dengan tahap `validate` (diulang, plus `endpoints_conflict` untuk port `Fixed`), `secrets` (`SecretService::put` dengan `PutTarget::New` per field secret, spec 0011), `layout` (`create_instance_layout`), `register` (`InsertInstance`, config revision awal, dan pemakaian secret dalam satu batch; host dari `defaultHost`, socket dari `socketName`), lalu `init` (`execute_init`). Kegagalan sebelum `register` menjalankan kompensasi spec 0007 (hanya path dan reference yang dibuat operasi ini); kegagalan di `init` meninggalkan instance `failed` untuk di purge (fitur 54). Fitur 20 menambah preflight, form, dan opsi lain secara additive.

### Kode error

Semua `retryable` false kecuali disebut. Pesan bahasa Inggris, huruf kecil di awal, tanpa titik, tanpa nilai settings teks atau secret.

| Kode | Kategori | Kapan | Kunci `details` |
| --- | --- | --- | --- |
| `supervisor.not_initialized` | `conflict` | start sebelum `init_succeeded` (BR-008) | `lifecycle` |
| `supervisor.instance_initializing` | `conflict` | stop saat `initializing` | |
| `supervisor.instance_stopping` | `conflict` | start saat `stopping` | |
| `supervisor.instance_retained` | `conflict` | aksi pada instance retained | |
| `supervisor.identity_unproven` | `conflict` | start atau stop saat `unknown` | `pid`; `items` ber `kind` `check` (field `name`, `result`) |
| `supervisor.data_in_use` | `conflict` | lock instance dipegang, atau proses observasi terakhir masih hidup | `lockPath`, `pid` |
| `supervisor.endpoint_in_use` | `conflict` | port dipakai proses lain | `endpoint`, `address`, `port` |
| `supervisor.endpoint_conflict` | `conflict` | port efektif instance Stackly lain | `endpoint`, `port`, `otherInstance` |
| `supervisor.auto_port_unavailable` | `unavailable`, retryable | tidak ada kandidat bebas setelah `AUTO_PORT_ATTEMPTS` | `endpoint` |
| `supervisor.runtime_unavailable` | `conflict` | runtime atau dependency tidak `installed` | `runtimeId`, `state` |
| `supervisor.package_unavailable` | `failed` | paket pinned hilang, tidak valid, atau digest beda | `provider`, `reason` |
| `supervisor.config_invalid` | `failed` | dry run `run_plan` gagal karena settings atau endpoint tidak lagi cocok | `field`, `reason` |
| `supervisor.executable_invalid` | `failed` | verifikasi `ExecutablePath` gagal | `reason` |
| `supervisor.data_unavailable` | `failed` | folder instance hilang, bukan milik user, atau tidak privat | `path`, `reason` |
| `supervisor.data_incompatible` | `conflict` | `preflight` selain `Compatible` | `outcome`, `data`, `runtime` |
| `supervisor.readiness_timeout` | `timeout` | deadline readiness habis, proses tetap hidup | `deadlineSeconds` |
| `supervisor.retry_budget_exhausted` | `failed` | crash loop saat operasi start | `attempts`, `lastExitCode`, `lastSignal` |
| `supervisor.stop_timeout` | `timeout` | anggaran stop habis | `timeoutSeconds` |
| `supervisor.stop_requires_override` | `conflict` | lease environment aktif (BR-015) | `items` ber `kind` `environment` (field `environmentId`) |
| `supervisor.stop_command_failed` | `failed` | perintah stop keluar dengan kode lain | `exitCode` |
| `supervisor.reload_failed` | `failed` | aksi atau verifikasi reload gagal | `reason` |
| `supervisor.capability_unsupported` | `unsupported` | capability tidak ada (BR-016) | `capability` |
| `supervisor.init_failed` | `failed` | init berakhir `failed` | `reason`, `step`, `exitCode` |
| `process.spawn_failed` | `failed` | `posix_spawn` gagal | `kind` |
| `process.identity_capture_failed` | `failed` | identitas tidak bisa ditangkap atau dicatat | |
| `process.signal_refused` | `conflict` | identitas berubah saat pemeriksaan ulang | `pid` |
| `process.unsupported` | `unsupported` | platform tanpa supervisi proses | `os` |
| `instance.name_taken` | `conflict` | nama sudah dipakai | `existing` |
| `instance.provider_not_found` | `not_found` | paket pinned atau provider revision tidak ada | `provider`, `revision` |
| `instance.provider_ambiguous` | `conflict` | lebih dari satu digest untuk nama dan revision itu | `provider`, `revision` |
| `instance.runtime_not_found` | `not_found` | runtime tidak ada | `runtimeId` |
| `instance.runtime_mismatch` | `conflict` | software runtime berbeda dengan provider | `runtimeSoftware`, `providerSoftware` |
| `instance.settings_invalid` | `invalid_input` | `SettingsError`; `fields[]` per issue | |
| `instance.endpoint_invalid` | `invalid_input` | endpoint tidak ada di manifest, port di luar 1 sampai 65535, atau permintaan untuk endpoint `unix` | `endpoint` |

`FailureCode` yang dipakai supervisor: `interrupted`, `process_exited`, `process_signaled`, `exit_status_unknown`, `crash_loop`, `spawn_failed`, `executable_invalid`, `identity_capture_failed`, `config_render_failed`, `endpoint_in_use`, `data_in_use`, `init_step_failed`, `init_step_timeout`, `init_timeout`.

### Model data

Tidak ada tabel atau kolom baru.

| Data | Tempat | Isi | Ditulis |
| --- | --- | --- | --- |
| `process_observations` | registry (spec 0005) | satu baris per spawn dan per adopsi, format di *Identitas majemuk*; bukan sumber kebenaran tunggal dan tidak pernah mengotorisasi kill | batch yang sama dengan `spawned` atau `adopted`, lewat `WriteOp::RecordProcessObservation` (update spec 0005) |
| observasi terakhir | registry | `Registry::latest_process_observation(InstanceId)` | dibaca rekonsiliasi, pemeriksaan ulang, dan pengaman start |
| config revision awal | registry (`config_revisions`) | nilai settings dengan reference secret | create, lewat `WriteOp::InsertConfigRevision` dan `Registry::active_config_revision` (update spec 0005) |
| lifecycle, desired, `last_failure` | registry (`instances`) | aggregate spec 0004 | CAS per commit |
| port `Auto` (`bound_port`) | registry (`instance_endpoints`) | port yang diberikan ke proses yang sekarang | batch `spawned` (aturan `bound` di spec 0004 diperbarui, Follow-up) |
| `instance.lock` | root instance | file kosong 0600 | supervisor saat pertama dibutuhkan |
| spool | folder run | output mentah proses utama | proses utama |
| state actor | memori daemon | `statusSeq`, riwayat restart, penghitung probe, observasi readiness dan health, blocker, ekor log, detail `lastFailure`, `adoptable`, sisa proses | hilang saat daemon berhenti |

### Konstanta

| Konstanta | Nilai |
| --- | --- |
| `READINESS_REFRESH_INTERVAL` | 10 detik (`// provisional: F28`) |
| `READINESS_FAILURE_THRESHOLD` | 2 kegagalan berturut turut |
| `READINESS_MAX_AGE` | 30 detik (spec 0004) |
| `HELPER_TERM_GRACE` | 5 detik |
| `UNKNOWN_RECHECK_INTERVAL` | 30 detik |
| `COMMIT_RETRY_LIMIT` | 5 |
| `REGISTRY_RETRY_INTERVAL` | 5 detik |
| `RECONCILE_START_CONCURRENCY` | 4 |
| `SPOOL_DRAIN_INTERVAL` | 500 ms |
| `SPOOL_READ_CHUNK` | 1 MiB |
| `LOG_LINE_MAX` | 16 KiB |
| `LOG_LINE_HARD_MAX` | 64 KiB |
| `SPOOL_BACKLOG_MAX` | 256 MiB per stream per instance |
| `TAIL_LINES` | 64 per stream |
| `FAILURE_EXCERPT_LINES` | 10 baris, masing masing paling banyak 256 karakter |
| `HELPER_CAPTURE_MAX` | 64 KiB per stream |
| `HTTP_HEADER_MAX` | 8 KiB |
| `AUTO_PORT_ATTEMPTS` | 16 |

Konstanta restart ada di *Restart policy*. Semuanya ada di `SupervisorConfig` dan bisa diganti di test; benchmark fitur 64 boleh menyetel ulang nilai produksi lewat update spec ini.

### Asumsi lintas spec

- **Spec 0007** (diselaraskan 2026-10-18): setiap operasi instance adalah `OperationWork` yang dimulai lewat `OperationEngine::begin(BeginRequest)`; tahap di *Startup transaction*, *Stop*, dan create menjadi langkah `StepSpec { name, repeatable, reversible }`, dengan `spawn` dan langkah sesudahnya `reversible: false` dan tanpa kompensator yang menghentikan proses (cancel setelah spawn adalah stop biasa di dalam pekerjaan). Origin `user` (dengan `ClientInfo` dan interaksi Keychain dari `OperationTrigger`) untuk aksi pengguna, dan `background` untuk restart policy dan rekonsiliasi, dengan pemicunya (`restart_policy` atau `reconcile`) dicatat di input journal. `BusyPolicy::Queue` untuk start, create, dan reload pengguna; `BusyPolicy::Reject` untuk stop pengguna selama start berjalan (AC-7) dan untuk restart policy, dan `BusyPolicy::Queue` untuk start sistem dari rekonsiliasi (langkah 7), sehingga ia menunggu operasi pengguna yang dibangun ulang alih alih ditolak. Untuk jenis `CloseInterrupted`, titik tak bisa dibalik hanya menahan kompensasi: kegagalan setelah spawn selesai `failed` dengan kode supervisor, dan cancel tetap sampai ke pekerjaan (stop proses, lalu `cancelled`). Operasi `start_instance`, `stop_instance`, `restart_instance`, dan `reload_instance` berkebijakan `CloseInterrupted` (ditutup `recover` sebelum rekonsiliasi); `create_instance` tetap dikompensasi spec 0007. Actor memakai `LockTable::holder` untuk tahu apakah keluarnya proses milik operasi yang sedang berjalan, dan `try_lock_internal` untuk menulis lifecycle setelah crash. Engine tidak memegang transaksi SQLite selama tahap berjalan.
- **Assumed (spec 0005)**: `WriteOp::RecordProcessObservation`, `Registry::latest_process_observation`, `WriteOp::InsertConfigRevision` (sekaligus mengisi `instances.config_revision_id`), dan `Registry::active_config_revision` ditambahkan lewat update spec itu, dengan fake dan conformance; beberapa `UpdateInstance` untuk instance yang sama boleh berurutan dalam satu batch dengan `expected` berurutan; retensi 20 observasi per instance tetap; `Instance::restore` memasang readiness dan health `unknown` dengan reason `daemon_restarted`.
- **Assumed (spec 0006)**: event `instance.state_changed`, `instance.health_changed`, dan `daemon.reconciling` didefinisikan di sini; area `supervisor`, `process`, dan `instance` (pemakai pertama) milik spec ini; supervisor mengisi slot start dan langkah berhenti, dan mengambil alih subsystem `supervisor`; `daemon.starting` dengan subsystem `supervisor` dijawab untuk method instance selama slot berjalan; resep client langkah 4 menerapkan `instance.health_changed` berdasarkan `statusSeq` (Follow-up).
- **Assumed (spec 0009)**: plan dibangun ulang tepat sebelum setiap aksi dari paket pinned; `RunPlan.render` dan destination `materializeSecret` berupa nama file ditambahkan lewat update spec itu (Follow-up); timeout dan exit code mengikuti tabel *Sumber timeout dan exit code*; `PlanError` dari dry run dipetakan ke `supervisor.config_invalid` dengan nama field di detail, dan `PlanError` di create ke `instance.settings_invalid` atau `instance.endpoint_invalid`; `executionMode: foreground` berarti proses utama tidak me re exec ke executable lain dan tidak menutup fd warisan.
- **Assumed (spec 0011)**: `resolve_for_start`, `ensure_not_in_argv`, `stdin_payload`, `SecretFile`, `remove_secret_material`, `sweep_secret_material`, `learn_running`, `Redactor`, dan `SecretService::put` dengan `PutTarget::New` dipakai apa adanya; interaksi mengikuti pemicu (percobaan pertama aksi pengguna dari klien interaktif `AllowPrompt`; respawn, `restart_policy`, dan `reconcile` `Never`); field `blocker` tetap satu nilai, dengan prioritas di *Status dan event*.
- **Assumed (spec 0008)**: fitur 14 membangun `stackly instance create`, `start`, `stop`, dan `get` di atas method ini dengan `--wait` dan `--json`.
- **Assumed (spec 0012 dan 0013)**: DTO status di atas memenuhi kebutuhan `StatusLabel` dan adapter Swift; Swift mendecode enum lifecycle, readiness, dan health tertutup dan menerapkan aturan `statusSeq`.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| create | `InstanceId` | `InstanceId::generate()` oleh daemon saat operasi dibuat |
| create | provider revision key | paket pinned di `<data>/providers/<namespace>/<name>/<revision>/<sha>/` yang cocok dengan `provider.name` dan `provider.revision` di params, dibaca lewat `read_package` dan `from_files` |
| create | settings tervalidasi | `SettingsSchema::validate(params.settings)` |
| create | reference secret | `SecretService::put(PutTarget::New)` per `params.secrets` |
| create | permintaan port | `params.endpoints`, default `Auto`; host dari `defaultHost` manifest |
| create, start | data line dan versi | record runtime (`line`, `version`) |
| start | `PlanContext.paths` | `DirectoryLayout::instance_paths` dan `instance_run_dir(short key)` |
| start | `PlanContext.runtime` | `runtime_dir(software, version, target, artifact sha)` di canonicalize, versi dan line dari record runtime |
| start | `PlanContext.settings` | `Registry::active_config_revision`, divalidasi ulang dengan `SettingsSchema::validate` |
| start | port `Fixed` | `PortRequest::Fixed` di endpoint instance |
| start | port `Auto` | `bound` tersimpan kalau `PortProbe` bisa bind; kalau tidak, port dari bind `127.0.0.1:0` atau `[::1]:0` di alamat endpoint, lalu `resolve_port` |
| start | nilai secret | `resolve_for_start` (spec 0011) |
| start | argv, env, cwd, timeout | `RunPlan` spec 0009 |
| start | `ProcessIdentity` | `ProcessFacts` dari `inspect(pid)` pada child yang tertahan, plus `boot_id()` |
| start | `process.startedAt` | `ProcessStart` dari fakta proses |
| start | `adoptable` | `holds_file` saat readiness pertama |
| start | `bound_port` | port yang dipakai di plan, dicommit di batch `spawned` |
| start | `endpoints[].bound` | readiness pertama yang berhasil untuk proses yang sekarang |
| readiness, health | state, reason, `observed_at` | hasil probe dan `Clock::now()` |
| semua perubahan status | `statusSeq` | penghitung actor, naik setiap perubahan isi summary |
| restart | jeda | rumus di *Restart policy* dengan `getrandom` |
| restart | `nextAttemptAt` | `Clock::now()` ditambah jeda |
| stop | sinyal | `StopPlan` spec 0009 |
| stop | sisa proses | `group_members(pid)` |
| rekonsiliasi | identitas tercatat | `latest_process_observation` |
| rekonsiliasi | boot ID | `boot_id()` |
| rekonsiliasi | state lock | `probe_lock(InstancePaths.lock)` |
| rekonsiliasi | port untuk probe proses yang diadopsi | `bound_port` atau port `Fixed` tersimpan |
| semua | cuplikan kegagalan | `TailSink`, sudah lewat `Redactor`; setelah daemon restart, ekor spool yang dibaca dan disamarkan saat rekonsiliasi |
| `instance.get` | readiness dan health | observasi actor lewat `effective(now, READINESS_MAX_AGE)` |
| `instance.list` | urutan dan halaman | `Registry::instances` urut `name` lalu `id`, dipotong per `limit` setelah `cursor` |
| test | fake runtime dan paket | `supervisor::fixture::install_fake` (fitur crate `fixture`): menyalin `fake_server::binary()` ke `runtime_dir` dengan SHA-256 nya dan menyalin `providers/fake` ke `provider_revision_dir`, lalu mengembalikan `Runtime`, `ArtifactRecord`, dan `ProviderRevisionRecord` untuk di commit test |

### Key invariants

- Paling banyak satu proses utama per instance pada satu waktu, dan tidak ada spawn selama `instance.lock` dipegang pihak lain atau proses observasi terakhir masih hidup.
- Tidak ada proses utama yang menjalankan kode sebelum identitasnya tercatat di registry.
- Setiap sinyal ke proses utama atau proses yang diadopsi didahului pemeriksaan ulang identitas; `SIGKILL` hanya ke group proses pembantu yang belum di reap dan ke child tertahan yang belum pernah berjalan. Tidak ada API yang menerima nama proses.
- `Unproven`, termasuk error saat mengumpulkan bukti, tidak pernah menghasilkan sinyal, hapus file, atau start.
- Proses yang diadopsi selalu melewati ketujuh pemeriksaan identitas.
- Readiness `ready` hanya dari probe yang berhasil saat lifecycle `running`, dan readiness atau health tidak pernah mengubah lifecycle atau desired state.
- Restart policy tidak pernah menjalankan ulang proses saat desired `stopped`, saat lifecycle `unknown`, saat ada commit actor yang tertunda, atau di luar budget.
- Output mentah proses hanya ada di spool runtime root; setiap baris logis lewat `Redactor` sebelum dipotong dan sebelum sampai ke sink mana pun.
- Supervisor tidak pernah menghapus `instance.lock`, folder data, atau file di luar spool, file secret spec 0011, `InitPlan.cleanup`, dan socket basi di folder run.
- Tidak ada `std::process::Command` atau `tokio::process` di kode non test `stackly-core` dan `stacklyd`, dan token `SIGKILL` hanya muncul di `stackly-platform-macos/src/process/kill.rs` (ditegakkan `cargo xtask check-deps`).
- Fitur crate `fixture` di core hanya boleh dinyalakan dari dev dependency (ditegakkan `check-deps`).

### Security model

- Hanya daemon yang men spawn dan menyinyal proses; CLI dan GUI hanya memanggil method yang diotorisasi lewat peer user yang sama (spec 0006).
- Proses anak berjalan dengan hak user, di session sendiri, dengan env tertutup dari plan, cwd di folder instance, dan tanpa fd daemon selain yang dipetakan (`CLOEXEC_DEFAULT`).
- Tidak ada shell di mana pun; satu entri argv adalah satu elemen argv.
- Secret tidak pernah di argv (`ensure_not_in_argv` sebelum `journal`), hanya lewat mekanisme spec 0011, dan output proses disamarkan sebelum disimpan atau dikirim.
- Spool, folder `secrets/`, dan folder run ada di runtime root 0700; lock 0600 di root instance 0700.
- Probe hanya ke endpoint loopback milik instance, tanpa redirect dan tanpa membaca body.
- Residual risk yang dinyatakan: proses lain milik user yang sama bisa membaca spool selama belum dibebaskan; selama daemon mati, spool provider yang cerewet tumbuh tanpa batas di volume sistem sampai daemon hidup lagi; macOS bisa membersihkan runtime root saat proses berjalan, sehingga output yang belum terbaca hilang; secret yang terpotong batas `LOG_LINE_HARD_MAX` bisa lolos redaksi; dan PID proses yang diadopsi bisa dipakai ulang di antara pemeriksaan ulang dan `kill(2)` (jendela mikrodetik, butuh putaran penuh ruang PID).

### Configuration required

Tidak ada variabel environment atau credential baru. Kode test memakai `STACKLY_ROOT` (spec 0004) lewat `SpawnedDaemon` (spec 0006) dan `SupervisorConfig` untuk konstanta yang lebih kecil.

### Menjalankan test di Mac ini

- Unit test core memakai `FakeProcessSupervision`, `FakeRegistry`, `FakeSecretStore`, `PortProbe` tiruan, dan waktu tokio yang dijeda (`tokio::time::pause`), jadi backoff 5 menit berjalan dalam milidetik.
- Integration test `crates/platform-macos/tests/process.rs` menjalankan `stackly-fake-server` asli lewat adapter, dengan `TestRoot` dan `allocate_port`; guard test me reap setiap child dan memastikan tidak ada proses tersisa.
- Integration test daemon di `apps/daemon/tests/supervisor.rs` memakai `SpawnedDaemon`, `install_fake`, dan registry SQLite di `TestRoot`; test restart membunuh proses daemon (bukan proses instance) dengan `SIGKILL`.
- Test yang membuktikan ID PRD (REQ-011, REQ-012, REQ-013, BR-010, AC-011 bagian fake, AC-012, AC-013 bagian fake, AC-058, AC-067, AC-068, NFR-003, NFR-006, RISK-03, RISK-14) memasang guard evidence `fixture` spec 0002. Bukti dengan provider nyata milik fitur 19, 23, 26, dan 30.
- Pengukuran AC-20 dijalankan dengan tangan lewat `cargo xtask evidence new` dengan nilai terukur.

### Critical test scenarios

- Happy path: `install_fake`, `instance.create` (dengan secret `authToken`), `instance.start`, readiness `not_ready` lalu `ready`, `instance.get` menampilkan keempat state terpisah, `instance.stop`, `stopped`. Memverifikasi **AC-3**, **AC-4**, **AC-5**, **AC-13**, **AC-16**.
- Path utuh: `TestRoot` di folder `Test Root Ünï cödé`, `label` berisi `` $(touch pwned); rm -rf ~ `id` "q" 'q' | & > ü 空 ``; `argv-serve.json` sama persis dan `pwned` tidak ada; `fd_numbers` child yang tertahan hanya 0 sampai 3, lalu `resume`. Memverifikasi **AC-1**.
- Spawn tertahan: commit yang gagal (fake registry) membuat child dibuang lewat `discard` sebelum menjalankan kode, `start_failed`, dan tidak ada proses tersisa; daemon yang dibunuh setelah commit tetapi sebelum `resume` membuat rekonsiliasi melanjutkan dan mengadopsi proses itu. Memverifikasi **AC-2**, **AC-9**.
- Stop: fake dengan `ignoreTerm` membuat stop gagal `supervisor.stop_timeout`, lifecycle `stopping`, tanpa sinyal lain; stop kedua mengirim ulang; setelah fake keluar sendiri (lewat `shutdown`), handler exit yang sama menghasilkan `stopped` dan menghapus material secret; anak fake yang ditinggal dilaporkan, tidak disinyal; identitas yang diubah test menghasilkan `process.signal_refused` dan `unknown`. Memverifikasi **AC-5**.
- Proses kedua: proses pembantu test memegang `instance.lock` (fd diwariskan), lalu `instance.start` gagal `supervisor.data_in_use` tanpa spawn; `instance.lock` yang dihapus saat fake berjalan tetap membuat start gagal lewat pengaman observasi; sepuluh start bersamaan dari dua client menghasilkan satu spawn. Memverifikasi **AC-6**.
- Crash loop: fake dengan `crashAfterMs` 100 di bawah waktu dijeda menghasilkan jeda dalam rentang jitter, total 6 spawn, lifecycle `failed`, `crash_loop`, `exhausted`; stop di tengah jeda berjalan tanpa `resource.busy`; respawn tidak memunculkan dialog (`FakeSecretStore` mencatat interaksi `Never`); run native yang sama selesai dalam 60 detik. Memverifikasi **AC-7**, **AC-8**, **AC-18**.
- Readiness timeout dan cancel: fake dengan `readyDelayMs` di atas deadline membuat operasi `failed` `supervisor.readiness_timeout`, proses tetap hidup, lalu readiness menjadi `ready` dan `bound` final setelah delay; cancel di tiga titik (sebelum `journal`, saat jeda, saat proses hidup) menghasilkan state di AC-8. Memverifikasi **AC-8**.
- Daemon crash: daemon dibunuh dengan `SIGKILL`, fake terus menulis spool, daemon baru mengadopsi PID yang sama, readiness kembali `ready` di port `Auto` tersimpan, batch pertama `resumed` tanpa baris terpotong. Memverifikasi **AC-9**, **AC-12**.
- PID dipakai ulang: observasi diubah test menjadi PID proses `sleep` milik test; dengan lock bebas instance dianggap hilang dan `sleep` tidak disinyal; dengan lock dipegang instance `unknown` dengan blocker `dataInUse`; `holds_file` yang diinjeksi gagal menghasilkan `identityUnproven`; start dan stop ditolak. Memverifikasi **AC-10**.
- Tabel rekonsiliasi: setiap sel tabel dengan `FakeProcessSupervision` (termasuk boot baru, recovery mode, error di setiap input klasifikasi, dan proses `Proven` pada lifecycle `stopped`), dan pemeriksaan bahwa setiap transisi yang dipakai lolos `Instance::apply_lifecycle`. Memverifikasi **AC-11**.
- Spool: spam log 2000 baris per detik selama 60 detik dengan blok teralokasi di bawah 2 MiB; sink `Busy` tidak memperlambat fake; backlog di atas batas test menghasilkan penanda truncation; nilai secret yang dikenal `Redactor` di baris 40 KiB tersamar walau baris dipotong; byte bukan UTF-8 menjadi U+FFFD. Memverifikasi **AC-12**.
- Init: init fake menulis `token.sha256` yang cocok dan file secret sementara hilang setelahnya; paket test di `crates/core/tests/fixtures/packages/` yang langkah `Run` nya menjalankan `serve --ignore-term=true` (tidak pernah keluar sendiri dan mengabaikan `SIGTERM`) dengan `timeoutSeconds: 1` dihentikan dengan TERM lalu KILL dan berakhir `init_step_timeout`; probe `exec` yang menggantung diperlakukan sama. Memverifikasi **AC-13**, **AC-15**.
- Reload: greeting berubah tanpa PID baru; paket tanpa `reload` ditolak tanpa sinyal. Memverifikasi **AC-14**.
- Port: listener luar di port `Fixed` menghasilkan `supervisor.endpoint_in_use` dan listener tetap hidup; `PortProbe` tiruan yang melaporkan port direbut setelah exit sebelum ready menggagalkan operasi tanpa respawn; port `Auto` yang tersimpan dipakai ulang dan dicek ulang saat respawn. Memverifikasi **AC-17**.
- Preflight: runtime `missing`, digest paket berbeda, executable diganti symlink ke luar root runtime, folder data mode 0755, penanda `FAKE_VERSION` berbeda, settings yang tidak lagi cocok dengan schema, dan `FakeSecretStore` terkunci masing masing gagal dengan kodenya sebelum `journal`. Memverifikasi **AC-3**, **AC-18**.
- Platform: `UnsupportedPlatform` dan `cargo check` Windows untuk keempat crate. Memverifikasi **AC-19**.
- Anggaran: 10 fake idle 5 menit di bawah 150 MiB dan 1 persen CPU. Memverifikasi **AC-20**.
- Shutdown daemon: `SIGTERM` saat satu operasi start berjalan, proses fake tetap hidup, operasi ditutup `failed` `interrupted` oleh `recover` spec 0007 pada start berikutnya, dan proses diadopsi rekonsiliasi. Memverifikasi **AC-21**.
- Commit tertunda: `FakeRegistry` yang menjawab konflik revision lalu `NoSpace` saat crash membuat actor mencoba lagi, subsystem `degraded`, dan tidak ada respawn sampai commit berhasil. Memverifikasi **AC-22**.
- Jawaban method: start pada `unknown`, stop pada `initializing`, start kedua pada `running`, dan stop pada `stopped` dengan desired `running` menjawab sesuai AC-23 tanpa membuat operasi (kecuali commit desired). Memverifikasi **AC-23**.

## Build plan

Urutan mengikuti pendekatan Journey: satu jalur dibuat lengkap dulu, yaitu "fake instance dibuat, dijalankan sampai ready, dan dihentikan dengan aman", termasuk semua jalur gagalnya, lalu crash dan daemon restart di atasnya, lalu permukaan IPC ujung ke ujung. Milestone 1 bisa mulai setelah update spec 0004 di Follow-up masuk. Milestone 2 butuh plan spec 0009 (milestone 1), `secret::delivery` spec 0011, port registry dan `FakeRegistry` spec 0005 (milestone 2, plus update di Follow-up), dan API operasi spec 0007. Milestone 4 butuh server daemon spec 0006 dan SQLite registry spec 0005.

**Milestone 1: Port proses dan adapter macOS**

1. Spike di Mac ini untuk setiap baris tabel *Adapter macOS*, termasuk `POSIX_SPAWN_START_SUSPENDED` dengan `SIGCONT` dan `SIGKILL`, proses yang bukan child, flock yang diwariskan setelah induknya keluar, `F_PUNCHHOLE` dan `SEEK_DATA` di darwin user temp, `getsid` untuk proses lain, dan `proc_pidpath` yang sama dengan path canonical executable di root runtime; hasilnya dicatat sebagai record manual. Memenuhi **AC-1**, **AC-2**, **AC-9**, **AC-12** (bukti kelayakan).
2. Domain dan platform: modul `process` di domain, trait `ProcessSupervision` beserta tipenya, port ketujuh di `Platform`, `UnsupportedProcessSupervision`, capability, `InstancePaths.lock` dan path spool di layout. Memenuhi **AC-19**.
3. `stackly-platform-macos::process`: spawn tertahan, resume, discard, spawn pembantu, inspect, signal dengan pemeriksaan ulang, terminate pembantu, watcher kqueue dengan mutex per PID, lock, spool, boot ID, `holds_file`, dan `fd_numbers`, dengan integration test memakai dummy server. Memenuhi **AC-1**, **AC-2**, **AC-5**, **AC-6**, **AC-15**.
4. `FakeProcessSupervision` di testkit dan pemasangannya di `FakePlatform`. Memenuhi **AC-6**, **AC-7** (prasyarat test core).

Sesi paralel: setelah langkah 2 masuk, langkah 3 (adapter) dan langkah 4 (fake) bisa dikerjakan dua sesi terpisah karena hanya bertemu di trait. Langkah 1 bisa jalan kapan saja di sesi sendiri, tetapi hasilnya harus ada sebelum langkah 3 selesai.

**Milestone 2: Start sampai ready dan stop dengan aman**

5. Core dasar: `classify` (fungsi murni dengan test tabel), rumus restart policy, `SupervisorConfig`, `PortProbe`, dan aturan *Commit*. Memenuhi **AC-7**, **AC-10**, **AC-22** (logika).
6. Runner probe `tcp`, `http` (client kecil), dan `exec`, dengan test terhadap dummy server dan listener test. Memenuhi **AC-4**, **AC-15**, **AC-19**.
7. Executor: `execute_init`, render sebelum spawn, startup transaction (termasuk dry run dan spawn tertahan), stop, dan alokasi port, di atas actor per instance, dengan test core memakai fake. Memenuhi **AC-2**, **AC-3**, **AC-5**, **AC-6**, **AC-8**, **AC-13**, **AC-17**, **AC-18**.
8. Readiness dan health rutin, `statusSeq`, perhitungan stale, `holds_file` saat ready, dan `execute_reload`. Memenuhi **AC-4**, **AC-14**.

Sesi paralel: langkah 5 dan 6 tidak bergantung satu sama lain dan bisa menjadi dua sesi; langkah 7 dan 8 menunggu keduanya dan sebaiknya satu sesi karena menyentuh actor yang sama.

**Milestone 3: Crash, spool, dan rekonsiliasi**

9. Restart policy di actor, termasuk retry di dalam operasi start, respawn dari `recovering` tanpa `start_begin`, timer tanpa lock resource, operasi sistem saat timer jalan, deteksi port direbut, dan reset setelah sehat. Memenuhi **AC-7**, **AC-8**, **AC-17**.
10. Spool: pembaca, UTF-8 lossy, redaksi baris utuh, `LogSink`, `SinkSet`, `TailSink`, punch hole, backlog, dan resume. Memenuhi **AC-12**.
11. Rekonsiliasi: tabel lengkap, lanjutkan proses tertahan, pemeriksaan ulang `unknown` dan `dataInUse`, operasi terputus, sweep dan `learn_running` spec 0011, commit actor tertunda, dan shutdown daemon tanpa sinyal. Memenuhi **AC-9**, **AC-10**, **AC-11**, **AC-21**, **AC-22**.

Sesi paralel: langkah 10 (spool) hanya menyentuh modul `spool` dan `sink` serta satu titik panggil di actor, jadi bisa sesi terpisah dari langkah 9 dan 11.

**Milestone 4: Daemon, IPC, dan bukti ujung ke ujung**

12. Protocol: DTO, params, event, `InstanceActionAccepted`, dan kode error dengan fixture di `protocols/fixtures/`, `cargo xtask codegen`, dan test kontrak Swift termasuk resep `statusSeq`. Memenuhi **AC-16**.
13. `stacklyd`: rekonsiliasi di slot start, langkah supervisor saat berhenti, subsystem `supervisor`, handler `ipc::instance` dengan penolakan sinkron, create minimal, pemetaan DTO, dan `install_fake` di fitur `fixture`. Memenuhi **AC-3**, **AC-11**, **AC-16**, **AC-21**, **AC-23**.
14. Integration test daemon untuk setiap skenario di *Critical test scenarios* yang butuh daemon, dengan guard evidence. Memenuhi **AC-3** sampai **AC-12**, **AC-17**, **AC-21**, **AC-23**.
15. Aturan `check-deps` baru (process API, `SIGKILL`, fitur `fixture`) dengan crate fixture yang melanggar, lalu pengukuran NFR-003 di Mac ini. Memenuhi **AC-15**, **AC-20**.

Sesi paralel: langkah 12 (protocol dan Swift) dan langkah 15 bagian `check-deps` bisa jalan di sesi terpisah sejak milestone 2 selesai; langkah 13 dan 14 satu sesi.

## Consequences

**Positive**:

- Crash atau restart daemon tidak mematikan database pengguna, dan jalur adopsi yang sama nanti bisa dipakai fitur 30 untuk update daemon tanpa downtime.
- Spawn tertahan menghapus jendela di mana proses berjalan tanpa catatan, dan lock yang diwariskan memberi bukti asosiasi data yang tidak bergantung pada PID sekaligus menjaga dari server kedua walau daemon, registry, atau observasi hilang.
- Ketidakpastian selalu menjadi `unknown` dengan bukti yang bisa dibaca doctor (fitur 34), tidak pernah kill.
- Log tidak pernah menahan proses, tetap ditulis saat daemon mati, dan selalu tersamar sebelum sampai ke penyimpanan tahan lama milik fitur 27.
- Fitur 14 bisa dibangun dari spec Foundation saja, dan fitur 20, 21, 26, 27, 30, serta 33 menambah di atas kontrak yang sama.

**Negative / tradeoffs**:

- Adapter macOS memakai banyak API libproc, kqueue, dan flag spawn khusus Apple yang tidak terdokumentasi lengkap; perilakunya harus dibuktikan spike di setiap versi macOS baru.
- Provider yang menutup fd 3, me re exec ke executable lain, atau menjalankan turunan berumur panjang tidak bisa diadopsi setelah daemon restart atau membuat `data_in_use` lebih lama; status `adoptable: false` hanya memperingatkan.
- Daemon yang mati di antara spawn dan commit (jendela milidetik) meninggalkan proses tertahan yang menjadi `unknown` sampai pengguna menghentikannya.
- Spool di runtime root bisa dibersihkan macOS, dan tumbuh tanpa batas selama daemon mati.
- Restart policy hanya di memori; daemon yang ikut crash loop memulai budget baru setiap kali.
- Instance yang `unknown` butuh tindakan pengguna di luar Stackly sampai fitur 26 dan 34 menyediakan jalan keluar yang lebih nyaman.
- Spec ini menambah method create minimal yang nanti diperluas fitur 20; perubahannya harus tetap additive supaya tidak memecah protocol.
- Empat update lintas spec (0004, 0005, 0006, 0009) harus masuk sebelum build milestone yang memakainya.

**Neutral**:

- Core mendapat dependency `tokio` dan `tracing` (sudah diizinkan spec 0001), dan fitur crate `fixture` khusus test.
- `Platform` mendapat port ketujuh lewat `with_processes`, dan `PlatformCapability::ProcessSupervision` menjadi supported di macOS.
- Layout instance mendapat `instance.lock` di root instance dan spool di folder run, dan port `Auto` tersimpan sejak spawn, bukan sejak ready.
- Enum lifecycle, readiness, dan health di wire tertutup, dan event status diurutkan dengan `statusSeq`, berbeda dari pola umum di spec 0006.

## Follow-up

Perubahan untuk spec 0004 (lewat `/architect` update spec 0004 pada pass rekonsiliasi):

- [x] *Port platform*: trait `ProcessSupervision` di modul `process` beserta tipenya (*Port `ProcessSupervision`*, termasuk `SuspendedChild`, `MainHandle`, `HelperHandle`), port ketujuh `processes` di `Platform` (lewat `with_processes`, tanpa mengubah konstruktor), `UnsupportedProcessSupervision` di `UnsupportedPlatform`, `FakeProcessSupervision` di `FakePlatform`, dan capability `process_supervision` supported di `MacosPlatform` setelah milestone 1. Diterapkan di spec 0004 (2026-10-18).
- [x] *Peta modul*: adapter macOS `stackly-platform-macos::process` dengan `posix_spawn` (`SETSID`, `SETSIGDEF`, `SETSIGMASK`, `CLOEXEC_DEFAULT`, `START_SUSPENDED`, `addchdir_np`), process group sendiri, waktu start dan status tertahan dari `proc_pidinfo`, identitas executable lewat `proc_pidpath`, session lewat `getsid`, boot session lewat `kern.bootsessionuuid`, fd lewat `PROC_PIDFDVNODEPATHINFO`, watcher kqueue, `flock`, `F_PUNCHHOLE`, dan `SEEK_DATA`; `unsafe` hanya di crate itu dengan `// SAFETY:`. Core mendapat `tokio` dan `tracing`. Diterapkan di spec 0004 (2026-10-18).
- [x] *Model domain*: modul `process` (`Pid`, `ProcessStart`, `BootId`, `ProcessIdentity`, `ProcessObservation`, `IdentityCheck`, `IdentityEvidence`); varian `InstanceBlocker::DataInUse { evidence }` dan `IdentityUnproven { evidence }` di samping `NeedsSecret` spec 0011; setter `Instance::set_bound_port(endpoint, port)` yang menaikkan revision; konstanta `FailureCode` dan `StageName` di *Kode error* dan *Startup transaction*. Diterapkan di spec 0004 (2026-10-18).
- [x] *Model domain*: aturan `PortRequest` diubah menjadi "`bound` diisi saat proses yang memakai port itu di spawn (batch `spawned`), dan ditampilkan sebagai final setelah readiness berhasil" (REQ-010, spec 0010). Diterapkan di spec 0004 (2026-10-18).
- [x] *Layout direktori*: `InstancePaths.lock` (`<root>/instance.lock`), `DirectoryLayout::instance_spool(short key, stream)` (`<run>/stdout.spool`, `<run>/stderr.spool`), dan baris pohon `i/<short-key>/` diganti dari "socket dan file PID per instance" menjadi "socket, spool, dan `secrets/` per instance". Diterapkan di spec 0004 (2026-10-18).
- [x] *Peta modul* `check-deps`: tidak ada `std::process::Command` atau `tokio::process` di kode non test `stackly-core` dan `stacklyd`, token `SIGKILL` hanya di `stackly-platform-macos/src/process/kill.rs`, dan fitur `fixture` core hanya dari dev dependency. Diterapkan di spec 0004 (2026-10-18).
- [x] Tandai Follow-up "Fitur 10" di spec 0004 selesai (`ensure_instance_run_dir` sebelum setiap start, capability supported), dan catat `READINESS_REFRESH_INTERVAL` 10 detik serta `READINESS_FAILURE_THRESHOLD` 2 di samping `READINESS_MAX_AGE`. Diterapkan di spec 0004 (2026-10-18).

Perubahan untuk spec 0001 (lewat `/architect` update spec 0001 pada pass rekonsiliasi):

- [x] *Batas keputusan*: baris "API proses macOS dan identitas proses" (fitur 10) sudah diputuskan spec 0010. Diterapkan di spec 0001 (2026-10-18).
- [x] *Peta crate*: `stackly-core` mendapat `tokio` (fitur `rt`, `time`, `net`, `sync`, `macros`, `io-util`) dan `tracing`, plus fitur crate `fixture` yang mati secara default. Diterapkan di spec 0001 (2026-10-18).
- [x] *Aturan arah dependency*: larangan process API dan aturan fitur `fixture` di atas ditegakkan `check-deps`. Diterapkan di spec 0001 (2026-10-18).

Perubahan untuk spec lain (lewat `/architect` update spec pemiliknya):

- [x] Spec 0005: `WriteOp::RecordProcessObservation(ProcessObservation)`, `Registry::latest_process_observation(InstanceId)`, `WriteOp::InsertConfigRevision(ConfigRevisionRecord)` yang mengisi `instances.config_revision_id`, dan `Registry::active_config_revision(InstanceId)`, dengan `FakeRegistry` dan `registry_conformance`; pernyataan bahwa beberapa `UpdateInstance` untuk instance yang sama boleh berurutan dalam satu batch; format teks kolom `process_observations` dari *Identitas majemuk*. Diterapkan di spec pemiliknya (2026-10-18).
- [x] Spec 0006: resep snapshot plus cursor langkah 4 menyebut bahwa `instance.health_changed` diterapkan berdasarkan `statusSeq`, bukan revision; urutan berhenti daemon mendapat langkah supervisor (hentikan actor dan watcher sebelum melepas lock daemon, tanpa menyinyal instance); tabel area menyebut `instance` milik spec 0010; catatan bahwa enum lifecycle, readiness, dan health tertutup. Diterapkan di spec pemiliknya (2026-10-18).
- [x] Spec 0009: tambah `RunPlan.render: Vec<InitStep>` (`EnsureDirectories` dan `WriteFile` untuk setiap `configFiles`, urut nama) yang dijalankan sebelum setiap spawn; `materializeSecret.destination` menjadi nama file saja yang ditaruh executor di `<run>/secrets/` spec 0011, dengan token folder `${instance.secrets}` (butuh `run` di `managedFilesystem`) untuk merujuknya dari `args`, `env`, dan template; arti `executionMode: foreground` mencakup tanpa re exec dan tanpa menutup fd warisan. Setelah masuk, hapus penanda `// provisional: F9` di executor. Diterapkan di spec pemiliknya (2026-10-18).
- [x] Spec 0011: `sweep_secret_material` dan `learn_running` menghitung instance `unknown` sebagai berjalan; field `blocker` tetap satu nilai dengan prioritas dari spec 0010. Diterapkan di spec pemiliknya (2026-10-18).

Untuk fitur lain:

- [ ] Fitur 14: `stackly instance create`, `start`, `stop`, dan `get` di atas method spec ini; walking skeleton memakai `install_fake` dan membuktikan daemon restart tanpa instance ganda atau proses yatim.
- [ ] Fitur 15: buktikan di LaunchAgent nyata bahwa proses instance di session sendiri tetap hidup saat job daemon di `kickstart -k` atau di bootout (launchd hanya membunuh process group job itu, `AbandonProcessGroup` tidak dibutuhkan); kalau tidak, kembali ke `/architect` untuk keputusan survival.
- [ ] Fitur 20: perluas `instance.create` secara additive (preflight, form dari schema, autostart) tanpa mengubah field yang ada.
- [ ] Fitur 26: force stop hanya untuk identitas terverifikasi dan sebagai aksi eksplisit, jalan keluar untuk proses tertahan yang `unknown`, restart yang menunggu `instance.lock` bebas, dan method `instance.reload` di atas `execute_reload`.
- [ ] Fitur 27: sink tahan lama di `SinkSet` dengan rotasi, cursor, follow, penanda truncation, penanganan `resumed`, dan file log yang dikelola provider (`RunPlan.log_files`).
- [ ] Fitur 28: tinjau `READINESS_REFRESH_INTERVAL`, `READINESS_FAILURE_THRESHOLD`, dan `READINESS_MAX_AGE`, pemeriksaan bahwa listener endpoint milik proses instance setelah startup (VAL-006), dan pakai `ProcessIdentity` untuk metrics proses.
- [ ] Fitur 29: config revision lengkap; perubahan `apply: restart` dirender ulang lewat render sebelum spawn.
- [ ] Fitur 30: pengerasan rekonsiliasi (runtime, endpoint, storage, operasi) dan restart daemon terencana yang menghentikan instance sampai handover terbukti.
- [ ] Fitur 31: start saat login setelah boot baru, dan perilaku instance saat logout.
- [ ] Fitur 33: banner crash loop dari `restart` dan `lastFailure` di `InstanceSummary`.
- [ ] Fitur 34: doctor menampilkan `IdentityEvidence` dan tidak pernah menghapus `instance.lock`.
- [ ] Fitur 36: harness konformitas provider memeriksa bahwa proses utama tetap memegang fd 3, tidak me re exec, dan tetap foreground.
- [ ] Fitur 50, 51, dan 54: ambil `instance.lock` sebelum cold backup, restore, retain, dan purge.
- [ ] Fitur 64: benchmark NFR-003 dan NFR-006 dengan supervisor nyata, lalu setel ulang konstanta lewat update spec ini kalau perlu.
- [ ] `/sync` setelah build: catat port proses, aturan sinyal dan `SIGKILL`, `instance.lock`, spool, dan `install_fake` di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, dan hasil cross check ada di [rationale.md](rationale.md).
