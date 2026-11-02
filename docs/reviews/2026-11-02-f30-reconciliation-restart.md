# Review, F30 rekonsiliasi dan restart daemon terencana (spec 0030), 2026-11-02

**Reviewed by**: tiga subagent review paralel di dalam `/check review`, satu per irisan. Irisan A1 dan A2 di Opus 5.5, untuk kode yang ditulis Sonnet 5.5. Irisan B di Sonnet 5.5, untuk kode yang ditulis Opus 5.5 di commit WIP `9a6bf944`. Penulis menurut trailer `Co-Authored-By` dan brief lead. Sesi utama (Opus 5.5, xhigh) memeriksa ulang setiap Blocker dan Major langsung di kode sebelum menuliskannya di sini.
**Scope**: `MODE=branch`, `BASE=main`, `MERGE_BASE=84d767f9`, range `84d767f9..657d2687`, dibatasi ke path F30: `crates/protocol/src/restart.rs`, `crates/domain/src/restart.rs`, `crates/storage/migrations/0009_daemon_restart_plan.sql`, `crates/storage/src/registry/restart.rs`, `crates/core/src/supervisor/{plan,integrity,prepare,reconcile,handover,daemon_restart,restore,pending}.rs`, `crates/core/src/operation/`, `apps/daemon/src/ipc/{restart,operation}.rs`, `apps/daemon/src/state.rs`, `apps/daemon/src/supervisor.rs`, `apps/cli/src/commands/daemon/`, `clients/swift/Sources/StacklyClient/Daemon/DaemonRestartClient.swift`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Shell/RestartBanner*.swift`, serta test di `apps/daemon/tests/{restart_plan*,native_launchd.rs,postgresql/reconcile.rs,postgresql/inventory.rs}` dan `crates/core/tests/supervisor_start/pid_reuse.rs`. Hunk F30 di file pendamping juga dibaca (`stop.rs`, `startup.rs`, `config.rs`, `crates/protocol/src/{codes,daemon,events,version}.rs`, `prune.rs`, testkit conformance, `apps/cli/tests/daemon_restart_plan.rs`, test StacklyUI di `Restart/`, fixture di `protocols/fixtures/ipc/`). Range itu juga memuat F28, F29, F31, dan F70; hunk fitur lain diabaikan. Nomor baris mengacu ke `657d2687`.
**Verdict**: Blocked

Irisan review: A1 (core supervisor, quiesce di engine, `stacklyd` ipc dan state, test daemon dan core), A2 (CLI, Swift client, StacklyUI, `docs/manual.md`), B (domain, protocol, storage dan migrasi, `supervisor/plan.rs`, testkit, fixture).

## Summary

F30 memperkuat rekonsiliasi spec 0010 dengan pemeriksaan data dan runtime, dan menambah restart daemon terencana: preview dengan digest, quiesce di `OperationEngine::begin`, operasi `daemon_restart` yang menulis rencana lewat `commit_guarded`, stop `PlannedRestart` tanpa menulis desired, abort dengan start ulang lewat operasi sistem, pengambilalihan rencana dan restore di daemon baru, `DaemonInfo.restart` dengan TTL, `stackly daemon restart`, dan bagian Daemon di Settings beserta banner dan toast.

Fondasinya kuat. Desired tidak pernah ditulis oleh rencana, tidak ada jalur rencana yang mengirim `SIGKILL`, pemeriksaan data jalan sebelum `ensure_instance_run_dir`, restore tidak pernah jalan di boot baru, guard menghitung ulang digest di dalam `BEGIN IMMEDIATE` yang sama, CHECK SQL cocok persis dengan enum domain, dan kontrak wire mengikuti aturan AGENTS.md dengan bump mayor ke 2.0.

Satu Blocker: outcome `skipped` untuk instance `data_missing` membawa `failure_code`, yang ditolak CHECK SQL, sehingga rencana macet `restoring` di registry dan restart berikutnya ditolak sampai reboot. Sembilan Major: cancel atau error sesudah rencana tertulis meninggalkan rencana terbuka dan instance mati; penantian quiesce tidak menunggu operasi `CloseInterrupted` (AC-10); cascade dari `operations` melanggar AC-19; timeout GUI 45 detik, bukan 190; model GUI kehilangan atau membekukan rencana di sekitar reconnect; Ctrl+C tidak bekerja di prompt dan selama request restart; keluaran `aborted` di CLI dan di GUI tidak memuat dua daftar dari `stoppedAt`; dan cabang error baru tanpa test.

Jumlah: Blocker 1, Major 9, Minor 18, Nit 23.

## Blockers

### 🔴 Outcome `skipped` membawa `failure_code`, ditolak CHECK SQL, rencana macet `restoring`, `crates/core/src/supervisor/restore.rs:183`
**Irisan**: B (domain dan migrasi), jalur pemicu di kode A1. Diverifikasi sesi utama.
**Problem**: `record_after` memanggil `record_plan_outcome(id, PlanOutcome::Skipped, false, flagged)` dengan `flagged = Some(FailureCode::DATA_MISSING)` untuk instance yang berakhir `failed` dengan `last_failure` `data_missing` (`restore.rs:181` sampai 186). `RestartPlan::record_outcome` (`crates/domain/src/restart.rs:522`) menerima `failure_code` untuk outcome apa pun. Migrasi punya `CHECK (failure_code IS NULL OR outcome = 'failed')` (`crates/storage/migrations/0009_daemon_restart_plan.sql:46`). `update_plan` (`crates/storage/src/registry/restart.rs:94`) menulis baris rencana dan semua baris instance dalam satu batch, jadi CHECK itu menggagalkan seluruh `UpdateRestartPlan`. `persist_plan` (`crates/core/src/supervisor/daemon_restart.rs:253`) hanya mengulang untuk `Busy`, dan `record_plan_outcome` (`restore.rs:104`) hanya menulis `warn`, lalu tetap menerbitkan event dan membuang rencana dari memori saat terminal.
**Why it matters**: Ini jalur yang spec tetapkan sendiri: instance `data_missing` di rencana menjadi `skipped` (*State rencana*, AC-4). Pemicunya jarang (folder data hilang di jendela restart, atau instance `keep` `respawn_pending` yang datanya hilang), tetapi akibatnya pasti dan diam. Event dan `DaemonInfo.restart` berkata `finished`, sedangkan registry tetap `restoring`. Setiap persist berikutnya dari rencana itu gagal dengan baris yang sama. Restart berikutnya ditolak `daemon.restart_pending` oleh guard (`crates/core/src/supervisor/plan.rs:35`) untuk rencana yang menurut pengguna sudah selesai. Daemon berikutnya di boot yang sama mengambil alih rencana `restoring` itu dan gagal lagi dengan cara yang sama, jadi restart terencana mati sampai reboot. `FakeRegistry` tidak punya CHECK ini, sehingga semua test berbasis fake lolos.
**Suggested fix**: Tegakkan aturan di domain: `record_outcome` menolak `failure_code` kalau outcome bukan `failed` (varian `RestartPlanError` baru). Ubah `restore.rs:183` supaya `skipped` tidak membawa kode; alasan `data_missing` sudah terlihat lewat finding rekonsiliasi dan `lastFailure` instance. Tambah test domain, test conformance `UpdateRestartPlan` dengan baris `failed` dan `skipped` terhadap SQLite, dan satu test daemon dengan registry SQLite untuk rencana yang memuat instance `data_missing`. Pertimbangkan juga agar `record_plan_outcome` tidak membuang rencana dari memori kalau persist terminal gagal.

## Major

### 🟠 Cancel atau error apa pun sesudah rencana tertulis meninggalkan rencana terbuka dan instance mati, `crates/core/src/supervisor/daemon_restart.rs:446`
**Irisan**: A1. Diverifikasi sesi utama.
**Problem**: Sesudah `write_plan` berhasil, setiap `?` di `restart_steps` keluar tanpa menutup rencana: `begin_step(STOP)` dan `set_stage` (baris 449 sampai 451), `begin_step(COMPLETE)` dan `set_stage` (baris 454 sampai 456), `persist_plan` untuk `handed_over` (baris 483 sampai 485), dan `finish_step` (baris 487). `run` (baris 416 sampai 425) hanya mencabut quiesce. `daemon_restart` memakai `CloseInterrupted`, jadi `stop_at_boundary` (`crates/core/src/operation/context.rs:152` sampai 158) mengembalikan `Cancelled` di batas langkah mana pun, juga sesudah langkah `irreversible`. Cancel bisa datang dari `operation.cancel` biasa (`stackly operation cancel`), atau dari handler sendiri saat penantian rencana tertulis habis (`apps/daemon/src/ipc/restart.rs:447` sampai 456) kalau operasi menulis rencana tepat sesudah receiver habis waktu.
**Why it matters**: Urutan nyata: rencana `stopping` tertulis, `stop_instances` menghentikan A dan mencatat `stoppedAt`, client mengirim `operation.cancel`, lalu `begin_step(COMPLETE)` mengembalikan `Cancelled`. Operasi `cancelled`, quiesce dicabut, rencana tetap `stopping` di registry, A tetap `stopped` dengan desired `running` karena `queue_restore` tidak dipanggil, dan daemon tetap jalan. Restart berikutnya ditolak `daemon.restart_pending` sampai daemon berikutnya di boot yang sama. CLI yang sudah menerima `ShutdownAccepted` akhirnya keluar 8, dan banner GUI "stacklyd is restarting" bertahan. Gagal simpan `handed_over` berakhir sama. Melanggar AC-11 (yang dihentikan rencana dinyalakan lagi, rencana `aborted`) dan Key invariants.
**Suggested fix**: Tangani semua `Err` sesudah rencana tertulis, kecuali `WorkError::Abandoned` (shutdown, yang diteruskan AC-14), dengan jalur abort yang sama: `plan.abort`, `persist_plan`, event, `queue_restore`, dan start sesudah operasi final. Alternatif yang lebih kecil: tolak cancel untuk `daemon_restart` sesudah VALIDATE. Untuk timeout handler, batalkan hanya operasi yang masih `queued`, atau biarkan work memeriksa `prepared.is_closed()` sebelum `commit_guarded`. Tambah test untuk cancel di langkah STOP dan untuk gagal simpan `handed_over`.

### 🟠 Penantian quiesce tidak menunggu operasi `CloseInterrupted`, berbeda dari AC-10, `crates/core/src/operation/mod.rs:906`
**Irisan**: A1. Diverifikasi sesi utama.
**Problem**: `active_operations` membuang setiap jenis dengan `interrupt_policy() == CloseInterrupted` (baris 906 sampai 911): start, stop, restart, reload, force stop, `apply_config`, dan `change_endpoints` (`crates/domain/src/operation.rs:286` sampai 297). `wait_for_operations` (`apps/daemon/src/ipc/restart.rs:262` sampai 282) memakai daftar itu, jadi operasi tersebut tidak ditunggu sama sekali. AC-10 berkata operasi lain yang `queued` dan `running` ditunggu sampai `RESTART_QUIESCE_WAIT`; filter "tidak resumable dan bukan `CloseInterrupted`" hanya untuk menentukan penolakan `restart_busy` sesudahnya.
**Why it matters**: Urutan pertama: `instance.restart` atau `instance.config.apply` pada instance `keep` (misalnya PostgreSQL) masih berjalan saat restart dikonfirmasi. Digest tidak berubah, dan `daemon_restart` tidak mengunci instance `keep`. Rencana `handed_over`, daemon keluar, shutdown engine memberi `abandon` ke operasi pengguna itu. Kalau terpotong sesudah stop dan sebelum spawn, instance berakhir `stopped` dengan desired `running`, tidak dinyalakan daemon baru, dan outcome `keep` menjadi `failed`. Urutan kedua: start instance `stopped` dari provider yang belum terbukti masih di tahap awal (lifecycle masih `stopped`) saat digest dihitung, jadi rencana ditulis tanpa instance itu, lalu prosesnya hidup melewati keluarnya daemon tanpa `stop_restore`. Itu yang ingin dicegah gerbang handover (AC-7, AC-8). Ini juga penyebab kode `daemon.restart_plan_changed` di catatan (b).
**Suggested fix**: Tunggu semua operasi aktif selain `daemon_restart` sampai `RESTART_QUIESCE_WAIT`, lalu terapkan filter penghalang hanya untuk `restart_busy` dan `blockingOperations`. Pisahkan dua daftar itu di engine. Tambah test dengan `instance.restart` lambat pada instance `keep`.

### 🟠 `ON DELETE CASCADE` dari `operations` menghapus rencana, termasuk rencana terbuka, melanggar AC-19, `crates/storage/migrations/0009_daemon_restart_plan.sql:15`
**Irisan**: B. Diverifikasi sesi utama.
**Problem**: `operation_id` memakai `REFERENCES operations (id) ON DELETE CASCADE`, dan koneksi tulis menyalakan `foreign_keys=1` (`crates/storage/src/connection.rs:104`). Prune operasi (`crates/storage/src/registry/prune.rs:114` sampai 122) menghapus operasi final yang lebih tua dari `operation_max_age` (30 hari) atau di atas `operation_max_rows`. Operasi `daemon_restart` sudah final sejak `handed_over` (atau `failed { interrupted }` sesudah crash), jadi cascade ikut menghapus rencananya apa pun statenya. Komentar migrasi (baris 8 sampai 10) dan doc `History::RestartPlans` (`prune.rs:36` sampai 37) menyebut perilaku ini disengaja, tetapi spec tidak memintanya. Fake menirunya (`crates/testkit/src/registry/fake.rs:1823`), dan test conformance memakai `operation_max_age: Duration::MAX` (`crates/testkit/src/registry/conformance/restart.rs:199`) sehingga tidak melihatnya.
**Why it matters**: AC-19 menyimpan 20 rencana terminal terakhir dan prune "tidak pernah menghapus rencana terbuka". Karena restart jarang, rencana yang lebih tua dari 30 hari hilang walau jumlahnya jauh di bawah 20, jadi retensi nyata 30 hari. Rencana terbuka yang operasinya sudah final dan tua (misalnya yang macet karena Blocker di atas, di Mac yang jalan berminggu minggu) juga ikut terhapus diam diam dan tidak masuk `PruneReport`.
**Suggested fix**: Retensi rencana hanya lewat `restart_plans`. Misalnya kecualikan operasi yang dirujuk rencana dari prune operasi (`AND id NOT IN (SELECT operation_id FROM daemon_restart_plan)`) dan cabut cascade, atau jadikan `operation_id` nullable dengan `ON DELETE SET NULL`. Ini migrasi baru, jadi putuskan bentuknya lewat lead (spec *Model data* hanya menulis FK). Samakan fake, dan tambah test conformance dengan operasi tua yang dirujuk rencana terbuka dan rencana terminal.

### 🟠 Timeout GUI untuk `daemon.shutdown { restart }` 45 detik, spec meminta 190 detik, `clients/swift/Sources/StacklyClient/Daemon/DaemonRestartClient.swift:17`
**Irisan**: A2. Diverifikasi sesi utama.
**Problem**: `restartTimeout` bernilai `.seconds(45)`. Spec (*API surface*, paragraf CLI) meminta `CallOptions(timeout:)` sebesar `SecretsClient.interactiveTimeout` (190 detik, `clients/swift/Sources/StacklyClient/Secrets/SecretsClient.swift:13`) dan menyebut reply sampai sekitar 70 detik normal (30 detik `RESTART_QUIESCE_WAIT` ditambah 40 detik penantian rencana tertulis). Doc comment baris 14 sampai 16 hanya menyebut 30 detik. Test `clients/swift/Tests/StacklyClientTests/Daemon/DaemonRestartClientTests.swift:57` mengunci nilai 45 detik.
**Why it matters**: Urutan: start yang masih memegang lock instance `stop_restore`, daemon menunggu 30 detik, operasi antre di lock dan baru menulis rencana di detik ke 50. GUI sudah menyerah di detik ke 45 dengan `outcomeUnknown` dan sheet menampilkan error, sementara daemon tetap menghentikan instance dan keluar 75. Melanggar AC-16 dan paragraf API surface.
**Suggested fix**: Pakai `SecretsClient.interactiveTimeout` (atau konstanta bersama yang merujuk ke sana), perbaiki doc comment, dan ubah test agar memeriksa 190 detik.

### 🟠 Model GUI kehilangan atau membekukan rencana di sekitar reconnect ke daemon baru, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/DaemonRestartModel.swift:91`
**Irisan**: A2. Diverifikasi sesi utama.
**Problem**: Dua celah. Pertama, `receive(info:)` (baris 91 sampai 102) menghapus rencana terbuka, `isAwaitingPlan`, dan `watchedPlan` kalau `DaemonInfo.restart` null. Daemon baru membuka socket (`apps/daemon/src/server.rs:390` sampai 403) sebelum `start_slot` mengambil alih rencana, jadi `get_info` pertama ke daemon baru biasanya melihat `restart: null`. Kedua, `DaemonModel` hanya meneruskan info di koneksi baru (`apps/macos/Packages/StacklyUI/Sources/StacklyUI/Connection/DaemonModel.swift:467`) dan tidak membaca ulang info saat `.resnapshot` (`DaemonModel.swift:499` sampai 509), dan `get_info` dibaca sebelum langganan event.
**Why it matters**: Urutan pertama: banner "stacklyd is restarting" dan tanda baris "Stopped for daemon restart" hilang di jendela disruption sampai event `restoring` tiba, dan karena `watchedPlan` sudah dihapus, toast bisa tidak muncul kalau event berikutnya langsung `finished`. Urutan kedua: event `finished` terlewat (di antara `get_info` dan subscribe, atau karena `events.gap`), model tetap `restoring`, banner bertahan, tombol Restart Daemon mati dengan "A restart is already in progress.", dan toast tidak muncul sampai koneksi berikutnya. Melanggar AC-16. Test `apps/macos/Packages/StacklyUI/Tests/StacklyUITests/Restart/DaemonRestartModelTests.swift:226` mengunci perilaku urutan pertama.
**Suggested fix**: Jangan hapus rencana terbuka hanya karena `restart` null dari daemon yang belum selesai rekonsiliasi (hapus sesudah `daemon.reconciling finished` tanpa rencana, atau saat info membawa ID rencana lain). Sesudah `.resnapshot` dan `.caughtUp`, baca ulang `daemon.get_info`, seperti CLI di `watch_once` yang berlangganan dulu lalu membaca info. Tambah test untuk kedua urutan.

### 🟠 Ctrl+C tidak bekerja di prompt y/N dan selama request restart, `apps/cli/src/commands/daemon/restart.rs:149`
**Irisan**: A2. Diverifikasi sesi utama.
**Problem**: `RestartArgs::run` memasang `SigInt` (`apps/cli/src/commands/daemon/mod.rs:182`) sebelum prompt. Sejak itu SIGINT ditangkap tokio, sedangkan `Confirm::ask` membaca `read_line` yang blocking (`apps/cli/src/confirm.rs:120`), jadi Ctrl+C di `[y/N]` tidak memutusnya. `deps.probe.restart(&request, eof_limit)` (`restart.rs:149`) juga tidak dibalap dengan `interrupt.wait()`, padahal di dalamnya ada call `RequestTimeout::Interactive` (200 detik) dan penantian EOF sampai `DAEMON_RESTORE_WAIT` (600 detik) untuk rencana dengan `stop_restore` (`apps/cli/src/commands/daemon/wait.rs:375` sampai 399).
**Why it matters**: Di TTY, Ctrl+C di prompt tidak terjadi apa apa; kalau orang lalu mengetik `y`, restart terkirim dan CLI baru keluar 130 sesudah instance dihentikan. Selama request dan penantian EOF, Ctrl+C diabaikan sampai sekitar 13 menit. AC-15 (exit 130) dan spec 0015 AC-10 dilanggar.
**Suggested fix**: Pasang `SigInt` sesudah prompt, atau buat prompt menghormati interrupt. Balap `restart_preview` dan `probe.restart` dengan `interrupt.wait()` supaya keluar 130 `cli.interrupted` tanpa membatalkan restart di daemon. Tambah test unit dengan interrupt selama `restart` yang ditahan.

### 🟠 Teks CLI untuk rencana `aborted` tidak memuat dua daftar dari `stoppedAt`, `apps/cli/src/commands/daemon/plan.rs:102`
**Irisan**: A2. Diverifikasi sesi utama.
**Problem**: `judge` hanya membuat `CliFailure::restart_aborted(plan.id, close_reason)` tanpa nama instance (baris 101 sampai 108), dan `render_plan` mengisi kolom RESULT dari `outcome` (baris 278 sampai 304), yang semuanya `skipped` pada rencana `aborted`. Hint di `apps/cli/src/failure.rs:480` menyuruh membaca `stackly daemon status`, yang juga hanya mencetak jumlah.
**Why it matters**: Spec (*Keluaran rencana aborted*) meminta dua daftar nama di teks manusia: `stoppedAt` terisi "stopped, starting again", `stoppedAt` kosong "did not stop, still running". Dengan A yang sudah berhenti dan B yang timeout, orang di terminal tidak tahu A sedang dinyalakan lagi dan B masih jalan. Melanggar AC-11 dan AC-15. Test `restart.rs:1137` dan `apps/cli/tests/daemon_restart_plan.rs:465` memakai rencana aborted tanpa instance.
**Suggested fix**: Untuk `PlanState::Aborted`, bagi baris `stop_restore` menurut `stopped_at` dan tulis dua daftar itu, jangan tampilkan `skipped` sebagai hasil, perbaiki hint, dan tambah test dengan dua instance yang `stoppedAt` nya berbeda.

### 🟠 Banner GUI untuk rencana `aborted` salah judul, satu daftar, dan klaim yang belum tentu benar, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Shell/RestartBannerContent.swift:116`
**Irisan**: A2. Diverifikasi sesi utama.
**Problem**: Judulnya "Daemon restart cancelled", padahal spec menulis "Daemon restart aborted". `items` hanya berisi baris `stop_restore` dengan `stoppedAt == nil` berlabel "Did not stop in time.", jadi daftar "stopped, starting again" tidak ada. Pesannya berkata "the instances it stopped started again", padahal saat event `aborted` tiba start ulang itu baru diantre sebagai operasi sistem yang bisa gagal. Label "Did not stop in time." juga dipakai untuk stop yang tidak pernah dimulai. Test `apps/macos/Packages/StacklyUI/Tests/StacklyUITests/Restart/RestartBannerContentTests.swift:106` mengunci judul itu.
**Why it matters**: Pengguna tidak melihat instance mana yang sedang dinyalakan lagi dan bisa mengira semuanya sudah jalan. Melanggar AC-16 dan AC-11.
**Suggested fix**: Ganti judul ke "Daemon restart aborted", tampilkan dua daftar dari `stoppedAt`, ubah pesan agar tidak mengklaim start ulang sudah selesai, lalu sesuaikan test dan snapshot.

### 🟠 Cabang error dan pemulihan baru tanpa test, termasuk dua skenario yang spec wajibkan, `apps/daemon/src/ipc/restart.rs:447`
**Irisan**: A1. Diverifikasi sesi utama (grep di `apps/daemon/tests` dan `crates/core/tests`).
**Problem**: Tidak ada test untuk: timeout `PREPARE_GRACE` dengan `engine.cancel` (`restart.rs:447` sampai 456); `PendingKind::PlannedStop` yang berhasil lalu restore (`crates/core/src/supervisor/pending.rs:167` sampai 188); cancel operasi `daemon_restart`; gagal simpan `handed_over` dan gagal `finish_step`; dua restart bersamaan; restart tanpa `idempotencyKey` (`operation.idempotency_key_required`, diminta *Critical test scenarios*); dan varian rencana untuk PID asing di instance `stop_restore` (AC-3, diminta untuk `/test`, tidak ada di `crates/core/tests/supervisor_start/pid_reuse.rs`).
**Why it matters**: Major pertama dan Minor outcome `adopted` di bawah ada tepat di cabang yang tidak dites. Dua dari daftar ini adalah skenario yang spec tetapkan untuk fitur GA.
**Suggested fix**: Tambah test daemon dengan `RestartHarness` untuk setiap cabang, bersama perbaikan Major terkait. Untuk `PlannedStop`, pakai instance yang berhenti pelan tetapi masih dalam budget, lalu cek PID baru dan outcome `restored`.

## Minor

### 🟡 `RestartPlan::restore` menolak rencana `finished` dari jalur crash AC-14, `crates/domain/src/restart.rs:358`
**Irisan**: B. Diverifikasi sesi utama; diturunkan dari Major karena pembacanya sekarang hanya test.
**Problem**: `restore` mewajibkan `handed_over_at` untuk `HandedOver` dan `Finished` (baris 358 sampai 364). Jalur AC-14 memakai `take_over` dari `Stopping` (baris 479 sampai 492) yang tidak pernah mengisi `handed_over_at`, jadi rencana itu `finished` tanpa `handed_over_at`. `latest_plan` (`crates/storage/src/registry/restart.rs:146`, `plan_where` baris 159 sampai 161) memetakan kegagalan `restore` ke `Corrupt`.
**Why it matters**: Data dari jalur sah dinilai korup oleh pembacanya sendiri. Sekarang `latest_restart_plan` hanya dipanggil test, jadi belum terlihat, tetapi Diagnostics nanti akan membacanya.
**Suggested fix**: Syarat `handed_over_at` hanya untuk `HandedOver`. Tambah test domain dan putaran tulis lalu baca di conformance untuk rencana jalur crash.

### 🟡 `UpdateRestartPlan` menimpa snapshot penuh tanpa penjaga, SQLite dan fake berbeda, `crates/storage/src/registry/restart.rs:94`
**Irisan**: B.
**Problem**: Update tidak membatasi state asal, jadi rencana final bisa ditimpa balik menjadi terbuka oleh salinan lama, dan baris instance yang tidak ada diabaikan diam diam. Fake (`fake.rs:1783` sampai 1791) mengganti seluruh objek dan tidak menegakkan unique index saat update.
**Why it matters**: Core sekarang menserialkan lewat mutex, jadi belum terpicu, tetapi conformance tidak menjaga kontrak update.
**Suggested fix**: Batasi update ke baris asal yang masih terbuka, cek jumlah baris instance yang terubah, buat fake meniru keduanya, dan tambah test conformance.

### 🟡 Domain tidak menegakkan hubungan decision, outcome, dan urutan, `crates/domain/src/restart.rs:502`
**Irisan**: B.
**Problem**: `Restored` untuk entri `keep` diterima, outcome yang dicatat sebelum `record_reconciled` membuat rencana `Finished` dengan `reconciled_at` kosong, dan `record_stopped` diterima untuk entri yang sudah punya outcome.
**Why it matters**: Durasi CLI (`daemonGap`, `restoring`) bergantung pada `reconciled_at`. Core sekarang mengatur urutannya dengan benar, jadi ini lapis kedua.
**Suggested fix**: Tolak `Restored` untuk `keep`, tolak outcome sebelum `reconciled_at`, dan tolak `record_stopped` sesudah outcome.

### 🟡 Celah test state machine dan kontrak, `crates/domain/src/restart.rs`
**Irisan**: B.
**Problem**: Tidak ada tabel test state dikali event untuk `RestartPlan` seperti `LIFECYCLE_TABLE`; efek agregat `apply_lifecycle(DataMissing)` (`crates/domain/src/instance.rs:792`) tidak dites; `ReconcileFindingCode::Unrecognized` tanpa fixture; test fixture event rencana (`crates/protocol/tests/ipc_fixtures.rs:1318`) tidak menegaskan `reason == Unrecognized`; test migrasi tidak mengetes dua CHECK silang; conformance tidak membaca balik rencana `aborted` dan `expired`.
**Why it matters**: Blocker dan Minor pertama lolos lewat celah ini.
**Suggested fix**: Tambah tabel transisi, test agregat, fixture dan assertion `Unrecognized`, dua CHECK di test migrasi, dan round trip `aborted` dan `expired`.

### 🟡 Dua restart bersamaan saling mencabut quiesce, `apps/daemon/src/ipc/restart.rs:371`
**Irisan**: A1.
**Problem**: `is_quiesced()` (baris 371) dan `Quiesce::on` (baris 393) dipisah oleh `await`. Quiesce adalah satu `AtomicBool` tanpa pemilik, dan `Drop` milik `Quiesce` menulis `false` tanpa melihat siapa yang menyalakan.
**Why it matters**: R1 dan R2 sama sama lolos; R2 habis waktu di lock `Manager` lalu mencabut quiesce saat op1 masih `stopping`, sehingga request pengguna dan `daemon.shutdown { mode: stop }` diterima selama rencana `stopping` (*Edge cases* baris 220 dan 221).
**Suggested fix**: `compare_exchange` dengan token pemilik; hanya pemilik yang mencabut, request kedua langsung `restart_pending`.

### 🟡 Pemeriksaan quiesce di `begin` jalan sebelum gate dan lookup key, `crates/core/src/operation/scheduler.rs:320`
**Irisan**: A1.
**Problem**: Flag dibaca di baris 320 sampai 325, sebelum `begin_gate` dan lookup key. Request yang lolos lalu menunggu gate bisa masuk sesudah `active_operations` dibaca; request yang diulang dengan key yang sudah diterima sebelum quiesce mendapat `restart_pending`, bukan reply pertamanya.
**Why it matters**: Create bisa lolos tanpa `restart_busy` (AC-10), dan replay spec 0007 rusak untuk method tanpa `replay_operation`.
**Suggested fix**: Pindahkan pemeriksaan ke dalam gate sesudah lookup key, dengan satu method yang menyalakan quiesce dan mengambil snapshot operasi aktif di bawah gate yang sama.

### 🟡 Outcome `adopted` untuk instance `stop_restore` yang sebenarnya dihentikan dan dinyalakan ulang, `crates/core/src/supervisor/restore.rs:203`
**Irisan**: A1. Diverifikasi sesi utama.
**Problem**: `Restored` hanya kalau `entry.stopped_at.is_some()`, dan `stoppedAt` hanya ditulis daemon lama. `PlannedStop` yang berhasil, instance `stopping` yang prosesnya sudah keluar, dan stop yang `stoppedAt` nya belum tersimpan semuanya mendapat PID baru tetapi outcome `adopted`.
**Why it matters**: Toast GUI dan hasil CLI salah hitung, padahal user story meminta pengguna tahu seberapa besar gangguannya. Ini juga sumber kedua flakiness (c).
**Suggested fix**: Catat `stoppedAt` di daemon baru saat stop rencana selesai atau saat proses `stopping` ternyata hilang, atau tentukan `restored` dari fakta restore memang berjalan.

### 🟡 Instance yang hilang sebelum pending run nya membuat rencana tidak pernah `finished`, `crates/core/src/supervisor/pending.rs:111`
**Irisan**: A1. Diverifikasi sesi utama.
**Problem**: `run_one` langsung `return` untuk `NotFound` dan error baca lain (baris 109 sampai 116) tanpa `settle_restored`, padahal entri itu sudah masuk `deferred`.
**Why it matters**: Instance dihapus saat menunggu permit restore: rencana tetap `restoring`, banner bertahan, CLI keluar 8, restart berikutnya ditolak. *Edge cases* baris 223 meminta `skipped`.
**Suggested fix**: Panggil `settle_restored(id, None)` sebelum kedua `return`.

### 🟡 Abort yang gagal disimpan tidak menerbitkan apa pun dan meninggalkan stop kedua untuk daemon berikutnya, `crates/core/src/supervisor/daemon_restart.rs:466`
**Irisan**: A1.
**Problem**: Saat persist `aborted` gagal, event tidak diterbitkan (baris 466 sampai 471), jadi `DaemonInfo.restart` tetap `stopping`. Instance yang macet tetap `stopping`; daemon berikutnya di boot yang sama mengambil rencana itu sebagai `restoring` dan memberinya `Then::PlannedStop` (`reconcile.rs:543` sampai 544), stop kedua dalam satu rencana.
**Why it matters**: Melanggar Key invariant "dihentikan paling banyak sekali per rencana", pada waktu yang tidak diminta pengguna. CLI menunggu sampai exit 8 alih alih exit 7.
**Suggested fix**: Terbitkan view `aborted` dari memori walau gagal disimpan, coba simpan lagi di latar, dan di `take_over_plan` tutup rencana `stopping` yang operasinya sudah final `failed` (bukan `interrupted`) atau `cancelled` sebagai `aborted`.

### 🟡 Instance yang stop nya habis waktu tidak pernah dinyalakan lagi kalau prosesnya keluar belakangan, `crates/core/src/supervisor/daemon_restart.rs:567`
**Irisan**: A1.
**Problem**: Sesudah abort, instance macet tetap `stopping` dengan desired `running` dan SIGTERM sudah terkirim. Kalau ia keluar beberapa saat kemudian, exit handler menulis `stopped` dan tidak ada yang menyalakannya, karena `queue_restore` hanya memakai entri dengan `stoppedAt`.
**Why it matters**: PostgreSQL yang menunggu koneksi panjang biasanya berperilaku begini, dan CLI serta GUI sudah melaporkannya "still running".
**Suggested fix**: Tandai instance itu supaya exit nya nanti mengantre `PendingKind::Restore`, atau catat perilaku ini di spec sebagai diterima.

### 🟡 Test (c) bergantung pada timing, `apps/daemon/tests/restart_plan/restore.rs:225`
**Irisan**: A1. Lihat catatan (c).
**Problem**: Budget stop 2 detik milik `stuck` harus lebih lama dari stop `fine` ditambah polling dan awal shutdown, dan `stoppedAt` milik `fine` harus sudah tersimpan sebelum daemon berikutnya membaca rencana.
**Suggested fix**: Tunggu `stoppedAt` milik `fine` di `DaemonInfo.restart` sebelum `harness.stop()`, beri `stuck` jarak waktu yang jauh lebih besar atau stop yang macet secara deterministik.

### 🟡 Test abort yang gagal disimpan menerima dua kode dan komentarnya salah, `apps/daemon/tests/restart_plan/summary.rs:196`
**Irisan**: A1. Diverifikasi sesi utama.
**Problem**: Komentar berkata guard menjawab `daemon.restart_plan_changed`, padahal guard memeriksa rencana terbuka lebih dulu dan menjawab `restart_pending` (`crates/core/src/supervisor/plan.rs:35` sampai 39). `restart_plan_changed` datang dari race test: `api.stop(fine)` (baris 189) kembali saat stop diterima, bukan saat selesai. Lihat catatan (b).
**Suggested fix**: Tunggu `fine` `stopped` sebelum restart kedua, lalu assert tepat `daemon.restart_pending` dengan `planId` rencana yang tertinggal.

### 🟡 `durations` bukan null untuk rencana `aborted` dan `expired`, `apps/cli/src/commands/daemon/restart.rs:271`
**Irisan**: A2. Diverifikasi sesi utama.
**Problem**: `plan_ended` selalu mengisi `result.durations = Some(...)`.
**Why it matters**: Spec dua kali menulis `durations` null kecuali `finished`; ini kontrak `--json` untuk penulis script.
**Suggested fix**: Isi hanya untuk `PlanState::Finished`, dan tambah assert di test aborted.

### 🟡 Batas `DAEMON_RESTORE_WAIT` dihitung dari awal perintah, dan timeout membuang rencana yang sudah diketahui, `apps/cli/src/commands/daemon/restart.rs:309`
**Irisan**: A2.
**Problem**: `started` diambil sebelum preview dan prompt (baris 119), jadi waktu berpikir di `[y/N]` ikut memakan 10 menit. Saat batas habis, `result.plan` diisi `last` yang bisa null walau `info.restart` atau `follow.plan` sudah ada.
**Suggested fix**: Mulai jam sesudah konfirmasi, dan di jalur timeout pakai `last.or(info.restart).or(follow.plan)`.

### 🟡 Preview yang ditolak `daemon.shutting_down` kini menggagalkan restart, regresi spec 0015, `apps/cli/src/commands/daemon/restart.rs:123`
**Irisan**: A2.
**Problem**: Preview dikirim lebih dulu, dan daemon yang sedang berhenti menolaknya `daemon.shutting_down`, jadi `restart()` keluar dengan error dan penanganan `SHUTTING_DOWN` sebagai "diterima" di `LiveProbe::restart` tidak tercapai.
**Why it matters**: Dua `stackly daemon restart` beruntun membuat yang kedua gagal, bukan menunggu daemon baru seperti spec 0015 AC-10.
**Suggested fix**: Saat preview gagal `daemon.shutting_down`, lanjutkan ke penantian daemon baru tanpa rencana, atau catat keputusan berbeda di spec.

### 🟡 Tombol Restart Daemon mati tanpa alasan yang terlihat saat recovery atau refreshing, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/DaemonSettingsSection.swift:23`
**Irisan**: A2.
**Problem**: Tombol `mutates: true` dengan gate mutasi shell, yang di recovery mode atau "Loading the latest state." memblokir tanpa alasan di Settings, sementara subtitle tetap "Connected over Unix socket".
**Why it matters**: AC-16 hanya menonaktifkan tombol saat tidak tersambung atau `manual`, dengan alasan di subtitle; daemon justru mendukung restart di recovery mode (AC-7, `supervisor_unavailable`). `apps/macos/AGENTS.md` mewajibkan aksi nonaktif menampilkan alasannya.
**Suggested fix**: Putuskan apakah restart boleh di recovery mode; kalau tidak, tulis alasannya di subtitle, kalau boleh, jangan biarkan gate mutasi memblokirnya.

### 🟡 GUI menghitung instance yang tetap `unknown` sebagai gagal dan tetap memberi toast sukses, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Shell/RestartBannerContent.swift:104`
**Irisan**: A2.
**Problem**: Banner `finished` memakai `counts.failed`, termasuk baris `keep` `identity_unproven` yang CLI anggap hanya warning `cli.restart_instance_unknown` (`plan.rs:73`). Toast `.success` (`DaemonRestartModel.swift:119` sampai 124) tetap dibuat walau ada outcome `failed`.
**Why it matters**: CLI dan GUI memberi vonis berbeda untuk rencana yang sama, dan banner caution tampil bersama toast sukses.
**Suggested fix**: Pisahkan instance yang tetap `unknown` seperti di CLI, dan beri toast nada lain (atau tanpa toast) bila ada kegagalan.

### 🟡 Celah test CLI dan GUI
**Irisan**: A2.
**Problem**: Tidak ada test untuk key baru pada konfirmasi ulang sesudah `daemon.restart_plan_changed` (`restart.rs:1042` sampai 1077 hanya memeriksa digest), daftar `stoppedAt` dan `durations` null pada aborted, Ctrl+C di prompt dan selama `probe.restart`, preview yang ditolak `daemon.shutting_down`, gap atau resnapshot di GUI, dan gate recovery di Settings. Dua test mengunci perilaku yang salah (timeout 45 detik, rencana dihapus saat info null).
**Suggested fix**: Tambah test bersama perbaikan masing masing.

## Nits

- ⚪ `crates/protocol/src/message.rs:100`, spec menulis input `RestartPreviewParams {}`, kode memakai varian unit `DaemonRestartPreview` (sejalan dengan `daemon.get_info`); spec perlu disamakan.
- ⚪ `protocols/fixtures/ipc/request-shutdown-restart.json:3`, fixture restart memakai `idempotencyKey: null`, padahal daemon menolak `mode: restart` tanpa key.
- ⚪ `crates/core/src/supervisor/plan.rs:30`, `write_plan_guard` pantas `#[must_use]`; judul modul masih menyebut nama lama `NoOpenRestartPlan`.
- ⚪ `crates/domain/src/instance.rs:751`, doc `apply_lifecycle` belum menyebut `data_missing` yang mengisi `last_failure`.
- ⚪ `crates/domain/src/restart.rs:340`, `restore` tidak memeriksa `server_instance_id_after` untuk `Restoring`, dan `RestartPlan::new` tidak memeriksa bahwa `digest` cocok dengan baris instance.
- ⚪ `crates/core/src/supervisor/prepare.rs:301`, penanda `provisional: F30` sudah basi: spec baris 70 sekarang sama dengan kode.
- ⚪ `crates/core/src/supervisor/daemon_restart.rs:288`, doc `stoppers` menyebut "The `PlanEntry`" padahal fungsi mengembalikan `InstanceId`.
- ⚪ `apps/daemon/src/ipc/restart.rs:347`, jalur recovery mode menerima `mode: restart` tanpa `idempotencyKey` dan mengabaikan `expectedPlanDigest`, berbeda dari *Alur* langkah 1 (tidak berbahaya, tidak ada yang dihentikan).
- ⚪ `crates/core/src/supervisor/restore.rs:244`, `settle_restored` memeriksa state di luar commit lalu menulis `record_start_refusal`; periksa lagi di dalam closure commit.
- ⚪ `crates/core/src/supervisor/startup.rs:766`, `DataUnavailable` ke `DATA_MISSING` juga mengenai folder yang izinnya salah dan folder run yang gagal dibuat; pertimbangkan varian error tersendiri.
- ⚪ `crates/core/src/supervisor/reconcile.rs:537`, instance `stop_restore` yang `stopping`, `Proven`, dan datanya hilang mendapat `Then::Stop` biasa yang tidak `wanted` karena desired `running`; barisnya belum ada di tabel.
- ⚪ `clients/swift/Sources/StacklyClient/Daemon/DaemonRestartClient.swift:69`, extension publik `DaemonLink.restart` tidak dipakai app dan tidak dites.
- ⚪ `apps/cli/src/commands/daemon/testing.rs:372`, `ScriptedPrompt::yes` menandai `--yes` dengan string sentinel `"yes"`; field `approved: bool` lebih jelas.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Fixtures/FixtureRestart.swift:79`, fixture memakai `official/postgresql` di `provider`, padahal preview membawa nama pendek.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/RestartConfirmationSheet.swift:103`, "30 seconds" ditulis keras di copy dan tidak menyebut penantian lock sampai sekitar 70 detik.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/RestartConfirmationSheet.swift:24`, tombol Cancel tampak aktif selama request berjalan tetapi tidak melakukan apa pun.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Shell/AppShell.swift:237`, `isFresh` dihitung dengan `Date()` saat render tanpa timer, jadi banner tidak hilang tepat 10 menit.
- ⚪ `apps/cli/src/commands/daemon/plan.rs:212`, dengan `-q` rencana tidak ditampilkan tetapi y/N tetap ditanyakan.
- ⚪ `apps/cli/src/commands/daemon/plan.rs:134`, `stop = len - keep` menghitung keputusan `Unrecognized` sebagai berhenti.
- ⚪ `apps/cli/src/commands/daemon/plan.rs:201`, `TerminalPrompt::new` tanpa `#[must_use]`.
- ⚪ `docs/manual.md:531`, menyebut `SettingsDaemonTests` dan `RestartBannerTests` yang tidak ada; nama sebenarnya `DaemonSettingsContentTests`, `RestartBannerContentTests`, dan `DaemonRestartModelTests`. Sisa bagian W cocok dengan Build plan langkah 14.
- ⚪ `apps/macos/design.md:783`, masih menjelaskan `SettingsPlaceholder` yang sudah dihapus.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Connection/DaemonModel.swift:218`, restart melewati `MutationPath` karena timeout panjang; sebaiknya `MutationPath` menerima `CallOptions` supaya aturan AGENTS.md tetap satu jalur.

## Strengths

- `plan_entries` dan `plan_digest` adalah fungsi murni dengan test tabel lengkap di `handover.rs`, dan guard menghitung ulang digest lewat `GuardView` di dalam `BEGIN IMMEDIATE` yang sama, sesuai AC-12 dan aturan `commit_guarded`.
- `StopIntent::PlannedRestart` dibawa sampai `journal_stop`, `settle_gone_process`, dan `run_stop`, jadi tidak ada jalur rencana yang menulis desired, dan stop rencana tidak pernah eskalasi ke `SIGKILL`.
- Pemeriksaan data di `prepare` jalan sebelum `ensure_instance_run_dir`, jadi start, respawn, dan restore menolak data hilang tanpa membuat folder; aturan folder kosong dengan dan tanpa marker punya unit test.
- CHECK SQL cocok persis dengan enum domain (6 state, 2 decision, 5 reason, 4 outcome, 2 close reason), unique index parsial benar bentuknya, dan prune menyisakan 20 rencana terminal tanpa menyentuh rencana terbuka.
- Kontrak wire rapi: setiap enum terbuka berakhir `Unrecognized`, field opsional ber `#[serde(default)]`, `ShutdownParams` punya `Validate`, bump mayor ke 2.0 tercatat, dan fixture menutup preview, shutdown, rencana (stopping, finished, aborted, nilai tak dikenal), temuan rekonsiliasi, dan tiga kode error baru.
- Logika CLI dipecah ke fungsi murni (`judge`, `RestartDurations::of`) dan trait `PlanPrompt`, dengan test jam tokio yang dijeda, daemon skrip dua run, dan PTY; `watch_once` berlangganan dulu lalu membaca `DaemonInfo.restart`, jadi CLI tidak punya celah saat pindah daemon.
- `RestartBannerContent` dan `DaemonSettingsContent` adalah nilai murni dengan test per state, `PlanState.rank` mencegah event telat memundurkan rencana, dan snapshot menutup light, dark, dan narrow tanpa identifier UI test atau shortcut baru.
- Infrastruktur test kuat: `RestartHarness`, trigger SQL yang menolak tulisan `aborted`, `MovableClock` untuk TTL, `ShellReopenTests` dengan binary nyata untuk AC-1, dan evidence native dengan sentinel dan hitungan postmaster.

## Test coverage

Tertutup: AC-3 untuk rekonsiliasi, recheck, stop, force stop, dan start; AC-4 dan AC-5 di fixture dan PostgreSQL native; AC-6 untuk respawn, start, dan restore; tabel AC-7; AC-9 sampai AC-14 untuk jalur utama, quiesce, `restart_busy`, `force`, abort dengan start ulang, digest di preview, expiry boot baru, daemon yang berakhir saat `stopping`, dan desired `stopped` sebelum restore; TTL `DaemonInfo.restart`; abort yang gagal disimpan; native AC-1, AC-2, AC-8, dan AC-17; CLI untuk prompt, `--json` tanpa `--yes`, plan changed, aborted, restore gagal, warning `unknown`, `--no-wait-instances`, dan timeout; GUI untuk model, banner, konten Settings, dan snapshot.

Belum tertutup: kombinasi outcome dan `failure_code` terhadap SQLite (Blocker), cancel dan gagal simpan sesudah rencana tertulis, timeout `PREPARE_GRACE`, `PlannedStop` yang berhasil, restart bersamaan, operasi `CloseInterrupted` selama quiesce, restart tanpa key, varian rencana untuk PID asing, instance yang dihapus saat `restoring`, prune operasi yang mengenai rencana, rencana jalur crash lewat `restore`, Ctrl+C di prompt dan selama request, keluaran aborted dengan instance, dan gap atau resnapshot di GUI. Test berbasis `FakeRegistry` tidak bisa menangkap kelas masalah CHECK SQL, jadi setiap aturan CHECK yang tidak ditegakkan domain butuh satu test terhadap SQLite.

## Catatan verify dan test

### (a) `daemonGap` sekitar 10 detik

Bukan cacat terhadap AC-17, karena AC-17 hanya meminta angka teramati tanpa batas lulus. Ini juga bukan batas OS untuk kasus biasa. Plist menulis `ThrottleInterval` dari `LAUNCHD_THROTTLE_INTERVAL_SECS` = 10 (`crates/platform/src/bundle.rs:68`, `xtask/src/build_app/plist.rs:42`), dan spike spec 0015 (`docs/specs/0015-background-agent-cli-install/rationale.md:453`) mencatat exit 75 dinyalakan lagi 0,3 detik kemudian kalau job sudah jalan lebih dari 10 detik, dan sekitar 10 detik sesudah launch nya kalau belum. `handed_over_at` diambil tepat sesudah `stop_instances` (`daemon_restart.rs:457`), dan `reconciled_at` di akhir rekonsiliasi daemon baru (`restore.rs:132`). Di kedua test native, restart dikirim beberapa detik sesudah `rig.bootstrap()` (`apps/daemon/tests/native_launchd.rs:892` sampai 896 dan 950 sampai 955), jadi job baru jalan kurang dari 10 detik dan launchd menahan relaunch. Itu cocok dengan angka 9,4 sampai 10,3 detik, termasuk untuk rencana kosong. Daemon pengguna yang sudah jalan lama seharusnya mendapat celah sekitar 0,3 detik ditambah urutan berhenti dan start. Saran (follow up, bukan gerbang GA): di evidence, tunggu lebih dari `ThrottleInterval` sesudah bootstrap sebelum restart, atau catat lama job sudah jalan bersama angkanya, supaya angka yang dicatat mewakili pengguna.

### (b) `daemon.restart_plan_changed` sesudah abort yang gagal disimpan

Kenapa kode itu yang muncul: guard memeriksa rencana terbuka lebih dulu (`crates/core/src/supervisor/plan.rs:35` sampai 39), jadi di dalam transaksi `restart_pending` selalu menang. `restart_plan_changed` datang dari hitungan digest di luar guard: handler sebelum quiesce (`apps/daemon/src/ipc/restart.rs:379` sampai 391) dan langkah operasi sebelum guard (`daemon_restart.rs:367` sampai 375). Test memanggil `confirmed_restart` langsung sesudah `api.stop(fine)` (`summary.rs:189` sampai 195), yang kembali saat stop diterima. Preview masih melihat `fine` `stopping`, penantian quiesce tidak menunggu stop itu (Major kedua), operasi `daemon_restart` antre di lock `fine`, lalu menghitung ulang sesudah `fine` `stopped`, sehingga digest berbeda. Kalau test menunggu `fine` `stopped` dulu, guard menjawab `daemon.restart_pending` dengan `planId`. Jadi kalimat spec tentang `restart_pending` masih benar terhadap kode; test dan komentarnya yang salah (Minor di atas).

Apakah spec basi: ya, sebagian, sejak `af813c99` (merge `5110756a`). Kode sekarang tetap memanggil `queue_restore` walau `aborted` gagal disimpan (`daemon_restart.rs:463` sampai 477), dan test `summary.rs` membuktikan `fine` mendapat proses baru. Yang basi di `index.md`: *Edge cases* baris 228 ("di kode sekarang `queue_restore` terlewat ... tetap `stopped` dengan desired `running` selama daemon ini jalan", dan "operasi tetap `failed` dengan error registry", padahal operasi gagal dengan error stop, baris 478), kalimat "Pengecualian" di *Alur* langkah 7 (baris 132), dan baris test di *Critical test scenarios* (baris 251). Yang masih benar: rencana tetap `stopping` di registry, restart berikutnya ditolak `restart_pending`, event terakhir dan `DaemonInfo.restart` tetap `stopping`, CLI keluar 8, banner GUI bertahan, dan daemon berikutnya di boot yang sama meneruskannya sebagai `restoring`. Perbarui lewat `/architect` atau `/sync`.

Penilaian yang spec minta (apakah start ulang tetap diantre walau rencana tidak tersimpan): tetap antre, yaitu perilaku kode sekarang. Desired instance itu `running` dan rencana tidak boleh mengubahnya, jadi membiarkannya `stopped` sampai daemon berikutnya (yang di launchd bisa berhari hari kemudian) adalah gangguan diam diam. Tidak ada risiko start ganda: restore memeriksa `stopped` atau `failed` dengan desired `running` (`pending.rs:125` sampai 130), dan daemon berikutnya yang mengambil rencana `stopping` itu menemukan `fine` sudah `running` dan `Proven` lalu mengadopsinya. Kalau registry sedang gagal menulis, `start_instance` gagal bersih dengan error registry. Risiko yang tersisa ada di Minor "Abort yang gagal disimpan": terbitkan view `aborted` dari memori, coba simpan lagi di latar, dan tutup rencana `stopping` yang operasinya sudah final `failed` atau `cancelled` sebagai `aborted` saat diambil alih. Pertimbangkan juga pemeriksaan rencana terbuka yang murah sebelum quiesce, supaya restart berikutnya langsung dijawab `restart_pending` tanpa menahan request pengguna selama penantian quiesce.

### (c) Flakiness `restore::a_daemon_that_ends_while_the_plan_stops_leaves_it_to_the_next_one`

Ada dua asumsi timing. Pertama, budget stop 2 detik milik `stuck` (`apps/daemon/tests/restart_plan/plan.rs:47` sampai 58) harus lebih lama dari stop `fine`, commit exit nya, polling `wait_instance` 20 ms lewat IPC, `drop(api)`, dan awal `harness.stop()` sampai shutdown memberi `abandon`. Kalau timer 2 detik berbunyi lebih dulu, jalur abort menyimpan `aborted`, daemon berikutnya tidak punya rencana untuk diambil alih, dan `wait_plan(Finished)` (baris 238) panic sesudah 30 detik. Di bawah load 14 itu mudah terjadi. Kedua, test hanya menunggu lifecycle `fine` `stopped`, sedangkan `stoppedAt` nya baru disimpan `stop_instances` sesudah `stop_process` kembali (`daemon_restart.rs:531` sampai 553). Kalau daemon berikutnya membaca rencana sebelum persist itu, outcome `fine` menjadi `adopted` (Minor outcome) dan assertion baris 246 gagal. Ini cacat test dan kode, bukan noise; perbaikannya ada di Minor test (c) dan Minor outcome `adopted`.

### (d) `daemon.starting` selama `Starting`

Dikonfirmasi tidak melanggar AC. Preview (`apps/daemon/src/ipc/restart.rs:140` sampai 144) dan `daemon.shutdown { mode: restart }` (baris 365 sampai 369) menjawab `daemon.starting` hanya selama supervisor belum siap. AC-7 mengatur preview untuk supervisor siap dan recovery mode, dan AC-10 sampai AC-12 tidak menyebut `Starting`. `daemon.starting` adalah kode umum spec 0006. Tidak ada yang dihentikan dan tidak ada quiesce yang tertinggal. Sheet GUI bisa sesaat menampilkan error ini; GUI bisa memperlakukannya sebagai sementara (follow up opsional).

### (e) `DaemonInfo.restart` dengan TTL per daemon run

Sesuai AC-15 dan baris API `daemon.info`. View disimpan setiap kali daemon run ini menerbitkan `daemon.restart_plan` (`apps/daemon/src/supervisor.rs:412` sampai 420) dan disaring saat dibaca (`apps/daemon/src/state.rs:248` sampai 263): rencana terbuka tampil berapa pun umurnya, rencana dengan `finishedAt` tampil selama kurang dari `RESTART_SUMMARY_TTL` (10 menit), dan daemon run baru mulai kosong. Test `summary.rs:51` sampai 102 menutup keempat kasus dengan jam test. Catatan kecil: reply handshake (`HandshakeAccepted`, `apps/daemon/src/connection.rs:660` sampai 670) tidak membawa `DaemonInfo`, jadi kata "dan handshake" di baris API hanya terpenuhi lewat `daemon.get_info` sesudahnya; spec perlu diselaraskan. `take_over_plan` yang gagal menyimpan (`restore.rs:68` sampai 71) tidak menerbitkan apa pun, jadi daemon run itu tidak menampilkan rencana walau rencana terbuka di registry.

## Gerbang GA

Menurut aturan tim, `Review it` dicentang setelah tidak ada Blocker atau Major yang terbuka. Minor dan Nit dicatat di log sebagai follow up.

| Temuan | Status untuk GA | Butuh keputusan |
| --- | --- | --- |
| Blocker `skipped` dengan `failure_code` (`restore.rs:183`) | blocker GA | tidak |
| Major cancel atau error sesudah rencana tertulis (`daemon_restart.rs:446`) | blocker GA | pilih jalur abort penuh atau tolak cancel sesudah VALIDATE |
| Major penantian quiesce melewatkan `CloseInterrupted` (`operation/mod.rs:906`) | blocker GA | tidak, AC-10 sudah jelas |
| Major cascade `operations` (`0009_daemon_restart_plan.sql:15`) | blocker GA | ya, bentuk migrasi baru (kecualikan dari prune operasi, atau `SET NULL`) |
| Major timeout GUI 45 detik (`DaemonRestartClient.swift:17`) | blocker GA | tidak |
| Major model GUI di sekitar reconnect (`DaemonRestartModel.swift:91`) | blocker GA | tidak |
| Major Ctrl+C (`restart.rs:149`, `mod.rs:182`) | blocker GA | tidak |
| Major teks CLI `aborted` (`plan.rs:102`) | blocker GA | tidak |
| Major banner GUI `aborted` (`RestartBannerContent.swift:116`) | blocker GA | tidak |
| Major cabang error tanpa test (`ipc/restart.rs:447`) | blocker GA, dikerjakan bersama perbaikan Major lain | tidak |
| Teks spec basi (catatan b: *Edge cases* baris 228, *Alur* langkah 7 baris 132, *Critical test scenarios* baris 251; catatan e: "dan handshake") | follow up spec lewat `/architect` atau `/sync` | tidak |
| Evidence `daemonGap` yang mewakili pengguna (catatan a) | follow up | tidak |
| 18 Minor, 23 Nit | follow up, dicatat di log | lihat Minor recovery mode di Settings |

## Perintah dijalankan

| Perintah | Hasil |
| --- | --- |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only cargo xtask check-deps` (sesi utama) | lulus, "13 crates follow the dependency, unsafe, and source rules" |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only cargo xtask codegen --check` (sesi utama) | lulus, "20 generated files are up to date" |
| `cargo test -p stackly-domain`, `-p stackly-protocol`, `-p stackly-storage`, test `supervisor::plan` di core, clippy `-D warnings` untuk empat crate irisan B, `cargo check --target aarch64-pc-windows-msvc` (subagent irisan B, tidak diulang sesi utama) | dilaporkan lulus |
| Probe di luar repo dan `sqlite3` pada SQL migrasi (subagent irisan B) | membuktikan CHECK `failure_code` menolak `skipped` dengan `data_missing`, `restore` menolak rencana jalur crash, dan cascade menghapus rencana terbuka |

Test native, launchd, dan suite Swift tidak dijalankan (instruksi brief). Irisan A1 dan A2 menilai dari membaca kode dan test.

## Konfirmasi fix (2026-11-02)

**Diperiksa oleh**: sesi ini (Opus 5.5, xhigh), yang membaca setiap diff fix langsung di kode, ditambah satu subagent Opus 5.5 yang membaca diff fix tanpa melihat review ini untuk mencari temuan baru. Sesi ini memeriksa ulang setiap temuan subagent di kode sebelum menuliskannya. Semua commit fix ditulis Sonnet 5.5 (trailer `Co-Authored-By`).
**Range**: `ba31bb71..5144380b`, yaitu merge Swift `9e649990` dan merge Rust `5144380b`. Dibatasi ke commit fix `189fb6f8`, `02e22b8a`, `65eba9e2`, `71f12756`, `7afbbf24`, `271176f8` dan file yang mereka sentuh, termasuk `apps/cli/src/failure.rs` dan `apps/cli/src/commands/daemon/testing.rs` yang lead terima. Hunk F31 di range yang sama (`b6146d74`, `0e607576`, `d98557c6`, `2e94fe73`, merge `ebb57df9`) diabaikan. Nomor baris mengacu ke `5144380b`.
**Verdict**: Blocked. Temuan 1 sampai 9 fixed dan temuan 10 partly, tetapi diff fix membawa dua Major baru (N1, N2), jadi `Review it` belum bisa dicentang.

Jumlah temuan baru: Major 2, Minor 5, Nit 3.

### Status temuan 1 sampai 10

| # | Temuan | Status | Bukti |
| --- | --- | --- | --- |
| 1 | Blocker `skipped` dengan `failure_code` | fixed | `RestartPlan::record_outcome` menolak kode di outcome selain `failed` dengan `RestartPlanError::FailureCodeWithoutFailure` (`crates/domain/src/restart.rs:540`). `record_after` kini mencatat `skipped` tanpa kode (`crates/core/src/supervisor/restore.rs:221`). Test domain `only_a_failed_outcome_carries_a_failure_code` dan test daemon dengan registry SQLite asli `an_instance_whose_data_went_missing_is_skipped_and_the_stored_plan_finishes` (`apps/daemon/tests/restart_plan/restore.rs:391`), yang membaca rencana `finished` langsung dari file registry. Saran opsional agar `record_plan_outcome` tidak membuang rencana dari memori saat persist terminal gagal (`restore.rs:143` sampai 148) tidak diambil; dengan akar masalah tertutup, ini tidak menghalangi. |
| 2 | Major cancel atau error sesudah rencana tertulis | fixed | Lead memilih jalur abort penuh. `restart_steps` mengarahkan setiap `Err` sesudah `write_plan`, kecuali `WorkError::Abandoned`, ke `abort_plan` (`crates/core/src/supervisor/daemon_restart.rs:454` sampai 457, `:515`). `stop_and_hand_over` memakai salinan `handed`, jadi `plan` tetap `stopping` sampai persist `handed_over` berhasil (`:490` sampai 498) dan abort selalu berangkat dari `stopping`. `abort_plan` menerbitkan view `aborted` dari memori walau persist gagal, lalu `queue_restore` dan start sesudah operasi final. Daemon berikutnya menutup rencana `stopping` yang operasinya `cancelled` atau `failed` bukan karena `operation.interrupted` sebagai `aborted` tanpa stop kedua (`restore.rs:61` sampai 73, `:96`). Test: `handed_over` ditolak trigger SQLite (`apps/daemon/tests/restart_plan/errors.rs:162`), cancel saat stop (`:218`), rencana `stopping` basi ditutup daemon berikutnya (`:270`). Jalur di daemon yang masih hidup benar. Cabang baru di daemon berikutnya membawa Major baru N1. |
| 3 | Major penantian quiesce melewatkan `CloseInterrupted` | fixed | `OperationEngine::in_flight_operations` mendaftar semua operasi aktif selain `daemon_restart` (`crates/core/src/operation/mod.rs:901`, `:905` sampai 935). `wait_for_operations` menunggu daftar itu, dan sesudah batas waktu baru memakai `active_operations` yang tidak resumable untuk `restart_busy` (`apps/daemon/src/ipc/restart.rs:273` sampai 285). Ini sama dengan AC-10. Test `an_instance_start_in_flight_is_waited_for_before_the_plan_is_written` (`errors.rs:106`) menerima `Ok` atau `daemon.restart_plan_changed`. Dua hasil itu benar menurut AC-12, karena tanpa `expectedPlanDigest` daemon membandingkan dengan digest saat request diterima, dan start yang selesai di penantian bisa mengubahnya. |
| 4 | Major cascade `operations` | fixed | Tanpa migrasi, sesuai keputusan lead: prune operasi melewati setiap operasi yang dirujuk rencana (`crates/storage/src/registry/prune.rs:121` sampai 122), fake disamakan (`crates/testkit/src/registry/fake.rs:1913`), dan test conformance baru `prune_keeps_referenced_operations` (`crates/testkit/src/registry/conformance/restart.rs:225`) berjalan terhadap SQLite dan fake. FK `ON DELETE CASCADE` masih ada di skema, tetapi prune tidak lagi memicunya. Sisa: komentar migrasi `0009_daemon_restart_plan.sql:8` sampai 10 masih berkata rencana ikut terhapus bersama operasinya (Nit baru N8). |
| 5 | Major timeout GUI 45 detik | fixed | `restartTimeout` kini `SecretsClient.interactiveTimeout`, 190 detik (`clients/swift/Sources/StacklyClient/Daemon/DaemonRestartClient.swift:19`), doc comment menyebut 30 ditambah 40 detik, dan test memeriksa 190 detik. |
| 6 | Major model GUI di sekitar reconnect | fixed | `receive(info:)` tidak lagi menghapus rencana terbuka saat `restart` null (`apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/DaemonRestartModel.swift:94` sampai 96). Rencana `stopping` atau `handed_over` dibuang saat `daemon.reconciling` `finished` (`:105`, `:116`), dengan `watchedPlan` dipertahankan supaya toast tetap muncul. `DaemonModel` membaca ulang `daemon.get_info` sesudah `.resnapshot` dan `.caughtUp` (`apps/macos/Packages/StacklyUI/Sources/StacklyUI/Connection/DaemonModel.swift:507`, `:521`), dan penjaga rank di `accept` (`DaemonRestartModel.swift:124`) mencegah jawaban lama menimpa state yang lebih baru. Enam test model baru menutup kedua urutan. Karena `restart` null kini tidak membersihkan apa pun, daemon tanpa supervisor membuat state macet: Major baru N2, dan Minor baru N4. |
| 7 | Major Ctrl+C | fixed | `LazyInterrupt` baru memasang handler sesudah jawaban y/N (`apps/cli/src/commands/daemon/mod.rs:132`, `:209`), jadi Ctrl+C di prompt pertama memakai perilaku default dan proses berakhir sebelum apa pun terkirim. Request restart dibalap dengan `interrupt.wait()` dan keluar 130 `cli.interrupted` dengan state `restart_request` tanpa membatalkan restart di daemon (`apps/cli/src/commands/daemon/restart.rs:152` sampai 164). Dua test unit baru. Penilaian AC-15: AC-15 sendiri tidak menyebut Ctrl+C; yang mengikat adalah exit 130 di tabel API dan *Edge cases* baris 229 (Ctrl+C saat menunggu restore keluar 130, rencana tetap jalan), dan keduanya kini dipenuhi. Exit default di prompt pertama memberi status 130 dari shell tanpa body `cli.interrupted`; itu cukup, karena belum ada yang dikirim dan `--json` tidak pernah bertanya. Batasan di prompt kedua dicatat sebagai Minor baru N3. |
| 8 | Major teks CLI `aborted` | fixed | `aborted_split` membagi baris `stop_restore` menurut `stoppedAt` (`apps/cli/src/commands/daemon/plan.rs:83`). `judge` meneruskan dua daftar ke hint `cli.restart_aborted` (`apps/cli/src/failure.rs:472` sampai 497), `render_plan` menulis baris "Stopped, starting again" dan "Did not stop, still running" (`plan.rs:301` sampai 311), dan kolom RESULT tidak lagi menampilkan `skipped` (`plan.rs:350`). Test unit `an_aborted_plan_exits_7_names_both_groups_and_has_no_durations` dan test binary di `apps/cli/tests/daemon_restart_plan.rs:487` dengan dua instance yang `stoppedAt` nya berbeda. |
| 9 | Major banner GUI `aborted` | fixed | Judul "Daemon restart aborted", pesan tidak lagi mengklaim start ulang sudah selesai, dan dua kelompok item dari `stoppedAt`, yang dihentikan lebih dulu (`apps/macos/Packages/StacklyUI/Sources/StacklyUI/Shell/RestartBannerContent.swift:119`, `:139`). Copy di `RestartCopy`, test konten, dan snapshot terang, gelap, dan sempit diperbarui. |
| 10 | Major cabang error tanpa test | partly | Ditambah: restart tanpa `idempotencyKey` (`errors.rs:56`), cancel saat stop (`:218`), gagal simpan `handed_over` (`:162`), rencana `stopping` basi (`:270`), start yang sedang jalan ditunggu (`:106`), `data_missing` dengan SQLite asli (`restore.rs:391`), dan varian rencana untuk PID asing (`crates/core/tests/supervisor_start/pid_reuse.rs:180`). Dua skenario yang spec wajibkan kini ada. Masih tanpa test: timeout `PREPARE_GRACE` dengan `engine.cancel` (`apps/daemon/src/ipc/restart.rs:453` sampai 462), `PendingKind::PlannedStop` yang berhasil lalu restore, dua restart bersamaan, dan gagal `finish_step`. Cabang itu sekarang berakhir di jalur abort yang sudah dites, jadi sisanya turun menjadi Minor N7, bukan blocker GA. |

### Ikut terbawa dari daftar Minor

- **fixed**: instance yang hilang sebelum pending run nya kini menutup entri rencana lewat `settle_restored` (`crates/core/src/supervisor/pending.rs:121`, `:126`).
- **fixed**: abort yang gagal disimpan kini menerbitkan view `aborted` dari memori dan tidak meninggalkan stop kedua, karena daemon berikutnya menutup rencana itu (`daemon_restart.rs:524`, `restore.rs:61`).
- **fixed**: `durations` null untuk rencana `aborted` dan `expired` (`apps/cli/src/commands/daemon/restart.rs:288`).
- **fixed**: tombol Restart Daemon memakai `mutates: false`, jadi gerbang mutasi shell tidak lagi mematikannya tanpa alasan (`apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/DaemonSettingsSection.swift:30`).
- **masih terbuka**: test abort yang gagal disimpan masih menerima `daemon.restart_pending` atau `daemon.restart_plan_changed` dengan komentar yang salah (`apps/daemon/tests/restart_plan/summary.rs:199`). Minor lain di daftar awal tidak disentuh dan tetap follow up.

### Temuan baru dari diff fix

#### 🟠 N1. Daemon berikutnya menutup rencana `stopping` yang operasinya `cancelled` tanpa menyalakan lagi yang sudah dihentikan, `crates/core/src/supervisor/restore.rs:61`
**Asal**: fix temuan 2 (`65eba9e2`). Ditemukan subagent, diverifikasi sesi ini.
**Problem**: Cabang baru di `take_over_plan` menganggap operasi `cancelled`, atau `failed` dengan kode selain `operation.interrupted`, berarti daemon lama sudah menyalakan lagi instance yang ia hentikan (komentar `restore.rs:65` sampai 67). Lalu cabang itu hanya menutup rencana `aborted`. Anggapan itu tidak selalu benar. `stop_instances` tidak memeriksa cancel, dan di batas langkah `abandon` menang atas `cancel` (`crates/core/src/operation/context.rs:155`). Jadi daemon yang berakhir sesudah cancel diminta mengembalikan `Abandoned`, dan `abort_plan` tidak jalan. Recovery di daemon berikutnya lalu menutup operasi itu `cancelled`, karena `cancel_requested_at` terisi (`crates/core/src/operation/recovery.rs:319`).
**Why it matters**: Urutan: restart tertahan stop PostgreSQL yang pelan, orang menjalankan `stackly operation cancel`, lalu daemon berakhir (logout, launchd bootout, atau crash) sebelum `begin_step(COMPLETE)`. Daemon berikutnya di boot yang sama menutup rencana `aborted` tanpa `queue_restore`. Instance yang sudah berhenti tetap `stopped` dengan desired `running`, padahal CLI dan banner menulisnya "stopped, starting again". Ini melanggar AC-14 (rencana yang belum terminal dipulihkan daemon berikutnya) dan janji AC-11. Celah yang sama ada untuk operasi `failed` kalau daemon lama berakhir di antara operasi final dan start ulangnya, karena runner pending berhenti saat `is_shutdown` (`crates/core/src/supervisor/pending.rs:99`).
**Suggested fix**: Di cabang itu panggil juga `queue_restore` untuk entri yang `stoppedAt` nya terisi. Run `Restore` hanya jalan kalau instance masih `stopped` atau `failed` dengan desired `running` (`pending.rs:137` sampai 141), jadi instance yang sudah dinyalakan daemon lama tidak disentuh, dan instance yang tidak berhenti tetap tidak dihentikan dua kali. Tambah test daemon: cancel saat stop yang pelan, daemon berakhir sebelum stop selesai, lalu daemon berikutnya menyalakan instance yang sudah berhenti.

#### 🟠 N2. Restart dari Settings di recovery mode membuat banner "restarting" dan tombol mati sampai app dibuka ulang, menutupi banner recovery, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/DaemonRestartModel.swift:239`
**Asal**: fix temuan 6 dan Minor tombol (`02e22b8a`). Ditemukan sesi ini dan subagent secara terpisah, diverifikasi sesi ini.
**Problem**: `confirm` mengisi `isAwaitingPlan` setiap kali restart diterima, juga untuk `ShutdownAccepted` dengan `planId` null, yaitu restart di recovery mode yang keluar 75 tanpa rencana (AC-7). Sesudah fix, hanya dua hal yang membersihkan state itu: event `daemon.restart_plan` dan `daemon.reconciling` `finished` (`:100` sampai 121). `receive(info:)` dengan `restart` null tidak lagi membersihkan apa pun (`:94` sampai 96). Daemon dengan registry dalam recovery tidak punya supervisor (`apps/daemon/src/server.rs:911` sampai 912), jadi tidak mengirim keduanya. `mutates: false` di `DaemonSettingsSection.swift:30` kini membuat tombol bisa ditekan di recovery mode, jadi urutan ini terjangkau.
**Why it matters**: Urutan pertama: registry dalam recovery, orang menekan Restart Daemon (langkah yang wajar untuk mencoba keluar dari recovery), daemon keluar 75, dan launchd menyalakan daemon baru yang masih recovery. Banner "stacklyd is restarting" yang tidak bisa ditutup tampil terus dan menggantikan banner recovery (`apps/macos/Packages/StacklyUI/Sources/StacklyUI/Shell/AppShell.swift:234` sampai 239). Tombol tetap mati dengan "A restart is already in progress." sampai app dibuka ulang. Urutan kedua: rencana `handed_over` dari daemon sehat, lalu daemon baru masuk recovery (misalnya migrasi gagal sesudah update); rencana tetap `handed_over` di model dengan akibat yang sama. Melanggar AC-16. Sebelum fix, `receive(info:)` membersihkan keduanya di handshake pertama.
**Suggested fix**: Isi `isAwaitingPlan` hanya kalau `ShutdownAccepted.planId` ada. Di handshake dengan `serverInstanceId` baru yang `health.recovery` nya terisi dan `restart` null, hapus rencana terbuka dan `isAwaitingPlan`. Tambah test model untuk dua urutan itu.

#### 🟡 N3. Ctrl+C di prompt kedua sesudah `daemon.restart_plan_changed` tertelan sampai Enter, `apps/cli/src/commands/daemon/restart.rs:135`
**Problem**: Handler tokio tidak bisa dilepas lagi sesudah `arm()` (`restart.rs:152`). Di putaran kedua, `restart_preview` (`:173`) tidak dibalap dengan interrupt, dan `read_line` yang blocking terus menunggu. Ctrl+C lalu Enter keluar 9 `cli.confirmation_declined`; Ctrl+C lalu `y` keluar 130 tanpa mengirim apa pun. Pemeriksaan `pending()` di putaran pertama tidak pernah bisa benar, dan test baru memakai `AfterInterrupt` yang mengabaikan `arm`, jadi `LazyInterrupt` sendiri tidak dites.
**Why it matters**: Tidak ada restart yang terkirim sesudah Ctrl+C, jadi aman. Tetapi di TTY, Ctrl+C di prompt kedua tampak tidak berbuat apa apa, dan exit code nya bisa 9, bukan 130.
**Suggested fix**: Sesudah jawaban di prompt kedua, kembalikan 130 kalau `pending()`, apa pun jawabannya, dan balap preview kedua dengan `interrupt.wait()`. Tambah test untuk `LazyInterrupt`.

#### 🟡 N4. Model GUI bisa memegang rencana `restoring` basi, atau mundur ke snapshot lama dengan rank sama, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/DaemonRestartModel.swift:116`
**Problem**: `dropPlanNobodyTook` tidak pernah membuang `restoring`. Kalau app melewatkan `finished` lalu tersambung lagi sesudah `RESTART_SUMMARY_TTL`, atau ke daemon run berikutnya, `restart` null tidak mengubah apa pun dan rencana tetap `restoring`. Penjaga di `accept` (`:124`) hanya menolak rank yang lebih rendah, jadi jawaban `get_info` dari pembacaan ulang (`DaemonModel.swift:521`) yang lebih tua tetapi dengan rank sama bisa menimpa view yang lebih baru sampai event berikutnya.
**Why it matters**: Banner dan tanda baris instance bisa bertahan atau muncul lagi untuk instance yang sudah jalan. Urutannya jarang dan sebagian besar sementara.
**Suggested fix**: Buang rencana terbuka saat handshake dengan `serverInstanceId` lain dari daemon yang rekonsiliasinya sudah selesai dan tidak membawa rencana; abaikan jawaban `get_info` yang diminta sebelum event terakhir.

#### 🟡 N5. Pembacaan instance yang gagal kini menutup entri rencana, bukan membaca lagi, `crates/core/src/supervisor/pending.rs:116`
**Problem**: Untuk error selain `NotFound`, `run_one` kini memanggil `settle_restored` (`:126`). Kalau pembacaan kedua di sana berhasil, entri dicatat `failed` dengan `not_running`, dan instance tetap `stopped` dengan desired `running`. Komentar di baris 116 ("a failed read decides nothing: it waits and reads again", AC-22) bertentangan dengan kode, sebelum dan sesudah fix.
**Why it matters**: Gangguan baca sesaat mengubah restore menjadi kegagalan permanen. Sebelum fix rencananya macet `restoring`; sekarang selesai, tetapi instance tetap tidak dinyalakan.
**Suggested fix**: Ulangi pembacaan dengan jeda sebelum menutup entri, atau masukkan lagi run ke antrean.

#### 🟡 N6. `active_matching` memasangkan jenis operasi dengan ID yang salah, `crates/core/src/operation/mod.rs:917`
**Problem**: Jenis disaring menurut state, lalu di `zip` dengan daftar ID yang tidak disaring (`:917` sampai 930). Kalau halaman `active_only` memuat operasi di state lain (misalnya yang perlu recovery), pasangan bergeser. Kode ini dari range F30 awal dan terlewat di review pertama; fix temuan 3 kini memakainya juga.
**Why it matters**: Jumlah, jadi penantian quiesce, tetap benar, tetapi `restart_busy` dan preview bisa menyebut `operationId` yang salah.
**Suggested fix**: Saring dan petakan dalam satu `filter_map` per view.

#### 🟡 N7. Sisa cabang tanpa test dari temuan 10
`PREPARE_GRACE` dengan `engine.cancel` (`apps/daemon/src/ipc/restart.rs:453` sampai 462), `PendingKind::PlannedStop` yang berhasil lalu restore, dua restart bersamaan, dan gagal `finish_step`. Lihat baris 10 di tabel.

#### Nit
- **N8**: komentar migrasi `crates/storage/migrations/0009_daemon_restart_plan.sql:8` sampai 10 masih berkata rencana ikut terhapus saat operasinya di prune.
- **N9**: cabang baru di `take_over_plan` diam saja kalau `abort` atau persist gagal (`restore.rs:68`); tambah `warn` seperti cabang lain.
- **N10**: `record_plan_outcome` mencatat outcome yang ditolak domain hanya di level `debug` dan membiarkan rencana `restoring` (`restore.rs:152`); kini domain bisa menolak kombinasi kode, jadi `warn` lebih berguna.

Diperiksa dan benar: tidak ada jalur yang menutup rencana `handed_over` sebagai `aborted`; gagal persist `handed_over` sesudah `finish_step` tetap menyimpan rencana `aborted` dan mengantre start ulang; tidak ada start ganda karena run memeriksa lifecycle dan desired lagi; `NOT IN` di prune aman karena `operation_id` `NOT NULL`; penantian quiesce dibatasi deadline; tidak ada pelanggaran aturan AGENTS.md (doc comment, `unwrap`, gaya pesan error) di diff fix.

### Gerbang GA sesudah fix

| Temuan | Status untuk GA |
| --- | --- |
| 1 sampai 9 | tertutup |
| 10 | sisanya turun ke Minor N7, follow up |
| N1 restore yang dilewati sesudah cancel dan daemon berakhir (`restore.rs:61`) | blocker GA |
| N2 banner dan tombol macet sesudah restart di recovery mode (`DaemonRestartModel.swift:239`) | blocker GA |
| N3 sampai N7, N8 sampai N10 | follow up, dicatat di log |
| Teks spec basi (bagian berikut dan catatan b dan e) | follow up lewat `/architect` atau `/sync` |

### Regresi F31 di `main`

`restore::a_new_boot_session_expires_the_plan_and_starts_nothing` (`apps/daemon/tests/restart_plan/restore.rs:167`) gagal di `main` sejak merge F31 langkah 2 sampai 6 (`ebb57df9`), menurut lead, dan `dev-f31-m1` sedang menanganinya. Fix F30 tidak menyentuh jalur ini: cabang baru di `take_over_plan` hanya berlaku untuk rencana `stopping` di boot yang sama (`restore.rs:61` sampai 63), sedangkan test ini mengganti boot ID; perubahan outcome hanya ada di `record_after`, yang tidak pernah dicapai rencana `expired` karena rencana itu tidak disimpan di memori (`restore.rs:87` sampai 89). Jadi regresi ini bukan temuan fix F30. Catatan untuk `dev-f31-m1`: AC-13 menyerahkan start instance di sesi baru ke fitur 31, jadi assertion "nothing starts" di test itu mungkin perlu dibaca ulang terhadap aturan antrean start F31, bukan hanya kodenya. Test ini tidak dijalankan di sini (disk).

### Follow up spec

Fix temuan 2 membuat dua kalimat spec makin basi, selain yang sudah dicatat di catatan (b): *Alur* langkah 7 (baris 132) masih berkata start ulang tidak terjadi saat rencana `aborted` gagal disimpan, padahal kode tetap menyalakan lagi dan daemon berikutnya menutup rencana `stopping` itu sebagai `aborted`; dan spec belum menyebut cancel operasi `daemon_restart` sebagai penyebab abort (kode memakai `closeReason` `stop_timeout` untuk itu, sesuai aturan satu nilai di *Edge cases* baris 227). Keduanya lewat `/architect` atau `/sync`.

### Perintah dijalankan (konfirmasi)

| Perintah | Hasil |
| --- | --- |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only cargo test -p stackly-domain` | lulus, delapan suite, termasuk `only_a_failed_outcome_carries_a_failure_code` |

Test daemon, CLI, storage conformance, dan suite Swift tidak dijalankan: disk sekitar 9 GiB dan brief meminta hanya test sempit. `check-deps` dan `codegen --check` tidak diulang, karena commit fix tidak mengubah dependency, kontrak, atau aturan source scan.
