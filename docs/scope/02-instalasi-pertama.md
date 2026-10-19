# Scope 02 · Instalasi pertama

[Kembali ke index](index.md)

Journey PRD JOURNEY-01 dengan PostgreSQL, dibangun lengkap: buka aplikasi, onboarding, izin background, catalog, install runtime, create instance, readiness, lalu copy connection. Fase ini ditutup dengan NATS dasar lewat manifest saja, supaya engine terbukti pada provider kedua sejak awal. Fase ini selesai kalau seluruh jalur itu bekerja dari GUI dan CLI, termasuk state empty, error, unsupported, dan konfirmasi, bukan hanya jalur sukses.

### 15. Background agent & CLI install · in-progress

Daemon berjalan sebagai helper yang dibundel di dalam `.app` dan didaftarkan lewat SMAppService sebagai background agent per user, plus pilihan memasang CLI. Bundling, signing development, dan registrasi helper dibuktikan di sini, bukan baru saat rilis.

**Done when:** helper ditandatangani dengan identity development nyata dan bundle identifier sementara yang dicatat di spec; status `requiresApproval`, `disabled`, dan `notRegistered` terlihat di GUI dan `stackly daemon status`, dan diuji dengan sungguh menolak izin di System Settings; izin yang ditolak menampilkan langkah pemulihan dan tidak diubah tanpa sepengetahuan pengguna; Keychain milik daemon tetap terbaca setelah helper ditandatangani; `stackly daemon install`, `start`, `stop`, dan `restart` bekerja; memasang CLI tidak mengubah PATH global tanpa persetujuan.

**Trace:** REQ-014 (bagian daemon), REQ-028, OQ-001 (identifier sementara), OQ-006, OQ-012, RISK-10, gate G0, NFR-015, PRD bagian 25 dan 32.2

**Pointer:** spec [0015](../specs/0015-background-agent-cli-install/index.md)

- [x] Design it (spec): `/architect background agent & CLI install`
- [ ] Build it: `/develop background agent & CLI install`
  - [ ] Bundle, port, shutdown, and CLI without signing: the ad hoc SMAppService spike (step 0, manual, after your approval because it registers a real agent on this Mac), bundle identifiers and `AppBundle`, the `BackgroundAgent` port with `relaunch`, fake, and `classify`, `daemon.shutdown` with the launch record and exit 75, `MacosBackgroundAgent`, `stackly daemon status`, `install`, `start`, `stop`, `restart`, completion and man page, and `cargo xtask build-app` with ad hoc signing and `--install` (spec 0015 AC-1 to AC-12, AC-21, AC-22; build steps 0 to 7; needs feature 8 built (`01-foundation.md`); after feature 6; steps 1 to 4 can run unattended while step 0 waits for approval, and a step 0 result that disproves an assumption holds steps 5 and 6 until `/architect` update)
  - [ ] launchd without signing, after your approval: temporary LaunchAgent tests in a native run you start (plist derived from the golden file) for restart, stop, crash, and lock takeover, plus instance survival on `kickstart -k` and `bootout` (spec 0015 AC-15, AC-16; build steps 8 and 9; after milestone 1; needs features 10 and 14 (`01-foundation.md`); a failed survival test returns to `/architect` for spec 0010)
  - [ ] GUI and CLI install: `StacklyUI/Agent` with `AgentActionError`, banners per agent state, the Background Agent and Command Line Tool Diagnostics sections, the shared models `BackgroundAgentModel` and `CommandLineToolModel` used by onboarding, and `CommandLineTool` with managed links in `~/.local/bin` (spec 0015 AC-4, AC-11, AC-13, AC-14; build steps 10 to 12; after milestone 1; needs the "Wiring, instances, and operations" milestone of feature 13 and feature 12 (`ui-macos.md`); can run in parallel with milestone 2)
  - [ ] Signing and real approval, BLOCKED until a real signing identity exists (reason `no code signing identity on this Mac (OQ-012)`, owner release owner), every step after your approval: Apple Development build with Team ID, the real SMAppService flow with a real denial in System Settings (branch A or B), Keychain after signing, and the NFR-015 part on a second macOS account (also BLOCKED until that account exists, owner user) (spec 0015 AC-17 to AC-20; build steps 13 to 16; after milestones 1 to 3; needs feature 11 built (`01-foundation.md`))
- [ ] Verify it: `/check verify background agent & CLI install`
- [ ] Test it: `/test background agent & CLI install`

### 16. Onboarding · in-progress

Halaman pertama yang menjelaskan status daemon dan izin background, lokasi storage, model isolasi yang jujur, trust catalog, dan pilihan memasang CLI, tanpa akun cloud.

**Done when:** teks onboarding menjelaskan isolasi logis dan tidak menyebut provider disandbox; state izin dari fitur 15 tampil beserta langkah pemulihan; onboarding bisa dilewati dan dibuka lagi tanpa mengubah state daemon; tampilan mengikuti lima langkah onboarding di mockup UI (Welcome, Background Agent, Storage, Catalog Trust, Command Line), dengan Not Now saat izin belum diberikan dan Get Started di langkah terakhir.

**Trace:** PAGE-01, AC-061, JOURNEY-01

**Pointer:** spec [0016](../specs/0016-onboarding/index.md)

- [x] Design it (spec): `/architect onboarding`
- [ ] Build it: `/develop onboarding`
  - [ ] Onboarding frame, Welcome, and Storage: `OnboardingModel` and the `onboarding.finished` record store, `OnboardingFooter.primaryLabel`, `MainWindowRoot` (window accessor, `openWindow`, and min frame moved from `AppShell`), the Help menu item "Welcome to Stackly", Escape to skip, step dots, focus and announcements, the Welcome step with honest isolation copy, the Storage step with daemon roots, spec 0004 defaults, and the `STACKLY_ROOT` variant, copy tests and the forbidden phrase scan (spec 0016 AC-1 to AC-4, AC-6, AC-9 to AC-11; build steps 1 to 3; needs feature 13 milestone 4 "Wiring, instances, and operations" (`ui-macos.md`) and the spec 0013 proposals accepted; uses feature 12 (`ui-macos.md`, built) and feature 67 components if built)
  - [ ] Background Agent and Command Line steps: the agent card on `BackgroundAgentModel` with `unavailableDetail` and the 15 s `TimelineView`, Start stacklyd only when the daemon is missing, Not Now from the real agent state, and the Command Line step on `CommandLineToolModel` with `bundledTool`, `CommandLineToolPrompts` shared with `CommandLineToolSection` (marker becomes `// provisional: F32`), PATH note, and command examples (spec 0016 AC-2, AC-5, AC-8; build steps 4 and 5; after milestone 1; needs feature 15 milestone 3 "GUI and CLI install" (spec 0015 build steps 10 to 12) with the spec 0015 proposals accepted, and spec 0017 build step 7 (`stackly provider list`); can run in parallel with milestone 3)
  - [ ] Catalog Trust step: one row from `CatalogStore.info`, error, and unsupported, a toggle only when the catalog is loaded, no community row, and the trust note (spec 0016 AC-7; build step 6; after milestone 1; needs spec 0017 build step 8 (`CatalogStore`, feature 17 milestone 3); can run in parallel with milestone 2)
  - [ ] Full walk and manual evidence: the no side effect walk test with fakes, copy tests for every table, previews in light, dark, and 900 × 600 (feature 68 snapshots if built), then manual native evidence on a `cargo xtask build-app --install` bundle, with Turn On pressed only after your approval (spec 0016 AC-3, AC-5, AC-8 to AC-11; build steps 7 and 8; after milestones 1 to 3; needs feature 15 milestone 1 `build-app`; the Turn On step joins spec 0015 AC-17, BLOCKED until a signing identity exists (OQ-012) if ad hoc registration is refused)
- [ ] Verify it: `/check verify onboarding`
- [ ] Test it: `/test onboarding`

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
  - [ ] Bundled catalog and install refusal end to end: catalog location from `AppBundle`, the in process `xtask::catalog::build` step in `cargo xtask build-app` (before signing) with the failing catalog step test, the bundle holds `Contents/Resources/catalog/` with only `official` packages, and F18's `runtime_install_refuses_an_artifact_for_another_target` passes against the `official/linuxonly` fixture with AC-002 evidence (spec 0017 AC-1, AC-2, AC-7; build steps 11 and 12; needs feature 15 `AppBundle` and `build-app` with the catalog step slot in `assemble.rs`, after feature 18 `runtime.install`)
- [ ] Verify it: `/check verify catalog & provider detail`
- [ ] Test it: `/test catalog & provider detail`

### 18. Install runtime managed · in-progress · GA

Resolusi selector (`@17`, `@stable`) ke exact version dan digest, download ke staging, verifikasi digest dan signature, ekstrak aman, validasi binary dan dependency, lalu commit atomik. Termasuk halaman Runtimes untuk melihat hasilnya.

**Done when:** download yang putus lalu diulang hanya menghasilkan runtime utuh yang terverifikasi; artefak dengan digest yang tidak cocok tidak pernah dijalankan; traversal, symlink escape, dan archive bomb ditolak; artefak x86_64 di ARM64 tanpa dukungan ditolak dengan diagnosis; binary yang ditolak OS saat spawn (signature tidak valid atau quarantine) menghasilkan error terdiagnosis, bukan hang; tanpa internet dan tanpa cache muncul status offline yang benar; versi yang dipasang punya compatibility record sesuai PRD bagian 32.2, dan versi tanpa record tetap `UNVERIFIED`; fuzz target ekstraksi archive berjalan di Mac ini; `stackly runtime install`, `list`, dan `inspect` setara dengan halaman Runtimes; halaman Runtimes dan progress install bertahap (Download, Verify digest, Extract, Validate binary, Commit) mengikuti mockup UI.

**Trace:** REQ-003, REQ-004, REQ-005, REQ-028, REQ-046 (cache kosong), VAL-003, VAL-010, VAL-011, PAGE-09, BR-025, AC-003, AC-004, AC-005, AC-049, AC-057, AC-059, AC-060, NFR-009, PRD bagian 27 dan 32.2

**Pointer:** spec [0018](../specs/0018-install-runtime-managed/index.md)

- [x] Design it (spec): `/architect install runtime managed`
- [ ] Build it: `/develop install runtime managed`
  - [ ] Catalog data, safe extractor, and fuzz: inventory and `versions.json` fields (`stable`, `build`, `unpackedSize`, `entryCount`, `dependencies`), manifest `runtime.versionCheck`, `pin` with the core scanner, the `Filesystem` additions and `TreeWriter` with the macOS adapter, the Rust extractor for `tar_gz`, `zip`, and `binary`, the Mach-O parser and closure check, and fuzz targets `runtime_archive` and `runtime_macho` (spec 0018 AC-1, AC-7 to AC-10, AC-19, AC-21; build steps 1 to 5; needs /architect update spec 0009 (the "Usulan update spec 0009" section), after feature 9 closes, and the spec 0003 and 0004 update items; steps 1 and 2 can run as two parallel sessions, then the extractor (step 3) and the Mach-O reader (step 4) as two more parallel sessions; the fuzz targets (step 5) follow as their own session)
  - [ ] Install pipeline in core: registry additions (migration, `ArtifactRecord` fields, `compatibility_records`), `Runtime::committed`, `Fetcher` port with the `ureq` adapter and resumable cache, `CodeSigning` port with the `codesign` adapter, `host_facts`, validation with signing policy, quarantine, and exec check, selector resolution, `support_status` and the spec 0017 gate changes (`&HostFacts`, `requires_newer_os`), commit with orphan reclaim, the `install_runtime` work and factory, and `install_cached_artifact` (spec 0018 AC-1 to AC-6, AC-10 to AC-15; build steps 6 to 9; needs milestone 1, feature 5 registry, feature 7 operation engine, and feature 10 milestone 1 `ProcessSupervision` port and adapter (`01-foundation.md`), feature 17 `core::catalog` (spec 0017 milestone 1, steps 1 to 4; step 11 also needs step 6), and the spec 0005 and 0010 update items; steps 6, 7, and 8 can run as three parallel sessions, and step 6 never runs beside another session writing registry `WriteOp`s)
  - [ ] Daemon, contract, and CLI: `runtime` protocol DTOs, methods, event, and error codes with fixtures and Swift contract tests, `stacklyd` handlers with the catalog gate and precheck, daemon integration tests (including `runtime_install_refuses_an_artifact_for_another_target` against the spec 0017 fixtures `official/linuxonly` and `official/intelonly`, for AC-002 and AC-005), `stackly runtime install`, `list`, and `inspect` with golden files, and the compatibility record generator with `cargo xtask runtime compat-record` (spec 0018 AC-1 to AC-3, AC-6, AC-10, AC-13 to AC-16, AC-18; build steps 10 to 13; needs milestone 2, feature 6 daemon server and feature 8 CLI framework (`01-foundation.md`); after step 10, steps 11, 12, and 13 can run as three parallel sessions)
  - [ ] Runtimes page and the Get button: `StacklyClient` runtime methods, `RuntimeStore`, `RuntimesPage` with Installing, table, locked remove button, and inspector, `RuntimeGetAction` in the Catalog version matrix, "{n} installed" on the Catalog provider card, and the runtime status table in `apps/macos/design.md` (spec 0018 AC-17, AC-18; build steps 14 and 15; needs milestone 3, feature 12 and feature 13 milestone 4 (`ui-macos.md`), and the feature 17 Catalog page)
  - [ ] Native proof: native install tests through the daemon from the xtask spike cache (no network) for every available line whose provider package exists: NATS and Traefik, PostgreSQL 17 and 18 after spec 0019 step 1, and MySQL, ClickHouse, and Valkey recorded `BLOCKED` `no provider package (features 40 to 43)`; no quarantine on installed files, no developer tools spawned, the `runtime spike --cleanup` extension, then `nats_installs_from_the_network` after your approval, and 10 minute fuzz runs with manual evidence (spec 0018 AC-5, AC-6, AC-12, AC-19, AC-20; build steps 16, 16a, and 17; needs milestone 3; step 16a needs your approval for the network run; the fuzz runs (step 17) can run as their own parallel session right after milestone 1)
- [ ] Verify it: `/check verify install runtime managed`
- [ ] Test it: `/test install runtime managed`
- [ ] Review it (fresh model): `/check review install runtime managed`
- [ ] Document it: `/document install runtime managed`

### 19. Provider PostgreSQL · in-progress

Manifest dan adapter PostgreSQL resmi: `initdb` dari runtime yang cocok, auth lokal yang aman, locale dan encoding eksplisit, endpoint dan Unix socket per instance, readiness lewat query terautentikasi dengan role berhak minimum, dan shutdown terkontrol. Termasuk deteksi versi mayor data directory dan preflight kompatibilitas data dan runtime yang dipakai start, rebind, adoption, dan restore.

**Done when:** cluster baru terinisialisasi tanpa default `trust` yang tidak disengaja; readiness gagal kalau query gagal walau TCP terbuka; stop normal memakai mode shutdown provider; start atau rebind binary mayor berbeda pada data yang sudah ada ditolak sebelum proses dijalankan; tool admin memakai runtime yang cocok, bukan `psql` dari PATH.

**Trace:** REQ-035 (dasar), BR-009, AC-035 (bagian penolakan), JOURNEY-01, PRD bagian 22.1

**Pointer:** spec [0019](../specs/0019-provider-postgresql/index.md)

- [x] Design it (spec): `/architect provider PostgreSQL`
- [ ] Build it: `/develop provider PostgreSQL`
  - [ ] Package and additive core changes on fixtures: `postgresqlConf` and `plain` template formats, `runExecutable` `stdin.text` in the plan, `x-stackly.printableOnly` checked by `SecretService::put` (minor protocol bump for the `reason` detail key), the `providers/postgresql` package with generated `versions.json`, `provider check`, golden render and plans, and fuzz seeds (spec 0019 AC-1 without the catalog part, AC-2 and AC-3 plan parts, AC-4 fixture part, AC-12 plan part; build steps 1 to 4; needs the feature 9 build in `01-foundation.md`, and step 3 needs spec 0011 step 4 (`SecretService::put`); steps 1, 2, and 3 can run as three parallel sessions, step 4 after them)
  - [ ] PostgreSQL 18 end to end through the daemon: `Stdin::Text` in the init executor, the `exec` probe failure detail format, the `pin_package` fixture, an in process recording `LogSink` hook, `runtime.versionCheck` in the manifest before any native run, and native tests for init, auth without trust, locale, endpoints and `ALTER SYSTEM`, honest readiness, fast stop, init failure, decoy `PATH`, reload, adoption, and the `postgres -C` round trip (spec 0019 AC-2 to AC-10, AC-12, AC-13, AC-15; build steps 5 to 8; after milestone 1; needs feature 10 built through its milestone 4 and feature 11 done in `01-foundation.md`, spec 0018 milestone 2 (`install_cached_artifact`), and step 7 needs spec 0018 step 1 (`runtime.versionCheck` loader); native tests need both PostgreSQL artifacts in the xtask runtime cache, else `BLOCKED`)
  - [ ] PostgreSQL 17, major mismatch refusal, evidence, and catalog: the suite for 17 with the catalog entry moved from `[[planned]]` to `[[provider]]` in the same commit, refusal in both directions plus a missing or changed `PG_VERSION`, and native evidence with `compatibility.json` per artifact from the final manifest (spec 0019 AC-1 catalog part, AC-5 to AC-11, AC-13, AC-14; build steps 9 to 11; after milestone 2; step 9 needs the feature 17 build (`providers/catalog.toml` and its ledger), step 11 needs spec 0018 step 13 (milestone 3, `cargo xtask runtime compat-record`))
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

### 22. Overview & daftar instance · in-progress

Halaman Overview (ringkasan ready, stopped, failed, operasi aktif, peringatan kapasitas) dan Instances (cari, filter, urutkan; nama, provider, exact version, endpoint, lifecycle, readiness, ownership).

**Done when:** keduanya menangani state loading, empty, error, stale, dan partial; error di satu instance tidak membuat seluruh layar kosong; ringkasan tidak dihitung dari PID saja; peringatan kapasitas memakai sumber yang terdefinisi (ruang disk) atau tampil unavailable sampai fitur 28 ada; `stackly instance list` dan `inspect` setara dengan GUI; layar bisa dipakai dengan keyboard dan VoiceOver; Overview dan Instances mengikuti mockup UI, termasuk daftar Needs Attention yang menautkan ke layar perbaikan, filter status, tabel yang bisa diurutkan, inspector yang bisa disembunyikan, dan state hasil filter kosong.

**Trace:** PAGE-02, PAGE-03, REQ-028, NFR-002, NFR-012, AC-027

**Pointer:** spec [0022](../specs/0022-overview-daftar-instance/index.md)

- [x] Design it (spec): `/architect overview & daftar instance`
- [ ] Build it: `/develop overview & daftar instance`
  - [ ] Daemon contract: `Filesystem::volume_space` with the `statfs` adapter and fake, capacity levels in domain and core, the `storage` protocol module with `storage.get_capacity` (recovery allowed, single flight, 5 s timeout) and `storage.capacity_unavailable`, `InstanceSummary.ownership` and `initialized`, wire fixtures in `protocols/fixtures/ipc/` plus the shared test data `groups.json`, `list-mixed.json`, and `attention-operations.json`, the protocol minor bump (next free minor at merge), and daemon tests (spec 0022 AC-1, AC-4, AC-12; build steps 1 to 4; needs feature 10 build through spec 0010 steps 12 and 13 and feature 6 done (`01-foundation.md`), and the spec 0004 and 0010 update items unless steps 1 and 3 add them; steps 1 and 2 can run as a parallel session beside step 3)
  - [ ] `stackly instance list` and `inspect`: shared status groups, name or UUID resolution, text and JSON output with counts, `cli.instance_not_found`, goldens from `list-mixed.json`, and a binary test against the real `stacklyd` (spec 0022 AC-1, AC-10, AC-11; build step 5; after milestone 1; needs feature 8 build (`01-foundation.md`), and after feature 14 only while feature 14 is changing `commands/instance`; can run in parallel with milestone 3)
  - [ ] Overview and Instances pages: pure models (status groups, attention rules, search, sort, actions, operation titles, `CapacityModel`), `ResourceStore.itemFailures`, retained filtering, `ShellNavigator` and `newInstanceAction`, the Instances table with filter, search, sort, inspector, actions with the provisional feedback banner, and View menu commands, the Overview cards, Needs Attention, Instances and Operations sections, every page and section state, replacing `ProvisionalInstanceList` and `UnbuiltPage(.overview)`, and the Overview and Instances tables in `apps/macos/design.md` (spec 0022 AC-1 to AC-9, AC-13; build steps 6 to 8; after milestone 1; needs feature 13 milestone 4 step 16 of spec 0013 (the `statusSeq` patch rule, which waits on spec 0010 step 12) and the rest of that milestone plus feature 12 (`ui-macos.md`), and spec 0017 milestone 3 (`CatalogStore`, `ProviderTile`) of feature 17; can run in parallel with milestone 2)
  - [ ] Runtimes card and evidence: the Runtimes card and install operation titles from `RuntimeStore`, the NFR-002 performance tests, and manual keyboard, VoiceOver, 640 pt, small disk image, and daemon stop evidence (spec 0022 AC-2, AC-5, AC-13, AC-14; build steps 9 and 10; step 9 after spec 0018 step 14 (`RuntimeStore`) of feature 18; step 10 after milestones 2 and 3)
- [ ] Verify it: `/check verify overview & daftar instance`
- [ ] Test it: `/test overview & daftar instance`

### 23. Provider NATS lewat manifest · in-progress

NATS dasar sebagai provider nyata kedua, hanya lewat manifest, template, dan primitive dari fitur 9, ditambah probe `nats-readiness` sebagai satu perubahan core yang ditinjau: install, create satu instance, start, readiness lewat probe NATS, stop, dan form GUI dari settings schema. Ini membuktikan engine tidak hanya cocok untuk PostgreSQL (RISK-07) sebelum fitur lain menumpuk di atasnya.

**Done when:** satu instance NATS berjalan lewat journey yang sama dengan PostgreSQL tanpa kode Rust atau Swift khusus NATS; readiness memakai probe NATS nyata, bukan TCP terbuka saja; perubahan core yang tetap dibutuhkan dicatat sebagai ukuran RISK-07.

**Trace:** REQ-029, REQ-036 (dasar), AC-029 (provider nyata), RISK-07, PRD bagian 21.2, 22.2, dan 33.2

**Pointer:** spec [0023](../specs/0023-provider-nats-manifest/index.md)

- [x] Design it (spec): `/architect provider NATS lewat manifest`
- [ ] Build it: `/develop provider NATS lewat manifest`
  - [ ] Native probe kind and `nats-readiness` on fixtures: the step 0 spike that records real NATS 2.15.1 transcripts from the xtask cache, the `native` probe kind in raw, validate, plan, and JSON Schema with the closed `NativeProbeKind` registry, the NATS package readiness swap with goldens and fuzz seeds, the sans IO `Exchange` state machine, the `nats_readiness_protocol` fuzz target (10 minutes, plus a rerun of `provider_manifest` and `provider_interpolation`), the generic duplex testable driver with `ProbeOutcome`, and the readiness and health loop (spec 0023 AC-1 to AC-7; build steps 0 to 6; step 0 needs only the cached artifact; steps 1 and 2 need the spec 0009 `architect (update)` item carrying spec 0023's *Usulan update spec 0009*, after feature 9 closes (`01-foundation.md`); steps 3 and 4 need neither spec update and can start right after step 0; steps 5 and 6 need feature 10's "Start to ready and safe stop on the fake provider" milestone (`01-foundation.md`) and steps 1 and 3; after step 0, steps 1 and 3 can run as two parallel sessions, then step 2 after 1 and step 4 after 3)
  - [ ] NATS 2.15.1 end to end through the daemon, evidence, and catalog: `nats_lifecycle_in_process` (Unicode data root, `statusSeq` order, `lsof`, `/varz`, log line, clean stop), `nats_compatibility` through `SpawnedDaemon` (honest readiness with `SIGSTOP`, foreign servers for identity, JetStream mode, and auth, an instance with JetStream off, adoption after `SIGKILL`), `compatibility.json` and `compat-record`, and the NATS catalog entry moved from `[[planned]]` to `[[provider]]` with the ledger in the evidence commit (spec 0023 AC-8 to AC-12; build steps 7 to 9; after milestone 1; needs feature 10 built through "Daemon, IPC, and end to end proof" and feature 11 done (`01-foundation.md`), spec 0018 milestone 2 ("Install pipeline in core", `install_cached_artifact`), feature 17 milestone "Catalog from repo to daemon", and `pin_package` plus the recording `LogSink` hook from spec 0019 step 6 (or built here first); step 9 also needs spec 0018 step 1 (NATS `runtime.versionCheck`, milestone "Catalog data, safe extractor, and fuzz") and step 13 (`compat-record`, milestone "Daemon, contract, and CLI"); one session, since steps 7 to 9 share the test file; native tests need the NATS artifact in the xtask cache, else `BLOCKED`)
  - [ ] Same journey from CLI and GUI, and the RISK-07 check: the `nats_journey` CLI test (create with defaults and with JetStream off, start, get, stop), extra assertions in spec 0020's `create-form-nats.json` Swift render test, the GUI New Instance and Summary pass prepared for verify, and the RISK-07 ledger with added lines per commit and numstat (spec 0023 AC-13, AC-14; build steps 10 to 12; after milestone 2; needs the feature 20 build (create form, `instance.get_create_form`, the fixture, `stackly instance create`) and the feature 21 build; steps 10 and 11 can run as two parallel sessions)
- [ ] Verify it: `/check verify provider NATS lewat manifest`
- [ ] Test it: `/test provider NATS lewat manifest`
