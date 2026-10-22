# Verify: Overview dan daftar instance · spec 0022 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0022. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah; saat ini Milestone 1, 2, dan 3._

## Milestone 1: Daemon contract

### UI / manual

- [x] Buka `protocols/error-codes.json` → area `storage` milik `spec 0022` dengan satu kode, `storage.capacity_unavailable`, kategori `unavailable`, `retryable` true, `details` `reason` dan `kind` → AC-4, AC-12
- [x] Buka `protocols/fixtures/instance/` → `groups.json` (30 kasus: 10 lifecycle kali 3 state readiness, `retained` selalu `null`), `list-mixed.json` (20 instance, empat kelompok `ready`, delapan `notReady`, empat `stopped`, tiga `failed`, satu `unknown`), `attention-operations.json` (lima operasi `needs_recovery` dengan hint `resume`, `abandon`, `manual`, `auto_compensate`, dan hint asing, ditambah operasi `running`, `queued`, `cancel_requested`, dan `succeeded`), dan `index.json` yang mendaftar ketiganya → AC-1
- [x] Buka `clients/swift/Sources/StacklyClient/Generated/OpenEnums.swift` → `CapacityLevel` punya fallback `.unrecognized` → AC-12

### Commands

- [x] `cargo test -p stackly-testkit --test fake_fs` → 20 test lulus, termasuk `a_delay_slows_every_call_of_one_operation_until_it_is_removed` (jeda `FakeFilesystem` per operasi), `volume_space_answers_from_the_longest_matching_prefix`, injeksi error `VolumeSpace`, dan suite `fs_conformance` → AC-4
- [x] `cargo test -p stackly-domain capacity` → batas `level` di 1 999 999 999 dan 2 000 000 000 tersedia, 4 999 999 999 dan 5 000 000 000 tersedia (volume 20 GB supaya aturan persen tidak ikut), 79, 80, 89, 90 persen terpakai; `total` 0 memberi `None`; `available` di atas `total` dijepit → AC-4
- [x] `cargo test -p stackly-core --lib storage` → 6 test lulus: sebuah pembacaan memuat ukuran, persen, level, dan waktu; batas level; `available` dijepit; `total` 0 menjadi `NoSize`; error `io` membawa kind `Other`, `PermissionDenied`, dan `NotFound` → AC-4
- [x] `cargo test -p stackly-protocol --lib` → 165 test lulus, termasuk tabel area `storage` (pemilik `spec 0022`), `storage.capacity_unavailable` di daftar kode retryable, `storage.get_capacity` bertanda `recovery allowed`, dan `CURRENT` 1.9 → AC-12
- [x] `cargo test -p stackly-protocol --test ipc_fixtures` → setiap fixture baru (`request-storage-get-capacity`, `frame-response-storage-get-capacity`, `-critical`, `-unknown-level`, dan tiga `frame-error-storage-capacity-unavailable-*`) berperilaku sesuai `index.json`; setiap method punya fixture request dan reply → AC-12
- [x] `cargo test -p stackly-protocol --test storage_fixtures` → 11 test lulus: request tanpa params, laporan `ok` dan `critical`, level asing menjadi `Unrecognized`, ketiga error membawa `reason` dan hint statis hanya untuk `timeout`, `groups.json` sama dengan tabel spec, `list-mixed.json` dan `attention-operations.json` terbaca dan `list-mixed.json` berbentuk kanonik → AC-1, AC-12
- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `protocols/contract-versions.json` memuat protokol 1.9 dan `protocols/fixtures/protocol-version.json` 1.9 → AC-12
- [x] `swift test --package-path clients/swift` → 177 test lulus, termasuk `StorageContractTests` (laporan, level asing, error dan hint, `groups.json`, `list-mixed.json`, `attention-operations.json`), `ContractTests.protocolIsOnePointNine`, dan `IpcFixtureTests.everyCallVariantHasARoundTripFixture` dengan 25 method. Kegagalan di `StacklyXCTestSupportTests` yang bernama `Probe` adalah probe sengaja gagal milik suite itu, bukan hasil perubahan ini → AC-12
- [x] `cargo test -p stacklyd --test server storage` → 6 test lulus: level di setiap batas lewat `FakeFilesystem` dengan waktu dari clock platform, `available` dijepit, `io` (kind `other` dan `permission_denied`) dan `no_size`, tiga call bersamaan memakai satu pembacaan, `timeout` dengan hint dan call berikutnya selama `statfs` macet dijawab seketika tanpa thread kedua, dan jawaban di recovery mode tanpa `mutations()` baru serta registry tidak berubah → AC-4
- [x] `cargo test -p stacklyd --lib storage` → 2 test lulus: nama kind I/O dalam snake case, dan setiap kegagalan dipetakan ke kode katalog dengan `reason` → AC-4
- [x] `cargo xtask fuzz seed` → `fuzz/seeds/ipc_request_decode/` dan `fuzz/seeds/ipc_server_frame_decode/` memuat fixture storage baru → AC-12
- [x] `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus; `stackly_domain::capacity` tidak memakai tokio atau libc → AC-4

### Value sourcing

- [x] `level` dan `usedPercent` dihitung `stackly_domain::capacity` dari `total` dan `available`; variasikan volume palsu di setiap batas level (langkah `cargo test -p stacklyd --test server storage`) → AC-4
- [x] `checkedAt` datang dari clock platform saat pembacaan selesai; majukan `FakeClock` di antara dua call dan bandingkan → AC-4
- [x] `total` dan `available` datang dari `Filesystem::volume_space` pada root data `RootSet` (`PreparedLayout::canonical_root(Data)`), bukan root runtime, log, atau cache → AC-4

### Acceptance-criteria coverage

- AC-1: data test bersama `groups.json`, `list-mixed.json`, dan `attention-operations.json` ada dan terbaca di Rust dan Swift (milestone ini); fungsi `InstanceGroup.of` dan `instance::group::of` serta tesnya milik milestone 2 dan 3.
- AC-4: sisi daemon lengkap (port `volume_space`, level di domain, core `storage::capacity`, handler dengan single flight dan timeout, test daemon). `CapacityModel` dan item S1 sampai S4 di GUI milik milestone 3, dan cek disk APFS kecil milik milestone 4.
- AC-12: kontrak lengkap (modul `storage`, method `storage.get_capacity` `recovery allowed`, `storage.capacity_unavailable` dengan hint, fixture, protokol 1.9, codegen, test Rust dan Swift); penanganan nilai asing di GUI dan CLI milik milestone 2 dan 3.

## Milestone 2: stackly instance list and inspect

### UI / manual

- [x] Di root sementara yang sudah di-seed (`cargo run -q -p stacklyd --example skeleton_seed -- --root <R>`) dengan `stacklyd` jalan dan dua instance fake, jalankan `STACKLY_ROOT=<R> stackly instance list` → tabel `NAME` sampai `OWNERSHIP` urut nama, baris kosong, lalu `2 instances: 1 running, 1 stopped` sebagai baris terakhir; tanpa instance stdout hanya `No instances.` → AC-10
- [x] Jalankan `stackly instance list --status failed` saat tidak ada instance gagal → `No failed instances.` lalu baris jumlah, keluar 0 → AC-10
- [x] Jalankan `stackly instance inspect <nama>`, lalu `stackly instance inspect <UUID huruf besar>`, lalu `stackly instance get <nama>` → blok `kv`, tabel endpoint, dan teks yang sama di ketiganya → AC-11
- [x] Jalankan `stackly instance inspect tidak-ada` → keluar 10, `cli.instance_not_found`, pesan `no instance named tidak-ada`, hint `list instances with stackly instance list` → AC-11

### Commands

- [x] `cargo test -p stackly-cli --lib group` → tiga test lulus: setiap pasangan lifecycle dan state readiness di `protocols/fixtures/instance/groups.json` (30 kasus, `retained` selalu `None`) masuk ke kelompok yang sama dengan tabel spec, filter Running sama dengan `ready` ditambah `notReady`, dan baris jumlah hanya menyebut kelompok di atas 0 (`1 instance:` untuk satu) → AC-1, AC-10
- [x] `cargo test -p stackly-cli --lib resolve` → UUID 36 karakter bertanda hubung (huruf besar atau kecil) menjadi ID, argumen lain menjadi nama → AC-11
- [x] `cargo build -p stacklyd -p stackly-fake-server` lalu `cargo test -p stackly-cli --test instance` → semua test lulus, termasuk golden `instance_list.txt`, `instance_list.json`, `instance_list_failed.txt`, `instance_list_empty.txt`, `instance_inspect.txt`, `instance_inspect.json` (dari `list-mixed.json`, `TZ=UTC`), semua halaman `instance.list` dibaca (limit 200), halaman yang gagal di tengah menggagalkan perintah tanpa tabel, nama tidak ketemu keluar 10 tanpa `instance.get`, daemon mati keluar 6, dan satu test terhadap `stacklyd` asli dengan dua instance fake → AC-1, AC-10, AC-11
- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `cli.instance_not_found` ada di `protocols/error-codes.json` dan digest kontrak tidak berubah → AC-11
- [x] `cargo clippy -p stackly-cli -p stackly-protocol --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, dan `cargo xtask check-deps` → lulus

### Value sourcing

- [x] Kelompok datang dari `lifecycle` dan `readiness.state` saja (`instance::group::of`); ubah `process`, `endpoints`, atau `restart` di summary dan kelompok tidak berubah → AC-1
- [x] `counts` di `--json` dihitung dari semua halaman, bukan dari `items` yang sudah disaring `--status`; bandingkan `--status failed --json` dengan `list --json` → AC-10
- [x] Nama dicari lewat semua halaman `instance.list` (`name` sama persis, huruf besar kecil dibedakan); `inspect Cache` tidak menemukan `cache` → AC-11

### Acceptance-criteria coverage

- AC-1: sisi CLI lengkap (`instance::group::of`, `InstanceCounts`, `StatusFilter`, test atas `groups.json`); sisi Swift (`InstanceGroup.of`) milik milestone 3.
- AC-10: lengkap (tabel, baris jumlah, `--json`, `--status`, semua halaman, kasus kosong dan gagal, golden). Test view model Swift atas `list-mixed.json` milik milestone 3.
- AC-11: lengkap (UUID, nama, `cli.instance_not_found`, `resource.not_found` dari daemon, exit 6, alias `get`). Key `detail`, `dataUsage`, dan flag `--size` milik spec 0021.

## Milestone 3: Overview and Instances pages

### UI / manual

- [ ] Jalankan app Debug dengan `STACKLY_UI_SCENARIO=connected-mixed` dan buka Overview → empat kartu menjadi tiga (Running, Stopped, Failed; kartu Runtimes menunggu milestone 4), Running memuat "{n} not ready", angka Failed merah, subtitle toolbar "{ready} running · {stopped} stopped · ...", Needs Attention memuat enam item dengan "Show {n} More", Instances enam baris dengan "Show All {n} Instances", dan Operations hanya operasi yang berjalan → AC-2, AC-3, AC-5
- [ ] Klik kartu Failed → Instances terbuka dengan filter Failed, pencarian kosong, dan jumlah baris sama dengan angka kartu; ulangi untuk Running (jumlah baris sama dengan `ready` ditambah `notReady`) dan Stopped → AC-2
- [ ] Di Instances: ketik "5432 pg" di pencarian → hanya instance yang cocok di kedua kata; klik header Version dua kali → urutan naik lalu turun dengan nilai kosong selalu di akhir; menu View > Sort Instances By memberi checkmark di kolom dan arah yang aktif; ⌥⌘I menyembunyikan dan menampilkan inspector → AC-6, AC-7, AC-13
- [ ] Pilih instance gagal → inspector memuat banner "{name} failed" dengan cuplikan output per baris; pilih instance running → tombol Stop; menu Instance > Stop (⇧⌘R) menghentikannya tanpa konfirmasi dan tombolnya sibuk sampai operasi selesai → AC-7, AC-8
- [ ] Hanya keyboard (Keyboard navigation menyala): Tab ke kartu Failed, Space, pilih baris dengan panah, ⌥⌘I, Tab ke tombol aksi di inspector, Space → AC-13 (evidence manual penuh milik milestone 4)
- [ ] Lebar konten 640 pt: Overview satu kolom tanpa teks terpotong; Instances dengan inspector tertutup menampilkan Name dan Status tanpa menggulir; jendela 900 pt dengan inspector terbuka menampilkan Name, Status, dan aksi → AC-13

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "InstanceGroupTests|InstanceTextTests|InstanceSearchSortTests|InstanceActionTests|InstanceActionModelTests"` → semua kasus `groups.json`, jumlah dan filter dari `list-mixed.json`, teks port, pencarian banyak kata, setiap pembanding naik dan turun, setiap baris tabel aksi, klik kedua saat `inFlight` tanpa kiriman, key dipakai ulang setelah `outcomeUnknown` (revision sama, di bawah 5 menit) lalu dibuang, `pending` dibersihkan oleh state akhir dan oleh resnapshot, dan umpan balik `rejected`, `notSent`, `outcomeUnknown`, operasi `failed` dan `cancelled` → AC-1, AC-6, AC-8
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "AttentionRulesTests|OperationTitleTests|CapacityModelTests"` → setiap aturan I1 sampai I10, O1, dan S1 sampai S4 dari `list-mixed.json`, `attention-operations.json`, dan laporan kapasitas; urutan item, `id` stabil, satu item per instance; judul setiap `OperationKind`; panggilan kapasitas satu per satu, balasan lama diabaikan, poll 60 detik hanya saat tersambung, `unknown_method` menjadi unsupported → AC-3, AC-4
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "ResourceStoreTests|InstanceStoreTests|ShellNavigatorTests"` → `itemFailures` terisi saat refetch gagal dan bersih saat item diterapkan, dihapus, atau resnapshot selesai; `retry` mengulang pada revision kegagalan dan menghapus item yang kini hilang; `ShellNavigator` dengan permintaan Instances → AC-9
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "InstancesPageTests|OverviewPageTests"` → state halaman (loading, failed, waiting, unsupported, stale, partial, empty, no results), gerbang seksi Needs Attention dan Operations, kata inspector, `ReadinessText`, dan perintah yang diterbitkan halaman → AC-2, AC-5, AC-6, AC-7, AC-9
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "InstancesSnapshotTests|OverviewSnapshotTests"` → 16 state Instances dan 14 state Overview cocok dengan referensi di `SnapshotTests/__Snapshots__/` pada enam varian → AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "SourceRuleTests|CommandMapTests|CommandSourceRuleTests"` → file baru lulus aturan sumber (tanpa `.secondary`, font tetap, `.inspector` di luar komponen), setiap halaman punya suite snapshot, dan baris `toggleInspector`, `sortInstances…`, `startInstance`, `stopInstance` built dengan tepat satu `CommandItem` → AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → 1022 test di 99 suite; satu kegagalan waktu, `SkeletonEndToEndTests.aProcessThatOutlivesItsLimitIsKilledAndReportedAsTimedOut` (23 detik dari batas 20) saat load rata rata mesin di atas 40, lulus saat suite itu dijalankan sendiri; `swift test --package-path clients/swift` → 185 test Swift Testing dan 35 XCTest lulus → AC-9
- [x] `cargo xtask build-app` → `** BUILD SUCCEEDED **`; `xcrun swift-format lint --strict` pada file Swift yang berubah bersih → AC-13

### Value sourcing

- [x] Kelompok, jumlah, dan filter datang dari `lifecycle` dan `readiness.state` lewat `InstanceGroup.of`; di `list-mixed.json` `analytics-db` (proses hidup, `not_ready`) terhitung `notReady` dan ada di filter Running tetapi bukan di angka Running kartu → AC-1, AC-2
- [x] Judul, isi, tone, dan tujuan Needs Attention datang dari tabel aturan di `AttentionRules`; ubah `restart.nextAttemptAt` ke hari lain dan jam I7 menyertakan tanggal; `lastFailure.code` memilih kalimat I1 lewat `InstanceFailureCopy` → AC-3
- [x] Level dan ukuran disk datang dari `CapacityReport` daemon lewat `CapacityModel`; GUI tidak menghitung ambang, dan laporan sukses terakhir tetap dipakai saat panggilan berikutnya gagal → AC-4
- [x] Nama tampil dan tile provider datang dari entri `CatalogStore` dengan `provider` sama dengan `InstanceSummary.provider.name`; tanpa entri baris memakai `software` dan tile abu abu → AC-6
- [x] Teks readiness datang dari `ReadinessText`, hanya untuk `running` dan `recovering`; uptime dari `process.startedAt` dikurangi `fixtureNow ?? context.date` → AC-7
- [x] Judul operasi datang dari `OperationTitle` (`kind`, `resource.id`, nama instance dari `InstanceStore`), persen install dari `StagedProgressValue.from` atas lima tahap spec 0018 → AC-5
- [x] Status menunggu tombol datang dari `accepted.operation.id`, `OperationCenter.updates(for:)`, `OperationCenter.active` setelah resnapshot, dan satu `operation.get` → AC-8

### Acceptance-criteria coverage

- AC-1: sisi Swift lengkap (`InstanceGroup.of`, `InstanceCounts`, test atas `groups.json`); sisi CLI di milestone 2.
- AC-2: kartu Running, Stopped, dan Failed lengkap dengan angka tertunda, keterangan not ready, dan navigasi berfilter. Kartu Runtimes menunggu `RuntimeStore` (milestone 4, langkah 9).
- AC-3: lengkap (aturan, urutan, batas enam, Show More, gerbang sumber). Event `instance.health_changed` memperbarui daftar lewat `InstanceStore` (test spec 0013); jalur lengkap dibuktikan di cek manual milestone 4.
- AC-4: sisi GUI lengkap (`CapacityModel`, S1 sampai S4); cek disk APFS kecil milik milestone 4.
- AC-5: seksi Instances dan Operations lengkap dengan state seksi; judul dan persen `install_runtime` dari baris `installing` menunggu `RuntimeStore` (milestone 4, langkah 9), sampai itu judul memakai "Install runtime".
- AC-6, AC-7, AC-8: halaman Instances, inspector, dan aksi lengkap. Penerbitan perintah menu lewat `CommandMap` (Follow-up fitur 69).
- AC-9: state halaman, state seksi, `itemFailures`, dan Retry lengkap; daemon mati dibuktikan di cek manual milestone 4.
- AC-13: bagian otomatis (label VoiceOver, aturan sumber, snapshot enam varian, 640 pt dan 664 pt); evidence manual keyboard dan VoiceOver milik milestone 4.
