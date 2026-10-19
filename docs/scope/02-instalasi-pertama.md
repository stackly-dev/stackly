# Scope 02 · Instalasi pertama

[Kembali ke index](index.md)

Journey PRD JOURNEY-01 dengan PostgreSQL, dibangun lengkap: buka aplikasi, onboarding, izin background, catalog, install runtime, create instance, readiness, lalu copy connection. Fase ini ditutup dengan NATS dasar lewat manifest saja, supaya engine terbukti pada provider kedua sejak awal. Fase ini selesai kalau seluruh jalur itu bekerja dari GUI dan CLI, termasuk state empty, error, unsupported, dan konfirmasi, bukan hanya jalur sukses.

### 15. Background agent & CLI install · needs a decision

Daemon berjalan sebagai helper yang dibundel di dalam `.app` dan didaftarkan lewat SMAppService sebagai background agent per user, plus pilihan memasang CLI. Bundling, signing development, dan registrasi helper dibuktikan di sini, bukan baru saat rilis.

**Done when:** helper ditandatangani dengan identity development nyata dan bundle identifier sementara yang dicatat di spec; status `requiresApproval`, `disabled`, dan `notRegistered` terlihat di GUI dan `stackly daemon status`, dan diuji dengan sungguh menolak izin di System Settings; izin yang ditolak menampilkan langkah pemulihan dan tidak diubah tanpa sepengetahuan pengguna; Keychain milik daemon tetap terbaca setelah helper ditandatangani; `stackly daemon install`, `start`, `stop`, dan `restart` bekerja; memasang CLI tidak mengubah PATH global tanpa persetujuan.

**Trace:** REQ-014 (bagian daemon), REQ-028, OQ-001 (identifier sementara), OQ-006, OQ-012, RISK-10, gate G0, NFR-015, PRD bagian 25 dan 32.2

- [ ] Design it (spec): `/architect background agent & CLI install`

### 16. Onboarding · needs a decision

Halaman pertama yang menjelaskan status daemon dan izin background, lokasi storage, model isolasi yang jujur, trust catalog, dan pilihan memasang CLI, tanpa akun cloud.

**Done when:** teks onboarding menjelaskan isolasi logis dan tidak menyebut provider disandbox; state izin dari fitur 15 tampil beserta langkah pemulihan; onboarding bisa dilewati dan dibuka lagi tanpa mengubah state daemon; tampilan mengikuti lima langkah onboarding di mockup UI (Welcome, Background Agent, Storage, Catalog Trust, Command Line), dengan Not Now saat izin belum diberikan dan Get Started di langkah terakhir.

**Trace:** PAGE-01, AC-061, JOURNEY-01

- [ ] Design it (spec): `/architect onboarding`

### 17. Catalog & provider detail · in-progress

Catalog bawaan (official) yang menampilkan publisher, revision, kategori, OS/arch, versi, trust, status kompatibilitas, dependency, kebutuhan disk, dan capability. Kombinasi yang unsupported terlihat jujur, bukan disembunyikan. Di baseline, catalog ikut terpasang bersama aplikasi dan hanya berubah lewat update aplikasi (fitur 61) atau import lokal (fitur 37); sinkronisasi catalog online ada di Deferred.

**Done when:** provider untuk platform lain berlabel unsupported dengan alasan dan install ditolak oleh daemon, bukan hanya oleh UI; catalog menampilkan waktu pembaruan terakhir dari catalog terpasang dan tidak mengklaim lebih baru dari itu; `stackly provider list` dan `inspect` setara dengan GUI; layar Catalog mengikuti mockup UI: filter kategori, pencarian, kartu provider dengan status, panel detail dengan matrix versi per OS, capability yang tidak didukung ditulis not supported, lisensi, sumber, ukuran unduhan, dan provider planned tampil redup tanpa tombol install.

**Trace:** REQ-002, REQ-033 (catalog bawaan), PAGE-08, BR-016, BR-025, NFR-014, AC-002, OQ-009, OQ-012

**Pointer:** spec [0017](../specs/0017-catalog-provider-detail/index.md)

- [x] Design it (spec): `/architect catalog & provider detail`
- [ ] Build it: `/develop catalog & provider detail`
  - [ ] Catalog from repo to daemon: `SupportStatus`, `providers/catalog.toml` with all six PRD 5.2 providers as planned, the revision ledger `providers/catalog-revisions.json`, `cargo xtask catalog build` and `check` (plus the pre commit step), strict `catalog.json` loading with per entry isolation and the `official` only namespace guard, status rules with the provisional F18 stand in, the install gate, `catalog.list_providers` and `catalog.get_provider` with codes and fixtures, the `catalog` subsystem, `STACKLY_CATALOG_DIR`, `CatalogBuilder`, and the default empty catalog in `SpawnedDaemon` (spec 0017 AC-1 to AC-8, AC-13; build steps 1 to 6; needs `/architect` update spec 0009: `ValidatedPackage::versions()` accessor, after feature 9 closes)
  - [ ] `stackly provider list` and `inspect` with text and JSON output and goldens shared with the Swift tests (spec 0017 AC-9; build step 7; needs feature 8 build in `01-foundation.md`; can run in parallel with the Catalog screen)
  - [ ] Catalog screen: `CatalogAppearance` and its table in `apps/macos/design.md`, `CatalogStore`, detail model, search, category filter, cards, detail panel with version matrix and capabilities, every `ViewState` shape, replacing `UnbuiltPage(section: .catalog)`, and manual keyboard and VoiceOver evidence (spec 0017 AC-9 to AC-12; build steps 8 to 10; needs feature 13 milestone 4 in `ui-macos.md`, uses feature 67 components if built; can run in parallel with the CLI)
  - [ ] Bundled catalog and install refusal end to end: catalog location from `AppBundle`, the bundle from `cargo xtask build-app` holds `Contents/Resources/catalog/` with only `official` packages, and F18's `runtime_install_refuses_an_artifact_for_another_target` passes against the `official/linuxonly` fixture with AC-002 evidence (spec 0017 AC-1, AC-2, AC-7; build steps 11 and 12; needs feature 15 `AppBundle` and `build-app`, after feature 18 `runtime.install`)
- [ ] Verify it: `/check verify catalog & provider detail`
- [ ] Test it: `/test catalog & provider detail`

### 18. Install runtime managed · needs a decision · GA

Resolusi selector (`@17`, `@stable`) ke exact version dan digest, download ke staging, verifikasi digest dan signature, ekstrak aman, validasi binary dan dependency, lalu commit atomik. Termasuk halaman Runtimes untuk melihat hasilnya.

**Done when:** download yang putus lalu diulang hanya menghasilkan runtime utuh yang terverifikasi; artefak dengan digest yang tidak cocok tidak pernah dijalankan; traversal, symlink escape, dan archive bomb ditolak; artefak x86_64 di ARM64 tanpa dukungan ditolak dengan diagnosis; binary yang ditolak OS saat spawn (signature tidak valid atau quarantine) menghasilkan error terdiagnosis, bukan hang; tanpa internet dan tanpa cache muncul status offline yang benar; versi yang dipasang punya compatibility record sesuai PRD bagian 32.2, dan versi tanpa record tetap `UNVERIFIED`; fuzz target ekstraksi archive berjalan di Mac ini; `stackly runtime install`, `list`, dan `inspect` setara dengan halaman Runtimes; halaman Runtimes dan progress install bertahap (Download, Verify digest, Extract, Validate binary, Commit) mengikuti mockup UI.

**Trace:** REQ-003, REQ-004, REQ-005, REQ-028, REQ-046 (cache kosong), VAL-003, VAL-010, VAL-011, PAGE-09, BR-025, AC-003, AC-004, AC-005, AC-049, AC-057, AC-059, AC-060, NFR-009, PRD bagian 27 dan 32.2

- [ ] Design it (spec): `/architect install runtime managed`

### 19. Provider PostgreSQL · in-progress

Manifest dan adapter PostgreSQL resmi: `initdb` dari runtime yang cocok, auth lokal yang aman, locale dan encoding eksplisit, endpoint dan Unix socket per instance, readiness lewat query terautentikasi dengan role berhak minimum, dan shutdown terkontrol. Termasuk deteksi versi mayor data directory dan preflight kompatibilitas data dan runtime yang dipakai start, rebind, adoption, dan restore.

**Done when:** cluster baru terinisialisasi tanpa default `trust` yang tidak disengaja; readiness gagal kalau query gagal walau TCP terbuka; stop normal memakai mode shutdown provider; start atau rebind binary mayor berbeda pada data yang sudah ada ditolak sebelum proses dijalankan; tool admin memakai runtime yang cocok, bukan `psql` dari PATH.

**Trace:** REQ-035 (dasar), BR-009, AC-035 (bagian penolakan), JOURNEY-01, PRD bagian 22.1

**Pointer:** spec [0019](../specs/0019-provider-postgresql/index.md)

- [x] Design it (spec): `/architect provider PostgreSQL`
- [ ] Build it: `/develop provider PostgreSQL`
  - [ ] Package and additive core changes on fixtures: `postgresqlConf` and `plain` template formats, `runExecutable` `stdin.text` in the plan, `x-stackly.printableOnly` checked by `SecretService::put` (minor protocol bump for the `reason` detail key), the `providers/postgresql` package with generated `versions.json`, `provider check`, golden render and plans, and fuzz seeds (spec 0019 AC-1 without the catalog part, AC-2 and AC-3 plan parts, AC-4 fixture part, AC-12 plan part; build steps 1 to 4; needs the feature 9 build in `01-foundation.md`, and step 3 needs spec 0011 step 4 (`SecretService::put`); steps 1, 2, and 3 can run as three parallel sessions, step 4 after them)
  - [ ] PostgreSQL 18 end to end through the daemon: `Stdin::Text` in the init executor, the `exec` probe failure detail format, the `pin_package` fixture, an in process recording `LogSink` hook, and native tests for init, auth without trust, locale, endpoints and `ALTER SYSTEM`, honest readiness, fast stop, init failure, decoy `PATH`, reload, adoption, and the `postgres -C` round trip (spec 0019 AC-2 to AC-10, AC-12, AC-13; build steps 5 to 7; after milestone 1; needs feature 10 built through its milestone 4 and feature 11 done in `01-foundation.md`, and spec 0018 milestone 2 (`install_cached_artifact`); native tests need both PostgreSQL artifacts in the xtask runtime cache, else `BLOCKED`)
  - [ ] PostgreSQL 17, major mismatch refusal, evidence, and catalog: the suite for 17 with the catalog entry moved from `[[planned]]` to `[[provider]]` in the same commit, refusal in both directions plus a missing or changed `PG_VERSION`, native evidence with `compatibility.json` per artifact, and `runtime.versionCheck` (spec 0019 AC-1 catalog part, AC-5 to AC-11, AC-13 to AC-15; build steps 8 to 11; after milestone 2; step 8 needs the feature 17 build (`providers/catalog.toml`), step 10 needs `cargo xtask runtime compat-record` from feature 18, step 11 needs the spec 0009 update from feature 18)
- [ ] Verify it: `/check verify provider PostgreSQL`
- [ ] Test it: `/test provider PostgreSQL`

### 20. Create instance · needs a decision · GA

Wizard dan command untuk membuat instance: ID stabil, nama, runtime, endpoint, auth, persistence, storage, dan autostart, dengan preflight dan error per field. Field settings, endpoint, auth, dan storage dirender dari settings schema dan resources manifest provider (kelas field VAL-012), tidak dikodekan khusus untuk PostgreSQL. Inisialisasi hanya terjadi di direktori baru milik operasi itu.

**Done when:** direktori yang sudah berisi data ditolak sebelum `initdb` berjalan; nama mengikuti VAL-001; port fixed yang konflik menghasilkan error, dan auto port dipersistenkan setelah bind berhasil; overlap IPv4/IPv6 dan wildcard terdeteksi; bind default ke loopback; path tidak keluar dari owned root, dan fuzz target validasi path berjalan di Mac ini; fake provider dari fitur 14 mendapat form create tanpa kode SwiftUI khusus provider, field secret tampil sebagai input aman, dan field `immutable-after-init` ditandai; alur bisa diselesaikan dengan keyboard dan VoiceOver tanpa informasi yang hanya tersedia lewat warna atau hover; sheet New Instance mengikuti mockup UI: preflight tampil sebelum Create (runtime terverifikasi, nama tersedia, endpoint bebas di loopback IPv4 dan IPv6, direktori data kosong, ruang disk, secret di Keychain), konflik port fixed menawarkan port bebas tanpa menggantinya otomatis, dan perintah CLI yang setara bisa disalin.

**Trace:** REQ-008, REQ-010, REQ-029 (bagian form), PAGE-04, BR-002, BR-004, BR-005, BR-008, VAL-001, VAL-004 sampai VAL-008, VAL-012, AC-008, AC-027, AC-029 (bagian form UI), AC-055, AC-056, NFR-009, NFR-012

- [ ] Design it (spec): `/architect create instance`

### 21. Start, readiness & connection

Menjalankan instance, menunggu readiness, dan menampilkan ringkasan instance: ID, versi dan digest, path dan ukuran data, endpoint, connection string yang dimasking, serta tombol copy. Inilah titik akhir JOURNEY-01.

**Done when:** status process dan readiness tampil terpisah; `stackly instance connection --format uri` menyembunyikan password secara default; copy endpoint dan reveal secret butuh aksi eksplisit; dengan jaringan mati, instance dengan runtime yang sudah terpasang tetap bisa start, menampilkan status, dan memberi connection dari GUI dan CLI tanpa menyentuh catalog atau cek trust online; GUI dan CLI menunjukkan hasil yang sama; alur bisa diselesaikan dengan keyboard dan VoiceOver; tab Summary mengikuti mockup UI: endpoint dengan tombol copy, readiness terpisah, connection string dengan Show Password dan Copy, secret reference, dependents, dan detail (ID, runtime, digest, provider revision, path dan ukuran data, uptime, restarts).

**Trace:** REQ-012, REQ-046 (offline), PAGE-05, BR-010, AC-012, AC-027, AC-046, NFR-012, JOURNEY-01

- [ ] Build it: `/develop start, readiness & connection`

### 22. Overview & daftar instance · needs a decision

Halaman Overview (ringkasan ready, stopped, failed, operasi aktif, peringatan kapasitas) dan Instances (cari, filter, urutkan; nama, provider, exact version, endpoint, lifecycle, readiness, ownership).

**Done when:** keduanya menangani state loading, empty, error, stale, dan partial; error di satu instance tidak membuat seluruh layar kosong; ringkasan tidak dihitung dari PID saja; peringatan kapasitas memakai sumber yang terdefinisi (ruang disk) atau tampil unavailable sampai fitur 28 ada; `stackly instance list` dan `inspect` setara dengan GUI; layar bisa dipakai dengan keyboard dan VoiceOver; Overview dan Instances mengikuti mockup UI, termasuk daftar Needs Attention yang menautkan ke layar perbaikan, filter status, tabel yang bisa diurutkan, inspector yang bisa disembunyikan, dan state hasil filter kosong.

**Trace:** PAGE-02, PAGE-03, REQ-028, NFR-002, NFR-012, AC-027

- [ ] Design it (spec): `/architect overview & daftar instance`

### 23. Provider NATS lewat manifest · needs a decision

NATS dasar sebagai provider nyata kedua, hanya lewat manifest, template, dan primitive dari fitur 9, ditambah probe `nats-readiness` sebagai satu perubahan core yang ditinjau: install, create satu instance, start, readiness lewat probe NATS, stop, dan form GUI dari settings schema. Ini membuktikan engine tidak hanya cocok untuk PostgreSQL (RISK-07) sebelum fitur lain menumpuk di atasnya.

**Done when:** satu instance NATS berjalan lewat journey yang sama dengan PostgreSQL tanpa kode Rust atau Swift khusus NATS; readiness memakai probe NATS nyata, bukan TCP terbuka saja; perubahan core yang tetap dibutuhkan dicatat sebagai ukuran RISK-07.

**Trace:** REQ-029, REQ-036 (dasar), AC-029 (provider nyata), RISK-07, PRD bagian 21.2, 22.2, dan 33.2

- [ ] Design it (spec): `/architect provider NATS lewat manifest`
