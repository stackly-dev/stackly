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

Catat konvensi dan pilihan tooling dari proyek hasil scaffold, lalu pasang format, lint, typecheck, CI dasar, dan aturan test integrasi yang wajib diikuti semua fitur berikutnya.

**Done when:** root `AGENTS.md` mencerminkan stack nyata dan mewajibkan spec serta test mempertahankan ID AC dari PRD; pipeline PR menjalankan format, lint, build, unit test, validasi schema, audit lisensi dependency, dan security static checks dengan hasil bersih; integration test memakai test root ephemeral (bukan direktori Application Support pengguna), port yang dialokasikan, dan fixture credential, dan cleanup hanya menghapus path milik test run itu; job fixture dan job runtime nyata terpisah; tiap test yang membuktikan ID PRD menghasilkan evidence record sesuai PRD bagian 28 (termasuk versi provider, langkah test, hasil, waktu, dan reviewer bila manual) berlabel fixture atau native, dengan status awal `NOT_RUN`; mesin benchmark baseline tercatat.

**Trace:** REQ-051, OQ-011, PRD bagian 27, 28, 31, 32.1, dan 32.3

**Pointer:** spec [0002](../specs/0002-coding-standards-tooling/index.md) · code in `.github/workflows/ci.yml`, `.githooks/pre-commit`, `rustfmt.toml`, `clippy.toml`, `.swift-format`, `deny.toml`, `crates/testkit/`, `crates/evidence/`, `xtask/src/evidence/`, `schemas/evidence/`

- [x] Capture conventions and tooling choices: `/audit`
- [x] Install the tooling: `/develop coding standards & tooling`
- [x] Decide evidence, native tests, and baseline (spec): `/architect coding standards & tooling`
- [ ] Build the evidence standard: `/develop coding standards & tooling`
  - [x] Rust evidence path end to end: `stackly-evidence`, `check-deps` rules, `cargo xtask evidence run`, `linux` and `macos-rust` through the wrapper (REQ-051)
  - [ ] `cargo xtask evidence check` and `evidence new`, shared examples in `schemas/evidence/`
  - [ ] Swift path: `StacklyTestSupport` trait, `macos-swift` through the wrapper
  - [ ] `native.yml`, nightly schedule, fixture credential leak scan
  - [ ] Baseline recorded on the self hosted CI Mac (needs the runner registered, OQ-011)
- [ ] Verify it: `/check verify coding standards & tooling`
- [ ] Test it: `/test coding standards & tooling`

### 3. Sumber & packaging runtime · in-progress

Tentukan dari mana artefak runtime enam provider berasal dan bagaimana mereka dipaketkan supaya bisa dipindah dan dipasang tanpa Homebrew global. Ini risiko terbesar proyek (RISK-01), jadi harus jelas sebelum install runtime (fitur 18) dibangun.

**Done when:** tiap provider wajib punya sumber artefak, dependency closure, versi konkret, status lisensi, dan kebijakan verifikasi serta signing binary (ditandatangani upstream, ditandatangani ulang maintainer, atau ad hoc saat install) yang tercatat; satu executable tiap provider (PostgreSQL 17 dan 18 keduanya) berjalan dari artefak managed di Mac uji tanpa Homebrew global, dijalankan sebagai child process oleh proses noninteractive, bukan dari Terminal; provider yang belum punya artefak managed dicatat `BLOCKED` dengan alasan dan owner; spec memutuskan apakah pipeline artefak buatan maintainer (OQ-003) diperlukan.

**Trace:** OQ-003, OQ-004, RISK-01, RISK-12, AC-078, gate G0, PRD bagian 20.2, 25, dan 33.2 (M0)

**Pointer:** spec [0003](../specs/0003-sumber-packaging-runtime/index.md) · code in `xtask/src/runtime/`, `runtimes/inventory.toml`, `.github/workflows/ci.yml`

- [x] Design it (spec): `/architect sumber & packaging runtime`
- [ ] Build it: `/develop sumber & packaging runtime`
  - [x] Inventory, `runtime pin`, `runtime verify`, and `runtime check` in the `linux` job (spec 0003 AC-1 to AC-3)
  - [ ] LaunchAgent spike end to end, proven on NATS, then Traefik, ClickHouse, MySQL 8.4 and 9.7, with evidence in `evidence/runtime/` (spec 0003 AC-1, AC-3 to AC-10, AC-13; needs spec 0002 step 1 built)
  - [ ] `stackly-runtimes` pipeline for PostgreSQL 17 and 18 and Valkey, published with attestation, then their spikes (spec 0003 AC-1, AC-2, AC-5, AC-7, AC-11, AC-12; needs the repo created first)
  - [ ] Automation: `native.yml` cleanup, cache, and timeout, plus weekly `runtime-watch.yml` (spec 0003 AC-14, AC-15; the CI part needs spec 0002 step 4 built, the self hosted runner registered, and `SELF_HOSTED_MACOS27` set to `true`)
- [ ] Verify it: `/check verify sumber & packaging runtime`
- [ ] Test it: `/test sumber & packaging runtime`

### 4. Domain core & platform abstraction · in-progress

Model domain (software, provider revision, runtime, instance, environment, operation, lease), tiga state machine yang terpisah, invariant bisnis, dan trait platform (termasuk resolver direktori per user untuk owned root, registry, runtimes, instances, logs, cache, retained, dan path socket pendek) dengan fake platform untuk test. Core tidak boleh tahu soal SwiftUI, WinUI, atau command macOS.

**Done when:** domain bisa build tanpa dependency GUI atau API OS; state runtime, lifecycle instance, dan readiness/health terpisah; fake platform menjalankan test invariant di CI; layout direktori per user diputuskan lewat resolver platform tanpa path hardcode dan bisa diarahkan ke root alternatif untuk integration test; tiap direktori instance membawa metadata yang cukup untuk inventarisasi tanpa registry; capability Windows yang belum dibuat mengembalikan not supported.

**Trace:** REQ-001, REQ-050, BR-001 sampai BR-025 (sebagai invariant), NFR-010, AC-050, AC-074, PRD bagian 6, 19.1, dan 20.1

**Pointer:** spec [0004](../specs/0004-domain-core-platform-abstraction/index.md)

- [x] Design it (spec): `/architect domain core & platform abstraction`
- [ ] Build it: `/develop domain core & platform abstraction`
  - [ ] Platform ports and directory layout end to end: `RootSet`, `DirectoryLayout`, `STACKLY_ROOT`, `MacosPlatform`, `UnsupportedPlatform`, testkit `FakePlatform`, and `prepare_layout` (spec 0004 AC-6 to AC-9, AC-12, AC-13, AC-15)
  - [ ] Instance folders, `instance-manifest.json`, and inventory without the registry (spec 0004 AC-5, AC-10 to AC-12, AC-15)
  - [ ] Domain model and state machines: runtime, instance lifecycle, readiness and health, operation, and lease (spec 0004 AC-1 to AC-4, AC-14, AC-16)
  - [ ] Invariants and enforcement: evidence for every BR marked F4, AC-050, AC-074, and NFR-010, plus the `check-deps` source scan (spec 0004 AC-1, AC-5, AC-6, AC-12 to AC-14; uses `stackly-evidence` from spec 0002 step 1)
- [ ] Verify it: `/check verify domain core & platform abstraction`
- [ ] Test it: `/test domain core & platform abstraction`

### 5. Data model registry · needs a decision

Skema SQLite untuk registry: entitas, relasi, constraint, migrasi, dan revision per resource. Model data adalah hal paling mahal untuk diulang, jadi ia diputuskan sebelum fitur apa pun menyimpan state.

**Done when:** semua entitas logis di PRD bagian 17 ada dengan foreign key dan transaksi aktif; migrasi berurutan teruji dari versi sebelumnya, dengan kebijakan forward dan rollback yang tertulis di spec; schema yang setengah teraplikasi terdeteksi saat daemon start tanpa init ulang registry; secret hanya disimpan sebagai reference; nama instance unik per user dan path data memakai ID yang stabil; semua waktu disimpan dalam UTC dan ditampilkan sesuai locale pengguna.

**Trace:** BR-002, BR-003, REQ-048, NFR-005, AC-065 (bagian migrasi), PRD bagian 17

- [ ] Design it (spec): `/architect data model registry`

### 6. Protokol IPC & daemon · needs a decision · GA

Daemon per user dengan protokol berversi di atas encoding dan framing yang dikunci di fitur 1, lewat Unix domain socket privat: handshake, request ID, error terstruktur, event dengan cursor, dan otorisasi user lokal. GUI dan CLI hanya client; hanya daemon yang menulis registry dan mengendalikan lifecycle.

**Done when:** handshake menolak protocol major yang berbeda dan peer dari user lain sebelum ada efek samping; client memverifikasi pemilik endpoint socket sebelum mengirim credential atau mutasi; hanya satu daemon per user yang aktif, daemon kedua berhenti tanpa menyentuh registry, socket, atau proses instance, dan `stackly daemon status` menunjukkan daemon pemiliknya; tidak ada listener TCP admin; batas ukuran pesan dan subscription berlaku; setiap error memakai satu envelope stabil sesuai VAL-020; reconnect memakai snapshot plus cursor; fixture kontrak dipakai bersama oleh Rust dan Swift; fuzz target framing IPC berjalan di CI.

**Trace:** REQ-047, REQ-048, BR-011, VAL-017, VAL-020, NFR-001, NFR-004, NFR-006, NFR-011, AC-047, AC-070, AC-071, AC-079, NFR-009, RISK-03, RISK-09, OQ-005 (generation tooling), PRD bagian 18

- [ ] Design it (spec): `/architect protokol IPC & daemon`

### 7. Operation engine · needs a decision · GA

Mesin operasi async yang dipakai semua mutasi: journal, idempotency key, lock per resource, state revision, progress, cancel, serta primitive cleanup dan kompensasi yang hanya menghapus path dan resource buatan operasi itu di bawah owned root. Inilah yang mencegah runtime atau instance ganda saat GUI dan CLI mengirim perintah bersamaan.

**Done when:** operasi memakai tujuh state di PRD bagian 18.2, dari `queued` sampai `needs_recovery`; dua client dengan idempotency key sama hanya menghasilkan satu efek samping; client yang putus tidak membatalkan pekerjaan; cancel di langkah yang tidak bisa dibalik dilaporkan apa adanya; kompensasi tidak pernah menyentuh direktori di luar buatan operasi itu, walau lewat symlink; resume hanya untuk operasi yang menyatakan bisa dilanjutkan dan tidak mengulang langkah yang tidak idempoten; SQLite tidak dipakai sebagai mutex selama download atau startup.

**Trace:** REQ-048, REQ-054, BR-023, VAL-007, VAL-015 (bagian cleanup), NFR-005, NFR-006, NFR-011, AC-048, AC-054

- [ ] Design it (spec): `/architect operation engine`

### 8. CLI client & kontrak output · needs a decision

CLI `stackly` sebagai client daemon dengan help, `--json`, exit code stabil, mode noninteractive, progress ke stderr, input secret yang aman, dan `--wait` untuk operasi async. Command lain disebut di Done when fitur yang memilikinya dan menempel ke kontrak ini.

**Done when:** `stackly daemon status` serta `stackly operation list`, `inspect`, dan `cancel` bekerja lewat daemon; output `--json` bisa diparse tanpa ANSI atau progress; exit code diturunkan dari kategori error code di envelope fitur 6, bukan dari pesan, dan cocok dengan tabel di PRD bagian 12; CLI tidak pernah menjalankan binary provider sendiri.

**Trace:** REQ-028, BR-011, AC-028, PRD bagian 12

- [ ] Design it (spec): `/architect CLI client & kontrak output`

### 9. Provider engine & manifest schema · needs a decision · GA

Schema manifest provider deklaratif (`stackly/v1alpha1`), loader, validasi, interpolasi terkontrol, dan primitive aman yang menghasilkan execution plan tervalidasi. Kontrak adapter juga mencakup preflight kompatibilitas data dan runtime per provider, supaya tool baru yang cukup dengan primitive yang ada tidak perlu patch core atau GUI.

**Done when:** manifest contoh NATS, Traefik (dengan daftar routes), dan fake provider lolos validasi di pipeline PR, dan fuzz target parser manifest serta template berjalan di CI; manifest yang merujuk primitive atau probe yang tidak dikenal ditolak saat validasi, bukan saat start; schema bisa menyatakan langkah reload (bila capability reload aktif) dan timeout readiness serta recovery per provider, bukan hanya stop timeout; field berisi shell metacharacter tidak pernah dijalankan lewat shell; field keamanan yang tidak dikenal dan capability yang tidak bisa dipenuhi ditolak; membaca catalog tidak menjalankan kode provider.

**Trace:** REQ-029, REQ-031, VAL-002, VAL-012, VAL-018, VAL-019, AC-029, AC-031, NFR-009, RISK-07, RISK-08, PRD bagian 21

- [ ] Design it (spec): `/architect provider engine & manifest schema`

### 10. Process supervisor · needs a decision · GA

Komponen di dalam daemon yang menjalankan executable terverifikasi dengan argv eksplisit, mencatat identitas proses (bukan PID saja), menjalankan probe readiness, dan menghentikan proses secara graceful sesuai strategi provider. Spec ini juga pemilik tunggal rekonsiliasi dasar setelah daemon restart dan restart policy (backoff, jitter, retry budget, reset setelah periode sehat, timeout per provider).

**Done when:** startup transaction di PRD bagian 19.2 berjalan untuk fake provider; process, readiness, health, dan desired state terlihat terpisah; stop normal tidak pernah memakai SIGKILL atau kill berdasarkan nama; setelah daemon restart, proses yang masih hidup hanya diadopsi bila identitas majemuknya cocok, dan identitas yang tidak terbukti berstatus unknown tanpa dibunuh; start tidak pernah memunculkan proses kedua pada data yang sama; provider yang terus crash berhenti di retry budget tanpa busy loop; path dengan spasi dan Unicode diteruskan utuh sebagai argumen.

**Trace:** REQ-011, REQ-012, REQ-013 (dasar), BR-010, AC-011, AC-012, AC-013 (fake provider), AC-058, AC-068, NFR-003, NFR-006, RISK-03, RISK-14, PRD bagian 19

- [ ] Design it (spec): `/architect process supervisor`

### 11. Secret store & redaction · needs a decision · GA

Simpan secret di Keychain dan hanya reference di SQLite atau manifest, lalu terapkan masking yang sama di GUI, CLI, log, diagnostics, dan export. Reveal atau copy secret selalu butuh aksi eksplisit.

**Done when:** sentinel credential tidak muncul di output default, registry, log, atau audit; secret noninteractive masuk lewat input aman, bukan argumen CLI; ada satu alur untuk mengisi atau mengganti nilai di balik secret reference dari GUI dan CLI, yang dipakai ulang oleh create instance, env up, dan restore; Keychain yang terkunci menghasilkan status needs secret, bukan auth kosong.

**Trace:** REQ-016, BR-018, AC-016, AC-062, OQ-008

- [ ] Design it (spec): `/architect secret store & redaction`

_Catatan:_ fitur 12 (Design system & UI foundation) dan 13 (App shell SwiftUI & Swift client) sekarang ada di [ui-macos.md](ui-macos.md), bersama fitur UI lintas layar lain yang diturunkan dari mockup UI. Nomor, urutan, dan statusnya tidak berubah.

## Skeleton

### 14. Walking skeleton

Satu irisan tipis yang membuktikan stack sudah tersambung: fake provider dibuat, dijalankan, dan dihentikan lewat CLI, statusnya tampil di GUI, dan registry mencatatnya. Belum ada download runtime atau provider nyata.

**Done when:** `stackly instance create`, `start`, dan `stop` untuk fake provider berhasil lewat daemon; GUI menampilkan perubahan state lewat event; menutup GUI tidak menghentikan instance; restart daemon saat instance fake berjalan tidak menghasilkan instance ganda atau proses yatim; test fake provider berjalan di pipeline PR.

**Trace:** AC-001 (awal), AC-013 (awal), AC-029 (awal), fixture dummy server di PRD bagian 32.3

- [ ] Build it: `/develop walking skeleton`
