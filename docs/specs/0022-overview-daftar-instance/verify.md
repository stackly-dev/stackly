# Verify: Overview dan daftar instance · spec 0022 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0022. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah; saat ini hanya Milestone 1._

## Milestone 1: Daemon contract

### UI / manual

- [ ] Buka `protocols/error-codes.json` → area `storage` milik `spec 0022` dengan satu kode, `storage.capacity_unavailable`, kategori `unavailable`, `retryable` true, `details` `reason` dan `kind` → AC-4, AC-12
- [ ] Buka `protocols/fixtures/instance/` → `groups.json` (30 kasus: 10 lifecycle kali 3 state readiness, `retained` selalu `null`), `list-mixed.json` (20 instance, empat kelompok `ready`, delapan `notReady`, empat `stopped`, tiga `failed`, satu `unknown`), `attention-operations.json` (lima operasi `needs_recovery` dengan hint `resume`, `abandon`, `manual`, `auto_compensate`, dan hint asing, ditambah operasi `running`, `queued`, `cancel_requested`, dan `succeeded`), dan `index.json` yang mendaftar ketiganya → AC-1
- [ ] Buka `clients/swift/Sources/StacklyClient/Generated/OpenEnums.swift` → `CapacityLevel` punya fallback `.unrecognized` → AC-12

### Commands

- [ ] `cargo test -p stackly-testkit --test fake_fs` → 20 test lulus, termasuk `a_delay_slows_every_call_of_one_operation_until_it_is_removed` (jeda `FakeFilesystem` per operasi), `volume_space_answers_from_the_longest_matching_prefix`, injeksi error `VolumeSpace`, dan suite `fs_conformance` → AC-4
- [ ] `cargo test -p stackly-domain capacity` → batas `level` di 1 999 999 999 dan 2 000 000 000 tersedia, 4 999 999 999 dan 5 000 000 000 tersedia (volume 20 GB supaya aturan persen tidak ikut), 79, 80, 89, 90 persen terpakai; `total` 0 memberi `None`; `available` di atas `total` dijepit → AC-4
- [ ] `cargo test -p stackly-core --lib storage` → 6 test lulus: sebuah pembacaan memuat ukuran, persen, level, dan waktu; batas level; `available` dijepit; `total` 0 menjadi `NoSize`; error `io` membawa kind `Other`, `PermissionDenied`, dan `NotFound` → AC-4
- [ ] `cargo test -p stackly-protocol --lib` → 165 test lulus, termasuk tabel area `storage` (pemilik `spec 0022`), `storage.capacity_unavailable` di daftar kode retryable, `storage.get_capacity` bertanda `recovery allowed`, dan `CURRENT` 1.9 → AC-12
- [ ] `cargo test -p stackly-protocol --test ipc_fixtures` → setiap fixture baru (`request-storage-get-capacity`, `frame-response-storage-get-capacity`, `-critical`, `-unknown-level`, dan tiga `frame-error-storage-capacity-unavailable-*`) berperilaku sesuai `index.json`; setiap method punya fixture request dan reply → AC-12
- [ ] `cargo test -p stackly-protocol --test storage_fixtures` → 11 test lulus: request tanpa params, laporan `ok` dan `critical`, level asing menjadi `Unrecognized`, ketiga error membawa `reason` dan hint statis hanya untuk `timeout`, `groups.json` sama dengan tabel spec, `list-mixed.json` dan `attention-operations.json` terbaca dan `list-mixed.json` berbentuk kanonik → AC-1, AC-12
- [ ] `cargo xtask codegen --check` → "20 generated files are up to date"; `protocols/contract-versions.json` memuat protokol 1.9 dan `protocols/fixtures/protocol-version.json` 1.9 → AC-12
- [ ] `swift test --package-path clients/swift` → 177 test lulus, termasuk `StorageContractTests` (laporan, level asing, error dan hint, `groups.json`, `list-mixed.json`, `attention-operations.json`), `ContractTests.protocolIsOnePointNine`, dan `IpcFixtureTests.everyCallVariantHasARoundTripFixture` dengan 25 method. Kegagalan di `StacklyXCTestSupportTests` yang bernama `Probe` adalah probe sengaja gagal milik suite itu, bukan hasil perubahan ini → AC-12
- [ ] `cargo test -p stacklyd --test server storage` → 6 test lulus: level di setiap batas lewat `FakeFilesystem` dengan waktu dari clock platform, `available` dijepit, `io` (kind `other` dan `permission_denied`) dan `no_size`, tiga call bersamaan memakai satu pembacaan, `timeout` dengan hint dan call berikutnya selama `statfs` macet dijawab seketika tanpa thread kedua, dan jawaban di recovery mode tanpa `mutations()` baru serta registry tidak berubah → AC-4
- [ ] `cargo test -p stacklyd --lib storage` → 2 test lulus: nama kind I/O dalam snake case, dan setiap kegagalan dipetakan ke kode katalog dengan `reason` → AC-4
- [ ] `cargo xtask fuzz seed` → `fuzz/seeds/ipc_request_decode/` dan `fuzz/seeds/ipc_server_frame_decode/` memuat fixture storage baru → AC-12
- [ ] `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus; `stackly_domain::capacity` tidak memakai tokio atau libc → AC-4

### Value sourcing

- [ ] `level` dan `usedPercent` dihitung `stackly_domain::capacity` dari `total` dan `available`; variasikan volume palsu di setiap batas level (langkah `cargo test -p stacklyd --test server storage`) → AC-4
- [ ] `checkedAt` datang dari clock platform saat pembacaan selesai; majukan `FakeClock` di antara dua call dan bandingkan → AC-4
- [ ] `total` dan `available` datang dari `Filesystem::volume_space` pada root data `RootSet` (`PreparedLayout::canonical_root(Data)`), bukan root runtime, log, atau cache → AC-4

### Acceptance-criteria coverage

- AC-1: data test bersama `groups.json`, `list-mixed.json`, dan `attention-operations.json` ada dan terbaca di Rust dan Swift (milestone ini); fungsi `InstanceGroup.of` dan `instance::group::of` serta tesnya milik milestone 2 dan 3.
- AC-4: sisi daemon lengkap (port `volume_space`, level di domain, core `storage::capacity`, handler dengan single flight dan timeout, test daemon). `CapacityModel` dan item S1 sampai S4 di GUI milik milestone 3, dan cek disk APFS kecil milik milestone 4.
- AC-12: kontrak lengkap (modul `storage`, method `storage.get_capacity` `recovery allowed`, `storage.capacity_unavailable` dengan hint, fixture, protokol 1.9, codegen, test Rust dan Swift); penanganan nilai asing di GUI dan CLI milik milestone 2 dan 3.
