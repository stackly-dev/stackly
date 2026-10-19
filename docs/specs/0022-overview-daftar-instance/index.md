# 0022. Overview dan daftar instance

**Date**: 2026-10-20
**Status**: Proposed

## Summary

Spec ini menetapkan dua halaman GUI yang paling sering dibuka, Overview dan Instances, beserta padanannya di CLI, `stackly instance list` dan `stackly instance inspect`. Semua angka dan daftar dihitung dari DTO daemon yang sudah ada (`InstanceSummary` spec 0010, operasi spec 0007, runtime spec 0018) dengan satu aturan kelompok status yang sama di Swift dan Rust, jadi instance yang prosesnya hidup tetapi belum siap tidak pernah dihitung "Running". Daftar Needs Attention menautkan setiap masalah (instance gagal, proses yang tidak terkonfirmasi, readiness yang gagal, operasi yang perlu recovery, subsystem daemon, dan ruang disk) ke layar perbaikannya, dan peringatan kapasitas memakai sumber yang terdefinisi: method baru `storage.get_capacity` yang membaca ruang disk volume tempat data Stackly berada. Error di satu instance atau satu seksi tidak pernah mengosongkan halaman.

## Requirements

**User stories**:

- Sebagai pengguna yang membuka Stackly, saya ingin melihat berapa instance yang benar benar siap, berhenti, dan gagal, serta apa yang butuh perhatian beserta jalan ke perbaikannya, supaya saya tahu dalam beberapa detik apakah lingkungan saya sehat.
- Sebagai pengguna dengan banyak instance, saya ingin mencari, menyaring, dan mengurutkan instance, lalu melihat detail singkat dan menjalankan atau menghentikan satu instance tanpa meninggalkan daftar.
- Sebagai pengguna yang disk nya hampir penuh, saya ingin diperingatkan sebelum database saya gagal menulis.
- Sebagai penulis script, saya ingin `stackly instance list` dan `stackly instance inspect` dengan JSON stabil yang isinya sama dengan GUI.
- Sebagai pengguna keyboard atau VoiceOver, saya ingin memakai kedua halaman tanpa informasi yang hanya ada lewat warna atau hover.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-027):

- **AC-1**: Fungsi murni `InstanceGroup.of(lifecycle:readiness:)` di `StacklyUI` dan `instance::group::of(&InstanceSummary)` di CLI memetakan setiap kombinasi lifecycle dan state readiness ke `ready`, `notReady`, `stopped`, `failed`, atau `unknown` persis seperti tabel *Kelompok status instance* (lifecycle `retained` tidak masuk kelompok mana pun). Test Swift dan test Rust membaca fixture bersama `protocols/fixtures/instance/groups.json` yang memuat setiap lifecycle dikali setiap state readiness, dan hasil keduanya sama. Kelompok hanya membaca `lifecycle` dan `readiness.state`, tidak pernah `process`, `endpoints`, atau `restart`, sehingga instance dengan proses hidup yang readiness nya belum `ready` tidak pernah dihitung Running (PAGE-02, "tidak hanya menghitung PID"). Filter Running sama dengan `ready` ditambah `notReady`, Stopped sama dengan `stopped`, dan Failed sama dengan `failed`, di GUI maupun CLI.
- **AC-2**: Halaman Overview menggantikan `UnbuiltPage(section: .overview)` dan penanda `// provisional: F22` nya di spec 0013, berlatar `surfaceWindow`, dengan empat kartu sesuai *Kartu ringkasan*: Running (jumlah `ready`, ditambah keterangan "{n} not ready" kalau `notReady` lebih dari 0), Stopped, Failed (angka bertone critical kalau lebih dari 0), dan Runtimes (jumlah item `RuntimeStore` spec 0018, sama dengan subjudul halaman Runtimes). Angka kartu dan subjudul toolbar hanya tampil setelah snapshot store sumbernya termuat, tidak pernah berupa nol sebelum itu. Subjudul mengikuti *Copy*. Mengaktifkan kartu Running, Stopped, atau Failed membuka Instances dengan filter yang sama dan pencarian kosong, dan jumlah baris yang tampil sama dengan angka kartu (untuk Running, angka ditambah keterangan not ready); kartu Runtimes membuka halaman Runtimes.
- **AC-3**: Seksi Needs Attention dibangun oleh fungsi murni `AttentionRules.items(instances:operations:capacity:health:)` sesuai tabel *Aturan Needs Attention*: paling banyak satu item per instance (aturan pertama yang cocok menurut nomor), urut tone (critical, caution, neutral), lalu kategori (instance, operasi, disk, daemon), lalu nomor aturan, lalu nama dengan `localizedStandardCompare`. Enam item pertama tampil, sisanya lewat "Show {n} More" yang membuka di tempat (lalu "Show Less"). Setiap item punya simbol tone, judul, isi, dan tujuan dari tabel; tanpa item, seksi menampilkan `InlineEmptyRow("Nothing needs attention.")`. Test membaca `protocols/fixtures/instance/list-mixed.json` dan fixture kapasitas, dan membuktikan setiap baris tabel. Event `instance.state_changed` dan `instance.health_changed` memperbarui daftar tanpa snapshot ulang.
- **AC-4**: `storage.get_capacity` (tanpa params, `recovery allowed`) membalas `CapacityReport` sesuai *Kapasitas disk*: satu `VolumeCapacity` per volume berbeda di antara root data, logs, dan cache (dikelompokkan lewat `VolumeId` dari `Filesystem::volume_space`), dengan `level` yang dihitung daemon lewat `stackly_domain::capacity::level` (`critical` di bawah 2 GB tersedia, `low` di bawah 10 GB tersedia atau terpakai 90 persen ke atas, `filling` terpakai 80 persen ke atas, selain itu `ok`), dan root yang gagal dibaca masuk `unavailable` dengan pesan, tanpa menggagalkan reply. Method ini tidak membaca atau menulis registry, tidak membuat operasi, dan dengan `FakePlatform` tidak menambah `mutations()`. GUI memanggilnya saat Overview tampil, lalu setiap 60 detik selama Overview tampil dan presence `connected`, dan menampilkan item S1 sampai S4; daemon yang menjawab `request.unknown_method` atau error lain sebelum ada laporan menghasilkan item S4 "Disk space unavailable" beserta alasannya.
- **AC-5**: Seksi Instances di Overview menampilkan enam instance pertama urut nama (tile, nama, "{provider} {versi} · {port}", `StatusLabel` konteks `.standard`, dan tombol aksi utama dari AC-8), "Show All {n} Instances" kalau lebih dari enam, dan empty "No Instances" dengan tombol Browse Catalog kalau tidak ada instance. Seksi Operations menampilkan operasi `OperationCenter` yang `queued`, `running`, atau `cancel_requested` (paling baru dulu, paling banyak lima, lalu "Show All in Operations"), masing masing dengan judul dari `OperationTitle`, `ToneBadge` state, label tahap, dan progress sesuai *Seksi Operations*, atau `InlineEmptyRow("Nothing running.")`. Setiap seksi yang sumbernya gagal atau unsupported menampilkan placeholder seksi nya sendiri sementara seksi lain tetap tampil.
- **AC-6**: Halaman Instances menggantikan `ProvisionalInstanceList` dan penanda `// provisional: F22` spec 0013 (`InstanceStore` tetap dipakai): toolbar dengan judul "Instances", subjudul "{n} instances", pencarian "Search", dan tombol Show Inspector; picker bersegmen All, Running, Stopped, Failed; `Table` native dengan kolom Name (tile dan nama), Provider, Version (exact version), Endpoints, Status, dan kolom aksi, yang bisa diurutkan per kolom dengan pembanding di *Halaman Instances* (default Name naik). Pencarian cocok dengan setiap kata di nama, nama tampil, provider, versi, dan port tanpa peduli huruf besar dan diakritik. Daftar kosong menampilkan "No Instances" dengan Browse Catalog; hasil filter kosong menampilkan "No Results for “{query}”" dengan Clear Filters yang mengembalikan filter All dan pencarian kosong. Menu konteks baris berisi aksi utama, Copy Name, dan Copy ID.
- **AC-7**: Inspector native (`.inspector`) di Instances menampilkan, untuk instance terpilih, isi *Inspector* (tile, nama, status, aksi utama, banner masalah dari aturan I1 sampai I4 dengan cuplikan `lastFailure.excerpt`, lalu Provider, Version, Endpoints, Ownership, Readiness, Health, Process, Restarts, Desired, dan ID dengan tombol copy), atau "No Selection" kalau tidak ada pilihan atau pilihan tersaring keluar. Ownership `manual` ditulis "Manual". Keadaan tampil atau tersembunyi disimpan `@SceneStorage("instances.inspector")` (default tampil) dan dibalik lewat tombol toolbar atau ⌥⌘I.
- **AC-8**: Aksi utama per instance mengikuti tabel *Aksi instance* (Start, Stop, Retry, tombol sibuk, atau tombol nonaktif dengan alasan yang terlihat) dan dijalankan lewat `MutationPath.perform` dengan `instance.start { instanceId }` atau `instance.stop { instanceId, overrideLeases: false }` dan idempotency key baru per klik (dipakai ulang setelah `outcomeUnknown`), tanpa konfirmasi (aturan spec 0012 untuk start dan stop tanpa dependen). Selama operasi yang dibuat aksi itu belum berstate akhir (diikuti lewat `OperationCenter.updates(for:)`), tombol instance itu sibuk. Penolakan, `notSent`, `outcomeUnknown`, dan operasi yang berakhir `failed` atau `cancelled` menampilkan umpan balik dengan copy di *Copy* dan diumumkan VoiceOver; balasan `alreadyInState` tidak menampilkan apa pun. Semua tombol dan item menu aksi memakai `.mutationGated()`.
- **AC-9**: Kedua halaman memakai `ViewState.resolve` spec 0012: loading sebelum snapshot pertama `InstanceStore`, failed ("Couldn’t Load Overview" atau "Couldn’t Load Instances", atau "Waiting for stacklyd" dari aturan presence spec 0013) hanya saat belum ada data, unsupported saat store `unsupported`, stale saat refresh gagal dengan data lama, dan partial saat `InstanceStore.itemFailures` tidak kosong ("Couldn’t refresh {n} instance(s)." dengan item per instance dan Retry). Instance yang gagal, `unknown`, atau berblocker tampil sebagai baris dan banner item, tidak pernah sebagai error halaman, dan seksi Overview yang gagal tidak mengubah bentuk halaman. Saat daemon tidak terjangkau, kedua halaman tetap menampilkan state terakhir tanpa mengubah status apa pun (spec 0013).
- **AC-10**: `stackly instance list [--status running|stopped|failed]` mengambil semua halaman `instance.list` (limit 200 per halaman), menyaring di client dengan aturan AC-1, menulis tabel `NAME`, `PROVIDER`, `VERSION`, `ENDPOINTS`, `LIFECYCLE`, `READINESS`, `HEALTH`, `OWNERSHIP` lalu baris jumlah di *CLI*, dan dengan `--json` memberi `result` `{ "items": [InstanceSummary], "counts": InstanceCounts }` dengan `counts` dari semua instance. Daftar kosong menulis `No instances.` atau `No <status> instances.` dan keluar 0. Golden CLI dan test view model Swift membaca fixture `list-mixed.json` yang sama dan menghasilkan kelompok dan jumlah yang sama (REQ-028).
- **AC-11**: `stackly instance inspect <instance>` menerima UUID (memanggil `instance.get`, termasuk instance retained) atau nama persis (dicari lewat halaman `instance.list`), menulis blok `kv` dan tabel endpoint di *CLI*, dan dengan `--json` memberi `InstanceSummary`. Nama yang tidak ditemukan keluar 10 dengan `cli.instance_not_found`; UUID yang tidak ada keluar 10 dengan `resource.not_found` dari daemon; daemon yang tidak jalan keluar 6. Kalau fitur 14 sudah menambah `stackly instance get`, perintah itu menjadi alias `inspect` dengan output yang sama.
- **AC-12**: Kontrak: `InstanceSummary.ownership: InstanceOwnership` (enum terbuka bertag, nilai `manual` dan `unrecognized`, diisi `stacklyd` dengan `manual` untuk setiap instance selama belum ada keanggotaan environment), modul `storage` (`CapacityReport`, `VolumeCapacity`, `RootUnavailable`, `StorageRoot`, `CapacityLevel`) dengan method `storage.get_capacity` di tabel `methods!` bertanda `recovery allowed`, fixture di `protocols/fixtures/` untuk call, reply, dan nilai asing setiap enum terbuka, kenaikan minor `ProtocolVersion::CURRENT` di commit yang sama, `cargo xtask codegen`, dan test kontrak Rust dan Swift yang lulus. Nilai asing ditampilkan sesuai *Nilai asing*.
- **AC-13**: Kedua halaman bisa dipakai penuh dari keyboard dan VoiceOver sesuai *Aksesibilitas*: setiap kartu, item Needs Attention, baris instance, tombol aksi, segmen filter, baris tabel, dan isi inspector bisa difokus dan dijalankan (Tab lalu Space saat Keyboard navigation menyala), pengurutan tersedia lewat menu View "Sort Instances By", inspector lewat ⌥⌘I, dan setiap elemen punya label VoiceOver yang memuat teks status, bukan hanya warna atau simbol. Tidak ada informasi yang hanya ada di hover. Di lebar konten 640 pt, Overview menjadi satu kolom tanpa teks terpotong, dan semua teks lolos kontras spec 0012 (NFR-012, bagian stop dan periksa dari PRD AC-027).
- **AC-14**: Dengan `stacklyd` asli dan 100 instance fake terdaftar di Mac ini, waktu dari `HandshakeAccepted` sampai `InstanceStore.loadedAt` terisi paling lama 2 detik (maksimum dari 5 percobaan, dicatat sebagai evidence `fixture` untuk NFR-002), decode JSON berjalan di aktor koneksi `StacklyClient`, bukan di main thread, dan model turunan halaman dihitung ulang hanya saat input nya berubah.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): kelompok status dihitung di client dengan satu aturan yang dijaga fixture bersama Swift dan Rust; kartu Running menghitung instance `ready` dengan keterangan not ready; Needs Attention diturunkan di client dari store yang sudah ada; kapasitas lewat method baru `storage.get_capacity` dengan level dihitung daemon; ownership lewat field baru `InstanceSummary.ownership` yang bernilai `manual` sampai fitur 44; kolom dan seksi Environment menunggu fitur 44; Start, Stop, dan Retry dibangun di sini lewat `InstanceActionModel` yang dipakai ulang fitur 21; "buka instance" sementara memilih baris di Instances; CLI `list` mengambil semua halaman dan `inspect` menjadi nama kanonik; dan seksi Overview gagal sendiri sendiri.

Overview dan Instances hanya membaca store yang sudah disinkronkan spec 0013 dan satu method baca baru; semua status, kelompok, dan item perhatian adalah fungsi murni dari DTO daemon, dan satu satunya mutasi adalah start dan stop lewat jalur mutasi yang sudah ada.

**Implementation skills**: `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya: kontrol native (`Table`, `.inspector`, `Picker` bersegmen, `.searchable`) menggantikan tiruan HTML di mockup, dan view model halaman tetap di main actor sementara decode berjalan di aktor client.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| PAGE-02 (Overview: ringkasan ready, stopped, failed, operasi aktif, peringatan kapasitas, navigasi cepat, tidak hanya PID) | AC-1, AC-2, AC-3, AC-4, AC-5 |
| PAGE-03 (Instances: cari, filter, urut; nama, provider, exact version, endpoint, lifecycle, readiness, ownership) | AC-6, AC-7, AC-12; environment reference milik fitur 44 |
| REQ-028 (paritas CLI) | AC-10, AC-11 |
| NFR-002 (snapshot awal 2 detik untuk 100 instance, UI thread tidak terblokir) | AC-14; benchmark formal di fitur 64 (PRD AC-076) |
| NFR-012 (keyboard, VoiceOver, status tidak hanya warna) | AC-13 |
| AC-027 (alur tanpa informasi yang hanya lewat warna atau hover) | AC-13 untuk bagian menghentikan dan memeriksa instance; bagian membuat instance milik fitur 20 |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| `InstanceSummary`, `instance.list`, `instance.get`, `instance.start`, `instance.stop`, event instance, aturan `statusSeq` | spec 0010 (dipakai apa adanya, plus field `ownership` di *Usulan additive untuk spec lain*) |
| DTO operasi, `operation.list`, event `operation.` | spec 0007 |
| `InstanceStore`, `OperationCenter`, `ResourceStore`, `ShellSync`, `StatusAdapter`, `MutationPath`, banner daemon, gate mutasi, aturan presence | spec 0013 (plus `itemFailures` dan `ShellNavigator` di *Usulan additive untuk spec lain*) |
| Token, `StatusAppearance`, `StatusLabel`, `ToneBadge`, `ViewState`, `StateBanner`, `StatePlaceholder`, `InlineEmptyRow`, `.disabled(reason:)`, `announce` | spec 0012 |
| `CatalogStore`, `ProviderTile`, nama tampil dan tile provider | spec 0017 (atau fitur 67 kalau sudah menyediakan tile) |
| `RuntimeStore`, daftar `installing`, aturan persen install | spec 0018 |
| Kelompok status, kartu, Needs Attention, kapasitas, halaman Instances, inspector, aksi Start, Stop, Retry, `OperationTitle`, CLI `instance list` dan `inspect` | spec ini |
| Tombol New Instance di Overview, Instances, dan empty state | fitur 20 |
| Halaman detail instance, Open, data path dan ukuran, connection string | fitur 21 |
| Restart, force stop, stop dengan override lease | fitur 26 dan 47 |
| Kolom dan baris Environment, ownership dedicated dan shared, seksi Environments di Overview | fitur 44 |
| Item drift config | fitur 29 |
| Umpan balik setelah aksi yang final | fitur 67 (spec ini memakai banner sementara) |
| Halaman Operations, Diagnostics | fitur 35, 34 |

### Kelompok status instance

`InstanceGroup` (Swift) dan `instance::group::InstanceGroup` (CLI) punya lima nilai. Hanya `lifecycle` dan `readiness.state` yang dibaca.

| Lifecycle | Readiness | Kelompok |
| --- | --- | --- |
| `running` | `ready` | `ready` |
| `running` | `not_ready` atau `unknown` | `notReady` |
| `initializing`, `starting`, `recovering`, `stopping` | apa pun | `notReady` |
| `stopped`, `uninitialized` | apa pun | `stopped` |
| `failed` | apa pun | `failed` |
| `unknown` | apa pun | `unknown` |
| `retained` | apa pun | tidak masuk kelompok (tidak ada di `instance.list`; kalau datang lewat event, store menghapusnya) |

`LifecycleStatus.unrecognized` di Swift (tidak bisa muncul dari enum tertutup, tetapi ada di enum UI) masuk `unknown`.

`InstanceCounts { total, ready, notReady, stopped, failed, unknown }` dihitung dari semua item store (Swift) atau semua halaman (CLI). `total` sama dengan jumlah kelima kelompok.

| Filter (GUI dan `--status`) | Isi |
| --- | --- |
| All | semua |
| Running (`running`) | `ready` dan `notReady` |
| Stopped (`stopped`) | `stopped` |
| Failed (`failed`) | `failed` |

Instance `unknown` hanya terlihat di All dan di Needs Attention.

### Kartu ringkasan

Empat kartu di `LazyVGrid(columns: [GridItem(.adaptive(minimum: 130), spacing: 12)])`, masing masing `Button` bergaya plain di atas `.cardSurface(radius: .large)` dengan padding kartu spec 0012. Isi: baris label (titik 7 pt warna fill lalu label `.callout` `textSecondary`), angka `.largeTitle.weight(.semibold).monospacedDigit()`, dan baris keterangan opsional `.footnote`.

| Kartu | Titik | Angka | Keterangan | Sumber | Tujuan |
| --- | --- | --- | --- | --- | --- |
| Running | fill `positive` | `counts.ready` | "{n} not ready" (teks tone `caution`) kalau `counts.notReady` > 0 | `InstanceStore` | Instances, filter Running |
| Stopped | fill `neutral` | `counts.stopped` | tidak ada | `InstanceStore` | Instances, filter Stopped |
| Failed | fill `critical` | `counts.failed`, teks tone `critical` kalau > 0 | tidak ada | `InstanceStore` | Instances, filter Failed |
| Runtimes | warna bagian Runtimes (`systemTeal`) | `RuntimeStore.count` | "Couldn’t load" kalau store error tanpa data | `RuntimeStore` (spec 0018) | Runtimes |

- Sebelum store sumbernya termuat, angka diganti `ProgressView().controlSize(.small)` setelah jeda 300 ms (`DelayedProgress`), tidak pernah 0.
- Kartu Runtimes hanya dirender kalau `DaemonModel` punya `RuntimeStore` (fitur 18 sudah dibangun); store `unsupported` membuat kartu tidak dirender.
- Instance `unknown` tidak punya kartu; jumlahnya ada di subjudul dan setiap instance nya ada di Needs Attention.

### Halaman Overview

Folder `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Overview/`: `OverviewPage`, `SummaryCards`, `AttentionRules`, `AttentionList`, `OverviewInstanceList`, `OverviewOperations`, `CapacityModel`, `OverviewCopy`.

- `.pageChrome(title: "Overview", subtitle:)` dengan subjudul dari *Copy* setelah snapshot `InstanceStore` pertama; tanpa aksi toolbar sampai fitur 20 menambah New Instance.
- Tata letak: `ScrollView` berlatar `surfaceWindow`, inset halaman 22 pt. Urutan: kartu, lalu dua kolom (kiri: Needs Attention lalu Instances; kanan 300 sampai 360 pt: Operations) saat lebar konten minimal 820 pt, atau satu kolom (Needs Attention, Instances, Operations) di bawahnya, lewat `ViewThatFits(in: .horizontal)`. Judul seksi `.headline`; isi seksi di `.cardSurface()` sebagai daftar berkelompok dengan pemisah.
- Seksi Environments di kolom kanan milik fitur 44 dan tidak dirender di build ini.

**Aturan Needs Attention.** Item instance memakai aturan pertama yang cocok (nomor terkecil). `{name}` adalah `InstanceSummary.name`. Tujuan "instance" berarti `ShellNavigator.openInstance(id)` (lihat *Navigasi*).

| No | Kondisi | Tone | Judul | Isi | Tujuan |
| --- | --- | --- | --- | --- | --- |
| I1 | lifecycle `failed` | `critical` | "{name} failed" | teks `lastFailure.code` di *Copy kegagalan* | instance |
| I2 | lifecycle `unknown`, atau blocker `identityUnproven` | `caution` | "Can’t confirm {name}’s process" | "Stackly won’t start or stop it until the process is confirmed or gone." | instance |
| I3 | blocker `dataInUse` | `critical` | "{name}’s data is in use" | "Another process holds its data lock. Stop that process, then start {name}." | instance |
| I4 | blocker `needsSecret` | `caution` | "{name} needs a secret" | "Store the missing secret, then start it again." | instance |
| I5 | `running`, readiness `ready`, health `unhealthy` | `critical` | "{name} is unhealthy" | `health.detail`, atau "Its health check is failing." | instance |
| I6 | `running`, readiness `not_ready` dengan reason selain `awaiting_probe` | `caution` | "{name} isn’t ready" | `readiness.detail`, atau "Its readiness check is failing." | instance |
| I7 | `recovering` | `caution` | "{name} is restarting" | "It crashed. Attempt {attempts} of {budget}, next at {time}." kalau `restart.nextAttemptAt` ada, selain itu "It crashed and Stackly is starting it again." | instance |
| I8 | `running`, readiness `ready`, health `degraded` | `caution` | "{name} is degraded" | `health.detail`, atau "Its health check reports a problem." | instance |
| I9 | `running`, readiness `unknown` dengan reason `stale` | `caution` | "Readiness of {name} is unknown" | "No readiness check has answered recently." | instance |
| I10 | `running` dan `process.adoptable` false | `neutral` | "{name} can’t be adopted again" | "Its process doesn’t hold the instance lock. If stacklyd restarts, Stackly will mark it Unknown." | instance |
| O1 | operasi `needs_recovery` di `OperationCenter.active` | `critical` | "{judul operasi} needs recovery" | "stacklyd stopped while it ran. Resume or abandon it in Operations." | Operations |
| S1 | volume `critical` | `critical` | "Disk is almost full" | "Only {available} free on the disk that holds {roots}. Instances can fail when it fills up." | Diagnostics |
| S2 | volume `low` | `caution` | "Disk space is low" | "{available} free of {total} on the disk that holds {roots}." | Diagnostics |
| S3 | volume `filling` | `neutral` | "Disk is {usedPercent}% full" | "{available} free on the disk that holds {roots}." | Diagnostics |
| S4 | laporan belum ada dan fetch gagal, method unsupported, root di `unavailable`, atau level `unrecognized` | `neutral` | "Disk space unavailable" (untuk level asing: "Disk state not recognized") | unsupported: "This version of stacklyd doesn’t report disk space."; error: pesan daemon; root: "Stackly couldn’t read the disk that holds {root}: {message}"; level asing: "Update Stackly to see this disk state." | Diagnostics |
| D1 | subsystem `degraded` atau status asing di `DaemonHealth` terakhir | `caution` | "stacklyd: {subsystem} degraded" (asing: "stacklyd: {subsystem} status unknown") | `detail`, atau "No details reported." | Diagnostics |
| D2 | subsystem `unavailable` | `critical` | "stacklyd: {subsystem} unavailable" | `detail`, atau "No details reported." | Diagnostics |

- Readiness `unknown` dengan reason `daemon_restarted` atau `not_running` tidak menjadi item (keadaan sementara setelah daemon restart). Subsystem `ok` dan `not_built` tidak menjadi item. Recovery mode registry tidak menjadi item karena shell sudah menampilkan banner critical.
- Satu item per volume untuk S1 sampai S3, satu per root untuk S4 root, dan satu S4 umum kalau seluruh laporan tidak tersedia.
- `{roots}` menggabungkan label root volume itu dengan "and" dan koma (data "Stackly data", logs "Stackly logs", cache "downloads", asing "other Stackly files"); `{available}` dan `{total}` memakai `ByteCountFormatter` gaya `.file` (desimal, sama dengan spec 0017).
- Item ditulis sebagai `Button` baris: simbol tone (`Tone.symbol`, warna fill, `accessibilityHidden`), judul `.body.weight(.semibold)`, isi `.callout` `textSecondary` yang membungkus, dan chevron dekoratif.
- `CapacityModel` hanya menyimpan laporan sukses terakhir; refresh yang gagal setelah pernah sukses tidak mengubah item, dan percobaan berikutnya berjalan di tick 60 detik berikutnya.

**Seksi Instances.** `OverviewInstanceList`: instance store urut `name` dengan `localizedStandardCompare` (seri dengan `id`), enam pertama. Baris: `ProviderTile` 26 pt, nama `.body.weight(.semibold)`, meta "{provider} {versi} · {port}" `.callout` `textSecondary`, `StatusLabel` konteks `.standard`, dan tombol aksi utama (*Aksi instance*). Seluruh baris kecuali tombol aksi adalah `Button` yang membuka instance. Lebih dari enam: baris "Show All {n} Instances" ke Instances filter All. Tanpa instance: `StatePlaceholder` ukuran `.section` dengan simbol `server.rack`, "No Instances", "Install a runtime from the catalog, then create an instance with its own data directory and port.", dan tombol Browse Catalog (`mutates: false`) ke Catalog.

**Seksi Operations.** `OverviewOperations` membaca `OperationCenter.active` tanpa `needs_recovery` (yang masuk O1), urut `createdAt` turun, paling banyak lima, lalu "Show All in Operations". Baris: judul `OperationTitle`, `ToneBadge(StatusAppearance.operation(state:partial:))`, baris bawah "{label tahap}" atau "{label tahap} · {persen}%", dan `ProgressView`. Seluruh baris adalah `Button` ke Operations.

- Label tahap: `stage` dengan garis bawah diganti spasi dan huruf pertama besar (`verify_digest` menjadi "Verify digest", sama dengan label spec 0018); tanpa `stage`, label state.
- Persen dan progress bar determinate hanya untuk `install_runtime`, dengan aturan persen keseluruhan spec 0018 (`(index tahap + pecahan tahap) / 5`); jenis lain memakai `ProgressView()` indeterminate.
- `OperationTitle.title(for:instances:runtimes:)` adalah fungsi murni yang juga dipakai fitur 35:

| Kind | Judul |
| --- | --- |
| `install_runtime` | "Install {displayName} {version}" dari baris `installing` spec 0018 dengan `operationId` sama, selain itu "Install runtime" |
| `start_instance`, `stop_instance`, `restart_instance`, `reload_instance`, `create_instance`, `remove_instance`, `purge_instance`, `backup_instance`, `restore_instance`, `rename_instance` | "Start", "Stop", "Restart", "Reload", "Create", "Remove", "Purge", "Back up", "Restore", "Rename", lalu nama instance dari `InstanceStore` lewat `resource.id`, atau kata "instance" kalau tidak dikenal |
| `migrate_data`, `rebind_runtime` | "Migrate data of {instance}", "Change runtime of {instance}" |
| `remove_runtime` | "Remove {displayName} {version}" dari `RuntimeStore`, atau "Remove runtime" |
| `register_external_runtime` | "Register external runtime" |
| `environment_up`, `environment_down` | "Bring up environment", "Bring down environment" (nama environment dari fitur 44) |
| `update_provider_revision`, `update_manager` | "Update provider", "Update Stackly" |
| `unrecognized` | "Operation" |

### Kapasitas disk

**Port** (usulan additive spec 0004): `Filesystem::volume_space(&self, path: &Path) -> Result<VolumeSpace, FsError>` dengan `VolumeSpace { total: u64, available: u64, volume: VolumeId }` dan `VolumeId(u64)`. Adapter macOS memanggil `statfs(2)` (blok `unsafe` dengan `// SAFETY:` di `stackly-platform-macos`): `total = f_blocks × f_bsize`, `available = f_bavail × f_bsize`, `volume` dari dua kata `f_fsid`. `FakeFilesystem` memberi volume per awalan path yang dipasang test dan injeksi error, dan `fs_conformance` mendapat kasus nya. `available_space` spec 0018 boleh diimplementasikan di atas method ini.

**Domain**: `stackly_domain::capacity` berisi `CapacityLevel { Ok, Filling, Low, Critical }`, `used_percent(total, available) -> u8` (pembulatan ke bawah, 0 kalau `total` 0), dan `level(total, available) -> CapacityLevel` dengan urutan cek:

| Level | Kondisi | Konstanta |
| --- | --- | --- |
| `Critical` | `available` < 2 000 000 000 | `CAPACITY_CRITICAL_AVAILABLE` |
| `Low` | `available` < 10 000 000 000, atau `used_percent` ≥ 90 | `CAPACITY_LOW_AVAILABLE`, `CAPACITY_LOW_USED_PERCENT` |
| `Filling` | `used_percent` ≥ 80 | `CAPACITY_FILLING_USED_PERCENT` |
| `Ok` | selain itu | |

**Core**: `stackly_core::storage::capacity(fs: &dyn Filesystem, roots: &[(StorageRoot, &Path)], now: Timestamp) -> CapacityReport` (tipe core) memanggil `volume_space` untuk root data, logs, lalu cache (dari `RootSet` spec 0004; runtime root tidak ikut karena hanya berisi socket dan spool kecil), mengelompokkan per `VolumeId` dengan urutan root pertama, menghitung level per volume, dan menaruh root yang gagal di `unavailable` dengan pesan `FsError` yang aman (tanpa isi file, path root boleh karena milik user yang sama).

**Method**: `storage.get_capacity` tanpa params, `recovery allowed`, dijawab dari `spawn_blocking` tanpa lock registry dan tanpa operasi. Area `storage` baru (aturan "fitur yang pertama kali memakainya" spec 0006); tidak ada kode error baru karena kegagalan per root ada di reply.

```rust
pub struct CapacityReport { pub checked_at: Timestamp, pub volumes: Vec<VolumeCapacity>, pub unavailable: Vec<RootUnavailable> }
pub struct VolumeCapacity { pub roots: Vec<StorageRoot>, pub total_bytes: Count, pub available_bytes: Count, pub used_percent: u8, pub level: CapacityLevel }
pub struct RootUnavailable { pub root: StorageRoot, pub message: String }
pub enum StorageRoot { Data, Logs, Cache, Unrecognized }          // terbuka, snake_case
pub enum CapacityLevel { Ok, Filling, Low, Critical, Unrecognized } // terbuka, snake_case
```

**GUI**: `CapacityModel` (`@MainActor @Observable`) dengan `report: CapacityReport?`, `failure: ErrorBody?`, `unsupported: Bool`. `OverviewPage` memasang `.task` yang memanggil `DaemonModel.read(.storageGetCapacity)` saat tampil, lalu setiap 60 detik selama halaman tampil dan presence `connected` (dengan `sleep` yang bisa diganti di test), dan sekali lagi setiap kali `ShellSync` selesai resnapshot. `request.unknown_method` membuat `unsupported` true. Model tidak terdaftar ke `ShellSync` karena tidak ada event kapasitas.

### Halaman Instances

Folder `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Instances/`: `InstanceStore` (sudah ada), `InstanceGroup`, `InstanceCounts`, `InstanceRowModel`, `InstanceSearch`, `InstanceSort`, `InstancesPage`, `InstancesTable`, `InstanceInspector`, `InstanceActionModel`, `InstanceFailureCopy`, `InstancesCommands`, `ActionFeedbackBanner` (`// provisional: F67`). `ProvisionalInstanceList` dihapus.

- Toolbar: `.pageChrome(title: "Instances", subtitle: "1 instance" atau "{n} instances")` setelah snapshot (jumlah total, bukan hasil filter), `.searchable(text:placement: .toolbar, prompt: "Search")`, dan tombol inspector (simbol `sidebar.trailing`, label "Show Inspector" atau "Hide Inspector", ⌥⌘I). New Instance milik fitur 20.
- Di atas tabel: `Picker` bersegmen berlabel "Status" dengan All, Running, Stopped, Failed. Filter dan pencarian tidak disimpan; pilihan disimpan `@SceneStorage("instances.selection")` sebagai UUID teks.
- `Table(rows, selection:, sortOrder:)` dengan kolom:

| Kolom | Isi | Pembanding (seri selalu `name` lalu `id`) |
| --- | --- | --- |
| Name | `ProviderTile` 18 pt dekoratif dan `name` | `name` dengan `localizedStandardCompare` |
| Provider | nama tampil provider | nama tampil |
| Version | `runtimeVersion`, atau "—" | `localizedStandardCompare` (angka diurutkan sebagai angka), kosong di akhir |
| Endpoints | teks port singkat | port pertama sebagai angka, socket dan `auto` di akhir |
| Status | `StatusLabel` konteks `.row` | urutan kelompok `failed`, `unknown`, `notReady`, `ready`, `stopped` |
| (tanpa judul) | tombol aksi utama `.controlSize(.small)` | tidak bisa diurutkan |

- Default urut Name naik. Lebar minimum kolom Name 120, Provider 90, Version 60, Endpoints 80, Status 110, aksi 72 pt; di lebar sempit tabel menggulir horizontal, dan sel yang terpotong punya nilai lengkap di `.help`, label VoiceOver, dan inspector.
- Nama tampil provider: `CatalogStore` entri `provider.name` (`displayName`), kalau tidak ada `software` apa adanya. Tile: `ProviderTile` dengan tile dan mark catalog, kalau tidak ada tile `gray` dengan mark dua huruf pertama `software` (huruf pertama besar).
- Teks port singkat: per endpoint urut DTO, `unix` menjadi "socket", `port` ada menjadi ":{port}", `port` null menjadi "auto"; dipisah spasi.
- Pencarian: query dipecah per spasi; setiap kata wajib `localizedStandardContains` di gabungan `name`, `displayName`, nama tampil provider, `provider.name`, `software`, `runtimeVersion`, dan setiap port (":5432" dan "5432").
- Return dan klik ganda pada baris memanggil `openInstance` (*Navigasi*). Menu konteks: aksi utama (gated), Copy Name, Copy ID (`NSPasteboard`, tanpa gate).
- Pilihan yang tersaring keluar atau instance yang hilang dari store membuat inspector "No Selection"; pilihan dikembalikan kalau barisnya terlihat lagi.
- Empty dan no results: `EmptyContent` `.noData` (simbol `server.rack`, "No Instances", "Each instance gets its own data directory, port and secret. Start by installing a runtime.", primary Browse Catalog `mutates: false`) dan `.noResults(query)` dengan query teks pencarian, atau label filter kalau pencarian kosong, pesan "Check the spelling or try another status.", primary Clear Filters.

**Inspector.** `.inspector(isPresented:)` dengan `.inspectorColumnWidth(min: 240, ideal: 280, max: 360)`, isi `ScrollView`:

1. Header: `ProviderTile` 40 pt, `name` `.title2.weight(.semibold)`, `displayName` kalau ada, `StatusLabel` konteks `.standard`.
2. Tombol aksi utama (`.borderedProminent` untuk Start dan Retry, `.bordered` untuk Stop dan tombol sibuk) beserta `DisabledReasonText` kalau nonaktif dengan alasan.
3. Banner masalah: aturan I1 sampai I4 yang cocok sebagai `StateBanner` (tone dan judul dari tabel Needs Attention, isi dari kolom Isi), dan untuk I1 blok kode `lastFailure.excerpt` (sudah tersamar daemon) kalau tidak kosong.
4. `LabeledContent` (nilai `textSecondary`, bisa dipilih):

| Baris | Isi |
| --- | --- |
| Provider | nama tampil, lalu `provider.name` dan revision di baris kedua `.footnote` |
| Version | `runtimeVersion` atau "—" |
| Endpoints | satu baris per endpoint, monospace: "{name}  {host}:{port}" (IPv6 dalam kurung siku), "{name}  {socketPath}", atau "{name}  {host}, port assigned at start" |
| Ownership | `manual` "Manual", asing "Unknown" |
| Readiness | "Ready", "Waiting for probe", "Not ready", atau "Unknown", ditambah `detail` dan "checked {waktu relatif}" dari `observedAt` |
| Health | "Healthy", "Degraded", "Unhealthy", atau "Unknown", ditambah `detail` |
| Process | "PID {pid} · up {durasi}" ditambah " · adopted" kalau `adopted`, atau "Not running" |
| Restarts | "{attempts} in the last {windowSeconds/60} min" kalau `attempts` > 0, "Stopped restarting" kalau `exhausted`, selain itu baris tidak tampil |
| Desired | "Running" atau "Stopped" |
| Logs | "Unavailable" kalau `logsUnavailable`, selain itu baris tidak tampil |
| ID | UUID monospace dan tombol Copy |

5. Tanpa pilihan: `StatePlaceholder` ukuran `.section` "No Selection", "Select an instance to see its details.".

Data path, ukuran data, connection string, dan Open milik fitur 21.

### Aksi instance

`InstanceActionModel` (`@MainActor @Observable`, satu per `DaemonModel`) memberi `primaryAction(for: InstanceSummary) -> InstanceAction` dari fungsi murni `InstanceAction.resolve(summary:pending:)`, dan `perform(_:for:)`.

| Keadaan | Tombol | Aktif | Call |
| --- | --- | --- | --- |
| aksi lokal masih menunggu operasinya | "Starting…" atau "Stopping…" dengan `ProgressView` kecil | tidak | |
| `stopped` | Start | ya | `instance.start` |
| `failed` | Retry | ya | `instance.start` |
| `running`, `recovering` | Stop | ya | `instance.stop` |
| `initializing` | "Initializing…" | tidak | |
| `starting` | "Starting…" | tidak | |
| `stopping` | "Stopping…" | tidak | |
| `unknown` | Stop | tidak, alasan "Stackly can’t confirm this process." | |
| `uninitialized` | Start | tidak, alasan "This instance isn’t initialized." | |

- Klik membuat key baru dengan `MutationPath.newKey()`; `outcomeUnknown` menyimpan key per (instance, aksi) supaya klik berikutnya memakai key yang sama sampai ada balasan pasti.
- `.completed` dengan `accepted` berisi operasi: instance ditandai menunggu sampai `OperationCenter.updates(for:)` berakhir; `succeeded` tidak menampilkan apa pun; `failed` atau `cancelled` menampilkan umpan balik dengan `failure.error.message`. `alreadyInState` tidak menampilkan apa pun dan tidak menandai menunggu.
- Saat diterima, VoiceOver mengumumkan "Starting {name}." atau "Stopping {name}.".
- Umpan balik ditampilkan lewat presenter fitur 67 kalau sudah ada; sampai itu `ActionFeedbackBanner` (`// provisional: F67`): satu `StateBanner` di atas konten halaman (di bawah banner jendela), yang terbaru menggantikan yang lama, dengan tombol Dismiss (`mutates: false`), dan diumumkan lewat `announce`.
- Restart, force stop, dan stop dengan override lease (`supervisor.stop_requires_override`) bukan bagian spec ini; penolakan itu tampil sebagai umpan balik biasa dengan pesan dan hint daemon.

### Navigasi

`ShellNavigator` (`@MainActor @Observable`, satu per jendela, dibuat `AppShell` dan dipasang di environment) membungkus pilihan sidebar `@SceneStorage("shell.section")` spec 0013 dan memberi `show(_ destination: ShellDestination)`:

- `ShellDestination`: `.overview`, `.instances(filter: InstanceFilter = .all, select: UUID? = nil)`, `.catalog`, `.runtimes`, `.operations`, `.diagnostics`.
- `.instances` menyimpan `InstancesRequest` sekali pakai yang dibaca `InstancesPage` saat tampil: filter dipasang, pencarian dikosongkan, dan kalau `select` ada, baris itu dipilih dan inspector ditampilkan.
- `openInstance(_ id:)` sama dengan `.instances(filter: .all, select: id)` (`// provisional: F21`, diganti halaman detail fitur 21).

### Kepemilikan

`InstanceSummary.ownership` adalah kepemilikan oleh environment, bukan asal data. Kolom registry `instances.ownership` (`created`, kelak `restored` dan `adopted` dari fitur 51 dan 57) adalah asal data dan tidak dikirim di field ini. `stacklyd` mengisi `{ "type": "manual" }` untuk setiap instance karena belum ada tabel keanggotaan environment; fitur 44 menambah varian (misalnya dedicated milik satu environment) dan mengganti derivasinya. GUI menulis "Manual"; CLI menulis nilai wire.

### State halaman

| Bentuk | Overview | Instances |
| --- | --- | --- |
| loading | `InstanceStore` belum pernah termuat | sama |
| failed | `FailureContent(title: "Couldn’t Load Overview", message:, code:)`, atau "Waiting for stacklyd" dari aturan presence spec 0013 | "Couldn’t Load Instances", atau "Waiting for stacklyd" |
| unsupported | "Overview Unavailable", "This version of stacklyd doesn’t list instances. Update Stackly." | "Instances Unavailable", alasan sama |
| empty | tidak ada (halaman tetap loaded; seksi Instances menampilkan empty nya sendiri) | `.noData` dan `.noResults` di atas |
| stale | banner stale spec 0012 dari error refresh `InstanceStore` | sama |
| partial | `PartialNote(summary: "Couldn’t refresh {n} instance(s).", items:)` dari `InstanceStore.itemFailures`, item berisi nama (atau ID kalau belum dikenal), pesan error, dan aksi Retry yang memanggil `retry(id)` | sama |

Seksi Overview punya state sendiri: Operations failed (`StatePlaceholder` `.section` "Couldn’t Load Operations" dengan Retry ke `OperationCenter.refresh()`) atau unsupported ("Operations Unavailable"); kartu Runtimes seperti di *Kartu ringkasan*; kapasitas lewat item S4. Kegagalan seksi tidak pernah membuat halaman failed.

### Aksesibilitas

- Overview: urutan fokus kartu, item Needs Attention, Show More, baris instance (baris lalu tombol aksi), Show All, baris operasi. Label VoiceOver: kartu "{label}, {angka}" ditambah ", {n} not ready" dengan hint "Shows {filter} instances." (Runtimes: "Shows Runtimes."); item "{Critical|Warning|Info}: {judul}. {isi}" dengan hint "Opens {tujuan}."; baris instance "{name}, {provider} {versi}, port {port}, Status: {label}" ditambah detail; tombol aksi "{Start|Stop|Retry} {name}"; baris operasi "{judul}, {label state}, {tahap}" ditambah ", {persen} percent".
- Instances: `Table` native (panah memindah pilihan, Return membuka); sel Name memberi label "{name}, {provider} {versi}"; tile disembunyikan dari VoiceOver; picker berlabel "Status filter"; pencarian berlabel "Search instances".
- `InstancesCommands` menambah ke menu View, hanya aktif saat halaman Instances fokus (lewat `FocusedValue`): "Sort Instances By" (Name, Provider, Version, Endpoints, Status), "Ascending" dan "Descending", serta "Show Inspector" atau "Hide Inspector" (⌥⌘I). Kalau peta perintah fitur 69 sudah ada, perintah ini didaftarkan di sana. Pencarian ⌘F memakai perilaku standar `.searchable` sampai fitur 69.
- Status selalu teks (`StatusLabel`, `ToneBadge`); angka Failed yang merah tetap berlabel "Failed"; simbol tone di item Needs Attention didampingi kata tingkat di label VoiceOver; tidak ada isi yang hanya ada di `.help`.
- Teks Overview, item, dan inspector membungkus tanpa dipotong di lebar konten 640 pt; file baru tunduk pada test aturan sumber spec 0012.

### Nilai asing

`InstanceOwnership.unrecognized` "Unknown"; `StorageRoot.unrecognized` "other Stackly files"; `CapacityLevel.unrecognized` item S4 versi level asing; `OperationKind.unrecognized` judul "Operation"; status subsystem asing aturan D1. CLI menulis nilai wire `unrecognized`.

### CLI

Modul `apps/cli/src/commands/instance/` (dibuat fitur 14; kalau belum ada, dibuat di sini): `group.rs` (kelompok dan `InstanceCounts`), `resolve.rs` (nama atau UUID ke `InstanceId`; kalau fitur 14 sudah punya resolver, yang itu dipakai), `list.rs`, `inspect.rs`.

- `list`: memanggil `instance.list { limit: 200, before }` sampai `next` null, berbeda dengan `operation list` spec 0008 yang berhalaman, karena jumlah instance terbatas dan jumlah serta filter harus dihitung dari semua instance. Tabel: `NAME`, `PROVIDER` (`provider.name`), `VERSION` (`-` kalau null), `ENDPOINTS` (`<name>=<host>:<port>`, IPv6 dalam kurung siku, `<name>=<socketPath>` untuk socket, `<name>=<host>:auto` untuk port null; dipisah `, `), `LIFECYCLE`, `READINESS` (`ready`, `not_ready (<reason>)`, `unknown (<reason>)`, atau `-` kalau reason `not_running`), `HEALTH` (aturan sama), `OWNERSHIP`. Warna sel `LIFECYCLE` mengikuti kelompok (`ready` hijau, `notReady` dan `unknown` kuning, `failed` merah, `stopped` tanpa warna). Setelah tabel: baris `<total> instances: <ready> running, <stopped> stopped` ditambah `, <failed> failed`, `, <notReady> not ready`, dan `, <unknown> unknown` kalau lebih dari 0 (`1 instance:` untuk satu), selalu dari semua instance. Halaman yang gagal di tengah membuat perintah gagal dengan error itu tanpa menulis tabel.
- `--json`: `result` `{ "items": [InstanceSummary], "counts": { "total", "ready", "notReady", "stopped", "failed", "unknown" } }`; `items` sudah disaring `--status`, `counts` dari semua instance.
- `inspect <instance>`: argumen yang terurai sebagai UUID memanggil `instance.get`; selain itu `resolve.rs` mencari `name` yang sama persis di semua halaman `instance.list`, dan tanpa hasil gagal `cli.instance_not_found` (kategori `not_found`, exit 10, pesan `no instance named <name>`, hint `list instances with stackly instance list`). Blok `kv`: `Name`, `Display name`, `ID`, `Provider` (`<name> revision <revision>`), `Manifest` (`sha256:` dan 12 hex pertama), `Software`, `Runtime`, `Version`, `Ownership`, `Lifecycle`, `Desired`, `Readiness` (`<state>`, ` (<reason>)`, `: <detail>`, `, checked <umur>`), `Health` (aturan sama), `Process` (`pid <pid>, started <waktu> (<umur>)`, `, adopted`, `, not adoptable`, atau `-`), `Restarts` (`<attempts> of <budget> in <menit>m`, `, next attempt <waktu>`, `, exhausted`), `Last failure` (`<code> at <waktu>`, `, exit code <n>` atau `, signal <n>`, atau `-`), `Blocker`, `Logs` (`available` atau `unavailable`), `Leftover processes` (hanya kalau ada), `Revision`. Lalu tabel `ENDPOINT`, `TRANSPORT`, `ADDRESS`, `PORT MODE`, `BOUND`, lalu `Failure output:` dengan baris `excerpt` berindentasi. Semua teks dari daemon lewat `escape_display` spec 0008.
- Golden di `apps/cli/tests/golden/` (`instance_list.txt`, `instance_list.json`, `instance_list_failed.txt`, `instance_list_empty.txt`, `instance_inspect.txt`, `instance_inspect.json`) dari daemon skrip yang membalas `protocols/fixtures/instance/list-mixed.json`, dengan `TZ=UTC`.

### Copy

| Konteks | Teks |
| --- | --- |
| Subjudul Overview | "{ready} running · {stopped} stopped" ditambah " · {failed} failed", " · {notReady} not ready", dan " · {unknown} unknown" untuk yang lebih dari 0 |
| Subjudul Instances | "1 instance" atau "{n} instances" |
| Keterangan kartu Running | "{n} not ready" |
| Seksi | "Needs Attention", "Instances", "Operations" |
| Needs Attention kosong | "Nothing needs attention." |
| Operations kosong | "Nothing running." |
| Tautan | "Show {n} More", "Show Less", "Show All {n} Instances", "Show All in Operations" |
| Umpan balik ditolak atau operasi gagal | judul "Couldn’t start “{name}”." atau "Couldn’t stop “{name}”.", isi pesan daemon, kode monospace |
| Umpan balik tidak terkirim | `MutationFeedback.notSent` spec 0013: "stacklyd is unavailable. Nothing was sent." |
| Umpan balik tidak pasti | `MutationFeedback.outcomeUnknown` spec 0013 |
| Umpan balik dibatalkan | "“{name}” didn’t {start\|stop}: the operation was cancelled." |
| Pengumuman | "Starting {name}.", "Stopping {name}." |

**Copy kegagalan** (`InstanceFailureCopy`, dari `lastFailure.code`):

| Kode | Teks |
| --- | --- |
| `crash_loop` | "Crash loop after {budget} restarts." |
| `process_exited` | "Exited with code {exitCode}." atau "Its process exited." kalau null |
| `process_signaled` | "Stopped by signal {signal}." |
| `exit_status_unknown` | "Its process stopped unexpectedly." |
| `endpoint_in_use` | "Another process uses one of its ports." |
| `data_in_use` | "Another process holds its data." |
| `spawn_failed` | "macOS couldn’t start its process." |
| `executable_invalid` | "Its runtime executable failed verification." |
| `identity_capture_failed` | "Stackly couldn’t record its process." |
| `config_render_failed` | "Its configuration couldn’t be written." |
| `init_step_failed` | "Initialization failed." |
| `init_step_timeout`, `init_timeout` | "Initialization took too long." |
| `interrupted` | "stacklyd stopped while it was changing state." |
| kode lain | "Failed with {code}." |

### API surface

| Method | Input | Output | Auth | Error utama |
| --- | --- | --- | --- | --- |
| `storage.get_capacity` (baru, `recovery allowed`) | tidak ada | `CapacityReport` | peer user yang sama (spec 0006) | `request.invalid_params` (params tidak kosong); kegagalan root ada di `unavailable` |
| `instance.list` (spec 0010) | `{ limit: 200, before? }` | `{ items: [InstanceSummary], next }` | sama | `request.invalid_params` |
| `instance.get` (spec 0010) | `{ instanceId }` | `InstanceSummary` | sama | `resource.not_found` |
| `instance.start`, `instance.stop` (spec 0010) | `{ instanceId }`, `{ instanceId, overrideLeases: false }` | `InstanceActionAccepted` | sama | tabel spec 0010 |
| `operation.list`, `runtime.list`, `catalog.list_providers`, `daemon.get_health` | lewat store dan heartbeat spec 0013, 0017, 0018 | | | |

| API Swift | Bentuk | Pemakai lain |
| --- | --- | --- |
| `InstanceGroup.of`, `InstanceCounts` | fungsi murni | fitur 32 (menu bar), 44 |
| `AttentionRules.items` | fungsi murni | fitur 29 dan 33 menambah aturan |
| `OperationTitle.title` | fungsi murni | fitur 35 |
| `InstanceActionModel` | `@Observable` main actor | fitur 21, 26, 32 |
| `ShellNavigator`, `ShellDestination` | `@Observable` main actor | semua halaman |
| `ResourceStore.itemFailures`, `retry(_:)` | tambahan store | semua store |

### Model data

Tidak ada tabel, kolom, atau file baru. State GUI hanya di memori (`CapacityModel`, `InstanceActionModel`, `ShellNavigator`), ditambah `@SceneStorage` milik macOS untuk `instances.selection` dan `instances.inspector`. Daemon tidak menyimpan laporan kapasitas; setiap call membaca `statfs` baru.

### Value sourcing

| Aksi atau view | Nilai | Sumber |
| --- | --- | --- |
| kelompok, filter, kartu, subjudul | kelompok per instance | `InstanceSummary.lifecycle` dan `readiness.state` lewat `InstanceGroup.of` (fixture `groups.json`) |
| kartu Runtimes | angka | `RuntimeStore.count` spec 0018 |
| kartu, subjudul | "sudah termuat" | `ResourceStore.loadedAt` tidak nil |
| Needs Attention | kondisi instance | `lifecycle`, `readiness`, `health`, `blocker`, `restart`, `process.adoptable`, `lastFailure` di `InstanceSummary` |
| Needs Attention | `{time}` I7 | `restart.nextAttemptAt` diformat jam singkat locale sistem |
| Needs Attention | operasi O1 | `OperationCenter.active` dengan state `needs_recovery`, judul dari `OperationTitle` |
| Needs Attention | volume dan level | `CapacityReport` dari `storage.get_capacity` |
| Needs Attention | subsystem | `DaemonHealth.subsystems` dari heartbeat terakhir (`DaemonModel`) |
| kapasitas | `total`, `available`, `volume` | `statfs` lewat `Filesystem::volume_space` |
| kapasitas | `level`, `usedPercent` | `stackly_domain::capacity::level` dan `used_percent` dengan konstanta di *Kapasitas disk* |
| kapasitas | daftar root | `RootSet` spec 0004 (data, logs, cache) |
| kapasitas | `checkedAt` | `Clock::now()` daemon |
| baris instance | nama tampil dan tile provider | `CatalogStore` entri `provider.name`; fallback `software` dan tile `gray` |
| baris instance | versi | `InstanceSummary.runtimeVersion` |
| baris instance | port | `InstanceSummary.endpoints[].port`, `transport`, `host`, `socketPath` |
| baris instance | status | `StatusAppearance.instance(lifecycle:readiness:health:)` lewat `StatusAdapter` |
| inspector | ownership | `InstanceSummary.ownership` (daemon: `manual` sampai fitur 44) |
| inspector | uptime | `now` dikurangi `process.startedAt` |
| inspector | waktu relatif readiness | `readiness.observedAt` |
| aksi | jenis tombol | `lifecycle` dan status menunggu lokal lewat `InstanceAction.resolve` |
| aksi | idempotency key | `MutationPath.newKey()` per klik, dipakai ulang setelah `outcomeUnknown` |
| aksi | status menunggu | `accepted.operation.id` lalu `OperationCenter.updates(for:)` |
| operasi | judul | `OperationTitle` dari `kind`, `resource`, `InstanceStore`, `RuntimeStore` (`installing`) |
| operasi | label tahap, persen | `OperationSummary.stage`; persen `install_runtime` dengan aturan spec 0018 dari `stage` dan `progress` |
| pencarian | teks yang dicocokkan | nama, nama tampil, provider, `software`, versi, port |
| urutan | pembanding | tabel kolom *Halaman Instances* |
| partial | item gagal | `ResourceStore.itemFailures` (refetch yang gagal) |
| navigasi | filter dan pilihan tujuan | `ShellDestination` dari tombol yang diklik |
| CLI | semua item | semua halaman `instance.list` dengan `limit` 200 |
| CLI | jumlah | `instance::group::of` atas semua item |
| CLI | instance dari nama | `name` sama persis di semua halaman `instance.list` |
| CLI | umur dan waktu | `format_age` dan `format_local` spec 0008 |

### Key invariants

- Kelompok, jumlah, dan filter hanya dihitung dari `lifecycle` dan `readiness.state`; `process` dan PID tidak pernah membuat instance dihitung Running.
- Tidak ada angka (kartu, subjudul, badge) sebelum snapshot store sumbernya termuat, dan koneksi yang putus tidak pernah mengubah status atau angka (state terakhir tetap, spec 0013).
- Level kapasitas hanya dihitung daemon; GUI dan CLI tidak pernah menghitung ulang ambang.
- GUI tidak pernah membaca filesystem untuk kapasitas, ukuran, atau status.
- `storage.get_capacity` tidak membaca atau menulis registry, tidak membuat operasi, dan berjalan dalam recovery mode.
- Satu satunya mutasi dari kedua halaman adalah `instance.start` dan `instance.stop` lewat `MutationPath`, selalu dengan gate mutasi dan idempotency key.
- Error satu instance, satu refetch, atau satu seksi tidak pernah mengosongkan halaman atau mengganti bentuk halaman menjadi failed.
- Fungsi kelompok di Swift dan Rust selalu lulus fixture yang sama.

### Security model

- Batas kepercayaan satu akun user (spec 0006): semua method lewat socket privat dengan peer user yang sama; CLI dan GUI tidak menambah jalur akses.
- `storage.get_capacity` hanya memberi angka ukuran volume dan nama root, tanpa path atau isi file. Pesan `RootUnavailable` boleh memuat path root milik user itu sendiri.
- `lastFailure.excerpt` sudah lewat `Redactor` daemon (spec 0010 dan 0011); GUI dan CLI menampilkannya apa adanya (CLI lewat `escape_display`).
- Copy Name dan Copy ID hanya menyalin nama dan UUID, tidak pernah secret.
- Daemon tetap memvalidasi setiap start dan stop walau tombol sudah nonaktif.

### Configuration required

Tidak ada variabel environment, flag, atau credential baru. Test memakai `STACKLY_ROOT`, `STACKLY_DAEMON_BIN`, `SupervisorConfig`, dan `FakeFilesystem` yang sudah ada.

### Critical test scenarios

- Kelompok: semua kasus `groups.json` di Swift dan Rust, termasuk `running` dengan readiness `not_ready` dan proses hidup dihitung `notReady`. Memverifikasi **AC-1**.
- Kartu: store belum termuat tanpa angka, store termuat dengan 0 instance memberi nol, keterangan not ready, angka Failed bertone critical, kartu Runtimes tidak dirender tanpa `RuntimeStore`, klik kartu membuka filter yang jumlah barisnya sama. Memverifikasi **AC-2**.
- Needs Attention: satu item per aturan dari `list-mixed.json`, satu instance yang cocok dengan beberapa aturan hanya memberi aturan bernomor terkecil, urutan, batas enam dan Show More, event `instance.health_changed` dengan `statusSeq` lebih besar memunculkan I6 tanpa snapshot. Memverifikasi **AC-3**.
- Kapasitas: `level` di setiap batas (1 999 999 999, 2 000 000 000, 9 999 999 999, 89 dan 90 persen, 79 dan 80 persen, `total` 0), dua root di volume yang sama digabung, root yang gagal masuk `unavailable` tanpa menggagalkan reply, daemon dalam recovery mode tetap menjawab, `mutations()` tidak bertambah; GUI dengan sleep tiruan memanggil ulang setiap 60 detik hanya saat tampil dan tersambung, `request.unknown_method` menjadi S4. Memverifikasi **AC-4**.
- Seksi Overview: enam instance urut nama dan Show All, empty dengan Browse Catalog, operasi tanpa `needs_recovery`, judul setiap kind, persen install, Operations failed tetap menampilkan kartu dan Needs Attention. Memverifikasi **AC-5**, **AC-9**.
- Instances: pencarian banyak kata dengan diakritik, setiap pembanding termasuk seri dan nilai kosong, filter dan pencarian bersamaan, no results dengan Clear Filters, pilihan yang tersaring keluar, Copy Name. Memverifikasi **AC-6**.
- Inspector: setiap baris tabel inspector, banner I1 dengan excerpt, No Selection, keadaan tersimpan. Memverifikasi **AC-7**.
- Aksi: setiap baris tabel aksi, key dipakai ulang setelah `outcomeUnknown`, status menunggu sampai operasi selesai, umpan balik `rejected`, `notSent`, operasi `failed`, `alreadyInState` tanpa umpan balik, tombol nonaktif saat gate `.blocked`. Memverifikasi **AC-8**.
- State: loading, failed tanpa data, unsupported, stale setelah refresh gagal, partial dari refetch yang gagal lalu Retry yang berhasil, daemon mati tidak mengubah status. Memverifikasi **AC-9**.
- CLI: golden list (semua, `--status failed`, kosong) dan inspect, nama tidak ditemukan exit 10, UUID retained lewat `instance.get`, halaman kedua yang gagal, binary test terhadap `stacklyd` asli dengan instance fake. Memverifikasi **AC-10**, **AC-11**.
- Kontrak: fixture call dan reply `storage.get_capacity`, nilai asing setiap enum terbuka di Rust dan Swift, `ownership` di `InstanceSummary`, kenaikan minor, `codegen --check`. Memverifikasi **AC-12**.
- Aksesibilitas: label VoiceOver per elemen dari fungsi label murni, menu View hanya aktif di Instances, preview 640 pt tanpa teks terpotong, test aturan sumber. Memverifikasi **AC-13**.
- Kinerja: integration test Swift (tag `.daemon`) dengan 100 instance fake di `stacklyd` asli, waktu sampai `loadedAt` paling lama 2 detik. Memverifikasi **AC-14**.
- Auth atau permission: tidak ada peran pengguna; akses dibatasi uid socket (spec 0006 dan 0013).

### Cek manual di /check verify

Dicatat sebagai evidence manual spec 0002, di app Debug dengan `stacklyd` asli dan `STACKLY_ROOT` ke folder sementara:

1. Dengan instance fake di setiap kelompok (ready, not ready lewat `readyDelayMs`, stopped, failed lewat crash loop, unknown lewat lock yang dipegang proses lain): kartu, subjudul, Needs Attention, dan `stackly instance list` menunjukkan jumlah dan kelompok yang sama (**AC-1**, **AC-2**, **AC-3**, **AC-10**).
2. Buat image disk APFS kecil dengan `hdiutil create -size 3g -fs APFS`, mount, dan arahkan `STACKLY_ROOT` ke dalamnya: Overview menampilkan S2; isi sampai kurang dari 2 GB tersedia: S1 dalam 60 detik (**AC-4**).
3. Hanya keyboard (Keyboard navigation menyala): Overview, kartu Failed, Instances, pilih baris, inspector, Stop, urutkan lewat menu View (**AC-13**).
4. VoiceOver: alur yang sama, setiap status terbaca sebagai teks (**AC-13**, PRD AC-027 bagian stop dan periksa).
5. Lebar konten 640 pt: Overview satu kolom, tidak ada teks terpotong; light, dark, dan Increase Contrast (**AC-13**).
6. Hentikan `stacklyd`: kedua halaman tetap menampilkan state terakhir, tombol aksi nonaktif dengan alasan; jalankan lagi: state disegarkan (**AC-9**).

## Build plan

Pendekatan proyek adalah Journey. Jalur yang dibangun lengkap di fitur ini adalah "setelah instance pertama ada, pengguna membuka Stackly dan langsung tahu apa yang siap, apa yang bermasalah, dan ke mana memperbaikinya, dari GUI dan CLI", termasuk semua state gagal. Kontrak daemon dulu karena CLI dan GUI sama sama membaca fixture nya, lalu CLI dan GUI bisa jalan paralel, lalu bagian yang menunggu fitur 18 dan bukti akhir. Test yang membuktikan ID PRD memakai trait `.evidence` dan guard evidence spec 0002.

**Milestone 1: Kontrak daemon** (needs fitur 10 terbangun sampai milestone yang membawa `instance.list`, `instance.get`, dan `InstanceSummary` di `01-foundation.md`, dan fitur 6 selesai; usulan spec 0004 dan 0010 di *Usulan additive untuk spec lain* masuk lewat update spec atau langsung di langkah 1 dan 3)

1. `Filesystem::volume_space`, `VolumeSpace`, `VolumeId` di `stackly-platform`, adapter `statfs` di `stackly-platform-macos`, `FakeFilesystem` dengan volume per awalan path dan injeksi error, dan kasus `fs_conformance`. Memenuhi **AC-4**.
2. `stackly_domain::capacity` (level, `used_percent`, konstanta) dan `stackly_core::storage::capacity` dengan unit test batas dan pengelompokan volume. Memenuhi **AC-4**.
3. Protokol: modul `storage` (DTO dan enum terbuka), `storage.get_capacity` di `methods!` dengan `recovery allowed`, `InstanceOwnership` dan field `ownership` di `InstanceSummary` (kalau belum masuk lewat update spec 0010), fixture call dan reply, `protocols/fixtures/instance/groups.json` dan `list-mixed.json`, fixture kapasitas, kenaikan minor `ProtocolVersion::CURRENT`, dan `cargo xtask codegen`. Memenuhi **AC-1**, **AC-12**.
4. `stacklyd`: handler `storage.get_capacity` lewat `spawn_blocking` dengan `RootSet`, pemetaan `ownership` `manual`, dan test daemon (volume rendah lewat `FakeFilesystem`, recovery mode, tanpa mutasi). Memenuhi **AC-4**, **AC-12**.

**Milestone 2: `stackly instance list` dan `inspect`** (after milestone 1; needs fitur 8 build dan fitur 14 build di `01-foundation.md`; bisa paralel dengan milestone 3)

5. `group.rs` dengan test fixture `groups.json`, `resolve.rs`, `list.rs` dan `inspect.rs` dengan output teks dan JSON, `cli.instance_not_found`, golden dari `list-mixed.json` dengan `TZ=UTC`, alias `get` kalau fitur 14 sudah menambahnya, dan binary test terhadap `stacklyd` asli dengan `install_fake`. Memenuhi **AC-1**, **AC-10**, **AC-11**.

**Milestone 3: Halaman Overview dan Instances** (after milestone 1; needs fitur 13 milestone "Wiring, instances, and operations" (`InstanceStore`, `OperationCenter`, `StatusAdapter`) dan fitur 12 di `ui-macos.md`, serta spec 0017 milestone 3 (`CatalogStore`, `ProviderTile`) di fitur 17; bisa paralel dengan milestone 2)

6. Model murni dan infrastruktur: `InstanceGroup` (test fixture), `InstanceCounts`, `InstanceRowModel`, `InstanceSearch`, `InstanceSort`, `InstanceAction.resolve`, `InstanceFailureCopy`, `AttentionRules` (test `list-mixed.json` dan fixture kapasitas), `OperationTitle`, `CapacityModel`, `ResourceStore.itemFailures` dan `retry(_:)`, `ShellNavigator` dan `ShellDestination`, `InstanceStore` dengan `limit` 200. Memenuhi **AC-1**, **AC-3**, **AC-4**, **AC-8**, **AC-9**.
7. Halaman Instances: `InstancesPage`, `InstancesTable`, filter, pencarian, pengurutan, menu konteks, `InstanceInspector`, `InstanceActionModel` dengan `ActionFeedbackBanner` (`// provisional: F67`), `InstancesCommands`, empty dan no results, state halaman, penghapusan `ProvisionalInstanceList` dan penanda `// provisional: F22` nya, dan preview light, dark, serta 640 pt. Memenuhi **AC-6**, **AC-7**, **AC-8**, **AC-9**, **AC-13**.
8. Halaman Overview: kartu Running, Stopped, Failed, Needs Attention, seksi Instances dan Operations, item kapasitas, state seksi, penggantian `UnbuiltPage(section: .overview)` dan penanda nya, preview, dan bagian Overview dan Instances di `apps/macos/design.md` (tabel Needs Attention dan aturan kartu). Memenuhi **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-9**, **AC-13**.

**Milestone 4: Kartu Runtimes dan bukti** (step 9 after fitur 18 milestone 4 "Runtimes page and the Get button"; step 10 after milestone 2 dan 3)

9. Kartu Runtimes dari `RuntimeStore`, judul dan persen operasi `install_runtime` dari baris `installing`, dan test nya. Memenuhi **AC-2**, **AC-5**.
10. Integration test Swift dengan 100 instance fake (evidence NFR-002), lalu evidence manual *Cek manual di /check verify* langkah 1 sampai 6 (keyboard, VoiceOver, 640 pt, image disk kecil, daemon mati). Memenuhi **AC-13**, **AC-14**.

**Sesi paralel yang aman**: setelah milestone 1, milestone 2 (folder `apps/cli/`) dan milestone 3 (folder `apps/macos/`) bisa berjalan di dua sesi karena hanya berbagi fixture yang sudah di commit; langkah 1 dan 2 juga bisa paralel dengan langkah 3.

## Consequences

**Positive**:

- Angka Overview jujur: instance yang hidup tetapi belum siap terlihat sebagai "not ready", bukan Running, dan setiap instance terhitung di tepat satu kelompok.
- GUI dan CLI memakai satu aturan kelompok yang dijaga fixture, jadi `stackly instance list` dan Overview selalu sepakat.
- Peringatan kapasitas punya sumber nyata sekarang, tanpa menunggu metrics fitur 28, dan dihitung sekali di daemon.
- `InstanceActionModel`, `OperationTitle`, `ShellNavigator`, dan `itemFailures` langsung dipakai ulang fitur 21, 26, 32, dan 35.

**Negative / tradeoffs**:

- Aturan kelompok ada di dua bahasa; fixture menjaga kesamaannya, tetapi perubahan aturan berarti tiga file (Swift, Rust, fixture).
- Kartu Running menampilkan angka siap, berbeda dengan mockup yang menghitung status mentah, dan filter Running memuat juga yang belum siap, jadi angka kartu dan jumlah baris hanya sama kalau keterangan not ready ikut dihitung.
- Ambang kapasitas (80, 90 persen, 10 GB, 2 GB) adalah pilihan tanpa data; `f_bavail` di APFS tidak menghitung ruang purgeable, jadi Stackly bisa memperingatkan lebih awal daripada Finder.
- Kapasitas di GUI diperbarui per 60 detik, bukan seketika, dan CLI belum punya padanan kapasitas sampai fitur 34.
- Field `ownership` hanya punya satu nilai sampai fitur 44; nilai itu benar hari ini, tetapi namanya bisa tertukar dengan kolom registry `instances.ownership` (asal data) kalau doc comment diabaikan.
- `stackly instance list` mengambil semua halaman, berbeda dengan pola `operation list` spec 0008.
- Sampai fitur 21 dan 67 ada, "membuka instance" hanya memilih baris di Instances dan umpan balik aksi memakai banner sementara.

**Neutral**:

- Area protokol `storage` dan kenaikan minor untuk satu method dan satu field.
- Folder baru `Overview/` dan isi baru `Instances/` di `StacklyUI`, modul `apps/cli/src/commands/instance/{group,resolve,list,inspect}.rs`, dan fixture baru di `protocols/fixtures/instance/` dan `protocols/fixtures/storage/`.
- Mockup diperbarui untuk keterangan not ready, kolom Environment yang menunggu fitur 44, dan item kapasitas tanpa "grew this week".

### Usulan additive untuk spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead menjadikannya item `architect (update)` atau `/sync`. Semuanya additive.

- **Spec 0010**: field `ownership: InstanceOwnership` di `InstanceSummary` (dan di `instance.state_changed`), enum terbuka bertag (`tag = "type"`, `content = "content"`) dengan varian `Manual` dan `Unrecognized`, diisi `stacklyd` dengan `Manual` selama belum ada keanggotaan environment, dengan fixture dan doc comment yang membedakannya dari kolom registry `instances.ownership`. Baris *Asumsi lintas spec* "fitur 14 membangun `stackly instance ... get`" menjadi "`inspect` dengan alias `get`" (spec 0022).
- **Spec 0004**: `Filesystem::volume_space(path) -> Result<VolumeSpace, FsError>`, `VolumeSpace { total, available, volume: VolumeId }`, adapter `statfs` macOS, `FakeFilesystem`, dan `fs_conformance`; module baru `capacity` di domain dan `storage` di core. Bisa digabung dengan tambahan `Filesystem` spec 0018 (`available_space` di atas `volume_space`).
- **Spec 0013**: `ResourceStore.itemFailures: [Item.ID: ErrorBody]` (diisi saat refetch gagal, dihapus saat item itu berhasil diterapkan dan saat resnapshot selesai) dan `retry(_ id:)` yang mengulang refetch pada revision terakhir yang diumumkan; `ShellNavigator` dan `ShellDestination` di `Shell/`; `InstanceStore` memakai `limit` 200. Baris Follow-up "Fitur 22 ... ganti `UnbuiltPage` dan daftar instance sementara" dijawab AC-2 dan AC-6, dan "kartu operasi Overview milik fitur 22" dijawab AC-5.
- **Spec 0006** (tidak perlu update isi): area `storage` masuk lewat aturan "fitur yang pertama kali memakainya"; baris tabel area bisa ditambah lewat `/sync`.
- **Spec 0008** (catatan saja): `stackly instance list` mengambil semua halaman karena jumlah instance terbatas, dan `cli.instance_not_found` ditambahkan ke katalog `cli`.

## Follow-up

- [ ] `/architect` update spec 0010 dan spec 0004 dengan item di *Usulan additive untuk spec lain* (atau biarkan milestone 1 langkah 1 dan 3 menambahkannya dengan kenaikan minor).
- [ ] Fitur 20: tombol New Instance di toolbar Overview dan Instances serta aksi kedua "New Instance…" di empty Instances.
- [ ] Fitur 21: ganti `openInstance` (`// provisional: F21`) dengan halaman detail, tambah Open di inspector, teks "Double-click a row to open it.", serta baris Data dan Connection di inspector.
- [ ] Fitur 44: kolom Environment dan baris Environment di inspector, varian ownership dedicated dan derivasinya, dan seksi Environments di Overview.
- [ ] Fitur 26 dan 47: Restart dan force stop di menu konteks dan inspector, dan stop dengan override lease.
- [ ] Fitur 29: aturan Needs Attention untuk drift config. Fitur 33: banner crash loop memakai `InstanceFailureCopy`.
- [ ] Fitur 34: Diagnostics menampilkan kapasitas dan menjadi tujuan item S dan D yang berguna; padanan kapasitas di CLI.
- [ ] Fitur 28: pertimbangkan pertumbuhan data ("grew this week") dan kapasitas dari metrics.
- [ ] Fitur 35: halaman Operations memakai `OperationTitle` dan menerima pilihan operasi dari Overview.
- [ ] Fitur 67: ganti `ActionFeedbackBanner` (`// provisional: F67`). Fitur 68: snapshot untuk state kedua halaman. Fitur 69: daftarkan perintah sort, inspector, dan ⌘F di peta perintah.
- [ ] `ui-macos.md` (lewat `/scope`): catat selisih mockup Overview dan Instances: kartu Running menghitung yang siap dengan keterangan not ready, kolom dan seksi Environment menunggu fitur 44, New Instance menunggu fitur 20, item disk tanpa pertumbuhan mingguan, dan item drift menunggu fitur 29.
- [ ] `/sync` setelah build: folder `Overview/` dan isi `Instances/` di `apps/macos/AGENTS.md`, area `storage` di tabel area spec 0006, dan perintah `stackly instance list` dan `inspect` di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan hasil cross check ada di [rationale.md](rationale.md).
