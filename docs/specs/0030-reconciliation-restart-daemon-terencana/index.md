# 0030. Rekonsiliasi dan restart daemon terencana

**Date**: 2026-11-02
**Status**: In Progress

## Summary

Spec ini memperkuat rekonsiliasi spec 0010 untuk instance nyata dan menambah restart daemon terencana. Setelah daemon crash atau restart, rekonsiliasi kini juga memeriksa folder data dan runtime, dan start memeriksa port, jadi data yang hilang tidak pernah diisi ulang diam diam dan proses luar tidak pernah disentuh. `stackly daemon restart` dan tombol **Restart Daemon** di Settings membuat rencana: instance dari provider yang survival nya sudah terbukti (fake, PostgreSQL, NATS) tetap jalan dan diadopsi daemon baru, instance dari provider lain dihentikan aman lalu dinyalakan lagi. Rencana itu disimpan di registry supaya daemon baru bisa menyelesaikannya, dan GUI serta CLI menampilkan jendela disruption (jeda kendali) dengan hasil per instance, tanpa klaim zero downtime.

## Requirements

**User stories**:

- Sebagai pengguna, saya ingin database saya tetap sama (PID, identitas, data) setelah GUI ditutup, daemon crash, atau daemon di restart, supaya pekerjaan saya tidak terputus dan tidak ada server kedua di data yang sama.
- Sebagai pengguna, saya ingin `stackly daemon restart` dan tombol Restart Daemon memberi tahu sebelumnya instance mana yang tetap jalan dan mana yang akan dihentikan sebentar, lalu melaporkan hasilnya, supaya saya tahu persis seberapa besar gangguannya.
- Sebagai pengguna, saya ingin instance yang folder datanya hilang, runtimenya terhapus, atau portnya dipakai program lain berhenti dengan alasan yang jelas, bukan dibuatkan data kosong baru atau merebut port.
- Sebagai penulis script, saya ingin restart terencana punya `--yes`, `--json`, dan exit code yang stabil, supaya automation tahu apakah desired state benar benar pulih.

**Acceptance criteria** (ID lokal spec ini; ID PRD tiga digit):

- **AC-1**: Dengan tiga instance PostgreSQL `ready`, menutup lalu membuka GUI (semua koneksi client daemon putus lalu tersambung lagi) tidak mengubah `process.pid`, `process.startedAt`, `id`, maupun `serverInstanceId`, query `SELECT 1` ke ketiganya tetap berhasil, dan app mengambil ulang state dari daemon. Dibuktikan test daemon dengan PostgreSQL native dan test integrasi Swift dengan fake, plus record manual `native` di `docs/manual.md` (REQ-001, AC-001).
- **AC-2**: Dengan tiga PostgreSQL `running` (sentinel berbeda) dan satu `stopped`, `SIGKILL` ke `stacklyd` lalu daemon baru: setiap folder data punya tepat satu postmaster (pemegang `instance.lock`), ketiganya `adopted` dengan PID dan `startedAt` yang sama, sentinel tiap cluster utuh, instance `stopped` tetap `stopped`, dan tidak ada file di folder data yang dihapus (REQ-013, AC-013, RISK-03).
- **AC-3**: Observasi tersimpan sebuah instance menunjuk PID yang kini milik executable lain (proses `sleep` milik test). Rekonsiliasi, recheck berkala, `instance stop`, force stop, start, dan rencana restart tidak pernah menyinyal proses itu. Hasilnya mengikuti tabel `classify` spec 0010: kalau `instance.lock` dipegang pihak lain, instance menjadi `unknown` dengan blocker `dataInUse` yang membawa evidence (check `executable_matches` gagal), terlihat di `stackly instance inspect --json`, teks manusianya, dan attention GUI, dan stop, force stop, serta start nya gagal `supervisor.identity_unproven` tanpa spawn; kalau lock bebas, proses lama dianggap hilang (`Gone(pid_reused)`) dan instance mengikuti baris `Gone` tabel rekonsiliasi spec 0010 (desired `stopped` menjadi `stopped`), tetap tanpa sinyal ke PID itu. Blocker `identityUnproven` hanya muncul kalau keenam pemeriksaan identitas lulus tetapi proses itu tidak memegang lock, atau kalau bukti gagal dikumpulkan (AC-067).
- **AC-4**: Folder data instance yang `stopped` atau `failed` (sudah diinisialisasi) dihapus, dikosongkan (untuk paket yang mendeklarasikan `compatibility.dataMarker`), atau kehilangan marker data nya saat daemon mati. Rekonsiliasi membuatnya `failed` dengan `lastFailure.code` `data_missing` (transisi domain baru), tidak menjalankan init, tidak membuat folder data, dan tidak menyalakannya walau desired `running` atau ada di rencana restore. Start, respawn, dan restore ditolak `supervisor.data_unavailable` sebelum folder run dibuat dan sebelum spawn; marker yang ada tetapi tidak cocok atau tidak terbaca tetap `supervisor.data_incompatible` (spec 0025). Proses yang `Proven` tetap diadopsi dan mendapat finding `data_missing` di `daemon.reconciling finished`.
- **AC-5**: Executable runtime instance dihapus dari folder install runtime. Proses yang `Proven` tetap diadopsi dengan finding `runtime_missing` di `daemon.reconciling finished`; start, respawn, dan restore gagal sebelum spawn dengan kode yang sudah ada (`supervisor.executable_invalid`, atau `supervisor.runtime_unavailable` kalau record runtime tidak terpasang) dan `lastFailure.code` `executable_invalid` atau `runtime_unavailable` (nilai baru di `error_failure_code`). Tidak ada blocker baru.
- **AC-6**: Port fixed instance dipegang listener luar saat respawn atau restore. Spawn tidak terjadi, start gagal `supervisor.endpoint_in_use` dengan `lastFailure.code` `endpoint_in_use` dan lifecycle menurut jalur start yang ada (`stopped` untuk restore), listener luar tidak disinyal, dan port pengganti tidak dipilih otomatis.
- **AC-7**: `daemon.restart_preview` mengelompokkan instance menurut lifecycle: `running`, `starting`, dan `stopping` dengan provider yang cocok di `HANDOVER_PROVEN` menjadi `keep` alasan `handover_proven`, dengan provider lain `stop_restore` alasan `handover_unproven`; `recovering` (menunggu respawn, tanpa proses) selalu `keep` alasan `respawn_pending`; `unknown` selalu `keep` alasan `identity_unproven`; saat registry dalam recovery mode semua `keep` alasan `supervisor_unavailable`. Instance `stopped`, `failed`, `initializing`, `uninitialized`, dan `retained` tidak masuk rencana. Preview juga membawa `digest` dan daftar operasi yang menghalangi.
- **AC-8**: Test native launchd (setelah persetujuan kamu) membuktikan instance PostgreSQL 17, PostgreSQL 18, dan NATS bertahan dengan PID dan `startedAt` yang sama melewati `launchctl kickstart -k`, `launchctl bootout`, dan `stackly daemon restart`, lalu diadopsi dengan identitas terverifikasi. Hanya provider yang lulus test ini masuk `HANDOVER_PROVEN`; kalau sebuah provider gagal, entri nya tidak ditulis dan instance nya ikut jalur `stop_restore`.
- **AC-9**: Restart terencana dengan satu instance `stop_restore` (daftar handover uji yang kosong) dan satu `keep`: instance `stop_restore` dihentikan dengan strategi stop provider tanpa mengubah desired (`running`), rencana tercatat `stopping` lalu `handed_over` sebelum daemon keluar 75, daemon baru menyalakannya lewat restore (`PendingKind::Restore`, pemicu `planned_restart`) sampai `ready`, instance `keep` diadopsi, lalu rencana `finished` dengan `outcome` `restored` dan `adopted` dan `readyAt` terisi.
- **AC-10**: Saat restart terencana diterima, daemon masuk quiesce (jeda penerimaan) di `OperationEngine::begin`: operasi baru dari `Origin::Request` ditolak `daemon.restart_pending` (kategori `conflict`), sedangkan operasi `Origin::Background` dan operasi `daemon_restart` itu sendiri tetap boleh. Operasi lain (`queued` dan `running`) ditunggu paling lama `RESTART_QUIESCE_WAIT` (30 detik), dan operasi `daemon_restart` baru dimulai sesudah penantian itu. Kalau sesudahnya masih ada operasi yang menghalangi (tidak resumable dan bukan `CloseInterrupted`, misalnya create), restart ditolak `daemon.restart_busy` (kategori `conflict`, `items` berisi `operationId` dan `kind` setiap operasi yang menghalangi, paling banyak 32, dan `details.omitted` untuk sisanya) tanpa membuat operasi `daemon_restart`, quiesce dicabut, tidak ada instance yang dihentikan, dan daemon tetap jalan. `force: true` melewati penolakan dan memakai aturan shutdown spec 0007.
- **AC-11**: Instance `stop_restore` yang tidak berhenti dalam timeout stop providernya, atau stop rencana lain yang gagal, membatalkan restart: stop yang belum dimulai tidak dijalankan, rencana `aborted` dengan `closeReason` `stop_timeout` (satu nilai untuk setiap stop yang gagal) di event `daemon.restart_plan`, dan instance yang tidak berhenti tampil sebagai baris `stop_restore` dengan `stoppedAt` kosong. Operasi `daemon_restart` berakhir `failed` dengan error stop pertama dan quiesce dicabut; tepat setelah operasi itu final, setiap instance yang sudah dihentikan rencana dinyalakan lagi lewat operasi sistem `start_instance` (`Origin::Background`, tanpa dialog), bukan di dalam operasi `daemon_restart`. Instance yang macet tetap diawasi, desired tidak berubah, daemon tidak keluar, dan CLI keluar 7 `cli.restart_aborted`. Tidak ada `SIGKILL`.
- **AC-12**: `daemon.shutdown { mode: restart, expectedPlanDigest }` dengan digest yang tidak sama dengan rencana yang dihitung saat itu ditolak `daemon.restart_plan_changed` (kategori `conflict`) tanpa quiesce dan tanpa stop. Digest dihitung lagi di transaksi yang menulis rencana (sesudah quiesce); kalau berbeda dari digest yang dikonfirmasi (atau dari digest awal saat client tidak mengirimnya), quiesce dicabut dan jawabannya juga `daemon.restart_plan_changed`.
- **AC-13**: Rencana yang belum terminal dipulihkan daemon berikutnya di boot yang sama, kapan pun ia muncul. Di boot baru rencana `expired` (`closeReason` `boot_changed`), instance `stop_restore` tetap `stopped`, desired tidak diubah, dan restore tidak menyalakan apa pun (menyalakan instance di sesi baru milik fitur 31).
- **AC-14**: Daemon yang crash saat rencana masih `stopping` diteruskan daemon berikutnya di boot yang sama sebagai `restoring`: instance `stop_restore` yang sudah `stopped` dinyalakan lagi; instance `stop_restore` yang masih `stopping` dan `Proven` menyelesaikan stop rencana (strategi provider, tanpa menulis desired) lalu dinyalakan lagi; instance yang belum sempat dihentikan diadopsi lewat tabel rekonsiliasi. Setiap instance dihentikan paling banyak sekali per rencana, dan outcome nya `restored` atau `adopted`.
- **AC-15**: `stackly daemon restart` menampilkan rencana lebih dulu. Kalau ada instance `stop_restore`: di TTY ia bertanya y/N (jawaban N keluar 9 `cli.confirmation_declined`); tanpa TTY atau dengan `--json` tanpa `--yes` keluar 2 `cli.confirmation_required`. `daemon.restart_plan_changed` di TTY menampilkan rencana baru dan bertanya sekali lagi, di luar TTY keluar 3. CLI mengikuti rencana lewat event `daemon.restart_plan` sampai terminal (paling lama `DAEMON_RESTORE_WAIT`, 10 menit, lalu exit 8 `cli.daemon_wait_timeout`), lalu mencetak hasil per instance plus tiga durasi (`stopping`, `daemonGap`, `restoring`). Rencana `aborted` keluar 7 `cli.restart_aborted`. Restore yang gagal untuk satu instance pun keluar 7 `cli.restart_restore_failed` dengan hasil lengkap di `--json`; instance yang sudah `unknown` di preview dan tetap `unknown` hanya warning `cli.restart_instance_unknown`. `--no-wait-instances` berhenti setelah handshake daemon baru. `stackly daemon status` menampilkan restart terakhir dari `DaemonInfo.restart`, jadi kosong kalau daemon run itu tidak melihat rencana apa pun atau rencananya selesai lebih dari `RESTART_SUMMARY_TTL` lalu.
- **AC-16**: GUI: Settings membuka tab General dengan bagian Daemon (baris `stacklyd` dan tombol Restart Daemon) sesuai mockup UI; tombol nonaktif saat daemon tidak tersambung atau konteks `manual`, dengan alasan di subtitle. Tombol membuka sheet konfirmasi yang mendaftar instance `keep` dan `stop_restore` serta operasi yang menghalangi. Selama jendela disruption banner jendela utama menampilkan "stacklyd is restarting" dan baris instance `stop_restore` bertanda "Stopped for daemon restart"; sesudahnya banner ringkasan (diadopsi, dinyalakan lagi, gagal, durasi) bertahan sampai rencana terminal, lalu toast "Daemon restarted. N instances adopted, M started again." Tab dan baris Settings milik fitur 32 tidak tampil.
- **AC-17**: Evidence run native dengan 10 PostgreSQL `running` (lima 17, lima 18) di binary daemon (`SpawnedDaemon`): setelah `SIGKILL` daemon, `daemon.reconciling finished` dan `instance.list` yang mengklasifikasikan semua 10 instance tersedia paling lama 10 detik sejak handshake pertama yang berhasil ke daemon baru (NFR-007). Durasi `daemonGap` restart terencana dengan 10 instance `keep` juga dicatat sebagai angka teramati, tanpa batas lulus.
- **AC-18**: Kontrak: method `daemon.restart_preview`, field baru di `ShutdownParams`, `ShutdownAccepted`, dan `DaemonInfo`, event `daemon.restart_plan`, `findings` di `DaemonReconciling`, dan kode baru punya fixture di `protocols/fixtures/` dan tipe Swift hasil codegen. Karena `EventKind` tertutup, event baru membuat client lama gagal decode, jadi `ProtocolVersion::CURRENT` naik mayor di commit yang sama.
- **AC-19**: Registry menyimpan paling banyak satu rencana yang belum terminal (unique index parsial) dan 20 rencana terminal terakhir (`RetentionPolicy.restart_plans`); task prune harian yang sudah ada menghapus sisanya beserta baris instance nya dan tidak pernah menghapus rencana terbuka.

## Decision

**Chosen option**: perbaikan di tempat (fix in place) atas rekonsiliasi spec 0010 dan `daemon.shutdown` spec 0015, dengan gerbang handover per provider dari daftar bawaan di core, rencana restart di registry, dan restore lewat jenis pending baru di rekonsiliasi.

Restart terencana menjadi operasi `daemon_restart` di `OperationEngine` yang menulis rencana, menghentikan instance `stop_restore` tanpa mengubah desired, lalu menyerahkan ke urutan berhenti spec 0006; rekonsiliasi daemon baru membaca rencana itu dan menyelesaikannya. Alasan dan pilihan lain ada di [rationale.md](rationale.md).

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (lihat `apps/macos/AGENTS.md`)

## Feature design

### Batas fitur

| Bagian | Pemilik |
| --- | --- |
| Tabel rekonsiliasi, identitas majemuk, adopsi, recheck `unknown` | spec 0010 (dipakai, diperluas dengan pemeriksaan data dan runtime, baris stop rencana, dan restore) |
| `daemon.shutdown`, exit 75, penantian restart CLI, relaunch | spec 0015 (diperluas dengan rencana) |
| Pemulihan operasi saat start, grace shutdown | spec 0007 (dipakai) |
| Kecocokan data dan marker data | spec 0025 (dipakai; marker `Missing` pindah ke `data_unavailable`) |
| Stop, restart, reload instance, force stop | spec 0026 (dipakai) |
| Rencana restart, gerbang handover, quiesce, jendela disruption, finding rekonsiliasi, bagian Daemon di Settings | spec ini |
| Sesi login, demand, antrean start, `agent_off` | spec 0031 (menyesuaikan diri dengan spec ini, lihat Follow-up) |
| Tab Settings lain | fitur 32 |

### Pemeriksaan rekonsiliasi tambahan

Langkah 3 rekonsiliasi spec 0010 mendapat dua pemeriksaan setelah `classify`, dalam urutan tetap, untuk setiap instance yang tidak `retained` dan tidak `uninitialized`. Keduanya hanya membaca.

| Pemeriksaan | Cara | Proses `Proven` | Instance tanpa proses |
| --- | --- | --- | --- |
| Data | `check_instance_folder` (`prepare.rs`) yang diperluas: folder data tidak ada, folder kosong untuk paket yang mendeklarasikan `compatibility.dataMarker`, atau `MarkerRead::Missing` berarti hilang; folder kosong untuk paket tanpa marker (`MarkerRead::NotDeclared`, misalnya NATS, yang tidak punya init yang menulis data) bukan data hilang; marker yang tidak cocok atau tidak terbaca tetap jalur `data_incompatible` | tetap diadopsi, finding `data_missing` | transisi domain `data_missing` (`stopped` atau `failed` ke `failed`, failure `data_missing`); tidak pernah masuk restore atau daftar "jalankan lagi" |
| Runtime | validasi executable yang sama dengan startup transaction (`steps.rs`), dipanggil tanpa efek samping, terhadap executable di folder install runtime (bukan folder run instance) | tetap diadopsi, finding `runtime_missing` | tidak diubah di rekonsiliasi; start nanti gagal `executable_invalid` atau `runtime_unavailable` sebelum spawn |

Di jalur start, pemeriksaan data berjalan di `prepare.rs` sebelum `ensure_instance_run_dir` (yang membuat folder run), sehingga start, respawn, dan restore menolak data yang hilang tanpa membuat folder apa pun dan tanpa init. Init hanya berjalan dari create (spec 0020). Port tidak diperiksa di rekonsiliasi; startup transaction sudah menolak listener luar dengan `supervisor.endpoint_in_use` lewat `PortProbe`.

`error_failure_code` (`startup.rs`) mendapat `DataUnavailable` ke `FailureCode::DATA_MISSING` dan `RuntimeUnavailable` ke `FailureCode::RUNTIME_UNAVAILABLE`, supaya start yang gagal sebelum spawn tidak jatuh ke `config_render_failed`.

`DaemonReconciling` kehilangan `Copy` dan mendapat `findings: Vec<ReconcileFinding>` (`#[serde(default)]`, kosong untuk phase `started`), satu entri per instance per masalah: `{ instanceId, name, code }` dengan `code` enum terbuka `data_missing`, `runtime_missing`, `unrecognized`. `ReconcileReport` di core membawa daftar yang sama.

### Gerbang handover

`stackly_core::supervisor::handover::HANDOVER_PROVEN: &[HandoverProof]` dengan `HandoverProof { provider: &'static str, line: &'static str, evidence: &'static str }`. `provider` adalah ID provider penuh (`official/postgresql`, `testing/fake`), dicocokkan persis dengan provider yang di pin instance (`ProviderRevisionKey.provider`), bukan nama pendek. `line` dicocokkan dengan semver revision provider yang di pin instance (`ProviderRevisionKey.revision`): untuk mayor 1 ke atas, mayornya (`"1"` cocok dengan `1.0.0` dan `1.0.1`); untuk mayor 0, mayor dan minor (`"0.3"`), karena minor di bawah 1.0 tidak kompatibel menurut semver. Revision yang tidak bisa di parse berarti `handover_unproven`. Isi sekarang: `testing/fake` line `0.3` dan `0.4` (evidence `spec-0015-ac-16`; `0.4` adalah revision paket `providers/fake` saat ini), lalu `official/postgresql` line `1` dan `official/nats` line `1` (evidence `spec-0030-ac-8`), yang masuk setelah test native AC-8 lulus (2026-11-02, Build plan langkah 10). `SupervisorConfig` membawa salinan daftar ini (`handover_proven`) supaya test bisa memakai daftar lain; daemon produksi selalu memakai konstanta. Manifest tidak pernah bisa menambah entri.

### Model data

Satu migrasi `NNNN_daemon_restart_plan.sql` (nomor bebas berikutnya saat build; spec 0031 juga merencanakan migrasi, jadi siapa pun yang build lebih dulu memakai nomor itu):

| Tabel | Kolom | Tipe | Null |
| --- | --- | --- | --- |
| `daemon_restart_plan` | `id` PK | TEXT uuid v7 | tidak |
| | `operation_id` FK `operations.id` | TEXT | tidak |
| | `state` | TEXT, CHECK salah satu dari `stopping`, `handed_over`, `restoring`, `finished`, `aborted`, `expired` | tidak |
| | `digest` | TEXT sha256 hex | tidak |
| | `boot_id` | TEXT | tidak |
| | `server_instance_id_before` | TEXT | tidak |
| | `server_instance_id_after` | TEXT | ya |
| | `created_at` | TEXT RFC 3339 UTC | tidak |
| | `handed_over_at`, `reconciled_at`, `finished_at` | TEXT RFC 3339 UTC | ya |
| | `close_reason` (`stop_timeout`, `boot_changed`) | TEXT | ya |
| `daemon_restart_plan_instance` | `plan_id` FK ke rencana (`ON DELETE CASCADE`), `instance_id` (TEXT tanpa FK), PK gabungan | TEXT | tidak |
| | `name` (nama instance saat rencana dibuat) | TEXT | tidak |
| | `decision` (`keep`, `stop_restore`) | TEXT | tidak |
| | `reason` (`handover_proven`, `handover_unproven`, `respawn_pending`, `identity_unproven`, `supervisor_unavailable`) | TEXT | tidak |
| | `stopped_at`, `ready_at` | TEXT RFC 3339 UTC | ya |
| | `outcome` (`adopted`, `restored`, `failed`, `skipped`) | TEXT | ya |
| | `failure_code` | TEXT | ya |

`instance_id` sengaja tanpa FK supaya menghapus instance tidak terhalang dan baris rencana tetap ada untuk outcome `skipped`. Unique index parsial `ON daemon_restart_plan((1)) WHERE state IN ('stopping','handed_over','restoring')` (bentuk yang sama dengan index sesi di spec 0031). Desired instance tidak pernah ditulis oleh rencana. Retensi: `History::RestartPlans` baru di prune registry dan field `restart_plans: 20` di `RetentionPolicy` (domain); prune hanya menyentuh rencana terminal, menurut `created_at`, dalam batch yang sama dengan jenis history lain.

Domain (`stackly-domain`): `RestartPlan` dengan state machine di bawah, `PlanDecision`, `PlanReason`, `PlanOutcome`, `FailureCode::DATA_MISSING` dan `RUNTIME_UNAVAILABLE`, transisi lifecycle `data_missing`, dan `OperationKind::DaemonRestart` (`resumable` false, `interrupt_policy` `CloseInterrupted`, resource `Manager`). Penulisan rencana memakai method `Registry` baru yang berjalan di thread registry dalam satu `BEGIN IMMEDIATE` dan dipanggil lewat `commit_guarded` dengan guard baru `write_plan_guard` (`supervisor/plan.rs`): ia menolak kalau ada rencana terbuka lain (`PlanRefusal::OpenPlan`) atau digest yang dihitung ulang dari registry di transaksi itu berbeda (`PlanRefusal::DigestChanged`).

### State rencana

```text
stopping ──semua stop_restore berhenti──▶ handed_over ──daemon baru, boot sama──▶ restoring ──semua outcome terisi──▶ finished
   │                                          │
   ├──stop gagal──▶ aborted                   └──boot baru──▶ expired
   └──daemon crash, boot sama──▶ restoring (AC-14)
```

Kemajuan setelah daemon lama keluar dicatat oleh rencana, bukan oleh operasi: operasi `daemon_restart` selesai `succeeded` di `handed_over` (atau `failed` di `aborted`) dan tidak pernah dibuka lagi. Operasi `daemon_restart` yang ditemukan `running` oleh `recover` spec 0007 (daemon crash saat `stopping`) ditutup `failed { interrupted }` seperti `CloseInterrupted` lain, dan rencananya diteruskan rekonsiliasi (AC-14).

`outcome` diisi: `keep` yang diadopsi `adopted`; `keep` dengan `respawn_pending` yang respawn nya `ready` `adopted`; `keep` yang menjadi `unknown` `failed` dengan `failure_code` `identity_unproven`, apa pun blockernya (`identityUnproven` atau `dataInUse`); `keep` lain yang lifecycle akhirnya bukan `running` `failed` dengan `lastFailure.code`; `stop_restore` yang `ready` `restored`; `stop_restore` yang gagal start `failed` dengan `lastFailure.code`; instance yang sudah dihapus atau `data_missing` `skipped`; `stop_restore` yang desired nya `stopped` saat restore akan berjalan `skipped` tanpa start, baik dibaca di langkah 10 (lifecycle `stopped` dengan desired `stopped`) maupun ditemukan start `PendingKind::Restore` di bawah lock (spec 0031 *Antrean start*, start sistem memeriksa desired), misalnya karena stop pengguna, Stop All, Turn Off, atau rotasi sesi spec 0031 lebih dulu. Pada `aborted` dan `expired` outcome yang kosong diisi `skipped`. Pada `aborted` ini juga berlaku untuk instance yang dinyalakan lagi sesudah abort (langkah 7): start ulang itu berjalan setelah rencana terminal dan tercatat di operasi `start_instance` nya sendiri, tidak di rencana.

### Alur restart terencana

1. `daemon.shutdown { mode: restart, expectedPlanDigest?, force }` dari client (konteks `launchd`), dengan `idempotencyKey` di envelope request (spec 0006 dan 0007), wajib untuk `mode: restart`: tanpa key daemon menolak `operation.idempotency_key_required` sebelum membaca apa pun. Konteks `manual` tetap `daemon.restart_unavailable` (spec 0015). Untuk `mode: restart` method ini adalah method yang memulai operasi: lewat `ipc::operation::replay_operation` lalu `start_operation`, sehingga request yang diulang dengan key yang sama mendapat reply pertama (dengan `planId` dari input operasi). `stop` tidak berubah dan tidak butuh key.
2. Quiesce yang sudah menyala (restart lain sedang berjalan): `daemon.restart_pending`. Lalu daemon menghitung rencana (fungsi yang sama dengan preview). Digest tidak cocok: `daemon.restart_plan_changed`, tanpa quiesce.
3. Quiesce dinyalakan di engine (flag di samping `check_phase`), lalu daemon menunggu operasi lain sampai `RESTART_QUIESCE_WAIT`. Masih ada yang menghalangi tanpa `force`: `daemon.restart_busy` dan quiesce dicabut; operasi `daemon_restart` belum dibuat, jadi tidak ada baris operasi `failed`. Sesudah penantian itu operasi `daemon_restart` dimulai (`Origin::Request`, resource `Manager`, lock key setiap instance `stop_restore`, `BusyPolicy::Queue`, karena start yang masih berjalan bisa memegang lock instance itu). Kalau operasi itu belum menulis rencana dalam `RESTART_QUIESCE_WAIT` ditambah `PREPARE_GRACE` (misalnya masih antre di lock instance), daemon membatalkannya (`operation.cancel`), menjawab `daemon.restart_busy` (dengan `items` yang bisa kosong, karena yang menahan adalah lock, bukan operasi penghalang), dan mencabut quiesce; baris operasi `daemon_restart` yang `cancelled` tetap ada. Jadi reply bisa datang paling lama sekitar 70 detik setelah request (30 detik penantian ditambah 40 detik penantian rencana tertulis).
4. `commit_guarded` menulis rencana `stopping` plus semua baris instance. Guard menolak kalau ada rencana terbuka lain (`daemon.restart_pending` dengan `planId` rencana itu, misalnya rencana yang masih `restoring`) atau digest yang dihitung ulang berbeda (`daemon.restart_plan_changed`); keduanya mencabut quiesce dan membuat operasi `failed`.
5. `ShutdownAccepted { serverInstanceId, mode, planId }` dikirim ke client. Ini satu satunya reply; semua kabar sesudahnya lewat event `daemon.restart_plan`.
6. Di dalam operasi yang sudah memegang lock key, instance `stop_restore` dihentikan dengan memanggil jalur stop supervisor langsung (bukan operasi `stop_instance` anak), paling banyak `RECONCILE_START_CONCURRENCY` bersamaan, dengan strategi stop provider. `StopRequest` mendapat `intent: StopIntent` (`User`, `Reconcile`, `PlannedRestart`); `PlannedRestart` melewati ketiga penulisan desired di `stop.rs` dan pengecekan lease environment (seperti `system`). `stopped_at` ditulis per instance.
7. Stop yang gagal (timeout stop provider, atau error lain seperti instance yang sudah menjadi `unknown`, atau rencana yang tidak bisa disimpan setelah sebuah stop): stop yang belum dimulai tidak dijalankan, stop yang sedang berjalan diselesaikan, lalu rencana `aborted` dengan `closeReason` `stop_timeout` untuk semua penyebab itu, quiesce dicabut, dan operasi `failed` dengan error stop pertama; daemon tetap jalan. Instance yang `stopped_at` nya terisi diantre sebagai `PendingKind::Restore` dan dinyalakan lewat operasi sistem `start_instance` (`Origin::Background`, `StartTrigger::Background`, tanpa dialog Keychain) segera setelah operasi `daemon_restart` final, bukan di dalam operasi itu: langkah jurnal sebuah operasi tidak bisa diulang per instance, dan lock key nya baru lepas saat operasi final. Pengecualian: kalau rencana `aborted` itu sendiri gagal disimpan, start ulang ini tidak terjadi (lihat *Edge cases*). Penyebab persis abort tidak ada di rencana (`RestartPlanView` tidak membawa `operationId`); ia ada di error operasi `daemon_restart` yang `failed` di `operation.list` dan di log daemon.
8. Semua berhenti: rencana `handed_over`, operasi `succeeded`, event, lalu urutan berhenti spec 0006 dengan reason `restart_requested` dan exit 75. Tanpa instance `stop_restore`, langkah 6 kosong.
9. Daemon baru: setelah `recover` spec 0007 dan sebelum langkah 3 rekonsiliasi, rencana terbuka dibaca. Boot sama: `restoring`, `server_instance_id_after` diisi. Boot baru: `expired`.
10. Rekonsiliasi berjalan biasa, ditambah dua aturan untuk instance `stop_restore` di rencana `restoring` yang `outcome` nya masih kosong: lifecycle `stopping` dan `Proven` mendapat `Then::PlannedStop` (selesaikan stop dengan `PlannedRestart`, lalu restore); lifecycle `stopped` dengan desired `running` mendapat `PendingKind::Restore`, yang dijalankan setelah `OperationEngine::start()` bersama daftar "jalankan lagi" sebagai operasi sistem `start_instance` (`Origin::Background`, pemicu `planned_restart`, `StartTrigger::Background`, tanpa dialog Keychain). `reconciled_at` diisi di phase `finished`.
11. Setiap perubahan outcome menerbitkan `daemon.restart_plan`; saat semua outcome terisi rencana `finished`.

### API surface

| Method atau event | Arah | Input | Output | Recovery | Error |
| --- | --- | --- | --- | --- | --- |
| `daemon.restart_preview` (baru) | client ke daemon | `RestartPreviewParams {}` | `RestartPreview { digest, launchContext, supervisorAvailable, instances: [{ instanceId, name, provider, lifecycle, decision, reason }], blockingOperations: [{ operationId, kind, resumable }] }` | `recovery allowed` | `daemon.shutting_down` |
| `daemon.shutdown` (diubah) | client ke daemon | `ShutdownParams { mode, expectedPlanDigest: Option<String>, force: bool }`, field baru `#[serde(default)]`; `idempotencyKey` di envelope request, wajib untuk `mode: restart` (CLI dan GUI mengirim key baru untuk setiap konfirmasi) | `ShutdownAccepted { serverInstanceId, mode, planId: Option<Uuid> }` | `recovery allowed` | `daemon.restart_unavailable`, `daemon.restart_plan_changed`, `daemon.restart_busy`, `daemon.restart_pending`, `daemon.shutting_down`, `operation.idempotency_key_required` |
| `daemon.info` dan handshake (diubah) | daemon ke client | | `DaemonInfo.restart: Option<RestartPlanView>`: rencana terakhir yang dilihat daemon run ini (diambil alih saat start, lihat langkah 9, atau dibuat di run ini), selama rencana itu terbuka atau `finishedAt` nya kurang dari `RESTART_SUMMARY_TTL` (10 menit) lalu. Rencana terminal yang ditutup daemon sebelumnya tidak dibaca lagi dari registry | | |
| `daemon.restart_plan` (event baru, daemon event) | daemon ke subscriber | | `RestartPlanView { id, state, createdAt, handedOverAt, reconciledAt, finishedAt, closeReason, instances: [{ instanceId, name, decision, reason, stoppedAt, readyAt, outcome, failureCode }] }` | | |
| `daemon.reconciling` (diubah) | daemon ke subscriber | | `findings` | | |
| `stackly daemon restart` (diubah) | CLI | `--yes`, `--force`, `--no-wait-instances`, `--json` | `{ previousServerInstanceId, agent, info, plan: RestartPlanView \| null, durations: { stopping, daemonGap, restoring } \| null }` | | exit 0, 2, 3, 5, 7, 8, 9, 130 |
| `stackly daemon status` (diubah) | CLI | | `lastRestart: RestartPlanView \| null`, salinan `DaemonInfo.restart` | | |

Kode baru: `daemon.restart_pending`, `daemon.restart_plan_changed`, `daemon.restart_busy` (kategori `conflict`, exit 3), `cli.restart_aborted` dan `cli.restart_restore_failed` (`failed`, exit 7), warning `cli.restart_instance_unknown`. Kode yang dipakai ulang: `supervisor.data_unavailable`, `supervisor.executable_invalid`, `supervisor.runtime_unavailable`, `supervisor.endpoint_in_use`, `cli.confirmation_required` (exit 2), `cli.confirmation_declined` (exit 9), `cli.daemon_wait_timeout` (exit 8), `cli.interrupted` (exit 130). Failure code baru: `data_missing`, `runtime_unavailable`.

CLI: `daemon.restart_preview` lewat call control; `daemon.shutdown { mode: restart }` lewat timeout interaktif (`RequestTimeout::Interactive`, 200 detik, bukan `CallSpec::control()` yang 10 detik), karena reply bisa datang sampai sekitar 70 detik (*Alur* langkah 3), dan selalu dengan `CallSpec.idempotency_key` berisi key baru (UUID v7) untuk setiap konfirmasi. Konfirmasi ulang setelah `daemon.restart_plan_changed` membawa digest lain, jadi memakai key baru juga (key lama dengan params lain ditolak `operation.idempotency_key_reused`, spec 0007). Tanpa key, daemon launchd menolak restart dengan `operation.idempotency_key_required` (exit 2), jadi milestone 4 wajib mengirimnya; sampai itu ada, `stackly daemon restart` ke daemon launchd gagal. Langganan event `daemon.` yang sudah dibuka penantian restart spec 0015 kini juga membaca `daemon.restart_plan` (sebelumnya semua event diabaikan) dan, setelah daemon baru menjawab, CLI berlangganan lagi ke daemon baru dan membaca `DaemonInfo.restart` untuk keadaan yang mungkin terlewat. GUI memanggil `daemon.shutdown { mode: restart }` dengan `CallOptions(timeout:)` sebesar anggaran interaktif yang sudah ada (`SecretsClient.interactiveTimeout`, 190 detik), bukan `ContractLimits.callTimeout` yang 10 detik, dan memperlakukan reply sampai sekitar 70 detik sebagai normal (sheet menampilkan progres "Waiting for operations to finish").

Keluaran rencana `aborted` (CLI dan GUI): `durations` null (hanya rencana `finished` yang punya tiga durasi), dan instance dibagi dari baris `stop_restore`, bukan dari `outcome` (semuanya `skipped`): `stoppedAt` terisi berarti "stopped, starting again", `stoppedAt` kosong berarti "did not stop, still running". CLI menulis `cli.restart_aborted` dengan dua daftar nama itu di teks manusia dan `plan` lengkap di `--json`; GUI menulis dua daftar yang sama di banner peringatan "Daemon restart aborted".

### GUI

- `SettingsPlaceholder` diganti `SettingsRoot` dengan satu tab General berisi satu bagian Daemon: baris "stacklyd" (subtitle "Connected over Unix socket · protocol N", "Disconnected · reconnecting", atau "Runs outside launchd, so it can't restart itself" untuk konteks `manual`) dan tombol Restart Daemon. Tidak ada tab lain sampai fitur 32.
- Tombol memanggil `daemon.restart_preview`, lalu sheet konfirmasi memakai komponen `Sheet` dan `Confirmation` yang ada: daftar "Keeps running" dan "Stops, then starts again" dengan alasan, daftar operasi yang menghalangi, dan tombol Restart. Restart mengirim `daemon.shutdown { mode: restart, expectedPlanDigest }` dengan `idempotencyKey` baru untuk setiap konfirmasi. Rencana tanpa `stop_restore` tetap menampilkan sheet dengan daftar `keep` saja. Jawaban `daemon.restart_plan_changed` memuat ulang preview di sheet yang sama; `daemon.restart_busy` menampilkan operasi yang menghalangi di sheet.
- `ShellPresentation` mendapat state `restarting`: banner "stacklyd is restarting" dengan jumlah instance yang tetap jalan dan yang berhenti; `InstanceStore` menandai baris `stop_restore` "Stopped for daemon restart" dari `RestartPlanView` (bukan dari lifecycle saja). Setelah reconnect, banner ringkasan dari `DaemonInfo.restart` dan event `daemon.restart_plan` sampai rencana terminal; lalu toast lewat `FeedbackCenter`. Rencana `aborted` atau `expired` memakai banner peringatan dengan alasannya. Karena `DaemonInfo.restart` hanya membawa rencana yang dilihat daemon run itu, app yang tersambung ke daemon lain (daemon restart lagi, atau crash, dalam 10 menit setelah rencana terminal) atau tersambung lebih dari 10 menit sesudahnya tidak menampilkan banner ringkasan maupun toast untuk rencana itu; itu perilaku yang diharapkan, bukan state yang hilang.
- Aturan interaksi proyek berlaku: kontrol pointer native, tanpa shortcut atau identifier UI test tambahan.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| preview | `decision`, `reason` | lifecycle instance dan `HANDOVER_PROVEN` dicocokkan dengan ID provider penuh (`ProviderRevisionKey.provider`, misalnya `official/postgresql`) dan line dari `ProviderRevisionKey.revision`; recovery mode dari subsystem registry |
| preview | `digest` | sha256 dari baris `instanceId:decision:reason` yang diurutkan menurut `instanceId`, dipisah newline |
| preview | `blockingOperations` | operasi `queued` dan `running` di `OperationEngine`; menghalangi kalau `OperationKind::resumable()` false dan `interrupt_policy()` bukan `CloseInterrupted` |
| preview | `launchContext` | `DaemonInfo.launch.context` (spec 0015) |
| rencana | `boot_id` | port platform yang dipakai `gather_evidence` (spec 0010) |
| rencana | `name` per instance | nama instance saat rencana ditulis |
| rencana | `server_instance_id_before`, `_after` | `DaemonInfo.serverInstanceId` daemon lama dan baru |
| CLI | `durations.stopping` | `handed_over_at` dikurangi `created_at` |
| CLI | `durations.daemonGap` | `reconciled_at` dikurangi `handed_over_at` |
| CLI | `durations.restoring` | `finished_at` dikurangi `reconciled_at` |
| CLI | warning `cli.restart_instance_unknown` | baris dengan `reason` `identity_unproven` dan `failure_code` `identity_unproven` |
| CLI, GUI | `idempotencyKey` restart | UUID v7 baru yang dibuat client untuk setiap konfirmasi |
| CLI | `lastRestart` di `daemon status` | `DaemonInfo.restart` daemon yang sedang jalan |
| CLI, GUI | daftar "did not stop" dan "starting again" saat `aborted` | baris `stop_restore` dengan `stoppedAt` kosong atau terisi di `RestartPlanView` |
| CLI | `durations` | null kecuali rencana `finished` |
| preview | `provider` per instance | nama pendek provider yang di pin instance (`ProviderRevisionKey.provider().name()`, misalnya `postgresql`), hanya untuk tampilan; pencocokan handover memakai ID penuh |
| daemon | `DaemonInfo.restart` | `RestartPlanView` terakhir yang diterbitkan daemon run ini sebagai `daemon.restart_plan` (disimpan di memori), disaring dengan `RESTART_SUMMARY_TTL` terhadap `finishedAt` |
| GUI toast | N dan M | jumlah outcome `adopted` dan `restored` di `RestartPlanView` |
| rekonsiliasi | `findings` | pemeriksaan data dan runtime di *Pemeriksaan rekonsiliasi tambahan* |
| evidence AC-17 | waktu inventory | jam monoton test: dari handshake pertama yang berhasil ke daemon baru sampai `daemon.reconciling finished` diterima dan `instance.list` memuat 10 instance dengan lifecycle selain `unknown` |

### Konstanta

| Nama | Nilai | Tempat |
| --- | --- | --- |
| `RESTART_QUIESCE_WAIT` | 30 detik | `stackly-core` supervisor config |
| `RESTART_SUMMARY_TTL` | 10 menit | `stacklyd` |
| `PREPARE_GRACE` | 10 detik, tetap (tidak lewat `SupervisorConfig`); penantian rencana tertulis adalah `RESTART_QUIESCE_WAIT` ditambah nilai ini | `stacklyd` `ipc/restart.rs` |
| `DAEMON_RESTORE_WAIT` | 10 menit | CLI `commands/daemon/wait.rs` |
| `RetentionPolicy.restart_plans` | 20 | `stackly-domain` retention |
| `RECONCILE_START_CONCURRENCY` | 4 (sudah ada) | dipakai juga untuk stop rencana |

### Key invariants

- Desired instance tidak pernah diubah oleh rencana restart, quiesce, abort, atau expiry.
- Restore tidak pernah menyalakan instance di boot baru.
- Proses yang identitasnya tidak terbukti tidak pernah disinyal, oleh jalur mana pun di spec ini.
- Init tidak pernah berjalan di luar create, jadi data yang hilang tidak pernah diganti data kosong; start yang menolak data hilang tidak membuat folder apa pun.
- Paling banyak satu rencana terbuka (unique index parsial); rencana baru ditulis lewat `commit_guarded` (guard `write_plan_guard`), dan perubahan state sesudahnya lewat `commit` biasa (`WriteOp::UpdateRestartPlan`) oleh daemon yang memegang rencana itu, karena aturannya tidak dibaca dari registry.
- Setiap instance dihentikan paling banyak sekali per rencana.
- Restart terencana tidak pernah mengirim `SIGKILL` dan tidak pernah memilih port baru.
- `HANDOVER_PROVEN` hanya berisi provider dengan evidence record native PASS.

### Security model

Semua method baru mengikuti cek peer uid BR-012 yang sama dengan `daemon.shutdown`; tidak ada peran lain. Rencana dan event tidak membawa secret; `failure_code` dan `findings` hanya kode. Pemeriksaan data dan runtime hanya membaca path milik instance dari `InstancePaths` dan folder install runtime dari registry.

### Configuration required

Tidak ada env var baru.

### Edge cases

- Dua client meminta restart bersamaan: yang kedua mendapat `daemon.restart_pending` selama quiesce, atau `daemon.shutting_down` setelah handover.
- `daemon.shutdown { mode: stop }` (atau `agent_off` spec 0031) selama quiesce atau rencana `stopping`: `daemon.restart_pending`; setelah `handed_over`: `daemon.shutting_down`.
- Instance dihapus antara preview dan konfirmasi: digest berubah, `daemon.restart_plan_changed`.
- Instance dihapus saat rencana `restoring`: outcome `skipped`.
- Paket tanpa `compatibility.dataMarker` (misalnya NATS) dengan folder data kosong, karena server belum pernah jalan atau belum menulis apa pun: bukan data hilang, jadi rekonsiliasi tidak memberi `data_missing` dan start, respawn, serta restore berjalan biasa. Folder yang tidak ada tetap `data_missing` untuk semua paket.
- Instance `stop_restore` yang desired nya menjadi `stopped` sebelum restore berjalan (stop pengguna, `instance.stop_all`, `agent_off`, atau rotasi sesi spec 0031): outcome `skipped` tanpa start, jadi rencana tetap bisa `finished`.
- Operasi `daemon_restart` yang masih antre di lock instance (misalnya start panjang) setelah `RESTART_QUIESCE_WAIT` ditambah `PREPARE_GRACE`: dibatalkan, jawabannya `daemon.restart_busy`, quiesce dicabut, tidak ada yang dihentikan.
- Stop rencana gagal karena instance menjadi `unknown`, `retained`, atau `initializing` di antara preview dan stop: rencana `aborted` dengan `stop_timeout` seperti timeout (AC-11), karena `closeReason` hanya punya satu nilai untuk abort.
- Registry gagal menyimpan rencana `aborted` (`persist_plan` error sesudah sebuah stop gagal): operasi tetap `failed` (dengan error registry, bukan error stop) dan quiesce dicabut, tetapi di kode sekarang `queue_restore` terlewat, jadi instance yang sudah dihentikan tetap `stopped` dengan desired `running` selama daemon ini jalan, dan rencana tetap `stopping` di registry (restart berikutnya ditolak `daemon.restart_pending`) sampai daemon berikutnya di boot yang sama meneruskannya sebagai `restoring` (AC-14). Event `daemon.restart_plan` terakhir dan `DaemonInfo.restart` tetap `stopping`, jadi CLI menunggu sampai `DAEMON_RESTORE_WAIT` lalu keluar 8 `cli.daemon_wait_timeout`, dan banner `restarting` di GUI bertahan sampai daemon berikutnya. Ini risiko yang dicatat: `/test` menguji jalur ini dengan registry yang menolak tulis, dan review menilai apakah start ulang harus tetap diantre walau rencana tidak tersimpan.
- CLI di Ctrl+C saat menunggu restore: exit 130 `cli.interrupted`, rencana tetap berjalan di daemon.
- GUI ditutup selama jendela disruption: tidak ada efek; app berikutnya membaca `DaemonInfo.restart`.
- launchd tidak menyalakan daemon (binary diganti): relaunch spec 0015, lalu restore seperti biasa.
- Restore gagal karena Keychain terkunci: outcome `failed` dengan `failure_code` dari blocker `needsSecret`, tanpa dialog (pemicu background).

### Critical test scenarios

AC-9 sampai AC-14 memakai helper baru `RestartHarness` di `apps/daemon/tests/common`, yang di satu `TestRoot` memegang lock daemon, menjalankan `server::start` dengan `LaunchContext::Launchd`, menunggu `handle.stop_requested()`, memanggil `handle.shutdown(RestartRequested)`, membiarkan test membaca registry di antara dua daemon, lalu menjalankan `server::start` lagi (pola `apps/daemon/tests/server/shutdown.rs`).

| Test | Jenis | Membuktikan |
| --- | --- | --- |
| `apps/daemon/tests/postgresql/reconcile.rs` `closing_every_client_keeps_three_postgresql_instances_unchanged` | native PostgreSQL | AC-1 |
| `StacklyUI` integrasi `ShellReopenTests` dengan fake | fixture | AC-1 |
| `postgresql/reconcile.rs` `sigkill_leaves_one_postmaster_per_data_folder_and_every_sentinel_intact` | native | AC-2 |
| `crates/core/tests/supervisor_start/pid_reuse.rs` `a_pid_owned_by_another_executable_is_never_signalled_and_becomes_unknown_with_evidence` (lock dipegang: `dataInUse`), `a_pid_owned_by_another_executable_with_a_free_lock_is_gone_and_never_signalled` (lock bebas: `stopped`), plus varian recheck, stop, force stop, dan start (operasi `failed` `supervisor.identity_unproven`); varian rencana restart (PID asing di instance `stop_restore`) untuk `/test` | fixture | AC-3 |
| `reconcile.rs` `missing_data_fails_the_instance_without_init_start_or_new_folders` | fixture plus native PostgreSQL | AC-4 |
| `reconcile.rs` `an_empty_data_folder_counts_as_missing_only_for_a_package_with_a_data_marker` (paket dengan marker: `data_missing`; paket tanpa marker: start biasa; folder yang tidak ada: `data_missing` untuk keduanya) | fixture plus native NATS | AC-4 |
| `reconcile.rs` `a_missing_runtime_executable_keeps_the_adopted_process_and_blocks_the_next_start` | fixture | AC-5 |
| `apps/daemon/tests/restart_plan.rs` `a_foreign_listener_fails_the_restore_with_endpoint_in_use` | fixture | AC-6 |
| `crates/core/src/supervisor/handover.rs` unit test tabel keputusan dan pencocokan line | unit | AC-7 |
| `apps/daemon/tests/native_launchd.rs` `postgresql_and_nats_survive_kickstart_bootout_and_planned_restart` | native launchd, setelah persetujuan | AC-8 |
| `apps/daemon/tests/restart_plan.rs` happy path, quiesce, busy (tanpa baris operasi `daemon_restart`), abort (start ulang lewat operasi sistem sesudah operasi `daemon_restart` final, outcome `skipped`), digest di preview dan di transaksi, expiry boot baru, crash di `stopping`, `stop_restore` yang desired nya `stopped` sebelum restore (outcome `skipped`, rencana `finished`), request tanpa `idempotencyKey` (`operation.idempotency_key_required`) | fixture, `RestartHarness`, `handover_proven` kosong | AC-9 sampai AC-14 |
| `restart_plan.rs` rencana `aborted` yang gagal disimpan (registry menolak tulis): operasi `failed`, quiesce dicabut, perilaku `queue_restore` dicatat (*Edge cases*) | fixture, untuk `/test` | AC-11 |
| `restart_plan.rs` `DaemonInfo.restart` hanya membawa rencana daemon run itu: terbuka, terminal di bawah 10 menit, kosong setelah 10 menit (jam test), kosong di daemon berikutnya untuk rencana terminal | fixture | AC-15, AC-16 |
| `apps/cli/tests/daemon_restart_plan.rs` prompt, `--yes`, `--json`, abort, restore gagal, warning `unknown`, exit code | fixture | AC-15 |
| `StacklyUI` `SettingsDaemonTests`, `RestartBannerTests`, snapshot sheet dan banner | fixture | AC-16 |
| `apps/daemon/tests/postgresql/inventory.rs` `ten_postgresql_instances_are_inventoried_within_ten_seconds` | evidence native, `SpawnedDaemon` | AC-17 |
| `cargo xtask codegen --check`, fixture decode test | kontrak | AC-18 |
| `crates/storage` test migrasi dan prune | integrasi | AC-19 |

## Build plan

Journey: setiap milestone menutup satu potongan perjalanan pengguna lengkap dengan state, error, dan polish, mulai dari "daemon mati tanpa rencana" lalu "restart terencana", baru kemudian GUI dan bukti kinerja.

**Milestone 1: kontrak dan data**

1. Kontrak di `stackly-protocol`: `RestartPreview`, `RestartPlanView`, `ShutdownParams` dan `ShutdownAccepted` yang diperluas, `DaemonInfo.restart`, event `daemon.restart_plan`, `ReconcileFinding` dan `DaemonReconciling` tanpa `Copy`, kode baru, fixture, bump mayor, `cargo xtask codegen`. Satisfies **AC-18**.
2. Domain: `RestartPlan` dan state machine nya, transisi `data_missing`, failure code baru, `StopIntent`, `OperationKind::DaemonRestart`, `RetentionPolicy.restart_plans`, unit test. Satisfies **AC-4**, **AC-9**, **AC-13**, **AC-19**.
3. Migrasi `daemon_restart_plan`, method `Registry` untuk rencana, guard `write_plan_guard` (nama awal `NoOpenRestartPlan`), `History::RestartPlans` di prune. Satisfies **AC-19**.

**Milestone 2: daemon mati tanpa rencana**

4. Pemeriksaan data di `check_instance_folder` (sebelum `ensure_instance_run_dir`) dan di rekonsiliasi, pemeriksaan runtime di rekonsiliasi, `findings`, pemetaan baru di `error_failure_code`. Satisfies **AC-4**, **AC-5**.
5. Test fixture PID reuse untuk rekonsiliasi, recheck, stop, dan rencana; test listener luar saat respawn. Satisfies **AC-3**, **AC-6**.
6. Test native PostgreSQL: tutup semua client, `SIGKILL` dengan sentinel, data hilang. Satisfies **AC-1**, **AC-2**, **AC-4**.

**Milestone 3: restart terencana di daemon**

7. `handover.rs` dengan `HANDOVER_PROVEN` (fake saja dulu), pencocokan line, `SupervisorConfig.handover_proven`, plus `daemon.restart_preview`. Satisfies **AC-7**.
8. Quiesce di `OperationEngine::begin`, operasi `daemon_restart` (replay, quiesce lalu penantian sebelum operasi dimulai, `restart_busy`, `force`, digest dua kali, penulisan rencana, stop `PlannedRestart` langsung di bawah lock key, abort dengan start ulang lewat operasi sistem sesudah operasi final, handover ke urutan berhenti), dan `RestartHarness`. Satisfies **AC-9**, **AC-10**, **AC-11**, **AC-12**.
9. Daemon baru: baca rencana, `restoring` atau `expired`, `Then::PlannedStop`, `PendingKind::Restore`, outcome, `finished`, `DaemonInfo.restart`, event. Satisfies **AC-6**, **AC-9**, **AC-13**, **AC-14**.
10. Setelah persetujuan kamu: test native launchd PostgreSQL 17, 18, dan NATS, lalu entri `official/postgresql` dan `official/nats` di `HANDOVER_PROVEN` dengan ID evidence. Satisfies **AC-8**.

**Milestone 4: CLI**

11. `stackly daemon restart` dengan preview, prompt, `--yes`, `--force`, `--no-wait-instances`, `idempotencyKey` baru di setiap `daemon.shutdown { mode: restart }` (wajib sejak milestone 3), timeout interaktif, pembacaan `daemon.restart_plan`, durasi, exit code dan warning; `daemon status` dengan `lastRestart` dari `DaemonInfo.restart`; completion dan man page ikut. Satisfies **AC-15**.

**Milestone 5: GUI**

12. `SettingsRoot` dengan bagian Daemon, sheet konfirmasi (restart dengan `idempotencyKey` baru), banner `restarting` dan ringkasan selama `DaemonInfo.restart` atau event membawa rencana, tanda baris instance, toast, snapshot. Satisfies **AC-16**, **AC-1** (test integrasi Swift).

**Milestone 6: bukti**

13. Evidence run native 10 PostgreSQL untuk NFR-007 dan angka `daemonGap`. Satisfies **AC-17**.
14. Bagian baru di `docs/manual.md` untuk record manual AC-1 (tutup dan buka app dengan tiga PostgreSQL) dan restart dari tombol Settings. Satisfies **AC-1**, **AC-16**.

## Migration plan

**Strategy**: no migration needed (hanya satu migrasi tambahan; tabel baru mulai kosong dan tidak ada data lama yang diubah).
**Phases**:
1. Migrasi rencana berjalan saat registry dibuka, seperti migrasi lain (spec 0005).
**Rollback**: binary lama menolak registry dengan schema lebih baru (aturan spec 0005); rollback berarti restore backup registry, sama dengan migrasi lain.
**Risks**: tidak ada data yang ditransformasi. Protokol naik mayor, jadi app dan daemon harus diperbarui bersama (sudah begitu karena keduanya ada di bundle yang sama).

## Consequences

**Positive**:

- Restart daemon tidak mengganggu PostgreSQL dan NATS sama sekali; gangguan hanya untuk provider yang belum terbukti, dan itu terlihat sebelum terjadi.
- Data yang hilang tidak pernah berubah menjadi cluster kosong baru, menutup celah RISK-03 dari sisi storage.
- Rencana di registry membuat restart tahan crash di tengah jalan dan bisa dijelaskan sesudahnya.

**Negative / tradeoffs**:

- Quiesce membuat aksi pengguna ditolak sampai 30 detik selama restart; create atau operasi lain yang tidak resumable dan masih berjalan membuat restart ditolak (install runtime yang resumable tidak menghalangi, ia dilanjutkan daemon baru).
- `HANDOVER_PROVEN` adalah daftar di kode: provider baru (fitur 36 sampai 43) tidak otomatis tanpa gangguan, setiap provider butuh test native launchd dan perubahan kode.
- Instance `stop_restore` tampil `stopped` dengan desired `running` selama jendela; spec 0031 memakai kombinasi itu sebagai keadaan istirahat yang sah, jadi keduanya harus tetap selaras.
- Event baru memaksa protokol naik mayor.
- `DaemonInfo.restart` hanya mengingat rencana yang dilihat daemon run itu, di memori: kalau daemon restart lagi atau crash dalam 10 menit setelah rencana terminal, ringkasan rencana itu hilang dari GUI (banner dan toast milestone 5) dan dari `stackly daemon status`, walau baris nya tetap ada di registry untuk Diagnostics nanti.
- Rencana `aborted` mengisi semua outcome dengan `skipped`, termasuk instance yang lalu dinyalakan lagi, jadi hasil start ulang sesudah abort dibaca dari lifecycle dan operasi `start_instance`, bukan dari rencana.

**Neutral**:

- Restart dari daemon konteks `manual` tetap tidak didukung.
- Marker data `Missing` pindah dari `data_incompatible` ke `data_unavailable`, mengubah sedikit perilaku spec 0025.
- Usulan update untuk spec lain: spec 0010 (pemeriksaan data dan runtime, `Then::PlannedStop`, `PendingKind::Restore`), spec 0004 (transisi `data_missing`, `StopIntent`), spec 0007 (quiesce di `begin`, `DaemonRestart`), spec 0015 (`ShutdownParams`, timeout interaktif, penantian yang membaca rencana), spec 0025 (marker `Missing`), spec 0006 (method dan event baru), dibawa `/sync` atau `/architect (update)`.

## Follow-up

- [x] Spec 0031: memperluas `ShutdownAccepted` dari spec ini (bukan mengganti namanya menjadi `ShutdownReply`), menolak `agent_off` selama quiesce atau rencana `stopping`, dan memastikan antrean start nya tidak menyalakan instance `stop_restore` dua kali (restore spec ini atau antrean start, satu saja, dengan restore yang menang selama rencana `restoring`): diterapkan di spec 0031 (2026-11-02).
- [ ] Fitur 36 sampai 43: setiap provider baru menambah test native launchd survival sebelum masuk `HANDOVER_PROVEN`.
- [ ] Fitur 61: update aplikasi memakai `daemon.restart_preview` dan rencana yang sama untuk jendela restart.
- [ ] Bukti signed (spec 0015 AC-17, AC-19) tetap `BLOCKED` sampai identity ada; restart terencana di build signed diuji ulang saat itu.

## Rationale

Alasan dan pilihan lain: lihat [rationale.md](rationale.md).
