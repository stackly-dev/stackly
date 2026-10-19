# Scope 01 · Foundation

[Kembali ke index](index.md)

Fondasi yang dipijak semua journey: stack, tooling, sumber runtime, model domain dan data, protokol IPC, operation engine, CLI, mesin provider, supervisor, dan secrets. Design system dan GUI shell dikerjakan di jalur paralel [ui-macos.md](ui-macos.md). Fase ini ditutup dengan walking skeleton yang membuktikan semuanya sudah tersambung.

## Foundation

### 1. Stack & arsitektur · done

Putuskan stack dan susunan monorepo, lalu scaffold proyek yang bisa dijalankan, supaya setiap journey dibangun di atas struktur nyata. PRD sudah mengunci Rust untuk core, daemon, dan CLI, SwiftUI untuk macOS, dan SQLite untuk registry; spec ini mengunci sisanya (toolchain, pembagian crate, encoding IPC) tanpa membuat crate kosong yang belum perlu.

**Done when:** keputusan stack tercatat di spec (termasuk OQ-005), workspace Rust dan proyek SwiftUI kosong bisa build dan jalan di Mac Apple Silicon, dan arah dependency dari domain ke adapter sesuai PRD bagian 15.

**Trace:** REQ-001, REQ-050, NFR-010, OQ-005, PRD bagian 15 dan 16

**Pointer:** spec [0001](../specs/0001-stack-arsitektur/index.md) · code in `crates/`, `apps/`, `clients/swift/`, `protocols/`, `xtask/`

- [x] Decide the stack (spec): `/architect stack & arsitektur`
- [x] Scaffold from the decision: `/develop stack & arsitektur`

### 2. Coding standards & tooling · in-progress

Catat konvensi dan pilihan tooling dari proyek hasil scaffold, lalu pasang format, lint, typecheck, dan aturan test integrasi yang wajib diikuti semua fitur berikutnya.

**Done when:** root `AGENTS.md` mencerminkan stack nyata dan mewajibkan spec serta test mempertahankan ID AC dari PRD; format, lint, build, unit test, validasi schema, audit lisensi dependency, dan security static checks berjalan di Mac ini dengan hasil bersih; integration test memakai test root ephemeral (bukan direktori Application Support pengguna), port yang dialokasikan, dan fixture credential, dan cleanup hanya menghapus path milik test run itu; run fixture dan run runtime nyata terpisah; tiap test yang membuktikan ID PRD menghasilkan evidence record sesuai PRD bagian 28 (termasuk versi provider, langkah test, hasil, waktu, dan reviewer bila manual) berlabel fixture atau native, dengan status awal `NOT_RUN`; mesin benchmark baseline tercatat.

**Trace:** REQ-051, OQ-011, PRD bagian 27, 28, 31, 32.1, dan 32.3

**Pointer:** spec [0002](../specs/0002-coding-standards-tooling/index.md) · code in `.githooks/pre-commit`, `rustfmt.toml`, `clippy.toml`, `.swift-format`, `deny.toml`, `crates/testkit/`, `crates/evidence/`, `xtask/src/evidence/`, `schemas/evidence/`

- [x] Capture conventions and tooling choices: `/audit`
- [x] Install the tooling: `/develop coding standards & tooling`
- [x] Decide evidence, native tests, and baseline (spec): `/architect coding standards & tooling`
- [x] Build the evidence standard: `/develop coding standards & tooling`
  - [x] Rust evidence path end to end: `stackly-evidence`, `check-deps` rules, `cargo xtask evidence run` (REQ-051)
  - [x] `cargo xtask evidence check` and `evidence new`, shared examples in `schemas/evidence/`
  - [x] Swift path: `StacklyTestSupport` trait
  - [x] Fixture credential leak scan
  - [x] Baseline recorded on this Mac (OQ-011)
- [x] Verify it: `/check verify coding standards & tooling`
- [x] Test it: `/test coding standards & tooling`

### 3. Sumber & packaging runtime · in-progress

Tentukan dari mana artefak runtime enam provider berasal dan bagaimana mereka dipaketkan supaya bisa dipindah dan dipasang tanpa Homebrew global. Ini risiko terbesar proyek (RISK-01), jadi harus jelas sebelum install runtime (fitur 18) dibangun.

**Done when:** tiap provider wajib punya sumber artefak, dependency closure, versi konkret, status lisensi, dan kebijakan verifikasi serta signing binary (ditandatangani upstream, ditandatangani ulang maintainer, atau ad hoc saat install) yang tercatat; satu executable tiap provider (PostgreSQL 17 dan 18 keduanya) berjalan dari artefak managed di Mac uji tanpa Homebrew global, dijalankan sebagai child process oleh proses noninteractive, bukan dari Terminal; provider yang belum punya artefak managed dicatat `BLOCKED` dengan alasan dan owner; spec memutuskan apakah artefak buatan maintainer (OQ-003) diperlukan.

**Trace:** OQ-003, OQ-004, RISK-01, RISK-12, AC-078, gate G0, PRD bagian 20.2, 25, dan 33.2 (M0)

**Pointer:** spec [0003](../specs/0003-sumber-packaging-runtime/index.md) · code in `xtask/src/runtime/`, `xtask/tests/native_runtime.rs`, `runtimes/inventory.toml`, `runtimes/keys/`, `evidence/runtime/`

- [x] Design it (spec): `/architect sumber & packaging runtime`
- [x] Build it: `/develop sumber & packaging runtime`
  - [x] Inventory, `runtime pin`, `runtime verify`, and `runtime check` (spec 0003 AC-1 to AC-3)
  - [x] LaunchAgent spike end to end, proven on NATS, then Traefik, ClickHouse, MySQL 8.4 and 9.7, with evidence in `evidence/runtime/` (spec 0003 AC-1, AC-3 to AC-10, AC-13; needs spec 0002 step 1 built). NATS 2.15.1, Traefik 3.7.15, MySQL 8.4.11 dan 9.7.2 `PASS`; ClickHouse 26.8 `blocked` karena aset macOS berupa binary self extracting yang ditolak codesign; aset `.zip` bertanda tangan Developer ID dipakai lewat format `zip` yang diputuskan spec 0003 pada 2026-10-17 (Milestone 2b)
  - [x] Milestone 2b: ClickHouse from the Developer ID signed `.zip` through the new `zip` format: format check, `/usr/bin/tar` extraction, mode 0755 for zip executables, `pin` support, the `archive_root` rule, then the ClickHouse entry and its evidence (spec 0003 AC-1 to AC-3, AC-5 to AC-10, AC-13, AC-16; build steps 15 and 16). ClickHouse 26.8.30.8 `PASS` dari `.zip` (evidence `01a14c52-92d2-75ee-b5ce-3e641036b90a`)
  - [x] Decide how PostgreSQL 17 and 18 and Valkey are built on this Mac (spec): `/architect sumber & packaging runtime`
  - [x] xtask support for maintainer artifacts: rule 5 `recipe` format without `attestation`, `runtime pin --file`, `runtime spike --no-quarantine`, the `manifest` probe, and the PostgreSQL and Valkey spike recipes with their native tests; needs Milestone 2b step 15 merged first (spec 0003 AC-2, AC-5, AC-7, AC-9, AC-10, AC-12, AC-18, AC-19; build step 17)
  - [x] Maintainer recipes in the new `stackly-dev/runtimes` repo: `build.sh` with a clean environment, then Valkey 9.1, OpenSSL, ICU, lz4, zstd, and PostgreSQL 17 and 18 built on this Mac (spec 0003 AC-7, AC-12, AC-17; build steps 9 and 10)
  - [x] PostgreSQL 17 and 18 and Valkey published and proven: `pin --file`, local spike, license gate per provider, upload after your approval, `runtime verify`, entries and evidence, then rule 9 (spec 0003 AC-1, AC-2, AC-5, AC-7, AC-10, AC-19; build step 12)
  - [x] Cache pruning: `runtime spike --prune-cache`, run by hand on this Mac (spec 0003 AC-4; build step 13)
- [x] Verify it: `/check verify sumber & packaging runtime`
- [x] Test it: `/test sumber & packaging runtime`

### 4. Domain core & platform abstraction · in-progress

Model domain (software, provider revision, runtime, instance, environment, operation, lease), tiga state machine yang terpisah, invariant bisnis, dan trait platform (termasuk resolver direktori per user untuk owned root, registry, runtimes, instances, logs, cache, retained, dan path socket pendek) dengan fake platform untuk test. Core tidak boleh tahu soal SwiftUI, WinUI, atau command macOS.

**Done when:** domain bisa build tanpa dependency GUI atau API OS; state runtime, lifecycle instance, dan readiness/health terpisah; fake platform menjalankan test invariant di Mac ini; layout direktori per user diputuskan lewat resolver platform tanpa path hardcode dan bisa diarahkan ke root alternatif untuk integration test; tiap direktori instance membawa metadata yang cukup untuk inventarisasi tanpa registry; capability Windows yang belum dibuat mengembalikan not supported.

**Trace:** REQ-001, REQ-050, BR-001 sampai BR-025 (sebagai invariant), NFR-010, AC-050, AC-074, PRD bagian 6, 19.1, dan 20.1

**Pointer:** spec [0004](../specs/0004-domain-core-platform-abstraction/index.md) · code in `crates/domain/`, `crates/platform/`, `crates/core/`, `crates/platform-macos/`, `crates/testkit/`, `apps/daemon/`

- [x] Design it (spec): `/architect domain core & platform abstraction`
- [x] Build it: `/develop domain core & platform abstraction`
  - [x] Platform ports and directory layout end to end: `RootSet`, `DirectoryLayout`, `STACKLY_ROOT`, `MacosPlatform`, `UnsupportedPlatform`, testkit `FakePlatform`, and `prepare_layout` (spec 0004 AC-6 to AC-9, AC-12, AC-13, AC-15)
  - [x] Instance folders, `instance-manifest.json`, and inventory without the registry (spec 0004 AC-5, AC-10 to AC-12, AC-15)
  - [x] Domain model and state machines: runtime, instance lifecycle, readiness and health, operation, and lease (spec 0004 AC-1 to AC-4, AC-14, AC-16)
  - [x] Invariants and enforcement: evidence for every BR marked F4, AC-050, AC-074, and NFR-010, plus the `check-deps` source scan (spec 0004 AC-1, AC-5, AC-6, AC-12 to AC-14; uses `stackly-evidence` from spec 0002 step 1)
- [x] Verify it: `/check verify domain core & platform abstraction`
- [x] Test it: `/test domain core & platform abstraction`

### 5. Data model registry · in-progress

Skema SQLite untuk registry: entitas, relasi, constraint, migrasi, dan revision per resource. Model data adalah hal paling mahal untuk diulang, jadi ia diputuskan sebelum fitur apa pun menyimpan state.

**Done when:** semua entitas logis di PRD bagian 17 ada dengan foreign key dan transaksi aktif; migrasi berurutan teruji dari versi sebelumnya, dengan kebijakan forward dan rollback yang tertulis di spec; schema yang setengah teraplikasi terdeteksi saat daemon start tanpa init ulang registry; secret hanya disimpan sebagai reference; nama instance unik per user dan path data memakai ID yang stabil; semua waktu disimpan dalam UTC dan ditampilkan sesuai locale pengguna.

**Trace:** BR-002, BR-003, REQ-048, NFR-005, AC-065 (bagian migrasi), PRD bagian 17

**Pointer:** spec [0005](../specs/0005-data-model-registry/index.md) · code in `crates/storage/`, `crates/domain/`, `crates/platform/`, `crates/testkit/`, `apps/daemon/`

- [x] Design it (spec): `/architect data model registry`
- [x] Build it: `/develop data model registry`
  - [x] Opening the registry end to end: `stackly-storage`, migration 0001 with every PRD 17 table, open with snapshot and recovery, fixture v1, and `stacklyd` start (spec 0005 AC-1 to AC-7, AC-13, AC-15; build steps 1 to 7)
  - [x] Registry port and fake: domain `registry` module, `restore` constructors, `FakeRegistry`, `RegistrySeed`, and `registry_conformance` (spec 0005 AC-8 to AC-11, AC-14, AC-16; build steps 8 and 9; needs spec 0004 milestone 3)
  - [x] SQLite registry: `Registry` for `SqliteRegistry`, aggregate mapping, queries, prune, and conformance on SQLite (spec 0005 AC-7 to AC-16; build steps 10 to 13)
- [x] Verify it: `/check verify data model registry`
- [x] Test it: `/test data model registry`

### 6. Protokol IPC & daemon · in-progress · GA

Daemon per user dengan protokol berversi di atas encoding dan framing yang dikunci di fitur 1, lewat Unix domain socket privat: handshake, request ID, error terstruktur, event dengan cursor, dan otorisasi user lokal. GUI dan CLI hanya client; hanya daemon yang menulis registry dan mengendalikan lifecycle.

**Done when:** handshake menolak protocol major yang berbeda dan peer dari user lain sebelum ada efek samping; client memverifikasi pemilik endpoint socket sebelum mengirim credential atau mutasi; hanya satu daemon per user yang aktif, daemon kedua berhenti tanpa menyentuh registry, socket, atau proses instance, dan `stackly daemon status` menunjukkan daemon pemiliknya; tidak ada listener TCP admin; batas ukuran pesan dan subscription berlaku; setiap error memakai satu envelope stabil sesuai VAL-020; reconnect memakai snapshot plus cursor; fixture kontrak dipakai bersama oleh Rust dan Swift; fuzz target framing IPC berjalan di Mac ini.

**Trace:** REQ-047, REQ-048, BR-011, VAL-017, VAL-020, NFR-001, NFR-004, NFR-006, NFR-011, AC-047, AC-070, AC-071, AC-079, NFR-009, RISK-03, RISK-09, OQ-005 (generation tooling), PRD bagian 18

**Pointer:** spec [0006](../specs/0006-protokol-ipc-daemon/index.md) · code in `crates/protocol/`, `protocols/`, `clients/swift/`, `xtask/src/codegen/`, `crates/ipc/`, `apps/daemon/`, `apps/cli/`

- [x] Design it (spec): `/architect protokol IPC & daemon`
- [x] Build it: `/develop protokol IPC & daemon`
  - [x] Contract and codec: wire DTOs, error envelope and catalog, protocol 1.0 with contract digest and `contract-versions.json`, framing codec, staged request decode, shared fixtures, and Swift contract tests (spec 0006 AC-1, AC-6 to AC-8, AC-10)
  - [x] `stackly-ipc` transport: listener with peer check, framed connection, verified client, and the new `check-deps` rules (spec 0006 AC-2, AC-3, AC-5, AC-6, AC-16; after milestone 1, can run in parallel with the next one and the fuzz milestone)
  - [x] Daemon library, lock, and log: `stacklyd` as library plus bin, `--print-roots`, single daemon lock, rotating log writer (spec 0006 AC-4, AC-12, AC-17; after milestone 1)
  - [x] Daemon server and `stackly daemon status` end to end: event bus, handshake, limits, socket watch, shutdown, CLI status, performance, and the manual two account check (spec 0006 AC-1, AC-2, AC-4, AC-5, AC-7, AC-9, AC-11, AC-12, AC-13, AC-15, AC-16, AC-18, AC-19; needs the three milestones above; the two account check and the temporary folder observation are recorded BLOCKED in `evidence/manual/` until this Mac has a second account and a reboot is observed)
  - [x] Framing fuzz targets on this Mac: `fuzz/` with a pinned nightly, `cargo xtask fuzz`, 5 minute runs with evidence (spec 0006 AC-14; after milestone 1)
- [x] Verify it: `/check verify protokol IPC & daemon`
- [x] Test it: `/test protokol IPC & daemon`
- [x] Review it (fresh model): `/check review protokol IPC & daemon`
- [x] Document it: `/document protokol IPC & daemon`

### 7. Operation engine · in-progress · GA

Mesin operasi async yang dipakai semua mutasi: journal, idempotency key, lock per resource, state revision, progress, cancel, serta primitive cleanup dan kompensasi yang hanya menghapus path dan resource buatan operasi itu di bawah owned root. Inilah yang mencegah runtime atau instance ganda saat GUI dan CLI mengirim perintah bersamaan.

**Done when:** operasi memakai tujuh state di PRD bagian 18.2, dari `queued` sampai `needs_recovery`; dua client dengan idempotency key sama hanya menghasilkan satu efek samping; client yang putus tidak membatalkan pekerjaan; cancel di langkah yang tidak bisa dibalik dilaporkan apa adanya; kompensasi tidak pernah menyentuh direktori di luar buatan operasi itu, walau lewat symlink; resume hanya untuk operasi yang menyatakan bisa dilanjutkan dan tidak mengulang langkah yang tidak idempoten; SQLite tidak dipakai sebagai mutex selama download atau startup.

**Trace:** REQ-048, REQ-054, BR-023, VAL-007, VAL-015 (bagian cleanup), NFR-005, NFR-006, NFR-011, AC-048, AC-054

**Pointer:** spec [0007](../specs/0007-operation-engine/index.md) · code in `crates/core/src/operation/`, `crates/core/src/ownership.rs`, `crates/domain/`, `crates/storage/`, `crates/platform/`, `crates/platform-macos/`, `crates/testkit/`, `crates/protocol/`, `apps/daemon/`

- [x] Design it (spec): `/architect operation engine`
- [x] Build it: `/develop operation engine`
  - [x] Journal di registry: tipe journal di domain, `Operation::touch`, `Outcome::Failed`, tambahan port `Registry`, `FakeRegistry`, migrasi `operation_journal` (key ganda, unique index, backfill), dan conformance di `SqliteRegistry` (spec 0007 AC-2, AC-10, AC-11, AC-17; build steps 1 to 3; langkah 1 dan 2 butuh spec 0005 milestone 2, langkah 3 butuh milestone 3)
  - [x] Primitive hapus aman: `Filesystem::remove_tree`, `FakeFilesystem`, adapter macOS berbasis file descriptor dengan test balapan dan `CrossesDevice`, dan `remove_owned_tree` (spec 0007 AC-8, AC-9; build steps 4 to 6; bisa mulai sekarang sebagai sesi paralel)
  - [x] Engine dari ujung ke ujung di atas fake: lock dan antrian FIFO, `begin` dengan idempotency, runner dengan langkah dan progress, cancel, kompensasi, `recover`, resume, abandon, shutdown, dan skenario AC-054 dengan evidence (spec 0007 AC-1 to AC-8, AC-10 to AC-16; build steps 7 to 12; butuh langkah 1 dan 2, kompensasi butuh langkah 4)
  - [x] Kontrak wire dan daemon: DTO, event, kode, sidik jari, fixture dan test Swift, lalu wiring engine di `stacklyd`, enam method, subsystem `operations`, hook shutdown, dan test integrasi daemon (spec 0007 AC-1, AC-3, AC-5, AC-6, AC-10, AC-11, AC-13, AC-14, AC-16, AC-18 to AC-20; build steps 13 to 15; langkah 13 bisa paralel dengan milestone engine setelah spec 0006 milestone 1, langkah 14 dan 15 butuh spec 0006 milestone 4)
- [x] Verify it: `/check verify operation engine`
- [x] Test it: `/test operation engine`
- [x] Review it (fresh model): `/check review operation engine`
- [ ] Document it: `/document operation engine`

### 8. CLI client & kontrak output · in-progress

CLI `stackly` sebagai client daemon dengan help, `--json`, exit code stabil, mode noninteractive, progress ke stderr, input secret yang aman, dan `--wait` untuk operasi async. Command lain disebut di Done when fitur yang memilikinya dan menempel ke kontrak ini.

**Done when:** `stackly daemon status` serta `stackly operation list`, `inspect`, dan `cancel` bekerja lewat daemon; output `--json` bisa diparse tanpa ANSI atau progress; exit code diturunkan dari kategori error code di envelope fitur 6, bukan dari pesan, dan cocok dengan tabel di PRD bagian 12; CLI tidak pernah menjalankan binary provider sendiri.

**Trace:** REQ-028, BR-011, AC-028, PRD bagian 12

**Pointer:** spec [0008](../specs/0008-cli-client-kontrak-output/index.md) · code in `apps/cli/`, `crates/protocol/`, `crates/ipc/`, `xtask/`

- [x] Design it (spec): `/architect CLI client & kontrak output`
- [ ] Build it: `/develop CLI client & kontrak output`
  - [ ] CLI framework and `stackly daemon status` end to end: `codes::cli`, the `check-deps` no spawn rule, exit code map, `--json` envelope, human output, progress, noninteractive mode and `Confirm`, `Session` with `call_detailed`, the final `daemon status`, scripted daemon, pty and golden tests (spec 0008 AC-1 to AC-6, AC-9 to AC-14; build steps 1 to 8; needs spec 0006 milestones 1, 2, and 4; step 2 can run as a parallel session)
  - [ ] `stackly operation list`, `inspect`, `cancel`, and `--wait`: argument parsers, operation rendering, the three commands, `Waiter` with timeout, Ctrl+C, and reconnect, and the wait matrix (spec 0008 AC-3, AC-4, AC-7 to AC-9, AC-12; build steps 9 to 12; needs milestone 1 and the spec 0007 operation DTOs; the real daemon tests need the spec 0007 engine)
- [ ] Verify it: `/check verify CLI client & kontrak output`
- [ ] Test it: `/test CLI client & kontrak output`

### 9. Provider engine & manifest schema · in-progress · GA

Schema manifest provider deklaratif (`stackly/v1alpha1`), loader, validasi, interpolasi terkontrol, dan primitive aman yang menghasilkan execution plan tervalidasi. Kontrak adapter juga mencakup preflight kompatibilitas data dan runtime per provider, supaya tool baru yang cukup dengan primitive yang ada tidak perlu patch core atau GUI.

**Done when:** manifest contoh NATS, Traefik (dengan daftar routes), dan fake provider lolos validasi di Mac ini, dan fuzz target parser manifest serta template berjalan di Mac ini; manifest yang merujuk primitive atau probe yang tidak dikenal ditolak saat validasi, bukan saat start; schema bisa menyatakan langkah reload (bila capability reload aktif) dan timeout readiness serta recovery per provider, bukan hanya stop timeout; field berisi shell metacharacter tidak pernah dijalankan lewat shell; field keamanan yang tidak dikenal dan capability yang tidak bisa dipenuhi ditolak; membaca catalog tidak menjalankan kode provider.

**Trace:** REQ-029, REQ-031, VAL-002, VAL-012, VAL-018, VAL-019, AC-029, AC-031, NFR-009, RISK-07, RISK-08, PRD bagian 21

**Pointer:** spec [0009](../specs/0009-provider-engine-manifest-schema/index.md) · code in `crates/core/src/provider/`, `crates/fake-server/`, `providers/`, `schemas/provider/`

- [x] Design it (spec): `/architect provider engine & manifest schema`
- [x] Build it: `/develop provider engine & manifest schema`
  - [x] Fake provider from package to plan: domain capability additions, `stackly-fake-server` and its testkit helper, `read_package` and strict decode, interpolation, settings dialect, template renderer, semantic validation, `InitPlan`, `RunPlan`, `ReloadPlan`, preflight, `providers/fake`, and the `fake-command` fixture (spec 0009 AC-1 to AC-12, AC-15, AC-18; build steps 0 to 8)
  - [x] NATS and Traefik packages with goldens, `versions.json` generated from the inventory, JSON Schema in `schemas/provider/`, and `cargo xtask provider check` (spec 0009 AC-13, AC-14, AC-17; build steps 9 to 13)
  - [x] Four fuzz targets run 10 minutes each on this Mac, the adversarial suite with evidence guards (including the billion laughs test), and spike markers moved to F36 (spec 0009 AC-1 to AC-10, AC-16, AC-18; build steps 14 to 16)
  - [x] Reconcile the build with spec reconciliation rounds 1 and 2: the `${instance.secrets}` folder token and its positions, `materializeSecret` as one file name in that folder, a secret as a whole env value only in `lifecycle.start` and `runExecutable`, `secret_purpose_collision`, `EnvValue`, `MaterializeSecret { secret, file_name }`, `RunPlan.render`, `materialize(&ResolvedSecrets)` with a provisional `ResolvedSecrets` (F11), `PathNotUtf8` for every context folder, and the `fake-command` fixture, goldens, and fuzz seeds that follow (spec 0009 AC-7, AC-8, AC-11; build steps 6, 7, 8, and 15)
  - [x] Wire error codes: `codes::manifest` and `codes::provider` in `stackly-protocol`, `ItemSpec` with `kind` `issue`, the provider `ResourceRef` text form, and the `stacklyd::map` mapping with a test per variant (spec 0009 AC-19; build step 17; needs the protocol, wave 2)
- [ ] Verify it: `/check verify provider engine & manifest schema`
- [ ] Test it: `/test provider engine & manifest schema`
- [ ] Review it (fresh model): `/check review provider engine & manifest schema`
- [ ] Document it: `/document provider engine & manifest schema`

### 10. Process supervisor · in-progress · GA

Komponen di dalam daemon yang menjalankan executable terverifikasi dengan argv eksplisit, mencatat identitas proses (bukan PID saja), menjalankan probe readiness, dan menghentikan proses secara graceful sesuai strategi provider. Spec ini juga pemilik tunggal rekonsiliasi dasar setelah daemon restart dan restart policy (backoff, jitter, retry budget, reset setelah periode sehat, timeout per provider).

**Done when:** startup transaction di PRD bagian 19.2 berjalan untuk fake provider; process, readiness, health, dan desired state terlihat terpisah; stop normal tidak pernah memakai SIGKILL atau kill berdasarkan nama; setelah daemon restart, proses yang masih hidup hanya diadopsi bila identitas majemuknya cocok, dan identitas yang tidak terbukti berstatus unknown tanpa dibunuh; start tidak pernah memunculkan proses kedua pada data yang sama; provider yang terus crash berhenti di retry budget tanpa busy loop; path dengan spasi dan Unicode diteruskan utuh sebagai argumen.

**Trace:** REQ-011, REQ-012, REQ-013 (dasar), BR-010, AC-011, AC-012, AC-013 (fake provider), AC-058, AC-068, NFR-003, NFR-006, RISK-03, RISK-14, PRD bagian 19

**Pointer:** spec [0010](../specs/0010-process-supervisor/index.md) · code in `crates/core/src/supervisor/`, `crates/platform/`, `crates/platform-macos/`, `crates/testkit/`, `crates/protocol/`, `crates/storage/`, `apps/daemon/`

- [x] Design it (spec): `/architect process supervisor`
- [ ] Build it: `/develop process supervisor`
  - [ ] Process port, registry ops, and macOS adapter: spike on this Mac (suspended spawn, libproc identity, kqueue, flock inheritance, punch hole), domain `process` types, `ProcessSupervision` trait as the seventh platform port, the registry ops spec 0005 designs for this feature (`RecordProcessObservation`, `latest_process_observation`, `InsertConfigRevision`, `active_config_revision`, several `UpdateInstance` per batch), `MacosProcessSupervision`, and `FakeProcessSupervision` (spec 0010 AC-1, AC-2, AC-5, AC-6, AC-15, AC-19; build steps 1, 2, 2a, 3, and 4; needs the spec 0004 update from spec 0010 Follow-up; step 2 needs spec 0011 step 1 merged first; after step 2, the registry ops, the adapter, and the fake can run as three parallel sessions; step 2a and spec 0011 step 0 touch the same registry files, so they never run at the same time, in either order)
  - [ ] Start to ready and safe stop on the fake provider: `classify`, restart policy math, `SupervisorConfig`, `PortProbe`, commit rules, probes `tcp`, `http`, and `exec`, `execute_init`, render before spawn, startup transaction with dry run and suspended spawn, stop, ports, readiness and health, `statusSeq`, and `execute_reload` (spec 0010 AC-2 to AC-8, AC-10, AC-13 to AC-15, AC-17 to AC-19, AC-22; build steps 5 to 8; needs spec 0009 milestone 1, spec 0011 step 4 (`SecretService` with `resolve_for_start`, `put`, and `secret::delivery`), step 2a, and the spec 0007 operation API; steps 5 and 6 can run as two parallel sessions)
  - [ ] Crash, spool, and reconciliation: restart policy in the actor, spool reader with redaction and punch hole, reconciliation table, adoption, `unknown` recheck, and daemon shutdown without signals (spec 0010 AC-7 to AC-12, AC-17, AC-21, AC-22; build steps 9 to 11; the spool step can run as its own session)
  - [ ] Daemon, IPC, and end to end proof: protocol DTOs, events, and error codes with fixtures and Swift contract tests, `stacklyd` start slot and shutdown step, `instance.create`, `start`, `stop`, `get`, and `list`, `install_fake`, daemon integration tests with evidence, `check-deps` rules, and the NFR-003 measurement (spec 0010 AC-3 to AC-12, AC-15 to AC-17, AC-20, AC-21, AC-23; build steps 12 to 15; needs the spec 0006 daemon server, the spec 0005 SQLite registry, and `ReplaceSecretUses` from spec 0011 step 0 for create; protocol and `check-deps` can run as parallel sessions)
- [ ] Verify it: `/check verify process supervisor`
- [ ] Test it: `/test process supervisor`
- [ ] Review it (fresh model): `/check review process supervisor`
- [ ] Document it: `/document process supervisor`

### 11. Secret store & redaction · in-progress · GA

Simpan secret di Keychain dan hanya reference di SQLite atau manifest, lalu terapkan masking yang sama di GUI, CLI, log, diagnostics, dan export. Reveal atau copy secret selalu butuh aksi eksplisit.

**Done when:** sentinel credential tidak muncul di output default, registry, log, atau audit; secret noninteractive masuk lewat input aman, bukan argumen CLI; ada satu alur untuk mengisi atau mengganti nilai di balik secret reference dari GUI dan CLI, yang dipakai ulang oleh create instance, env up, dan restore; Keychain yang terkunci menghasilkan status needs secret, bukan auth kosong.

**Trace:** REQ-016, BR-018, AC-016, AC-062, OQ-008

**Pointer:** spec [0011](../specs/0011-secret-store-redaction/index.md)

- [x] Design it (spec): `/architect secret store & redaction`
- [ ] Build it: `/develop secret store & redaction`
  - [ ] Secret port, references, and Keychain adapter: `SecretRef`, `SecretStore`, `FakeSecretStore`, and `MacosSecretStore` with the `OSStatus` spike (spec 0011 AC-1 to AC-4, AC-6, AC-7; build steps 1 to 3; needs features 4 and 5)
  - [ ] Secret registry and core service: the secret migration and the registry ops spec 0005 designs for this feature (reference insert, update, observe, delete, and restore, `ReplaceSecretUses`, `InsertAuditEvent`, and the secret and audit reads), then `SecretWorker`, `put`, reveal, delete, `resolve_for_start`, `Redactor`, delivery rules, and the `check-deps` allowlist (spec 0011 AC-4 to AC-6, AC-8 to AC-13, AC-15, AC-20; build steps 0, 4, and 6; does not need feature 6; step 0 needs step 1 and never runs at the same time as spec 0010 step 2a, in either order, because both touch the same registry files; step 4 needs steps 0 and 2; step 6 can run as its own session)
  - [ ] Secret service through the daemon: IPC handlers, `codes::secret` and the error mapping, the redacting log writer, `Redactor` on envelopes and events, and the daemon sentinel suite (spec 0011 AC-3, AC-5, AC-8, AC-9, AC-11, AC-13, AC-14, AC-16, AC-21; build steps 5 and 7; needs feature 6 and the two milestones above)
  - [ ] CLI secret commands and secure input: `stackly secret list`, `status`, `set`, `reveal`, `delete` (spec 0011 AC-16, AC-17; build step 8; needs feature 8)
  - [ ] Swift client and GUI secret flows: `FixtureCredential`, `RevealedSecret`, `SecretField`, `SecretEntrySheet`, `RevealableSecretText` (spec 0011 AC-18, AC-19; build steps 9 to 11; needs features 12 and 13)
- [ ] Verify it: `/check verify secret store & redaction`
- [ ] Test it: `/test secret store & redaction`
- [ ] Review it (fresh model): `/check review secret store & redaction`
- [ ] Document it: `/document secret store & redaction`

_Catatan:_ fitur 12 (Design system & UI foundation) dan 13 (App shell SwiftUI & Swift client) sekarang ada di [ui-macos.md](ui-macos.md), bersama fitur UI lintas layar lain yang diturunkan dari mockup UI. Nomor, urutan, dan statusnya tidak berubah.

## Skeleton

### 14. Walking skeleton

Satu irisan tipis yang membuktikan stack sudah tersambung: fake provider dibuat, dijalankan, dan dihentikan lewat CLI, statusnya tampil di GUI, dan registry mencatatnya. Belum ada download runtime atau provider nyata.

**Done when:** `stackly instance create`, `start`, dan `stop` untuk fake provider berhasil lewat daemon; GUI menampilkan perubahan state lewat event; menutup GUI tidak menghentikan instance; restart daemon saat instance fake berjalan tidak menghasilkan instance ganda atau proses yatim; test fake provider berjalan di Mac ini.

**Trace:** AC-001 (awal), AC-013 (awal), AC-029 (awal), fixture dummy server di PRD bagian 32.3

- [ ] Build it: `/develop walking skeleton`
