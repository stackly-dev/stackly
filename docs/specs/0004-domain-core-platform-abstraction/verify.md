# Verify: Domain core & platform abstraction · spec 0004 · updated 2026-10-18

_Langkah diturunkan dari AC-1 sampai AC-16 dan tabel Value sourcing spec 0004. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Fitur ini berupa crate library plus `stacklyd` yang hanya me-resolve root lalu keluar, jadi "menjalankan app" berarti menjalankan binary `stacklyd` dan `cargo xtask check-deps` sungguhan, plus API publik lewat test di `FakePlatform` dan di filesystem asli Mac ini. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Probe yang mengubah file selalu dikembalikan sesudahnya (`git status` bersih lagi). Folder override dibuat di scratch, mode 0700, dan tidak pernah di dalam root data default._

## UI / manual

Tidak ada. Fitur ini tidak punya layar; `stacklyd` dan `check-deps` diperiksa di bagian Commands.

## Commands

### Batas crate dan portabilitas

- [x] `cargo metadata --no-deps --format-version 1` → dependency normal `stackly-domain` hanya `jiff`, `serde`, `thiserror`, `uuid` (dev: `serde_json`, `stackly-evidence`); `stackly-platform` tanpa tokio dan libc (normal: `jiff`, `stackly-domain`, `thiserror`; dev: `stackly-evidence`) → AC-1
- [x] `cargo xtask check-deps` → exit 0, "10 crates follow the dependency, unsafe, and source rules" → AC-1, AC-6
- [x] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 → AC-1, AC-13
- [x] Tambahkan sementara `std::env::var_os("HOME")` di `crates/core/src/daemon.rs` dan `"/opt/homebrew"` di `crates/platform/src/info.rs`, jalankan `cargo xtask check-deps` → exit 1 dengan `stackly-platform crates/platform/src/info.rs:24: library crates never hardcode a machine path (...)` dan `stackly-core crates/core/src/daemon.rs:18: library crates never read the environment (...)`; kembalikan, run berikutnya exit 0 → AC-6
- [x] `cargo test -p xtask --bin xtask check_deps` → 19 test lulus: aturan tokio dan libc untuk `stackly-platform`, workspace fixture yang membaca env dan memuat `/opt/homebrew` gagal dengan file dan baris, target bin ikut dipindai sedangkan `tests/`, `examples/`, dan `benches/` tidak, modul lewat `#[path]` diikuti, kode `#[cfg(not(test))]` tetap dipindai, kode test, doc comment, `env!`, dan fungsi senama seperti `home_dir()` milik adapter macOS tidak ikut tertangkap, dan kelima crate yang dipindai memang anggota workspace → AC-1, AC-6

### Resolver macOS dan root alternatif (binary `stacklyd` asli)

- [x] `env -u STACKLY_ROOT HOME=/nonexistent target/debug/stacklyd` → exit 0, `data_root` `/Users/ojiepermana/Library/Application Support/Stackly`, `logs_root` dan `cache_root` di `Library/Logs/Stackly` dan `Library/Caches/Stackly`, `runtime_root` sama dengan `getconf DARWIN_USER_TEMP_DIR` ditambah `stackly`, `root_override: false`; tidak ada folder yang dibuat → AC-6
- [x] `STACKLY_ROOT=<scratch>/root target/debug/stacklyd` (folder 0700) → exit 0, satu baris log dengan `R/data`, `R/logs`, `R/cache`, runtime root `<darwin user temp>/stackly-e7759b69`, `root_override: true`; folder override tetap kosong → AC-7
- [x] `STACKLY_ROOT=` → exit 1, `root override "" rejected: empty`; `STACKLY_ROOT=rel/x` → exit 1, `not absolute`; folder 0775 → exit 1, `writable by group or others`; symlink ke folder override → exit 1, `is a symlink`. Folder 0755 diterima, sesuai aturan `mode & 0o022 == 0` → AC-7
- [x] `cargo test -p stacklyd` → 4 test lulus (override diterima dan keluar 0, home dari database user bukan `HOME`, kosong ditolak, relatif atau tidak ada ditolak) → AC-6, AC-7
- [x] `cargo test -p stackly-testkit` → 32 test lulus, termasuk validasi override dengan `FakeIdentity` (user lain, 0777, symlink, root data default dan isinya, root default lewat symlink) → AC-7, AC-12

### Port platform, layout, dan Windows

- [x] `cargo test -p stackly-platform` → 21 test lulus: `under_override`, FNV-1a `"a"` = `af63dc4c8601ec8c`, runtime root 76 byte diterima dan 77 ditolak, 1000 ID acak muat 103 byte, layout murni → AC-7, AC-8
- [x] `cargo test -p stackly-platform unsupported` → `windows_capabilities_report_not_supported` lulus: delapan capability not supported dengan reason `windows adapter not built`, `clock` supported, dan setiap method directory, filesystem, dan identity menjawab `NotSupported` → AC-13
- [x] `cargo test -p stackly-domain --test native_support` → host Windows dengan artefak macOS saja atau linux saja bernilai false → AC-14

### Domain

- [x] `cargo test -p stackly-domain` → 115 test lulus → AC-2, AC-3, AC-4, AC-14, AC-16
- [x] `runtime_and_lifecycle_tables_are_exhaustive` → setiap pasangan (state, event, payload) runtime dan lifecycle sesuai tabel, pasangan lain `TransitionError` tanpa mengubah state, setiap kode state bolak balik → AC-2
- [x] `readiness_and_health_follow_the_lifecycle`, `running_process_alone_is_not_ready`, `stale_ready_reads_as_unknown`, test `observation::tests` → aturan lifecycle, `awaiting_probe`, `not_running`, observasi lama diabaikan, `stale` setelah `max_age` → AC-3
- [x] `operation_table_is_exhaustive` dan `operation::tests` → tujuh state, cancel sebelum dan sesudah pernah mulai, state akhir menolak semua event, `ProgressError` → AC-4
- [x] `instance_aggregate.rs` (4 test) dan `instance::tests` → lahir `uninitialized` dan desired `stopped`, `start_begin` ditolak BR-008 sebelum `init_succeeded`, rebind hanya saat `stopped` dan untuk software yang sama, 100 observasi tidak menaikkan revision, rename menaikkan tepat 1 → AC-16

### Core di FakePlatform

- [x] `cargo test -p stackly-core` → 46 test lulus → AC-5, AC-9, AC-10, AC-11
- [x] `cargo test -p stackly-core layout` → prepare membuat root dan subfolder 0700, run kedua tanpa mutasi, root milik user lain, symlink, 0755, file, subfolder symlink, dan root bertumpuk ditolak, parent root tidak dibuat → AC-9
- [x] `cargo test -p stackly-core create` dan `failed_create` → manifest ditulis sebelum folder lain dengan `initializedAt` null, folder di `instances/` atau `retained/` ditolak (BR-008), short key yang sama `ShortKeyTaken`, gagal di tengah mengembalikan `Partial` dengan tepat root, manifest, dan `data` tanpa penghapusan (BR-023) → AC-10
- [x] `cargo test -p stackly-core inventory` → urutan aktif lalu retained per nama, setiap kelas entri kotor (manifest hilang, rusak, 64 KiB plus 1, FIFO, symlink, folder, schema v2, ID beda, lokasi beda, symlink ke folder lain, `not-an-id`, huruf besar, `notes.txt`) terklasifikasi, `.DS_Store` dilewati, mutasi dan pohon disk tidak berubah → AC-11
- [x] `cargo test -p stackly-core --test fake_platform` → `core_use_cases_run_on_the_fake_platform` menjalankan semua use case publik di `FakePlatform` tanpa pelanggaran dan semua mutasi di dalam test root; di `UnsupportedPlatform` windows, `prepare_layout` menjawab `NotSupported` → AC-12

### Filesystem asli Mac ini

- [x] `cargo test -p stackly-platform-macos --test layout` → 12 test lulus: root default dari database user dan temp dir user, override memindahkan semua root, `fs_conformance` untuk `MacosFilesystem`, empat capability supported dan lima menyebut fitur pemiliknya, prepare, create, dan inventaris lewat `MacosPlatform::with_roots(test_root.roots())` tanpa menyentuh root default, folder yang ada dan alias data root ditolak, manifest yang bukan file biasa tidak dibuka, tulis atomik gagal tanpa file `.tmp-` → AC-6, AC-12, AC-15

### Evidence

- [x] `cargo xtask evidence run --kind fixture --out <scratch>/ev -- cargo test -p stackly-domain -p stackly-platform -p stackly-core` → 27 record fixture, semua PASS, 0 masalah: satu record per nama test di tabel BR (20 test untuk 18 BR berstatus F4) dan tabel test lain (REQ-012 dua, REQ-054, REQ-053, AC-050 dengan REQ-050, AC-074, NFR-010), nama persis sama dengan spec → AC-5, AC-12, AC-13, AC-14
- [x] Error penolakan BR membawa ID lewat `rule()`: test BR-002, 005, 007, 008 (domain dan core), 009, 010, 013, 016, 017, dan 023 memeriksa `rule()` → AC-5
- [x] `cargo xtask evidence check` → "baseline valid, 120 Rust and 39 Swift file(s) follow the rules" → AC-5

## Value sourcing

- [x] Home dari `pw_dir`, bukan `HOME`: `stacklyd` dengan `HOME=/nonexistent` tetap mencatat `/Users/ojiepermana/...` → resolver macOS, home directory
- [x] Temp dir user dari `confstr`: `runtime_root` sama dengan `getconf DARWIN_USER_TEMP_DIR` ditambah `stackly` → resolver macOS, temp dir user
- [x] `STACKLY_ROOT` dibaca sekali di `main` `stacklyd` sebagai `OsString`, kosong ditolak; library tidak membaca env (dijaga `check-deps`) → resolver macOS, root override
- [x] `fnv8`: FNV-1a 64 bit atas path canonical folder override, dihitung terpisah dengan Python, menghasilkan `e7759b69`, sama dengan runtime root dari `stacklyd` → `under_override`, `fnv8`
- [x] User saat ini dari `UserIdentity::current_user`; pemilik, mode, jenis, dan identitas dari `symlink_metadata` (`lstat`): override 0775 dan symlink ditolak di binary asli, user lain lewat `FakeIdentity` → `prepare_layout`, `validate_root_override`
- [x] Nama file sementara `.<nama>.tmp-<pid>-<counter>` (`crates/platform-macos/src/fs.rs:106`), dan tidak ada yang tertinggal setelah tulis gagal → `write_file_atomic`
- [x] ID instance dibuat pemanggil dengan `InstanceId::generate()`; `ShortKeyTaken` saat short key sama dengan instance di `known`; `known` di test berasal dari `inventory_instances` → `create_instance_layout`
- [x] `createdAt`, `updatedAt`, `retainedAt`, dan `data.initializedAt` dari `Clock::now` (`FakeClock` di test, dicek di `inventory_reads_folders_without_the_registry` dan `core_use_cases_run_on_the_fake_platform`) → manifest, waktu
- [x] `createdByOperation` dan `byOperation` dari `OperationId::generate()` di test (operasi nyata menunggu fitur 7) → manifest, operasi
- [x] `stacklyVersion` dari `daemon_info().version`, diisi pemanggil lewat `record_write` sebelum tulis ulang; core menulis manifest apa adanya → manifest, `stacklyVersion`
- [x] `data.initializedLine` dan `initializedVersion` dari line dan versi runtime saat create (`new_manifest_starts_uninitialized_active_and_unchanged_since_create`) → manifest, data
- [x] `data_initialized_at` dari `at` di `apply_lifecycle(init_succeeded, at)`, `last_failure` dari payload event, `started_at` dari `start` pertama → `Instance`, `Operation`
- [x] `LineCompatibility`, `accepted_diff_sha256`, `RuntimeReferences`, `session`, dan kandidat auto port semuanya parameter (test BR-009, BR-017, BR-007, BR-014, BR-015, BR-024, BR-005) → domain
- [x] `effective` memakai `now` dari parameter dan default `READINESS_MAX_AGE` 30 detik di core (`a_ready_observation_goes_stale_after_the_default_max_age`) → readiness dan health
- [x] Reason not supported adalah konstanta adapter, short key 8 karakter terakhir ID, anggaran socket 103, 15, dan 76 (`budget_constants_match_the_spec`), entri inventaris hanya dari isi disk → capability, `ShortKey`, anggaran socket, inventaris

## Acceptance-criteria coverage

- AC-1: batas crate, `check-deps`, cek target Windows, test check-deps
- AC-2: `runtime_and_lifecycle_tables_are_exhaustive`
- AC-3: test readiness dan health di domain
- AC-4: `operation_table_is_exhaustive` dan `operation::tests`
- AC-5: run evidence 27 record, pemeriksaan `rule()`, `evidence check`
- AC-6: `stacklyd` tanpa override, probe `check-deps`, test check-deps, test platform-macos
- AC-7: `stacklyd` dengan override, kosong, relatif, 0775, symlink; test stacklyd dan testkit
- AC-8: test `stackly-platform` (76 dan 77 byte, 1000 ID acak)
- AC-9: `cargo test -p stackly-core layout`
- AC-10: test create di core
- AC-11: test inventaris di core
- AC-12: `core_use_cases_run_on_the_fake_platform`, `fs_conformance` untuk fake dan macOS, run evidence
- AC-13: `windows_capabilities_report_not_supported`, cek target Windows
- AC-14: `windows_native_support_needs_a_windows_artifact`
- AC-15: `cargo test -p stackly-platform-macos --test layout`
- AC-16: `instance_aggregate.rs` dan `instance::tests`

## Pilihan build yang belum tercatat di spec

Dibuat sesi build di tempat spec diam atau longgar. Sesi rekonsiliasi mencatatnya di spec 0004; daftar ini hanya penunjuk.

- `Instance` punya `data_version: ExactVersion` di samping `data_line`, karena `record_data_migration(operation, line, version)` dan `data.initializedVersion` butuh versi.
- `Runtime::apply(event, at)`: `committed` mengisi `installed_at`, `verified` dan `reverified` mengisi `last_verified_at`. `apply` menolak `removed`; removal hanya lewat `Runtime::remove(refs)`.
- `Endpoint::Ip(IpEndpoint)` dan `Endpoint::Unix(UnixEndpoint)` membungkus struct, jadi `resolve_port(&IpEndpoint, ...)` tidak bisa menerima endpoint Unix.
- `MemberAlias` dan `EnvironmentName` adalah value type baru dengan pola `InstanceName`.
- `InventoryEntry::Found { location, manifest: Box<InstanceManifest> }`, dibungkus `Box` supaya clippy `large_enum_variant` lolos.
- `read_instance_manifest` dan `write_instance_manifest` juga menerima `&Platform`, seperti semua use case core.
- `write_instance_manifest` menulis manifest apa adanya; pemanggil memanggil `record_write(clock.now(), daemon_info().version)` sebelum tulis ulang.
- `READINESS_MAX_AGE` ada di modul core baru `observation`, bertipe `std::time::Duration`, jadi core tidak butuh `jiff`.
- Pindaian `check-deps` ada di `xtask/src/check_deps/source_scan.rs`: mengikuti pohon modul dari target lib dan bin, melewati atribut (termasuk doc comment), membaca token di dalam macro, menandai `use ...env::*`, dan memeriksa literal byte string serta C string juga. Nomor baris memakai fitur `span-locations` milik `proc-macro2`, yang kini jadi dependency langsung xtask.
