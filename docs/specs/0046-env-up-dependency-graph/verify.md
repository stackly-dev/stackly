# Verify: Env up & dependency graph · spec 0046 · updated 2026-11-13

_Langkah diturunkan dari acceptance criteria spec 0046. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27, dengan `export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`._

## Milestone 1: fondasi operasi anak

Langkah Build plan 1 sampai 3: engine operasi pengatur dan anak, domain dan storage (migrasi 15), lalu create dan start untuk Up. Protokol tidak berubah di milestone ini (`OperationSummary.parent` dan `coordinators` di `OperationStats` masuk di langkah 5).

### UI / manual

- [x] Buka `crates/domain/src/operation.rs` → `OperationKind::is_coordinator()` benar hanya untuk `environment_up`; `environment_up` tidak `resumable()` dan `interrupt_policy()` nya `CloseInterrupted`; `environment_down` tidak berubah (resumable, `Compensate`, milik spec 0047) → AC-3, AC-15
- [x] Buka `crates/core/src/operation/limits.rs` → `MAX_RUNNING_COORDINATORS` 4 dan `EngineLimits.max_coordinators`; buka `crates/core/src/operation/scheduler.rs` → `QueueEntry.coordinator` diturunkan dari jenis operasi, dispatcher memeriksa `coordinators < max_coordinators` untuk pengatur dan `running < max_running` untuk operasi lain, dan `finish_slot` mengurangi penghitung kelasnya sendiri → AC-3, AC-24
- [x] Buka `crates/storage/migrations/0015_environment_up.sql` → dua `ALTER TABLE` saja: `environments.last_up` (JSON objek, nullable) dan `operation_journal.parent_id` (UUID v7, nullable, tanpa FK) → AC-24, AC-10
- [x] Buka `crates/core/src/instance/create/binding.rs` → guard `register` membaca environment di dalam transaksinya, menulis `UpdateEnvironment` dengan revision yang baru dibacanya dan `InsertLease` environment `active` sesudah `InsertInstanceAt`; `instance.create` dari klien tidak pernah membuat `EnvironmentBinding` (`grep -rn with_binding apps/` kosong) → AC-7, AC-13
- [x] Buka `crates/core/src/supervisor/startup.rs` → `StartTrigger::Environment` diperlakukan seperti `User` di setiap cabang (caller percobaan pertama, tidak dilewati saat sesi berakhir, menulis desired `running`), dan `writes_manual_lease()` benar hanya untuk `User` → AC-13

### Commands

- [x] `cargo test -p stackly-domain --lib operation::tests` → `only_environment_up_is_a_coordinator`, daftar resumable tanpa `environment_up`, dan daftar `CloseInterrupted` dengan `environment_up` lulus → AC-3, AC-15
- [x] `cargo test -p stackly-core --test operation_engine coordinator` → 7 test lulus: tiga pengatur dengan `max_running` 2 masing masing memulai satu anak dan menunggunya lewat `wait_final` tanpa deadlock; pengatur kelima `queued` selagi empat berjalan dan `running` 0 sementara operasi biasa tetap jalan; anak mencatat `parent`, pemicu induk, tanpa method, fingerprint, dan idempotency key, dan ditolak `RestartPending` selama quiesce (operasi background tidak); `wait_final` selesai di `succeeded`, `failed`, dan `needs_recovery`, `None` untuk operasi yang tidak ada, dan `ShuttingDown` saat shutdown; `recover` menutup pengatur yang terputus beserta anak `create_instance` dan `start_instance` yang antre (`failed`, alasan `interrupted`, `operation.interrupted`) dan membangun ulang anak `install_runtime`; pengatur yang masih antre ditutup bersama anak antrenya → AC-3, AC-15, AC-24
- [x] `cargo test -p stackly-core --test operation_engine` → semua test engine lama tetap lulus (89 sebelum milestone ini); satu test restart memakai `import_environment` sebagai jenis tanpa factory menggantikan `environment_up`, yang kini pengatur dan langsung berjalan di slotnya sendiri → AC-24
- [x] `cargo test -p stackly-storage --test open environments` → fixture v15 membaca `lastUp` (state `failed`, service `db` `failed` tahap `start` dengan `supervisor.readiness_timeout`), `last_operation_id`, `updated_at`, dan `parent` journal anak; `last_up` yang bukan objek JSON ditolak skema → AC-10, AC-24
- [x] `cargo test -p stackly-storage --test open` → 78 test lulus, termasuk fingerprint migrasi 15, fixture beku v15 (`STACKLY_WRITE_REGISTRY_FIXTURE`), dan kolom baru di model data → AC-24
- [x] `cargo test -p stackly-storage --test registry conformance` dan `cargo test -p stackly-testkit` → `registry_conformance` lulus pada fake dan SQLite: `journal_parent_round_trips`, `environment_up_report_round_trips` (laporan Up, operasi terakhir, `dependency_conditions`, dan stempel tulis), dan `delete_created_member` (`DeleteInstanceCreatedBy` menghapus baris member dan lease instance itu, revision environment naik satu, member dan lease lain tetap) → AC-4, AC-7, AC-12, AC-13
- [x] `cargo test -p stackly-core --test create_operation bound` → 4 test lulus: create terikat menulis instance, member `dedicated` dengan kondisinya, dan satu lease environment yang sudah ada saat init berjalan, desired tetap `stopped`; create terikat yang init nya gagal menghapus hanya instance, member, dan lease miliknya, sedangkan instance pengguna bernama mirip, member `shared` dan lease nya, dan runtime utuh; environment yang hilang gagal `env.not_found` tanpa folder; input journal tanpa ikatan sama persis dengan `encode_input`, dan create yang dibangun ulang dari input terikat tetap menulis member dan lease → AC-7, AC-12, AC-13
- [x] `cargo test -p stackly-core --test supervisor_start leases` → start ber pemicu `environment` menulis desired `running` tanpa lease apa pun, juga pada instance `running` yang desired nya `stopped`; test lease `manual` lama tetap lulus → AC-13
- [x] `cargo test -p stackly-core --test create_operation` dan `cargo test -p stacklyd` → golden dan test create serta start yang ada tetap lulus tanpa perubahan harapan → AC-7, AC-13
- [x] `cargo test -p stacklyd --test environment updated_at` → `updatedAt` summary sama dengan stempel baris `environments`, tidak bergeser oleh import yang tidak mengubah apa pun (di `env.import`, `env.list`, dan `env.get`), dan bergeser oleh import yang mengubah baris (perbaikan spec 0044 AC-14)
- [x] `cargo xtask check-deps`, `cargo xtask evidence check`, dan `cargo xtask codegen --check` → lulus tanpa perubahan file hasil generate
- [x] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0

### Catatan `/check verify` 2026-11-13 (sesi `chk-f46-m1`, kode `75784ccc`)

Verdict Milestone 1: **PASS** untuk bagian AC-3, AC-4, AC-7, AC-12, AC-13, AC-15, dan AC-24 yang menjadi lingkup milestone ini. Semua kotak di atas sudah `[x]` sebelum checker mulai (dicentang saat build); checker menjalankan ulang setiap langkah Commands kecuali suite penuh `cargo test -p stacklyd` (lead: 737 lulus di `main`), dan membaca lima file langkah manual.

Hasil checker: `stackly-domain --lib operation::tests` 11; `stackly-core --test operation_engine` 96 (tujuh test `coordinator::` ikut); `stackly-storage --test open` 78; `--test registry conformance` 3; `stackly-testkit` 43; `stackly-core --test create_operation` 31 (empat `bound::`); `--test supervisor_start leases` 8; `stacklyd --test environment` 12; `check-deps`, `evidence check`, `codegen --check` (24 file mutakhir, kontrak tidak berubah), dan `cargo check` Windows lulus.

Probe `stacklyd` asli (registry baru di skema 15): create, start, restart, dan stop oleh pengguna berjalan seperti sebelumnya; start pengguna tetap menulis satu lease `manual` `active`. `with_binding`, `StartRequest::environment`, dan `Origin::Child` belum dipakai kode non test di `apps/`, jadi klien tidak bisa mengirim ikatan.

Yang dibaca di kode: dispatcher menelusuri seluruh antrean dan memeriksa slot per kelas, jadi kelas yang penuh tidak menahan kelas lain; `after_commit` membangunkan `wait_final` di setiap perubahan state yang terlihat dan `shutdown` membangunkannya sekali; `rebuild` create menolak input ber `binding` yang tidak terbaca (tidak pernah membuat instance tanpa ikatannya); test lama hanya berubah di tiga tempat kecil (`parent: None`, jenis tanpa factory diganti `import_environment`, pemecahan helper harness), golden tidak berubah.

Temuan, semuanya rendah atau catatan:

1. `env.plan_changed` sudah bisa dihasilkan core (`BindingFault::MemberTaken`) tetapi belum ada di katalog protokol; tidak terjangkau klien di milestone ini, harus masuk bersama langkah 5.
2. `orphaned_child` menganggap induk `needs_recovery` masih aktif. Untuk `environment_up` itu tidak terjadi (`CloseInterrupted`), tetapi pengatur `environment_down` spec 0047 resumable; anak create dan start yang antre di bawah induk `needs_recovery` akan dibangun ulang dan berjalan. Perlu diputuskan saat spec 0047 dibangun.
3. `wait_final` membalas `ShuttingDown` juga untuk operasi yang sudah final bila dipanggil sesudah shutdown mulai; cukup untuk Up, hanya perlu diingat pemanggil lain.

### Acceptance-criteria coverage

- AC-3 · bagian engine: kelas slot pengatur, batas 4, tiga pengatur tanpa deadlock; `BusyPolicy::Reject`, `LockKey::EnvironmentName`, dan `expected_revision` operasi Up dipasang handler di Milestone 2 (langkah 4 dan 6)
- AC-4 · bagian domain dan storage: `bind_member`, `set_member_conditions`, `begin_up`, `finish_up`, baca dan tulis `last_operation_id`, `last_up`, `dependency_conditions`; guard `accept` dan commit nya di langkah 4. `lock_sha256` di commit `accept` menunggu field `Environment.lock_sha256` dari spec 0045
- AC-7 · ikatan create, input journal dan `rebuild`, batch `register`, kompensasi; `environment::plan::draft_for` dan pemanggilan dari Up di langkah 4 dan 6
- AC-12 · kompensasi create hanya atas milik sendiri (fixture); Up sendiri tanpa kompensasi menunggu langkah 4
- AC-13 · bentuk lease, lease di batch `register`, start `environment` tanpa lease `manual` (juga cabang `no_op_with_desired` di `start_precheck` daemon, yang menerima pemicu); test klien putus, restart daemon, dan rotasi sesi di langkah 6
- AC-15 · bagian engine: `environment_up` `CloseInterrupted`, anak antre ditutup `recover`, install antre dibangun ulang; `lastUp.state` `interrupted` di `env.get` di langkah 6
- AC-24 · kelas slot, `Origin::Child` dan quiesce, `parent` di journal, `wait_final`, aturan `recover`, `EngineStats.coordinators` dan `max_coordinators`; `OperationSummary.parent` dan `operation.get_stats` di wire di langkah 5
- AC-10 · hanya kolom `last_up` dan bentuk `UpReport`; isi laporannya di langkah 4
- AC-1, AC-2, AC-5, AC-6, AC-8, AC-9, AC-11, AC-14, AC-16 sampai AC-23, AC-25 · belum, milik Milestone 2 sampai 4

## Milestone 2: Up di daemon

Langkah Build plan 4 sampai 6: core `environment::up` (precheck, guard `accept`, graph murni, driver dengan commit penutup, `UpBoard`), protokol 4.4 dengan `env.up`, lalu handler `stacklyd` dengan port `EnvPorts` dan tambahan `env.get` serta `env.list`. Tidak ada migrasi di milestone ini (migrasi terakhir tetap `0015`) dan `EventKind` tidak bertambah.

### UI / manual

- [x] Buka `crates/core/src/environment/up/graph.rs` → `next` fungsi murni tanpa IO dan tanpa jam: dependency yang berakhir bukan `ready` membuat dependent `skipped` `failed` dan merambat dalam satu panggilan, timer edge hanya berjalan sejak dependency `starting`, paling banyak `UP_START_CONCURRENCY` start dipilih menurut `order` → AC-8
- [x] Buka `crates/core/src/environment/up/accept.rs` → guard membaca environment dan instance di dalam transaksi, menolak `env.plan_changed` bila instance milik service hilang, `retained`, atau provider nya lain, dan menulis satu `UpdateEnvironment` per mutasi (revision tepat naik satu), member `shared` untuk `instanceRef`, `dependency_conditions`, lease `active` untuk setiap member yang alias nya ada di manifest (member `detached` tidak), dan `begin_up` → AC-4, AC-5, AC-13
- [ ] Buka `crates/core/src/environment/up/driver.rs` → driver hanya memulai anak lewat `UpChildren` dan menunggu lewat `wait_final`, tidak pernah memanggil `ProcessSupervision`, downloader, atau menulis file; cancel meneruskan `OperationEngine::cancel` ke anak dengan pemicu induk; anak start yang `queued` lebih dari `CHILD_QUEUE_TIMEOUT_SECONDS` dibatalkan dengan `resource.busy`; penolakan `daemon.restart_pending` menghentikan anak baru dan menandai sisanya `cancelled`; `ShuttingDown` dari `wait_final` berarti abandon tanpa commit penutup → AC-3, AC-8, AC-11, AC-12, AC-15
- [x] Buka `apps/daemon/src/ipc/env/children.rs` dan `apps/daemon/src/ipc/{runtime.rs,instance/create.rs,instance/mod.rs}` → install, create, dan start anak dibangun oleh `install_request`, `create_request`, dan `start_request` yang sama dengan handler `runtime.install`, `instance.create`, dan `instance.start`, dan dimulai lewat `start_child` dengan `Origin::Child`; tidak ada jalur install, create, atau start kedua (BR-023); `instance.create` dari klien tetap tanpa ikatan (`create_request(..., None)`) → AC-6, AC-7
- [x] Buka `apps/daemon/src/ipc/env/status.rs` → `lastUp` diambil dari `UpBoard` selama operasi `environment_up` itu aktif, dari kolom `last_up` sesudahnya, dan `running` tanpa operasi aktif tampil `interrupted`; urutan aturan *Alasan belum ready* 1 sampai 7 sama dengan spec → AC-15, AC-16

### Commands

- [x] `cargo test -p stackly-core --lib environment::up` → 11 test lulus: graph rantai, berlian, gagal merambat, `needs_secret`/`skipped`/`cancelled` dihitung gagal, timer edge, dependency yang menunggu tanpa timer, batas start, `UpBoard`, dan laporan `env.up_incomplete` → AC-8, AC-10
- [x] `cargo test -p stackly-core --test environment up` → 13 test lulus dengan engine nyata dan port palsu: Up mendaftarkan, membuat, memberi lease, dan menyalakan sesuai dependency; dependency gagal membuat dependent `skipped` dan Up `env.up_incomplete` dengan tahap `start`; Up kedua hanya mengerjakan sisanya tanpa lease kedua; timeout edge; cancel; `instanceRef` yang sudah jalan `ready` tanpa anak dengan satu lease dan baris instance tak berubah; `accept` gagal `env.plan_changed` tanpa tulisan; dua service satu artefak satu install dan install gagal menutup keduanya di tahap `install`; create `needs_secret` dengan reference nya; penolakan quiesce; precheck menolak manifest tidak valid, digest lain, dan reference kosong urut teks → AC-1, AC-2, AC-4, AC-5, AC-6, AC-8, AC-9, AC-10, AC-11
- [ ] `cargo test -p stacklyd --test env_up` → 6 test lulus dengan provider `fake`: penolakan tanpa mutasi (`env.manifest_invalid`, `env.plan_changed`, `secret.needs_secret`), rantai db, api, web naik berurutan dengan lease, laporan, endpoint ber port, dan `parent` di anak; create gagal di init membuat dependent `skipped`, Up `env.up_incomplete` (`ready` 1 dari 3, `stage` `create`), dan `env.get` memberi `up_failed` dan `dependency_not_ready`; Up kedua `resource.busy` dan cancel menulis `cancelled`; instance pinjaman satu lease `shared` tanpa restart; restart daemon di tengah Up memberi `lastUp` `interrupted` dan lease tetap → AC-1 sampai AC-5, AC-7, AC-8, AC-10, AC-11, AC-13, AC-15, AC-16
- [x] `cargo test -p stackly-protocol --no-fail-fast` → lulus, termasuk fixture `env/detail-bringing-up.json`, `env/detail-up-incomplete.json`, `ipc/request-env-up.json`, `ipc/frame-response-env-up.json`, `ipc/frame-error-env-up-incomplete.json`, `ipc/frame-error-env-plan-blocked.json`, penolakan `ipc/reject-env-up-digest-format.json`, enum terbuka baru dibaca `unrecognized`, dan katalog kode lima kode `env` baru dengan hint → AC-16, AC-23
- [x] `cargo xtask codegen --check` → lulus; `protocols/contract-versions.json` mencatat 4.4; `cargo xtask fuzz seed` tidak mengubah apa pun lagi → AC-23
- [ ] `cargo test -p stacklyd --no-fail-fast` dan `cargo test -p stackly-core --no-fail-fast` → lulus, golden `instance.*` dan `runtime.*` yang ada tidak berubah oleh pemecahan handler → AC-7, BR-023
- [ ] `cargo test -p stackly-cli --no-fail-fast` → lulus; golden yang berubah hanya menambah `parent`, `activeOperation`, dan field baru lain, isi lain identik → AC-23
- [x] `swift test --package-path clients/swift` dan `swift build --package-path apps/macos/Packages/StacklyUI --build-tests` → lulus dengan tipe hasil codegen 4.4 → AC-23
- [x] `cargo xtask check-deps`, `cargo xtask evidence check`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus → AC-23

### Catatan `/check verify` 2026-11-14 (sesi `chk-f46-m2`, kode `7ccd58fb`)

Verdict Milestone 2: **FAIL sempit**. Dua hal belum sesuai spec: AC-15 (daemon berhenti di tengah Up kadang tercatat sebagai Up yang gagal, temuan 1) dan satu klausa AC-10 (hint `env.up_incomplete` tidak pernah sampai ke klien, temuan 2). AC lain di lingkup milestone ini (AC-1 sampai AC-9, AC-11 sampai AC-14, AC-16, AC-23) lulus, sebagian dengan temuan di bawah.

Yang dicentang hanya langkah yang checker jalankan sendiri dan lulus. Empat langkah dibiarkan kosong:

- Langkah baca `driver.rs`: kalimat "`ShuttingDown` dari `wait_final` berarti abandon tanpa commit penutup" tidak benar di kode (temuan 1). Bagian lain langkah itu benar.
- `cargo test -p stacklyd --test env_up`: tiga kali jalan penuh lulus 6 dari 6, tetapi test restart daemon tidak stabil (temuan 1), jadi langkah ini belum bisa disebut lulus.
- Dua langkah suite penuh (`stacklyd`, `stackly-core`, `stackly-cli`): tidak diulang karena disk; gerbang lead di `main` `7ccd58fb` mencatat `stackly-core` 1828, `stacklyd` 755, `stackly-cli` 640. Bagian golden nya checker periksa lewat `git diff a5663277..7ccd58fb`: 12 golden CLI dan 35 fixture lama hanya bertambah field (`parent`, `activeOperation`, `lastUp`, `endpoints`, `coordinators`, `maxCoordinators`, dan field `ServiceNotReady` baru), tidak ada nilai lama yang berubah atau hilang.

**Hasil perintah.** `stackly-core --lib environment::up` 11 lulus. `stackly-core --test environment up` 15 lulus: 11 test `up::` (sebelas skenario di langkah itu) dan 4 test lain yang namanya memuat `up`; angka 13 di langkah itu keliru. `stackly-protocol` lulus di semua binary (241 di lib). `stacklyd --test env_up --test runtime --test skeleton --test instance_create_secrets --test operations` lulus (6, 17, 10, 3, 32). `codegen --check` 26 file mutakhir, kontrak 4.4 tercatat, `fuzz seed` tidak mengubah apa pun. `check-deps`, `evidence check`, dan cek Windows lulus. `swift test` klien 285 lulus, `swift build --build-tests` StacklyUI selesai.

**Probe terhadap `stacklyd` asli** (server asli di proses test, socket Unix asli, proses `stackly-fake-server` asli, provider `fake`; test probe sementara, sudah dicabut):

| Probe | Hasil |
| --- | --- |
| Tiga penolakan plus `env.plan_blocked` dan `env.manifest_unreadable` | generation, jumlah operasi, jumlah baris `runtimes`, `instances`, `environment_instances`, `instance_demands`, `environments`, dan pohon file sama sebelum dan sesudah. Reference urut teks. Plan yang berubah sesudah ditinjau (manifest diedit) ditolak `env.plan_changed`. → AC-1, AC-2 |
| Member dengan blocker `needsSecret` | `env.up` membalas `secret.needs_secret` dengan reference, `reason`, dan `generateAllowed` dari blocker itu, tanpa mutasi. → AC-2 |
| Store terkunci saat Up (status `unknown`) | Up tidak ditolak; hanya service itu berakhir `needs_secret` di tahap `start` dengan reference nya, dependent `skipped`. → AC-2, AC-10 |
| Dua environment meminjam satu instance yang berhenti, Up serentak | tepat dua lease `active`, satu proses, `restart_total` 0; start kedua selesai tanpa spawn. → AC-5, AC-14 |
| Dua manifest membuat instance bernama sama, Up serentak | tepat satu instance; yang kalah `failed` di tahap `create` dengan `resource.busy`, dependent nya `skipped`. → AC-14 |
| Up gagal di service lain (create gagal di init) | PID, `restart_total`, revision, endpoint, dan desired dua instance yang sehat tidak berubah; instance pengguna bernama mirip utuh byte demi byte dan barisnya sama; Up berikutnya hanya membuat anak untuk sisanya. → AC-9, AC-10, AC-12 |
| Start dependency gagal (crash sebelum ready) | dependent `skipped` `failed`, tidak pernah punya operasi start, lifecycle dan desired nya `stopped`, lease nya tetap; service tanpa dependency tetap `ready`; `stage` operasi `start`. → AC-8, AC-10 |
| Klien putus di tengah Up | Up jalan terus. → AC-13 |
| Rotasi sesi `new` (daemon launchd, sesi audit baru) | lease tetap `active`, desired tetap `running`, tidak ada operasi baru. → AC-13 |
| Daemon berhenti di tengah Up lalu Up lagi | anak start ditutup `interrupted`, tidak ada anak antre, lease tetap; Up kedua hanya menyalakan sisanya. `operation.get_stats` di tengah Up: `running` 1 (anak), `coordinators` 1. → AC-3, AC-15 |
| `.stackly.local.yaml` dengan `dataLocation` | `PlanItem.storage.locationId` sampai ke create: data instance itu dibuat di lokasi tersebut. → kontrak `--locked` butir 7 |
| Keanggotaan sesudah Up | summary instance langsung menyebut `Dedicated` begitu Up selesai. |

**Probe native** (NATS 2.15.1 asli dari cache runtime xtask, tanpa unduhan; ini probe, bukan record evidence): `env.lock`, plan `locked`, lalu Up `locked` membuat dan menyalakan dua `nats-server` berurutan; `env.get` menampilkan port `auto` yang benar benar di bind dan keempat port menerima koneksi; `environments.lock_sha256` sama dengan SHA-256 file `stackly.lock`; Up tidak mengubah file lock; Up kedua tidak memulai anak. Provider `fake` tidak bisa dikunci (`catalog.not_installable`), jadi `lock_sha256` hanya terbukti di probe native ini.

Tidak dijalankan: anak install nyata lewat daemon (`BLOCKED`, `reason`: butuh unduhan dari jaringan, `owner: user`; milik test native Milestone 4 langkah 11). Jalur install terbukti hanya di test core dengan port palsu.

**Tafsiran developer.**

1. `generateAllowed` turunan di `apps/daemon/src/ipc/env/children.rs`: **tidak sama** dengan aturan asli (temuan 4). Selebihnya diterima karena arahnya aman.
2. Fake `UpChildren` di `crates/core/tests/environment/up_fake.rs`: diterima.
3. `ChildStart::Started` hanya membawa operasi: diterima; cuplikan kode di spec perlu diselaraskan.
4. `environment::children::ChildOrigin` membungkus `operation::ChildOrigin` dan `SecretCaller`: diterima, sesuai spec.
5. `EnvUpParams` tanpa `idempotencyKey`: diterima, sama dengan setiap mutasi lain (key ada di envelope); teks spec perlu diselaraskan.
6. Up menyusun plan ulang di `accept` lewat `UpReads`: diterima, sesuai tabel langkah `accept`.

**Temuan.**

Sedang:

1. **AC-15, race saat daemon berhenti.** Test `a_daemon_restart_in_the_middle_of_an_up_shows_it_interrupted_and_keeps_the_leases` gagal 5 dari 12 kali saat dijalankan sendiri (`lastUp.state` `failed`, bukan `interrupted`); probe 10 putaran mengenainya 1 kali. Saat itu terjadi, commit penutup berjalan: `last_up` berisi `failed` dengan service `cancelled` dan dependent `skipped` (`env.dependency_not_ready`, `reason` `failed`), dan sesudah restart operasi Up tercatat `failed` dengan `env.up_incomplete`, bukan `operation.interrupted`. Sebabnya di `crates/core/src/environment/up/driver.rs`: `wait_child` mengubah `EngineError::ShuttingDown` menjadi akhir `cancelled` untuk service itu, dan loop `start` tidak memeriksa sinyal berhenti sesudah sebuah tugas selesai, sehingga `run` berakhir `Ending::Done`. `install_one` dan `create_one` punya bentuk yang sama. Spec meminta abandon tanpa commit penutup.
2. **AC-10, hint tidak sampai.** Error `env.up_incomplete` yang dibaca lewat `operation.get` selalu ber `hint` kosong (empat probe). `catalog_body` di `apps/daemon/src/ipc/operation/dto.rs` memasang hint create, runtime, secret, dan supervisor, tetapi tidak hint `env`. Fixture `ipc/frame-error-env-up-incomplete.json` menggambarkan balasan error ber hint yang tidak pernah dikirim daemon, karena kode ini hanya muncul sebagai kegagalan operasi.
3. **`lastUp.services[].references` tidak membawa alasan aslinya.** `apps/daemon/src/ipc/env/status.rs` selalu menulis `reason` `missing` dan `generateAllowed` true. Probe: anak start gagal dengan `store_locked` dan `generateAllowed` false, `lastUp` menampilkan `missing` dan true. `notReady` aturan 2 benar selama blocker instance ada. Ini menyentuh AC-21 Milestone 4 (Set Secret… di banner).

Rendah:

4. `generateAllowed` di penolakan `env.up` untuk item `create`: aturan turunan memberi false begitu ada instance terinisialisasi yang memakai reference itu, apa pun kelas field nya. Aturan asli (`UseRules::of_instance`) hanya memberi false untuk field `immutableAfterInit` atau field tanpa aturan. Field secret Valkey dan NATS (`restart`) serta ClickHouse (`reload`) jadi berbeda hasil. Dibaca di kode saja; field secret `fake` `immutableAfterInit`, jadi tidak bisa di probe dengan `fake`.
5. Laporan Up yang terputus tidak punya isi per service: `last_up` berstatus `running` disimpan dengan `services` kosong, jadi `lastUp` `interrupted` tidak menjelaskan service mana yang sedang dikerjakan.
6. Item service `skipped` di `env.up_incomplete` membawa `operationId` operasi create nya, yang bisa terbaca sebagai operasi yang gagal.
7. Belum ada test untuk: batas `CHILD_QUEUE_TIMEOUT_SECONDS` (`resource.busy`), `SERVICE_SETTLE_TIMEOUT_SECONDS` (`env.service_not_ready`), instance `stopping` yang ditunggu lalu dinyalakan, batas `UP_INSTALL_CONCURRENCY`, `lock_sha256` yang ditulis Up, `storage.locationId` sampai ke create, `env.plan_blocked` lewat socket, member ber blocker `needsSecret`, dan setiap baris tabel probe di atas selain tiga penolakan dasar. Semua itu lulus di probe checker, tetapi belum dikunci test.

### Acceptance-criteria coverage

- AC-1, AC-2 · precheck core (`up::check`) dan handler `env.up`; test penolakan di core dan `env_up.rs`
- AC-3 · `BusyPolicy::Reject`, `LockKey::EnvironmentName`, `expected_revision` dari plan, kelas pengatur dari Milestone 1; Up kedua `resource.busy`
- AC-4, AC-5 · guard `accept`; core dan `env_up.rs`
- AC-6 · install satu kali per artefak, gagal menutup tahap `install` (core); install anak lewat `install_request`. Test daemon untuk install nyata menunggu artefak yang belum terpasang (native, Milestone 4 langkah 11)
- AC-7, AC-12 · create anak lewat `create_request` dengan `EnvironmentBinding`; kompensasi milik create (Milestone 1) dan create gagal di init di `env_up.rs`
- AC-8, AC-9, AC-10, AC-11 · graph, driver, commit penutup; core dan `env_up.rs`
- AC-13, AC-15 · lease bertahan sesudah gagal, batal, dan restart daemon; `lastUp` `interrupted`. Rotasi sesi `new` dan klien putus di tengah Up belum punya test sendiri di milestone ini (aturan rotasi tidak berubah, spec 0031)
- AC-14 · serentak: dua Up satu environment `resource.busy`; dua environment yang meminjam satu instance dan dua manifest dengan nama instance sama belum punya test daemon sendiri
- AC-16 · `env.get` dan `env.list`: `activeOperation`, `lastUp`, `endpoints`, *Alasan belum ready*
- AC-23 · protokol 4.4, `env.up` `recovery refused`, DTO, kode, fixture; skenario fixture GUI `environment-bringing-up` dan `environment-up-incomplete` spec 0068 milik Milestone 4
- AC-24 · `OperationSummary.parent`, `OperationStats.coordinators` dan `maxCoordinators` di wire
- AC-17, AC-18 · belum, Milestone 3 (CLI)
- AC-19 sampai AC-22 · belum, Milestone 4 (app)
- AC-25 · belum, Milestone 4 langkah 11 (native)
