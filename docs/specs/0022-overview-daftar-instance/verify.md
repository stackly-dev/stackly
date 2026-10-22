# Verify: Overview dan daftar instance · spec 0022 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0022. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah; saat ini Milestone 1 sampai 4._

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

Status checker 2026-10-22: keenam langkah di atas `NOT_RUN` (kind `native`), karena butuh aplikasi nyata, keyboard, atau VoiceOver yang hanya user yang menjalankan. Bukti pengganti dari snapshot (`fixture`): `overview-mixed` memuat tiga kartu dengan "3 not ready" dan Failed merah, Needs Attention enam item dengan "Show 3 More", Instances enam baris, dan Operations dengan dua operasi; `instances-failed-chosen` memuat banner "queue failed" dengan cuplikan per baris dan tombol Retry; `instances-inspector-closed.narrow` (640 pt) menampilkan Name, Provider, Status, dan aksi tanpa menggulir; `overview-mixed.narrow` satu kolom. Setiap pasangan HC berbeda dari standarnya kecuali `instances-loading` dan `overview-loading`. Referensi di `__Snapshots__/` berupa lima PNG per state (`light`, `dark`, `hc-light`, `hc-dark`, `narrow`), sedangkan langkah snapshot di bawah menyebut enam varian; selisih itu dicatat ke lead untuk dijelaskan atau dikoreksi.

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "InstanceGroupTests|InstanceTextTests|InstanceSearchSortTests|InstanceActionTests|InstanceActionModelTests"` → semua kasus `groups.json`, jumlah dan filter dari `list-mixed.json`, teks port, pencarian banyak kata, setiap pembanding naik dan turun, setiap baris tabel aksi, klik kedua saat `inFlight` tanpa kiriman, key dipakai ulang setelah `outcomeUnknown` (revision sama, di bawah 5 menit) lalu dibuang, `pending` dibersihkan oleh state akhir dan oleh resnapshot, dan umpan balik `rejected`, `notSent`, `outcomeUnknown`, operasi `failed` dan `cancelled` → AC-1, AC-6, AC-8
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "AttentionRulesTests|OperationTitleTests|CapacityModelTests"` → setiap aturan I1 sampai I10, O1, dan S1 sampai S4 dari `list-mixed.json`, `attention-operations.json`, dan laporan kapasitas; urutan item, `id` stabil, satu item per instance; judul setiap `OperationKind`; panggilan kapasitas satu per satu, balasan lama diabaikan, poll 60 detik hanya saat tersambung, `unknown_method` menjadi unsupported → AC-3, AC-4
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "ResourceStoreTests|InstanceStoreTests|ShellNavigatorTests"` → `itemFailures` terisi saat refetch gagal dan bersih saat item diterapkan, dihapus, atau resnapshot selesai; `retry` mengulang pada revision kegagalan dan menghapus item yang kini hilang; `ShellNavigator` dengan permintaan Instances → AC-9
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "InstancesPageTests|OverviewPageTests"` → state halaman (loading, failed, waiting, unsupported, stale, partial, empty, no results), gerbang seksi Needs Attention dan Operations, kata inspector, `ReadinessText`, dan perintah yang diterbitkan halaman → AC-2, AC-5, AC-6, AC-7, AC-9
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "InstancesSnapshotTests|OverviewSnapshotTests"` → 16 state Instances dan 14 state Overview cocok dengan referensi di `SnapshotTests/__Snapshots__/` pada enam varian → AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "SourceRuleTests|CommandMapTests|CommandSourceRuleTests"` → file baru lulus aturan sumber (tanpa `.secondary`, font tetap, `.inspector` di luar komponen), setiap halaman punya suite snapshot, dan baris `toggleInspector`, `sortInstances…`, `startInstance`, `stopInstance` built dengan tepat satu `CommandItem` → AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → 1022 test di 99 suite; satu kegagalan waktu, `SkeletonEndToEndTests.aProcessThatOutlivesItsLimitIsKilledAndReportedAsTimedOut` (23 detik dari batas 20) saat load rata rata mesin di atas 40, lulus saat suite itu dijalankan sendiri; `swift test --package-path clients/swift` → 185 test Swift Testing dan 35 XCTest lulus → AC-9. Dijalankan ulang oleh checker (2026-10-22, load 35 sampai 45): 1039 test di 100 suite dengan 27 isu (23 known issues) dari tiga test yang kena stall (`ShellEndToEndTests.theShellWaitsForStacklydThenConnectsByItself`, `SkeletonEndToEndTests.aProcessThatOutlivesItsLimitIsKilledAndReportedAsTimedOut`, `GallerySnapshotTests.componentsSheetPage`), ketiganya lulus saat dijalankan sendiri (8 test, 4 suite); `clients/swift` 192 test di 27 suite lulus
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

## Milestone 4: Runtimes card and evidence

### UI / manual

Enam catatan evidence manual sudah dibuat sebagai `NOT_RUN` lewat `cargo xtask evidence new` (kind `fixture`, folder `evidence/manual/`, berjudul "Spec 0022 manual check 1" sampai "6"); user yang menjalankannya di app Debug dengan `stacklyd` asli dan `STACKLY_ROOT` ke folder sementara (`docs/manual.md`).

- [ ] Dengan instance fake di kelompok ready, not ready (lewat `readyDelayMs`), stopped, failed (crash loop), dan data in use (lock dipegang proses lain, aturan I3): kartu, subjudul, Needs Attention, dan `stackly instance list` menunjukkan jumlah dan kelompok yang sama; kartu Runtimes menunjukkan jumlah yang sama dengan subjudul "{n} installed" halaman Runtimes dan membuka Runtimes → AC-1, AC-2, AC-3, AC-10
- [ ] Image disk APFS 3 GB (`hdiutil create -size 3g -fs APFS`), mount, `STACKLY_ROOT` di dalamnya: Overview menampilkan S2 (di bawah 5 GB tersedia); isi sampai kurang dari 2 GB tersedia: S1 dalam 60 detik → AC-4
- [ ] Hanya keyboard (Keyboard navigation menyala): Overview, kartu Failed, Instances, pilih baris, inspector, Stop lewat tombol inspector, urutkan lewat menu View; kartu Runtimes bisa difokus dan dijalankan dengan Space → AC-13
- [ ] VoiceOver: alur yang sama; setiap status terbaca sebagai teks, kartu Runtimes terbaca "Runtimes, {n}" dengan hint "Shows Runtimes.", dan baris install terbaca "Install {nama} {versi}, Running, {tahap}, {n} percent" → AC-13, AC-5
- [ ] Lebar konten 640 pt: Overview satu kolom tanpa teks terpotong (empat kartu), Instances dengan inspector terbuka menampilkan Name dan Status tanpa menggulir; light, dark, dan Increase Contrast; lalu jendela 900 pt dengan inspector terbuka (kolom halaman 384 pt) → AC-13
- [ ] Hentikan `stacklyd`: kedua halaman tetap menampilkan state terakhir, tombol aksi nonaktif dengan alasan; jalankan lagi: state disegarkan → AC-9

Status 2026-10-22: keenam langkah `NOT_RUN`, karena butuh app nyata, keyboard, VoiceOver, atau image disk yang hanya user yang menjalankan. Bukti pengganti `fixture` dari snapshot: `overview-runtimes` (kartu Runtimes "4" dan baris "Install PostgreSQL 18.1 · Download · 8%" di Operations), `overview-runtimes-failed` (kartu "–" dengan "Couldn’t load" dan halaman tetap utuh), dan `overview-runtimes-loading` (spinner kecil, bukan nol), masing masing di enam varian termasuk 640 pt.

### Commands

- [x] `cargo build -p stacklyd -p stackly-fake-server && cargo build -p stacklyd --example skeleton_seed` → selesai tanpa error
- [x] `cargo test -p stacklyd --test supervisor list_performance` → `a_list_of_100_instances_is_answered_in_at_most_500_ms` lulus; di bawah `cargo xtask evidence run --kind fixture` (batas spec 500 ms, bukan batas 10 kali di `cargo test` biasa) tercatat "instance.list of 100 instances answered in 16 ms" pada load rata rata mesin 160 sampai 270 → AC-14
- [x] `cargo xtask evidence run --kind fixture -- swift test --package-path apps/macos/Packages/StacklyUI --filter InitialSnapshotTests` → 2 test lulus, tercatat "100 instances in the store 27 ms after the handshake was accepted" (batas spec 1 detik) dan "20 operation events left the recompute counter at 1; one instance event raised it to 2" pada load rata rata mesin sekitar 125 → AC-14
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "OverviewRuntimesTests|OperationTitleTests|OverviewPageTests"` → 13 test baru: kartu tanpa store tidak dirender, `loading` sebelum snapshot, jumlah sama dengan subjudul Runtimes, nol setelah termuat, "Couldn’t load" tanpa data, angka lama tetap saat refresh gagal, `unsupported` tidak dirender, label VoiceOver; judul "Install PostgreSQL 18.1" dari baris `installing`, "Install runtime" tanpa baris itu, dan "{judul}, Running, Download, 8 percent" → AC-2, AC-5
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter OverviewSnapshotTests` → 17 test (14 lama tidak berubah, 3 baru `overview-runtimes`, `overview-runtimes-failed`, `overview-runtimes-loading` di enam varian) → AC-2, AC-5, AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` (run penuh) → 1140 test di 109 suite lulus dengan 23 known issue yang sudah ada sebelumnya (`ContrastProbe`), pada load rata rata mesin sekitar 115
- [x] `cargo xtask build-app` → `built .../target/app/Debug/Stackly.app`; `xcrun swift-format lint --strict` pada `Sources` dan `Tests` StacklyUI bersih; `cargo fmt --all --check`, `cargo clippy -p stacklyd --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, dan `cargo xtask evidence check` bersih
- [x] Langkah 9a: `swift test --package-path apps/macos/Packages/StacklyUI --filter "OperationsUnreadableTests|AttentionRulesTests|OverviewPageTests"` → `operationsState` untuk setiap kombinasi (unsupported dengan dan tanpa data lama, data dengan error, error tanpa data, belum termuat), gerbang `attentionItems` nil hanya untuk operasi yang belum menjawab, instance yang belum termuat, atau disk yang belum dicoba, item O2 versi error dan unsupported (id `operations`, neutral, tujuan Operations, label VoiceOver "Info: Operations unavailable. ..."), urutan `["S3","O2","I10"]`, O2 masuk hitungan enam item, dan `OperationCenter` dengan `fetchPage` yang ditahan: selama refresh berjalan `error` tetap dan `loadedAt` nil (O2 tetap, bukan progress), setelah snapshot berhasil `error` kosong dan `loadedAt` terisi → AC-3, AC-5
- [x] Langkah 9a: `swift test --package-path apps/macos/Packages/StacklyUI --filter OverviewSnapshotTests` → 18 test; `overview-operations-failed` dan `overview-operations-unsupported` direkam ulang dengan input baru (dua instance `pg17-dev` dan `queue`, disk `filling`) sehingga Needs Attention menampilkan I1, S3, dan O2 tanpa Show More, dan `overview-operations-loading` baru menampilkan progress di Needs Attention dan Operations sementara kartu dan Instances tampil, masing masing di enam varian → AC-13

### Value sourcing

- [x] Angka kartu Runtimes datang dari `RuntimeStore.loadedCount`, sama dengan subjudul halaman Runtimes ("{n} installed"); tidak pernah 0 sebelum snapshot pertama → AC-2
- [x] Judul `install_runtime` datang dari `RuntimeStore.installing` (`displayName` dan `version` baris yang `operationId` nya sama dengan operasi), judul `remove_runtime` dari `RuntimeStore.runtimes`; persen dari `StagedProgressValue.from` atas lima tahap spec 0018 dan `OperationProgress` operasi di `OperationCenter` → AC-5
- [x] Keadaan operasi untuk Needs Attention dan seksi Operations datang dari satu fungsi, `OverviewPage.operationsState` (`store.unsupported`, `loadedAt`, `error`, dan `OperationCenter.active`); isi O2 tetap dan tidak memuat pesan daemon, karena kegagalan yang sama tampil di placeholder seksi Operations → AC-3
- [x] Hitungan dan baris Instances di Overview dihitung ulang (`OverviewDerivation`) hanya saat `[InstanceSummary]`, katalog, atau kumpulan kegagalan refetch berubah → AC-14

### Acceptance-criteria coverage

- AC-2: lengkap di tampilan (kartu Runtimes dengan angka, tertunda, gagal, tidak dirender tanpa store atau saat `unsupported`, membuka Runtimes). **Terbuka:** `AppShell.swift` (milik `dev-f20-m3` dan `dev-f21-m3`, dilarang di brief ini) belum meneruskan `runtimes:` ke `OverviewPage(...)`, jadi di app nyata kartu belum muncul sampai satu baris `runtimes: runtimes,` ditambahkan di pemanggilan `OverviewPage` itu.
- AC-5: lengkap: judul dan persen install dari baris `installing` (di app nyata sama menunggu baris `AppShell` di atas, karena `RuntimeNames` dibaca dari `OverviewPage.runtimes`).
- AC-3: lengkap sejak langkah 9a (2026-10-22): `OperationCenter` yang unsupported atau gagal tanpa data tidak lagi menahan Needs Attention; item instance dan disk tetap tampil ditambah item O2 "Operations unavailable" (neutral, ke Operations) yang hilang setelah snapshot berhasil, dan progress hanya tampil selama operasi belum menjawab. Gagal dengan data lama memakai data itu tanpa O2. Menggantikan catatan sebelumnya bahwa spinner tanpa akhir mengikuti teks AC-3 lama.
- AC-13, AC-9, AC-4: bagian otomatis lengkap sejak milestone 3; evidence manual (keyboard, VoiceOver, 640 pt, daemon mati, image disk kecil) `NOT_RUN` untuk user.
- AC-14: lengkap. Test daemon 16 ms (batas 500 ms), test Swift 27 ms (batas 1 detik), penghitung recompute tidak naik untuk 20 event operasi dan satu perubahan status koneksi halaman, naik satu untuk satu event instance. Snapshot awal NFR-002 (2 detik) terpenuhi dengan selisih besar; benchmark formal di fitur 64.

