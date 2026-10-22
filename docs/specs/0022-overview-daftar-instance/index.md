# 0022. Overview dan daftar instance

**Date**: 2026-10-20
**Status**: In Progress

## Summary

Spec ini menetapkan dua halaman GUI yang paling sering dibuka, Overview dan Instances, beserta padanannya di CLI, `stackly instance list` dan `stackly instance inspect`. Semua angka dan daftar dihitung dari DTO daemon yang sudah ada (`InstanceSummary` spec 0010, operasi spec 0007, runtime spec 0018) dengan satu aturan kelompok status yang sama di Swift dan Rust, jadi instance yang prosesnya hidup tetapi belum siap tidak pernah dihitung "Running". Daftar Needs Attention menautkan setiap masalah (instance gagal, proses yang tidak terkonfirmasi, readiness yang gagal, operasi yang perlu recovery, dan ruang disk) ke tempat perbaikannya, dan peringatan kapasitas memakai sumber yang terdefinisi: method baru `storage.get_capacity` yang membaca ruang disk volume tempat data Stackly berada. Error di satu instance atau satu seksi tidak pernah mengosongkan halaman, dan sumber Needs Attention yang gagal dibaca (operasi atau ruang disk) tampil sebagai item yang menyebut apa yang tidak terbaca, bukan sebagai progress tanpa akhir.

## Requirements

**User stories**:

- Sebagai pengguna yang membuka Stackly, saya ingin melihat berapa instance yang benar benar siap, berhenti, dan gagal, serta apa yang butuh perhatian beserta jalan ke perbaikannya, supaya saya tahu dalam beberapa detik apakah lingkungan saya sehat.
- Sebagai pengguna dengan banyak instance, saya ingin mencari, menyaring, dan mengurutkan instance, lalu melihat detail singkat dan menjalankan atau menghentikan satu instance tanpa meninggalkan daftar.
- Sebagai pengguna yang disk nya hampir penuh, saya ingin diperingatkan sebelum database saya gagal menulis.
- Sebagai penulis script, saya ingin `stackly instance list` dan `stackly instance inspect` dengan JSON stabil yang isinya sama dengan GUI.
- Sebagai pengguna keyboard atau VoiceOver, saya ingin memakai kedua halaman tanpa informasi yang hanya ada lewat warna atau hover.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-027):

- **AC-1**: Fungsi murni `InstanceGroup.of(lifecycle:readiness:)` di `StacklyUI` (atas enum DTO hasil generate) dan `instance::group::of(&InstanceSummary)` di CLI memetakan setiap kombinasi lifecycle dan state readiness ke `ready`, `notReady`, `stopped`, `failed`, atau `unknown` persis seperti tabel *Kelompok status instance*, dan mengembalikan nil (Rust `None`) hanya untuk lifecycle `retained`. Test Swift dan test Rust membaca data test bersama `protocols/fixtures/instance/groups.json` yang memuat setiap lifecycle dikali setiap state readiness (termasuk `retained` dengan hasil null), dan hasil keduanya sama. Kelompok hanya membaca `lifecycle` dan `readiness.state`, tidak pernah `process`, `endpoints`, atau `restart`, sehingga instance dengan proses hidup yang readiness nya belum `ready` tidak pernah dihitung Running (PAGE-02, "tidak hanya menghitung PID"). Filter Running sama dengan `ready` ditambah `notReady`, Stopped sama dengan `stopped`, dan Failed sama dengan `failed`, di GUI maupun CLI.
- **AC-2**: Halaman Overview menggantikan `UnbuiltPage(section: .overview)` dan penanda `// provisional: F22` nya di spec 0013, berlatar `surfaceWindow`, dengan empat kartu sesuai *Kartu ringkasan*: Running (jumlah `ready`, ditambah keterangan "{n} not ready" kalau `notReady` lebih dari 0), Stopped, Failed (angka bertone critical kalau lebih dari 0), dan Runtimes (jumlah item `RuntimeStore` spec 0018, sama dengan subjudul halaman Runtimes). Angka kartu dan subjudul toolbar hanya tampil setelah snapshot store sumbernya termuat, tidak pernah berupa nol sebelum itu. Subjudul mengikuti *Copy*. Mengaktifkan kartu Running, Stopped, atau Failed membuka Instances dengan filter yang sama dan pencarian kosong; jumlah baris yang tampil sama dengan `ready + notReady` untuk Running, `stopped` untuk Stopped, dan `failed` untuk Failed. Kartu Runtimes membuka halaman Runtimes.
- **AC-3**: Seksi Needs Attention dibangun oleh fungsi murni `AttentionRules.items(instances:operations:runtimes:capacity:now:locale:timeZone:)` yang mengembalikan `AttentionItem` dengan `id` stabil, sesuai tabel *Aturan Needs Attention*: paling banyak satu item per instance (aturan bernomor terkecil yang cocok), urut tone (critical, caution, neutral), lalu kategori (disk, operasi, instance), lalu nomor aturan, lalu kunci urut di tabel. Enam item pertama tampil, sisanya lewat "Show {n} More" yang membuka di tempat (lalu "Show Less"). Setiap item punya simbol tone, judul, isi, dan tujuan dari tabel. Selama `InstanceStore` belum termuat, `OperationCenter` belum termuat (tanpa data, tanpa error, dan tidak unsupported), atau kapasitas belum pernah dicoba, seksi menampilkan `DelayedProgress`. `OperationCenter` yang unsupported, atau yang gagal tanpa data, tidak menahan seksi: item instance dan disk tetap tampil, ditambah item O2 "Operations unavailable" (tone neutral, ke Operations). O2 versi error tetap tampil selama snapshot operasi diulang dan hilang begitu snapshot itu berhasil, lewat Retry seksi Operations atau pengulangan otomatis setiap kali `ShellSync.completedRounds` berubah. `OperationCenter` yang gagal dengan data lama (dan tidak unsupported) memakai data itu tanpa O2. Di luar keadaan menunggu itu, daftar tanpa item menampilkan `InlineEmptyRow("Nothing needs attention.")`. Test membaca `protocols/fixtures/instance/list-mixed.json`, `protocols/fixtures/instance/attention-operations.json`, dan fixture kapasitas, dan membuktikan setiap baris tabel. Event `instance.state_changed` dan `instance.health_changed` memperbarui daftar tanpa snapshot ulang.
- **AC-4**: `storage.get_capacity` (tanpa params, `recovery allowed`) membalas `CapacityReport { checkedAt, totalBytes, availableBytes, usedPercent, level }` untuk volume yang memuat root data (`RootSet` spec 0004), dengan `availableBytes` dijepit ke `totalBytes` dan `level` dihitung daemon lewat `stackly_domain::capacity::level`: `critical` di bawah 2 GB tersedia, `low` terpakai 90 persen ke atas atau di bawah 5 GB tersedia, `filling` terpakai 80 persen ke atas, selain itu `ok`. `statfs` yang gagal, `total` 0, atau pembacaan yang melewati 5 detik menghasilkan `storage.capacity_unavailable` (kategori `unavailable`, retryable, details `reason` `io`, `no_size`, atau `timeout`), dan call yang datang bersamaan menunggu satu pembacaan yang sama. Method ini tidak membaca atau menulis registry, tidak membuat operasi, dan dengan `FakePlatform` tidak menambah `mutations()`. GUI lewat `CapacityModel` milik `DaemonModel` memanggilnya saat Overview tampil, setiap 60 detik selama Overview tampil dan presence `connected`, dan setiap kali `ShellSync.completedRounds` berubah, paling banyak satu call berjalan, dan reply dengan `checkedAt` lebih lama dari laporan yang dipegang diabaikan. Item S1 sampai S4 tampil sesuai tabel; laporan sukses terakhir tetap dipakai saat refresh berikutnya gagal.
- **AC-5**: Seksi Instances di Overview menampilkan enam instance pertama urut nama (tile, nama, "{provider} {versi} · {teks port}", `StatusLabel` konteks `.standard`, dan tombol aksi utama dari AC-8), "Show All {n} Instances" kalau lebih dari enam, dan empty "No Instances" dengan tombol Browse Catalog kalau tidak ada instance. Seksi Operations menampilkan operasi `OperationCenter` yang `queued`, `running`, atau `cancel_requested` (paling baru dulu, paling banyak lima), "Show All in Operations" hanya kalau lebih dari lima, masing masing dengan judul dari `OperationTitle`, `ToneBadge` state, label tahap, dan progress sesuai *Seksi Operations*, atau `InlineEmptyRow("Nothing running.")`. Seksi yang sumbernya belum termuat menampilkan `DelayedProgress`; seksi yang gagal tanpa data atau unsupported menampilkan placeholder seksi nya sendiri, seksi yang gagal dengan data lama tetap menampilkan data itu, dan seksi lain tetap tampil.
- **AC-6**: Halaman Instances menggantikan `ProvisionalInstanceList` dan penanda `// provisional: F22` spec 0013 (`InstanceStore` tetap dipakai): toolbar dengan judul "Instances", subjudul "{n} instances", pencarian "Search", dan tombol Show Inspector; picker bersegmen All, Running, Stopped, Failed; `Table` native dengan kolom Name (tile dan nama), Provider, Version (exact version), Endpoints, Status, dan kolom aksi, yang bisa diurutkan per kolom dengan pembanding di *Halaman Instances* (default Name naik; seri selalu nama lalu id naik; nilai kosong selalu di akhir). Kolom Version dan Endpoints disembunyikan saat lebar tabel di bawah 760 pt. Setiap kata pencarian wajib cocok dengan salah satu field (nama, nama tampil, provider, `software`, versi, port) tanpa peduli huruf besar dan diakritik. Daftar kosong menampilkan "No Instances" dengan Browse Catalog; hasil filter kosong menampilkan "No Results for “{query}”" dengan Clear Filters yang mengembalikan filter All dan pencarian kosong. Menu konteks baris berisi aksi utama, Copy Name, dan Copy ID.
- **AC-7**: Inspector native (`.inspector`) di Instances menampilkan, untuk instance terpilih, isi *Inspector* (tile, nama, status, aksi utama, banner masalah dari aturan I1 sampai I4 dengan cuplikan `lastFailure.excerpt` per baris, lalu Provider, Version, Endpoints, Ownership, Readiness, Health, Process, Restarts, Desired, Logs, dan ID dengan tombol copy), atau "No Selection" kalau tidak ada pilihan atau pilihan tersaring keluar. Ownership `manual` ditulis "Manual". Keadaan tampil atau tersembunyi disimpan `@SceneStorage("instances.inspector")` (default tampil) dan dibalik lewat tombol toolbar atau ⌥⌘I.
- **AC-8**: Aksi utama per instance mengikuti tabel *Aksi instance* (Start, Stop, Retry, tombol sibuk, atau tombol nonaktif dengan alasan yang terlihat, termasuk instance `failed` yang belum pernah terinisialisasi) dan dijalankan lewat `MutationPath.perform` dengan `instance.start { instanceId }` atau `instance.stop { instanceId, overrideLeases: false }`, tanpa konfirmasi (aturan spec 0012 untuk start dan stop tanpa dependen). Tombol menjadi sibuk sejak diklik sampai ada reply, lalu selama operasi yang dibuatnya belum berstate akhir (lewat `OperationCenter.updates(for:)`); status menunggu juga dibersihkan saat resnapshot selesai dan operasi itu tidak lagi aktif, dengan satu `operation.get` untuk umpan balik. Idempotency key baru per klik; key dari `outcomeUnknown` dipakai ulang hanya untuk klik aksi yang sama pada instance yang sama selama `revision` summary nya belum berubah dan paling lama 5 menit, dan dibuang setelah reply pasti. Penolakan, `notSent`, `outcomeUnknown`, dan operasi yang berakhir `failed` atau `cancelled` menampilkan umpan balik dengan copy di *Copy* dan diumumkan VoiceOver; balasan `alreadyInState` tidak menampilkan apa pun. Semua tombol dan item menu aksi memakai `.mutationGated()`.
- **AC-9**: Kedua halaman memakai `ViewState.resolve` spec 0012: loading sebelum snapshot pertama `InstanceStore`, failed ("Couldn’t Load Overview" atau "Couldn’t Load Instances", atau "Waiting for stacklyd" dari aturan presence spec 0013) hanya saat belum ada data, unsupported saat store `unsupported`, stale saat refresh gagal dengan data lama, dan partial saat `InstanceStore.itemFailures` tidak kosong ("Couldn’t refresh {n} instance(s)." dengan item per instance dan Retry, dan baris instance itu menampilkan detail status "Refresh failed"). Instance yang gagal, `unknown`, atau berblocker tampil sebagai baris dan banner item, tidak pernah sebagai error halaman, dan seksi Overview yang gagal tidak mengubah bentuk halaman. Saat daemon tidak terjangkau, kedua halaman tetap menampilkan state terakhir tanpa mengubah status apa pun (spec 0013).
- **AC-10**: `stackly instance list [--status running|stopped|failed]` mengambil semua halaman `instance.list` (limit 200 per halaman), menyaring di client dengan aturan AC-1, menulis tabel `NAME`, `PROVIDER`, `VERSION`, `ENDPOINTS`, `LIFECYCLE`, `READINESS`, `HEALTH`, `OWNERSHIP` ke stdout lalu baris jumlah di *CLI* sebagai baris terakhir stdout, dan dengan `--json` memberi `result` `{ "items": [InstanceSummary], "counts": InstanceCounts }` dengan `counts` dari semua instance. Tanpa instance, stdout hanya `No instances.`; filter tanpa hasil menulis `No <status> instances.` lalu baris jumlah; keduanya keluar 0. Golden CLI dan test view model Swift membaca `list-mixed.json` yang sama dan menghasilkan kelompok dan jumlah yang sama (REQ-028).
- **AC-11**: `stackly instance inspect <instance>` menerima UUID dalam bentuk 36 karakter bertanda hubung, huruf besar atau kecil (memanggil `instance.get`, termasuk instance retained), atau nama persis untuk argumen lain (dicari lewat semua halaman `instance.list`), menulis blok `kv` dan tabel endpoint di *CLI*, dan dengan `--json` memberi `result` `{ "instance": InstanceSummary }`; fitur 21 menambah key `detail` dan `dataUsage` dan flag `--size` (spec 0021). Nama yang tidak ditemukan keluar 10 dengan `cli.instance_not_found` (details `name`); UUID yang tidak ada keluar 10 dengan `resource.not_found` dari daemon; daemon yang tidak jalan keluar 6. Kalau fitur 14 sudah menambah `stackly instance get`, perintah itu menjadi alias `inspect` dengan output yang sama.
- **AC-12**: Kontrak: spec ini memakai field `InstanceSummary.ownership: InstanceOwnership` (enum terbuka tanpa data, `manual` dan `unrecognized`, `#[serde(default)]` bernilai `manual`) dan `InstanceSummary.initialized: bool` (`#[serde(default)]` bernilai true, true kalau `data_initialized_at` terisi) yang dibangun langkah 12 spec 0010 beserta default, nilai asing, dan fixture summary tanpa keduanya (K4, diselaraskan 2026-10-20), dan tidak menambahkannya sendiri; spec ini menambah modul `storage` (`CapacityReport`, `CapacityLevel` terbuka) dengan method `storage.get_capacity` di tabel `methods!` bertanda `recovery allowed` dan kode `storage.capacity_unavailable` dengan hint statis lewat `codes::storage::hint`; fixture wire di `protocols/fixtures/ipc/` yang terdaftar di `index.json` untuk call, reply, error, dan nilai asing `CapacityLevel`; kenaikan `ProtocolVersion::CURRENT` di commit yang sama ke `CURRENT` di main ditambah 1 saat merge, dengan codegen diulang saat rebase (K4); `cargo xtask codegen`; dan test kontrak Rust dan Swift yang lulus. Nilai asing ditampilkan sesuai *Nilai asing*.
- **AC-13**: Kedua halaman bisa dipakai penuh dari keyboard dan VoiceOver sesuai *Aksesibilitas*: kartu, item Needs Attention, baris dan tombol aksi di Overview, segmen filter, pencarian, baris tabel, dan isi inspector bisa difokus dan dijalankan (Tab lalu Space saat Keyboard navigation menyala), aksi instance di Instances dijalankan lewat tombol utama inspector (⌥⌘I menampilkannya) atau menu Instance (Start ⌘R, Stop ⇧⌘R), pengurutan lewat menu View "Sort Instances By", dan setiap elemen punya label VoiceOver yang memuat teks status, bukan hanya warna atau simbol. Tidak ada informasi yang hanya ada di hover. Di lebar konten 640 pt, Overview menjadi satu kolom tanpa teks terpotong dan Instances dengan inspector tertutup menampilkan Name dan Status tanpa menggulir; dengan inspector terbuka di jendela 900 pt (kolom halaman 384 pt), Instances tetap menampilkan Name, Status, dan aksi tanpa menggulir dan tanpa teks terpotong; semua teks lolos kontras spec 0012 (NFR-012, bagian stop dan periksa dari PRD AC-027).
- **AC-14**: Dengan 100 instance fake, `instance.list` (limit 200) di `stacklyd` asli dijawab paling lama 500 ms (test daemon), dan dari `HandshakeAccepted` sampai `InstanceStore.loadedAt` terisi di app dengan server palsu spec 0013 yang membalas 100 summary paling lama 1 detik (test Swift), sehingga snapshot awal NFR-002 tetap di bawah 2 detik (evidence `fixture`). Decode tetap di aktor koneksi `StacklyClient` (test spec 0013), dan model turunan halaman dihitung ulang hanya saat input nya berubah: penghitung recompute tidak naik saat event yang tidak mengubah instance (misalnya event operasi) diterapkan.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md), ditambah semua perbaikan cross check yang disetujui lead atas nama user (2026-10-20): kelompok status dihitung di client dengan satu aturan yang dijaga data test bersama Swift dan Rust; kartu Running menghitung instance `ready` dengan keterangan not ready; Needs Attention diturunkan di client dari store yang sudah ada; kapasitas lewat method baru `storage.get_capacity` untuk volume root data dengan level dihitung daemon; ownership lewat field baru `InstanceSummary.ownership` yang bernilai `manual` sampai fitur 44 (dibangun langkah 12 spec 0010, K4); kolom dan seksi Environment menunggu fitur 44; Start, Stop, dan Retry dibangun di sini lewat `InstanceActionModel` yang dipakai ulang fitur 21; "buka instance" sementara memilih baris di Instances; CLI `list` mengambil semua halaman dan `inspect` menjadi nama kanonik; dan seksi Overview gagal sendiri sendiri. *Ditambah 2026-10-22:* sumber Needs Attention yang gagal tanpa data menjadi item neutral (S4 untuk disk, O2 untuk operasi), bukan progress yang menunggu selamanya.

Overview dan Instances hanya membaca store yang sudah disinkronkan spec 0013 dan satu method baca baru; semua status, kelompok, dan item perhatian adalah fungsi murni dari DTO daemon, dan satu satunya mutasi adalah start dan stop lewat jalur mutasi yang sudah ada.

**Implementation skills**: `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`, untuk pembacaan `statfs` single flight dengan timeout)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya: kontrol native (`Table`, `.inspector`, `Picker` bersegmen, `.searchable`) menggantikan tiruan HTML di mockup, dan view model halaman tetap di main actor sementara decode berjalan di aktor client.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| PAGE-02 (Overview: ringkasan ready, stopped, failed, operasi aktif, peringatan kapasitas, navigasi cepat, tidak hanya PID) | AC-1, AC-2, AC-3, AC-4, AC-5 |
| PAGE-03 (Instances: cari, filter, urut; nama, provider, exact version, endpoint, lifecycle, readiness, ownership) | AC-6, AC-7, AC-12; ownership tampil di inspector dan CLI, tanpa kolom tabel (selisih mockup dicatat); environment reference milik fitur 44 |
| REQ-028 (paritas CLI) | AC-10, AC-11 |
| NFR-002 (snapshot awal 2 detik untuk 100 instance, UI thread tidak terblokir) | AC-14; benchmark formal di fitur 64 (PRD AC-076) |
| NFR-012 (keyboard, VoiceOver, status tidak hanya warna) | AC-13 |
| AC-027 (alur tanpa informasi yang hanya lewat warna atau hover) | AC-13 untuk bagian menghentikan dan memeriksa instance; bagian membuat instance milik fitur 20 |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| `InstanceSummary`, `instance.list`, `instance.get`, `instance.start`, `instance.stop`, event instance, aturan `statusSeq` | spec 0010 (dipakai apa adanya, termasuk field `ownership` dan `initialized` serta DTO `instance.removed` dari langkah 12 spec 0010, K4) |
| DTO operasi, `RecoveryInfo`, `operation.list`, `operation.get`, event `operation.` | spec 0007 |
| `InstanceStore`, `OperationCenter`, `ResourceStore`, `ShellSync`, `StatusAdapter`, `MutationPath`, banner daemon, gate mutasi, aturan presence | spec 0013. Aturan `InstanceStore` dari keputusan K16 (2026-10-20) dibangun langkah 16 milestone 4 spec 0013: efek observasi tanpa revision (misalnya `.observe(id, apply:)`) sehingga `instance.health_changed` dan field status `instance.state_changed` hanya diterapkan kalau `statusSeq` nya lebih besar, tanpa mengubah revision atau tombstone; batas `statusSeq` di reset saat `server_restarted`; instance retained disaring di closure `fetchPage`, di `fetchOne` (yang memberi nil, sehingga refetch dan retry memperlakukannya sebagai delete dengan tombstone), dan di `reduce` (`instance.state_changed` dengan lifecycle `retained` menjadi `.delete`); `limit` 200; dan pemetaan `instance.removed` ke `.delete`. Spec ini menambah `itemFailures` dan `ShellNavigator` (*Usulan update spec lain*) dan tidak memakai aturan observasi per jenis yang umum (`observedAt`), yang menjadi milik store fase 02 pertama yang mengirim observasi seperti itu (`CatalogStore` atau `RuntimeStore`) |
| Token, `StatusAppearance`, `StatusLabel`, `ToneBadge`, `ViewState`, `StateBanner`, `StatePlaceholder`, `InlineEmptyRow`, `DelayedProgress`, `.disabled(reason:)`, `announce` | spec 0012 |
| `CatalogStore`, `ProviderSummary` | spec 0017 |
| `ProviderTile` (`ProviderTile(mark:tile:name:size:)` dan `ProviderTile(_:size:)` dengan `TileSpec`), `.stacklyInspector`, `InspectorHeader`, `CardSection`, `.labeledContentStyle(.stackly)`, `StacklyAction`, `ActionButton`, `ActionMenuItems`, `CopyButton` | fitur 67 ([spec 0067](../0067-komponen-dasar-stackly/index.md), diperbarui 2026-10-21) |
| `RuntimeStore`, daftar `installing`, aturan persen install | spec 0018 |
| Kelompok status, kartu, Needs Attention, kapasitas, halaman Instances, inspector, aksi Start, Stop, Retry, `OperationTitle`, CLI `instance list` dan `inspect` | spec ini |
| Aksi dan sheet New Instance, termasuk mengisi `newInstanceAction` dan memilih instance baru lewat `ShellNavigator` | fitur 20 (spec 0020) |
| Halaman detail instance, Open, data path dan ukuran, connection string | fitur 21 |
| Restart, force stop, stop dengan override lease | fitur 26 dan 47 |
| Kolom dan baris Environment, varian ownership environment, seksi Environments di Overview | fitur 44 |
| Item drift config | fitur 29 |
| Umpan balik setelah aksi | fitur 67 (`FeedbackCenter`, `Feedback.outcome`; diperbarui 2026-10-21) |
| Perintah menu dan shortcut halaman Instances (`find`, `toggleInspector`, urutan, `startInstance`, `stopInstance`) | fitur 69 ([spec 0069](../0069-menu-aplikasi-perintah-shortcut/index.md)): baris `CommandMap` yang diterbitkan halaman ini lewat `.publishesCommands(.page(.instances), …)` (2026-10-21) |
| Halaman Operations, Diagnostics | fitur 35, 34 |

### Kelompok status instance

`InstanceGroup` (Swift) dan `instance::group::InstanceGroup` (CLI) punya lima nilai. Hanya `lifecycle` dan `readiness.state` (enum DTO, bukan enum UI) yang dibaca.

| Lifecycle | Readiness | Kelompok |
| --- | --- | --- |
| `running` | `ready` | `ready` |
| `running` | selain `ready` | `notReady` |
| `initializing`, `starting`, `recovering`, `stopping` | apa pun | `notReady` |
| `stopped`, `uninitialized` | apa pun | `stopped` |
| `failed` | apa pun | `failed` |
| `unknown` | apa pun | `unknown` |
| `retained` | apa pun | nil (tidak ada di daftar) |

`InstanceCounts { total, ready, notReady, stopped, failed, unknown }` dihitung dari semua item store (Swift) atau semua halaman (CLI). `total` sama dengan jumlah kelima kelompok.

| Filter (GUI dan `--status`) | Isi |
| --- | --- |
| All | semua |
| Running (`running`) | `ready` dan `notReady` |
| Stopped (`stopped`) | `stopped` |
| Failed (`failed`) | `failed` |

Instance `unknown` hanya terlihat di All dan di Needs Attention. `stopping` tetap `notReady` karena prosesnya masih hidup sampai keluar.

### Kartu ringkasan

Empat kartu di `LazyVGrid(columns: [GridItem(.adaptive(minimum: 130), spacing: 12)])`, masing masing `Button` bergaya plain di atas `.cardSurface(radius: .large)` dengan padding kartu spec 0012. Isi: baris label (titik 7 pt warna fill lalu label `.callout` `textSecondary`), angka `.largeTitle.weight(.semibold).monospacedDigit()`, dan baris keterangan opsional `.subheadline`.

| Kartu | Titik | Angka | Keterangan | Sumber | Tujuan |
| --- | --- | --- | --- | --- | --- |
| Running | fill `positive` | `counts.ready` | "{n} not ready" (teks tone `caution`) kalau `counts.notReady` > 0 | `InstanceStore` | Instances, filter Running |
| Stopped | fill `neutral` | `counts.stopped` | tidak ada | `InstanceStore` | Instances, filter Stopped |
| Failed | fill `critical` | `counts.failed`, teks tone `critical` kalau > 0 | tidak ada | `InstanceStore` | Instances, filter Failed |
| Runtimes | warna bagian Runtimes (`systemTeal`) | `RuntimeStore.count` | "Couldn’t load" kalau store error tanpa data | `RuntimeStore` (spec 0018) | Runtimes |

- Sebelum store sumbernya termuat, angka diganti `DelayedProgress` kecil, tidak pernah 0.
- Kartu Runtimes hanya dirender kalau `DaemonModel` punya `RuntimeStore` (fitur 18 sudah dibangun); store `unsupported` membuat kartu tidak dirender.
- Instance `unknown` tidak punya kartu; jumlahnya ada di subjudul dan setiap instance nya ada di Needs Attention.

### Halaman Overview

Folder `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Overview/`: `OverviewPage`, `SummaryCards`, `AttentionRules`, `AttentionItem`, `AttentionList`, `OverviewInstanceList`, `OverviewOperations`, `OverviewCopy`. `CapacityModel` ada di `Connection/` karena dimiliki `DaemonModel`.

- `.pageChrome(title: "Overview", subtitle:)` dengan subjudul dari *Copy* setelah snapshot `InstanceStore` pertama. Tombol New Instance hanya kalau `newInstanceAction` terisi (*Titik sambung New Instance*), sebagai `ActionButton` atas `StacklyAction` `newInstance` (2026-10-21, lihat *Titik sambung New Instance*).
- Tata letak: `ScrollView` berlatar `surfaceWindow`, inset halaman 22 pt. Urutan: kartu, lalu dua kolom (kiri: Needs Attention lalu Instances; kanan 300 sampai 360 pt: Operations) saat lebar konten minimal 820 pt, atau satu kolom (Needs Attention, Instances, Operations) di bawahnya, lewat `ViewThatFits(in: .horizontal)`. Judul seksi `.headline`; isi seksi di `.cardSurface()` sebagai daftar berkelompok dengan pemisah.
- Seksi Environments di kolom kanan milik fitur 44 dan tidak dirender di build ini.

**Aturan Needs Attention.** Item instance memakai aturan pertama yang cocok (nomor terkecil). `{name}` adalah `InstanceSummary.name`. Tujuan "instance" berarti `ShellNavigator.openInstance(id)` (lihat *Navigasi*). Tujuan Operations dan Diagnostics masih placeholder sampai fitur 35 dan 34, jadi untuk O1 dan S1 sampai S4 isi item memuat jalan perbaikan yang ada hari ini.

| No | Kondisi | Tone | Judul | Isi | Tujuan |
| --- | --- | --- | --- | --- | --- |
| I1 | lifecycle `failed` | `critical` | "{name} failed" | "Initialization failed." kalau `initialized` false, selain itu teks `lastFailure.code` di *Copy kegagalan* ("It failed." kalau `lastFailure` null) | instance |
| I2 | lifecycle `unknown`, atau blocker `identityUnproven` | `caution` | "Can’t confirm {name}’s process" | "Stackly won’t start or stop it until the process is confirmed or gone." | instance |
| I3 | blocker `dataInUse` | `critical` | "{name}’s data is in use" | "Another process holds its data lock. Stop that process, then start {name}." | instance |
| I4 | blocker `needsSecret` | `caution` | "{name} needs a secret" | "Store the missing secret, then start it again." | instance |
| I5 | `running`, readiness `ready`, health `unhealthy` | `critical` | "{name} is unhealthy" | `health.detail`, atau "Its health check is failing." | instance |
| I6 | `running`, readiness `not_ready` dengan reason selain `awaiting_probe` | `caution` | "{name} isn’t ready" | `readiness.detail`, atau "Its readiness check is failing." | instance |
| I7 | `recovering` | `caution` | "{name} is restarting" | "It crashed. Restart {attempts + 1} of {budget} at {time}." kalau `restart.nextAttemptAt` ada dan `attempts + 1` ≤ `budget`, selain itu "It crashed and Stackly is starting it again." | instance |
| I8 | `running`, readiness `ready`, health `degraded` | `caution` | "{name} is degraded" | `health.detail`, atau "Its health check reports a problem." | instance |
| I9 | `running`, readiness `unknown` dengan reason `stale` | `caution` | "Readiness of {name} is unknown" | "No readiness check has answered recently." | instance |
| I10 | `running` dan `process.adoptable` false | `neutral` | "{name} can’t be adopted again" | "Its process doesn’t hold the instance lock. If stacklyd restarts, Stackly will mark it Unknown." | instance |
| O1 | operasi `needs_recovery` di `OperationCenter.active` dengan `recovery.hint` selain `auto_compensate` | `critical` | "{judul operasi} needs recovery" | menurut `recovery.hint` (`// provisional: F35`): `resume` "It can continue. Resume or abandon it with stackly operation resume or abandon."; `abandon` "It can’t continue. Abandon it with stackly operation abandon."; `manual` "It needs manual cleanup. Check it with stackly operation inspect."; asing "Check it with stackly operation inspect." | Operations |
| O2 *(2026-10-22)* | `OperationCenter.store.unsupported` true (dengan atau tanpa data lama), atau `store.loadedAt` nil dengan `store.error` terisi | `neutral` | "Operations unavailable" | unsupported: "This version of stacklyd doesn’t list operations. Any that need recovery are missing from this list."; error: "Stackly couldn’t read operations. Any that need recovery are missing from this list." | Operations |
| S1 | laporan `critical` | `critical` | "Disk is almost full" | "Only {available} free on the disk that holds Stackly data. Instances can fail when it fills up." | Diagnostics |
| S2 | laporan `low` | `caution` | "Disk space is low" | "About {available} free of {total} on the disk that holds Stackly data." | Diagnostics |
| S3 | laporan `filling` | `neutral` | "Disk is {usedPercent}% full" | "About {available} free on the disk that holds Stackly data." | Diagnostics |
| S4 | belum ada laporan sukses dan call terakhir gagal atau unsupported, atau level laporan `unrecognized` | `neutral` | "Disk space unavailable" (level asing: "Disk state not recognized") | unsupported: "This version of stacklyd doesn’t report disk space."; error: pesan daemon; level asing: "Update Stackly to see this disk state." | Diagnostics |

- `id` item: `instance:<uuid>`, `operation:<uuid>`, `operations` (O2), `disk`. Kunci urut dalam satu aturan: nama instance dengan `localizedStandardCompare` (seri `id`), judul operasi lalu `id`; aturan O2 dan aturan S masing masing hanya satu item.
- *Operasi yang tidak terbaca (2026-10-22).* Satu fungsi murni `OverviewPage.operationsState(active:loaded:failed:unsupported:) -> AttentionOperations?` membaca `OperationCenter` dengan urutan berikut: `store.unsupported` true memberi `.unsupported` (juga kalau store masih memegang data lama dari daemon sebelumnya, karena `ResourceStore` menandai unsupported tanpa membuang `loadedAt`); `store.loadedAt` terisi memberi `.loaded(active)`, juga saat refresh terakhir gagal (data lama tetap dipakai, aturan AC-5); `store.error` terisi memberi `.failed`; selain itu nil (belum termuat). `OverviewPage.operationsContent` memetakan hasil fungsi yang sama untuk seksi Operations (`.unsupported` ke "Operations Unavailable", `.loaded` ke baris tanpa `needs_recovery`, `.failed` ke placeholder dengan Retry, nil ke progress), jadi kedua seksi tidak pernah membaca `OperationCenter` dengan cara berbeda.
- *`AttentionOperations` (2026-10-22).* `nonisolated public enum AttentionOperations: Sendable, Equatable` di `AttentionRules.swift`, dengan doc comment, case `loaded([OperationSummary])`, `failed`, dan `unsupported`. `AttentionRules.items` menerimanya lewat label `operations:` yang sama dengan AC-3. `OverviewPage.attentionItems(summaries:instancesLoaded:operations:capacity:runtimes:now:locale:timeZone:)` menerima `operations: AttentionOperations?` dan memberi nil (seksi menunggu) kalau instance belum termuat, `operations` nil, atau kapasitas belum pernah dicoba; keadaan belum termuat tidak punya case. `.failed` tidak membawa data: operasi yang masuk lewat event setelah snapshot gagal (`ResourceStore` menerapkan event langsung sesudah `fail`) tidak menjadi O1, karena tanpa snapshot daftar itu tidak lengkap, dan isi O2 sudah menyebut bahwa operasi yang perlu recovery hilang dari daftar ini. Item O2: `id` `operations`, kategori operasi, nomor aturan 2, `sortKey` kosong (hanya satu item), jadi di antara item neutral O2 ada setelah S3 dan S4 dan sebelum I10.
- *Isi, Retry, dan kapan O2 hilang (2026-10-22).* Isi O2 tetap tanpa pesan daemon: kegagalan yang sama sudah tampil di placeholder seksi Operations di halaman yang sama, jadi item cukup menyebut apa yang hilang dari daftar ini. O2 tidak punya tombol Retry sendiri, karena setiap item adalah satu `Button` navigasi. Snapshot operasi yang gagal diulang dengan dua cara: Retry di placeholder seksi Operations ("Couldn’t Load Operations"), dan `ShellSync` spec 0013, yang memuat ulang semua store pada `.resnapshot` dan, pada `.caughtUp` (koneksi yang pulih lewat cursor tanpa resnapshot), memuat ulang sekali setiap store yang snapshot terakhirnya gagal dengan error; `completedRounds` naik setelah muat ulang itu selesai, berhasil atau gagal. Jadi "pengulangan otomatis setiap kali `ShellSync.completedRounds` berubah" di AC-3 terpenuhi lewat muat ulang `ShellSync` itu: setiap putaran yang menaikkan `completedRounds` sudah memuat ulang store yang gagal. Store yang gagal lagi menunggu `.caughtUp`, gap, atau Retry berikutnya. Halaman ini tidak mengulang snapshot operasi sendiri; `.task(id: completedRounds)` Overview hanya memanggil `capacity.refresh()` dan `actions.reconcile`. *(Diperbarui 2026-10-22: pengulangan `operations.store.refresh()` di `.task(id: completedRounds)` Overview yang dibangun langkah 9a dihapus di `28cbe678`, karena `ShellSync` kini memuat ulang store yang gagal sekali pada `.caughtUp` (spec 0013, butir `.caughtUp` di *Sinkronisasi snapshot plus cursor*; varian ii di rationale spec 0013) dan snapshot yang berhasil membersihkan `error` dan `unsupported` (`9ee06a25`). AC-3 tidak berubah.)* `ResourceStore` tidak mengosongkan `error` saat refresh mulai, jadi O2 tetap tampil selama refresh berjalan, tidak pernah kembali ke progress, dan hilang begitu snapshot berhasil (O1 muncul kalau ada). O2 versi unsupported tetap tampil selama store unsupported, sama dengan placeholder "Operations Unavailable": `.caughtUp` tidak mencoba ulang store yang unsupported tanpa error, karena method baru hanya datang bersama run daemon baru, dan run itu selalu memberi `.resnapshot`. Snapshot yang berhasil mengosongkan `unsupported` (spec 0013), jadi O2 versi unsupported hilang setelah daemon diperbarui dan snapshot operasinya berhasil *(2026-10-22, `9ee06a25`)*. O2 neutral, jadi dengan enam item atau lebih yang tone nya lebih tinggi ia ada di balik Show More; placeholder seksi Operations tetap terlihat. Tujuan Operations masih placeholder (`UnbuiltPage`, tanpa Retry) sampai fitur 35, sama dengan O1.
- Readiness `unknown` dengan reason `daemon_restarted` atau `not_running` tidak menjadi item (keadaan sementara setelah daemon restart). Operasi `needs_recovery` dengan hint `auto_compensate` tidak menjadi item karena daemon sedang menutupnya sendiri. Status subsystem daemon tidak menjadi item: footer dan seksi Connection di Diagnostics (spec 0013) sudah menampilkannya, dan subsystem `operations` serta `supervisor` yang degraded hanya mengulang O1 dan I2. Recovery mode registry tidak menjadi item karena shell sudah menampilkan banner critical.
- `{available}` dan `{total}` memakai `ByteCountFormatter` gaya `.file` (desimal, sama dengan spec 0017). `{time}` I7 adalah jam singkat locale sistem dari `restart.nextAttemptAt`.
- Item ditulis sebagai `Button` baris: simbol tone (`Tone.symbol`, warna fill, `accessibilityHidden`), judul `.body.weight(.semibold)`, isi `.callout` `textSecondary` yang membungkus, dan chevron dekoratif.

**Seksi Instances.** `OverviewInstanceList`: instance store urut `name` dengan `localizedStandardCompare` (seri `id`), enam pertama. Baris: `ProviderTile` `rowSize` (26 pt) spec 0067, nama `.body.weight(.semibold)`, meta "{provider} {versi} · {teks port}" `.callout` `textSecondary` (versi "—" kalau null), `StatusLabel` konteks `.standard`, dan tombol aksi utama (*Aksi instance*). Seluruh baris kecuali tombol aksi adalah `Button` yang membuka instance. Lebih dari enam: baris "Show All {n} Instances" ke Instances filter All. Tanpa instance: `StatePlaceholder` ukuran `.section` dengan simbol `server.rack`, "No Instances", "Install a runtime from the catalog, then create an instance with its own data directory and port.", dan tombol Browse Catalog (`mutates: false`) ke Catalog.

**Seksi Operations.** `OverviewOperations` membaca `OperationCenter.active` tanpa `needs_recovery` (yang masuk O1), urut `createdAt` turun, paling banyak lima, lalu "Show All in Operations" kalau lebih dari lima. Baris: judul `OperationTitle`, `ToneBadge(StatusAppearance.operation(state:partial:))`, lalu baris progress. *Diperbarui 2026-10-21 (spec 0067):* untuk jenis yang punya definisi tahap, baris progress adalah `StagedProgress(value, layout: .compact, accessibilityLabel: judul)` dengan lima tahap `install_runtime` spec 0018 atau enam tahap `create_instance` spec 0020; jenis lain belum punya definisi tahap (fitur 35, 50, 52), jadi barisnya tetap "{label tahap}" dengan `ProgressView()` indeterminate, pengecualian yang dicatat sampai pemilik operasi menulis definisi tahapnya. Seluruh baris adalah `Button` ke Operations.

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

**Port** (spec 0004, dicatat 2026-10-20): `Filesystem::volume_space(&self, path: &Path) -> Result<VolumeSpace, PlatformError>` dengan `VolumeSpace { total: u64, available: u64 }`. Adapter macOS memanggil `statfs(2)` (blok `unsafe` dengan `// SAFETY:` di `stackly-platform-macos`): `total = f_blocks × f_bsize`, `available = f_bavail × f_bsize`. `FakeFilesystem` memberi `VolumeSpace` dari tabel volume per awalan path yang dipasang test, injeksi error, dan jeda yang bisa disetel; `fs_conformance` mendapat kasus nya. `available_space` spec 0018 adalah provided method `volume_space(path)?.available`, dan `volume_facts` spec 0020 memakai helper `statfs` dan tabel volume yang sama (spec 0004). Yang pertama di merge di antara fitur 18, fitur 20, dan langkah 1 spec ini membangun helper, tabel, dan `volume_space`; yang datang belakangan hanya menambah bagian yang belum ada (untuk spec ini: injeksi error dan jeda di `FakeFilesystem` dan kasus `fs_conformance` nya).

**Domain**: `stackly_domain::capacity` berisi `CapacityLevel { Ok, Filling, Low, Critical }`, `used_percent(total, available) -> u8` (dengan `available` dijepit ke `total`, dihitung `(total − available) × 100 / total` dalam `u128`, dibulatkan ke bawah), dan `level(total, available) -> Option<CapacityLevel>` (`None` kalau `total` 0) dengan urutan cek:

| Level | Kondisi | Konstanta |
| --- | --- | --- |
| `Critical` | `available` < 2 000 000 000 | `CAPACITY_CRITICAL_AVAILABLE` |
| `Low` | `used_percent` ≥ 90, atau `available` < 5 000 000 000 | `CAPACITY_LOW_USED_PERCENT`, `CAPACITY_LOW_AVAILABLE` |
| `Filling` | `used_percent` ≥ 80 | `CAPACITY_FILLING_USED_PERCENT` |
| `Ok` | selain itu | |

**Core**: `stackly_core::storage::capacity(fs: &dyn Filesystem, data_root: &Path, now: Timestamp) -> Result<CapacityReading, CapacityError>` memanggil `volume_space` untuk root data `RootSet` spec 0004 (runtime, logs, dan cache tidak diukur; di produksi dan di bawah `STACKLY_ROOT` semuanya ada di volume yang sama, dan root lain bisa ditambah secara additive kalau fitur 28 atau 55 membutuhkannya). `CapacityError`: `Io { kind }`, `NoSize`.

**Daemon**: handler `storage.get_capacity` menjalankan core di `spawn_blocking` lewat satu slot single flight: call yang datang selama pembacaan berjalan menunggu hasil yang sama; pembacaan yang melewati `CAPACITY_READ_TIMEOUT` (5 detik) menjawab `storage.capacity_unavailable` reason `timeout`, dan thread yang masih menunggu `statfs` dibiarkan selesai lalu hasilnya dibuang, jadi paling banyak satu thread tertahan. Tanpa lock registry, tanpa operasi, tanpa event. Area `storage` sudah tercatat di tabel area spec 0006 dengan spec ini sebagai pemiliknya.

```rust
pub struct CapacityReport { pub checked_at: Timestamp, pub total_bytes: Count, pub available_bytes: Count, pub used_percent: u8, pub level: CapacityLevel }
pub enum CapacityLevel { Ok, Filling, Low, Critical, Unrecognized } // terbuka, snake_case
```

| Kode | Kategori | Retryable | Kapan | Kunci `details` | Hint statis (`codes::storage::hint`) |
| --- | --- | --- | --- | --- | --- |
| `storage.capacity_unavailable` | `unavailable` | ya (hanya baca) | `statfs` gagal (`io`, dengan `kind`), ukuran volume 0 (`no_size`), atau lebih dari 5 detik (`timeout`) | `reason`, `kind` | `timeout`: "The disk that holds Stackly data didn’t answer. Check that it is still connected."; lainnya tanpa hint |

Pesan: `disk space for the data folder is unavailable`.

**GUI**: `CapacityModel` (`@MainActor @Observable`, dimiliki `DaemonModel`, jadi tidak hilang saat pindah halaman) dengan `report: CapacityReport?`, `failure: ErrorBody?`, `unsupported: Bool`, `attempted: Bool`. Ia memanggil `DaemonModel.read(.storageGetCapacity)` saat Overview tampil, setiap 60 detik selama Overview tampil dan presence `connected` (dengan `sleep` yang bisa diganti di test), dan setiap kali `ShellSync.completedRounds` berubah. Paling banyak satu call berjalan; permintaan selama call berjalan dilewati. Reply sukses dengan `checkedAt` lebih lama dari `report` diabaikan. `request.unknown_method` membuat `unsupported` true. Kegagalan setelah pernah sukses hanya mengisi `failure`; item tetap dari `report`.

### Halaman Instances

Folder `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Instances/`: `InstanceStore` (sudah ada), `InstanceGroup`, `InstanceCounts`, `InstanceRowModel`, `InstanceSearch`, `InstanceSort`, `InstanceText` (teks port, `UptimeText`, `RelativeAge`), `InstancesPage`, `InstancesTable`, `InstanceInspector`, `InstanceActionModel`, `InstanceFailureCopy`, dan `InstancesCommands` hanya menurut aturan sementara di *Aksesibilitas*. `ProvisionalInstanceList` dihapus. Tidak ada banner umpan balik sementara (2026-10-21, spec 0067).

- Toolbar: `.pageChrome(title: "Instances", subtitle: "1 instance" atau "{n} instances")` setelah snapshot (jumlah total, bukan hasil filter), `.searchable(text:placement: .toolbar, prompt: "Search")`, tombol New Instance kalau `newInstanceAction` terisi (`ActionButton` atas `StacklyAction` `newInstance`), dan tombol inspector (simbol `sidebar.trailing`, label "Show Inspector" atau "Hide Inspector"). *Diperbarui 2026-10-21 (spec 0067 dan 0069):* tombol inspector adalah `InspectorToggle` dari `.stacklyInspector`, tidak memasang `.keyboardShortcut`, dan tooltip nya `CommandMap.helpText(for: .toggleInspector, title: isPresented ? "Hide Inspector" : "Show Inspector")` ("Hide Inspector (⌥⌘I)"), diberikan sebagai `help:` `.stacklyInspector`; ⌥⌘I datang dari baris `toggleInspector` di menu View.
- Di atas tabel: `Picker` bersegmen berlabel "Status" dengan All, Running, Stopped, Failed. Filter dan pencarian tidak disimpan; pilihan disimpan `@SceneStorage("instances.selection")` sebagai UUID teks.
- `Table(rows, selection:, sortOrder:)` dengan kolom:

| Kolom | Isi | Pembanding |
| --- | --- | --- |
| Name | `ProviderTile` `smallSize` (18 pt) dekoratif dan `name` | `name` dengan `localizedStandardCompare` |
| Provider | nama tampil provider | nama tampil; `software` selama `CatalogStore` belum termuat atau unsupported |
| Version | `runtimeVersion`, atau "—" | `localizedStandardCompare` (angka diurutkan sebagai angka), kosong di akhir |
| Endpoints | teks port | port pertama sebagai angka, socket dan `auto` di akhir |
| Status | `StatusLabel` konteks `.row` | urutan kelompok `failed`, `unknown`, `notReady`, `ready`, `stopped` |
| (tanpa judul) | tombol aksi utama `.controlSize(.small)` | tidak bisa diurutkan |

- Default urut Name naik. Arah urut hanya membalik pembanding kolom itu; seri selalu `name` lalu `id` naik, dan nilai kosong selalu di akhir di kedua arah.
- Lebar minimum kolom Name 120, Provider 90, Version 60, Endpoints 80, Status 110, aksi 72 pt. Saat lebar tabel (diukur lewat `onGeometryChange`) di bawah 760 pt, kolom Version dan Endpoints tidak dirender (nilainya ada di inspector dan di label VoiceOver baris). *Lebar dengan inspector (2026-10-21, keputusan user atas temuan langkah 14 spec 0067):* di jendela minimum 900 pt dengan sidebar (236 pt) dan inspector (280 pt) terbuka, kolom halaman hanya 384 pt (424 pt dengan inspector 240 pt), dan inspector dibatasi paling lebar 280 pt di jendela di bawah 1000 pt (spec 0067), jadi 384 pt adalah batas bawahnya; jaminan konten minimum 640 pt spec 0013 hanya berlaku saat inspector tertutup. Supaya Name, Status, dan aksi tetap terlihat tanpa menggulir di kolom itu, kolom Provider juga tidak dirender saat lebar tabel di bawah 480 pt (nilainya ada di inspector dan di label VoiceOver baris). Sel yang terpotong punya nilai lengkap di `.help` dan label VoiceOver.
- Nama tampil provider: entri `CatalogStore` dengan `ProviderSummary.provider` sama dengan `InstanceSummary.provider.name` (`displayName`); kalau tidak ada, `software` apa adanya. Tile: `ProviderTile(TileSpec(mark:tile:name:), size:)` spec 0067 dengan tile dan mark catalog dan `name` nama tampil; tanpa entri, `TileSpec(mark: nil, tile: nil, name: software)` memberi tile `gray` dengan mark default spec 0017.
- Teks port (`InstanceText.ports`): per endpoint urut DTO, `unix` menjadi "socket", `port` ada menjadi ":{port}", `port` null menjadi "auto"; dipisah spasi.
- Pencarian: query dipecah per spasi; setiap kata wajib `localizedStandardContains` di salah satu field berikut, masing masing dicek sendiri: `name`, `displayName`, nama tampil provider, `provider.name`, `software`, `runtimeVersion`, dan setiap port (":5432" atau "5432").
- Return dan klik ganda pada baris memanggil `openInstance` (*Navigasi*). Menu konteks: `ActionMenuItems` spec 0067 dengan bagian `[[aksi utama], [Copy Name, Copy ID]]`; aksi utama gated, Copy Name dan Copy ID adalah `StacklyAction` dengan `mutates` false yang menyalin lewat `CopyConfirmation.copy(_:announcement:)` dengan pengumuman "Name copied." dan "ID copied.", tanpa gate (diperbarui 2026-10-21).
- Pilihan yang tersaring keluar atau instance yang hilang dari store membuat inspector "No Selection"; pilihan dikembalikan kalau barisnya terlihat lagi.
- Empty dan no results: `EmptyContent` `.noData` (simbol `server.rack`, "No Instances", "Each instance gets its own data directory, port and secret. Start by installing a runtime.", primary Browse Catalog `mutates: false`, secondary "New Instance…" kalau `newInstanceAction` terisi) dan `.noResults(query)` dengan query teks pencarian, atau label filter kalau pencarian kosong, pesan "Check the spelling or try another status.", primary Clear Filters.

**Inspector.** `.stacklyInspector(isPresented:help:content:)` spec 0067 (lebar kolom 240, 280, dan 360 pt, dengan maksimum 280 pt di jendela di bawah 1000 pt; isi di `ScrollView` di atas latar opak `surfaceWindow`; diperbarui 2026-10-21):

1. Header: `InspectorHeader(title: name, subtitle: displayName, tile: TileSpec(mark:tile:name:), status: StatusAppearance.instance(...))` spec 0067, dengan `TileSpec` yang sama dengan baris tabel, tile `mediumSize` (40 pt), `displayName` kalau ada, dan status konteks `.standard`.
2. Tombol aksi utama `ActionButton` (`prominence: .primary` untuk Start dan Retry, `.secondary` untuk Stop dan tombol sibuk, `showsReason: true`, sehingga `DisabledReasonText` tampil di bawahnya kalau nonaktif dengan alasan).
3. Banner masalah: aturan I1 sampai I4 yang cocok sebagai `StateBanner` (tone, judul, dan isi dari tabel Needs Attention), dan untuk I1 blok kode `lastFailure.excerpt` (sudah tersamar daemon), satu baris per baris cuplikan (dipecah di newline kalau DTO nya satu teks), kalau tidak kosong.
4. `LabeledContent` dengan `.labeledContentStyle(.stackly)` di dalam `CardSection` spec 0067 (nilai `textSecondary`, bisa dipilih):

| Baris | Isi |
| --- | --- |
| Provider | nama tampil, lalu `provider.name` dan revision di baris kedua `.footnote` |
| Version | `runtimeVersion` atau "—" |
| Endpoints | satu baris per endpoint, monospace: "{name}  {host}:{port}" (IPv6 dalam kurung siku), "{name}  {host}, port assigned at start" kalau `port` null, "{name}  {socketPath}", atau "{name}  socket, created at start" kalau `socketPath` null |
| Ownership | `manual` "Manual", asing "Unknown" |
| Readiness | `ReadinessText` spec 0021 ("Ready", "Waiting for probe", "Not ready" ditambah detail, "Unknown" ditambah reason, atau "—" saat tidak running), tanpa umur pemeriksaan karena spec 0010 tidak mengirim pembaruan `observed_at` saja |
| Health | "Healthy", "Degraded", "Unhealthy", atau "Unknown", ditambah `detail` |
| Process | "PID {pid} · up {UptimeText}" ditambah " · adopted" kalau `adopted`, atau "Not running" |
| Restarts | "{attempts} in the last {windowSeconds / 60} min" kalau `attempts` > 0, "Stopped restarting" kalau `exhausted`, selain itu baris tidak tampil |
| Desired | "Running" atau "Stopped" |
| Logs | "Unavailable" kalau `logsUnavailable`, selain itu baris tidak tampil |
| ID | UUID monospace dan `CopyButton(id, what: "instance ID", announcement: "ID copied.")` di luar `LabeledContent` |

5. Tanpa pilihan: `StatePlaceholder` ukuran `.section` "No Selection", "Select an instance to see its details. Double-click a row to open it." (spec 0021).

- `UptimeText` dan `RelativeAge` (view memberi waktu acuan `fixtureNow ?? context.date`, aturan spec 0068) adalah fungsi murni dengan gaya spec 0008 (dua satuan terbesar tanpa spasi, misalnya "3m04s" dan "1h02m"; umur "now", "{n}s ago", "{n}m ago", "{n}h ago", "{n}d ago"), diperbarui lewat `TimelineView(.periodic(from:by: 1))` hanya di inspector.
- Fitur 21 (spec 0021) menambah tombol Open di samping aksi utama, baris Data (ukuran dari `instance.get_data_usage` dengan `cachedOnly: true` atau "—", path di baris kedua) sesudah Version, dan baris Connection (entri pertama tersamar dengan Show Password dan Copy) sesudah Endpoints.

### Aksi instance

`InstanceActionModel` (`@MainActor @Observable`, satu per `DaemonModel`) memberi `primaryAction(for: InstanceSummary) -> InstanceAction` dari fungsi murni `InstanceAction.resolve(summary:local:)`, dengan `local` berisi status klik lokal (`none`, `inFlight(action)`, `pending(action, operationId)`), dan `perform(_:for:)`.

*Komponen dan perintah (2026-10-21, spec 0067 dan 0069):* tombol Start, Stop, dan Retry adalah `StacklyAction` yang dibangun dari `InstanceAction` (judul tombol, simbol, `disabledReason` dari kolom Aktif, `isBusy` untuk baris sibuk, `mutates` true) dan dirender `ActionButton`; menu konteks memakai `ActionMenuItems`. Adapter nya `InstanceActionModel.stacklyAction(for:)` (dibangun langkah 6, dipakai juga spec 0021), dengan `id` `start`, `stop`, atau `retry`, dan `isBusy` true untuk baris `inFlight` atau `pending`, "Initializing…", "Starting…", dan "Stopping…". Per permukaan: kolom aksi tabel dan baris Overview memakai `ActionButton(action, showsReason: false)` dengan `.controlSize(.small)`; di Overview alasan nonaktif tampil sebagai `DisabledReasonText` di bawah baris meta, dan di Instances alasan baris terpilih tampil di inspector (`showsReason: true`), selain `.help` di tombol tabel. Aksi utama juga dipetakan ke `startInstance` dan `stopInstance` menurut *Penerbit* spec 0069 (Start atau Retry ke `startInstance`, Stop ke `stopInstance`, dengan alasan dan status sibuk yang sama) dan diterbitkan halaman Instances lewat `.publishesCommands(.page(.instances), …)` untuk instance target. Pemetaan dan penerbitan itu dibangun langkah 7 kalau milestone 2 spec 0069 sudah di merge; kalau langkah 7 lebih dulu, langkah 6 (milestone 3) spec 0069 yang menerbitkannya. Kalimat `.mutationGated()` di AC-8 dan di sini diwujudkan lewat `availability(gate:)` `StacklyAction`, dan `.inspector` di AC-7 lewat `.stacklyInspector`.

| Keadaan | Tombol | Aktif | Call |
| --- | --- | --- | --- |
| `inFlight` atau `pending` | "Starting…" atau "Stopping…" dengan `ProgressView` kecil | tidak | |
| `stopped` | Start | ya | `instance.start` |
| `failed`, `initialized` true | Retry | ya | `instance.start` |
| `failed`, `initialized` false | Retry | tidak, alasan "Initialization failed. Remove this instance and create it again." | |
| `running`, `recovering` | Stop | ya | `instance.stop` |
| `initializing` | "Initializing…" | tidak | |
| `starting` | "Starting…" | tidak | |
| `stopping` | "Stopping…" | tidak | |
| `unknown` | Stop | tidak, alasan "Stackly can’t confirm this process." | |
| `uninitialized` | Start | tidak, alasan "This instance isn’t initialized." | |

- Klik langsung menandai `inFlight` sebelum call dikirim, jadi klik kedua selama menunggu reply tidak mengirim apa pun.
- Key: `MutationPath.newKey()` per klik. Setelah `outcomeUnknown`, key disimpan per (instance, aksi) bersama `revision` summary dan waktunya; klik berikutnya untuk aksi yang sama memakai key itu hanya kalau `revision` belum berubah dan belum lewat 5 menit. Key dibuang setelah reply pasti (`completed` atau `rejected`).
- `.completed` dengan `accepted` berisi operasi: status menjadi `pending` sampai `OperationCenter.updates(for:)` memberi state akhir. Status `pending` juga dibersihkan saat `ShellSync.completedRounds` berubah dan operasi itu tidak ada di `OperationCenter.active`; saat itu satu `operation.get` lewat `DaemonModel.read` mengambil state akhir untuk umpan balik (gagal membaca berarti tanpa umpan balik). `succeeded` tidak menampilkan apa pun; `failed` atau `cancelled` menampilkan umpan balik dengan `failure.error.message`. `alreadyInState` tidak menampilkan apa pun dan langsung kembali ke `none`.
- Saat diterima, VoiceOver mengumumkan "Starting {name}." atau "Stopping {name}.".
- Umpan balik lewat `FeedbackCenter` spec 0067 (diperbarui 2026-10-21): `Feedback.outcome` untuk `notSent` dan `outcomeUnknown` (dengan `showOperations` yang memanggil `ShellNavigator.show(.operations)`); untuk `rejected` dan operasi `failed`, kartu tetap menyebut instance nya: `Feedback(tone: .critical, message: "Couldn’t start “{name}”." atau "Couldn’t stop “{name}”.", detail: DaemonText.sentence(body.message), code: body.code)`, dengan `body` dari error yang ditolak atau `OperationFailureInfo.error`; dan `Feedback.neutral` dengan copy spec ini untuk `cancelled`. Kartu umpan balik mengumumkan dirinya sendiri, jadi tidak ada panggilan `announce` untuk umpan balik.
- Restart, force stop, dan stop dengan override lease (`supervisor.stop_requires_override`) bukan bagian spec ini; penolakan itu tampil sebagai umpan balik biasa dengan pesan dan hint daemon.
- Start, Stop, dan Retry tidak bergantung capability provider, jadi spec ini tidak menambah capability ke `InstanceSummary`; aksi berikutnya yang bergantung capability (misalnya reload atau restart) menambahkannya saat membutuhkannya (spec 0012).

### Navigasi

`ShellNavigator` (`@MainActor @Observable`, satu per jendela, dibuat `AppShell` dan dipasang di environment) memberi `show(_ destination: ShellDestination)`, `openInstance(_ id:)`, `section`, dan `instancesRequest`, ditambah `instanceRequest` dari langkah 10 spec 0021:

- `AppShell` tetap memegang `@SceneStorage("shell.section")` spec 0013 dan mencerminkannya ke `ShellNavigator.section` (diisi saat `init`, ditulis balik lewat `onChange` di kedua arah), karena `@SceneStorage` hanya bisa hidup di view.
- `ShellDestination`: `.overview`, `.instances(filter: InstanceFilter = .all, select: UUID? = nil)`, `.instance(UUID)` (halaman detail fitur 21, ditambah langkah 10 spec 0021), `.catalog`, `.runtimes`, `.operations`, `.diagnostics`.
- `.instances` mengisi `instancesRequest` (filter dan pilihan). `InstancesPage` membacanya lewat `.onChange(of: navigator.instancesRequest, initial: true)`, jadi permintaan tetap diterapkan saat Instances sudah tampil: filter dipasang, pencarian dikosongkan, dan kalau `select` ada, baris itu dipilih dan inspector ditampilkan; lalu `instancesRequest` dikosongkan.
- *(Ditambah langkah 10 spec 0021.)* `.instance(id)` memilih bagian Instances dan mengisi `instanceRequest`. `InstancesPage` memegang `NavigationStack` dengan path `[InstanceRoute]` dan membaca request itu lewat `.onChange(of: navigator.instanceRequest, initial: true)`: path diisi `[InstanceRoute(id)]`, baris itu dipilih, lalu `instanceRequest` dikosongkan, jadi permintaan berlaku juga saat Instances belum tampil. Klik ulang Instances di sidebar saat path tidak kosong mengosongkan path (spec 0021).
- `openInstance(_ id:)` sama dengan `.instances(filter: .all, select: id)` (`// provisional: F21`); langkah 10 spec 0021 menggantinya menjadi `.instance(id)`.
- Setelah langkah 2 milestone 1 spec 0016, `MainWindowRoot` menjadi isi jendela dan menyerap pemasangan model ke `AppShell` (K18); `AppShell` tetap ada dan tetap membuat `ShellNavigator`. Langkah 6 dan 7 spec ini, langkah 2 milestone 1 spec 0016, dan langkah 14 milestone 3 spec 0020 sama sama mengubah file shell (`AppShell.swift`, `StacklyApp.swift`), jadi tidak ada dua di antaranya yang berjalan sebagai sesi bersamaan.

**Titik sambung New Instance** (disepakati dengan arc-f20-spec, tercatat di spec 0020): `EnvironmentValues.newInstanceAction: NewInstanceAction?`, struct `@MainActor` dengan `callAsFunction()`, diisi fitur 20 di `AppShell`. Selama nil, Overview dan Instances tidak merender tombol New Instance maupun aksi kedua empty state; kalau terisi, keduanya dirender sebagai `ActionButton` atas `StacklyAction` `newInstance` yang diterbitkan `AppShell` (id `newInstance`, judul "New Instance" di toolbar dan "New Instance…" di empty state, `mutates` true, `perform` = `newInstanceAction()`), dengan tooltip `CommandMap.helpText(for: .newInstance)`; gate mutasi diterapkan lewat `availability(gate:)` (diperbarui 2026-10-21). Setelah create berhasil, fitur 20 memanggil `ShellNavigator.show(.instances(filter: .all, select: id))`. Langkah 16 spec 0013 membangun `ProvisionalInstanceList` tanpa aksi dan tidak mendefinisikan `NewInstanceAction` (K17). Yang lebih dulu di merge di antara langkah 6 spec ini dan langkah 14 milestone 3 spec 0020 mendefinisikan environment key nya; kalau milestone 3 spec ini sudah di merge, fitur 20 hanya mengisi halaman spec ini dan tidak menyentuh daftar sementara.

### Kepemilikan

`InstanceSummary.ownership` (dibangun langkah 12 spec 0010, dan `stacklyd` mengisinya di langkah 13 spec itu, K4) adalah kepemilikan oleh environment, bukan asal data. Doc comment nya juga membedakannya dari `Ownership` environment di domain (`Dedicated` atau `Shared`). Kolom registry `instances.ownership` (`created`, kelak `restored` dan `adopted` dari fitur 51 dan 57) adalah asal data dan tidak dikirim di field ini. Enum nya enum terbuka tanpa data sesuai aturan spec 0006 (`"manual"`, `"unrecognized"`), dan field nya `#[serde(default)]` bernilai `manual`, sehingga daemon yang belum mengirimnya tetap terbaca benar (sebelum fitur 44 semua instance memang manual). `stacklyd` mengisi `manual` untuk setiap instance; fitur 44 menambah field saudara untuk environment pemilik dan nilai baru di enum ini. GUI menulis "Manual"; CLI menulis nilai wire.

### State halaman

| Bentuk | Overview | Instances |
| --- | --- | --- |
| loading | `InstanceStore` belum pernah termuat | sama |
| failed | `FailureContent(title: "Couldn’t Load Overview", message:, code:)`, atau "Waiting for stacklyd" dari aturan presence spec 0013 | "Couldn’t Load Instances", atau "Waiting for stacklyd" |
| unsupported | "Overview Unavailable", "This version of stacklyd doesn’t list instances. Update Stackly." | "Instances Unavailable", alasan sama |
| empty | tidak ada (halaman tetap loaded; seksi Instances menampilkan empty nya sendiri) | `.noData` dan `.noResults` di atas |
| stale | banner stale spec 0012 dari error refresh `InstanceStore` | sama |
| partial | `PartialNote(summary: "Couldn’t refresh {n} instance(s).", items:)` dari `InstanceStore.itemFailures`, item berisi nama (atau ID kalau belum dikenal), pesan error, dan aksi Retry yang memanggil `retry(id)` | sama, dan baris instance itu memakai detail status "Refresh failed" (tone `caution`) menggantikan detail lain |

Seksi Overview punya state sendiri: Operations loading (`DelayedProgress`), failed tanpa data (`StatePlaceholder` `.section` "Couldn’t Load Operations" dengan Retry ke `OperationCenter.refresh()`), unsupported ("Operations Unavailable"), atau data lama yang tetap tampil saat refresh gagal; kartu Runtimes seperti di *Kartu ringkasan*; kapasitas lewat item S4. Needs Attention tidak menunggu operasi yang unsupported atau gagal tanpa data: item O2 menggantikan progress, dengan keadaan dari `operationsState` yang sama dengan seksi Operations, dan snapshot operasi yang gagal tanpa data dimuat ulang oleh `ShellSync` spec 0013 (sekali pada `.caughtUp`, dan bersama semua store pada `.resnapshot`), bukan oleh halaman ini (*Aturan Needs Attention*, 2026-10-22; diperbarui 2026-10-22 setelah `28cbe678`). Kegagalan seksi tidak pernah membuat halaman failed.

### Aksesibilitas

- Overview: urutan fokus kartu, item Needs Attention, Show More, baris instance (baris lalu tombol aksi), Show All, baris operasi. Label VoiceOver: kartu "{label}, {angka}" ditambah ", {n} not ready" dengan hint "Shows {filter} instances." (Runtimes: "Shows Runtimes."); item "{Critical|Warning|Info}: {judul}. {isi}" dengan hint "Opens {tujuan}." (O2 misalnya "Info: Operations unavailable. Stackly couldn’t read operations. Any that need recovery are missing from this list." dengan hint "Opens Operations."; O2 tidak diumumkan saat muncul, sama dengan item lain, dan ada di urutan fokus item seperti item lain); baris instance "{name}, {provider} {versi}, {teks port lisan}, Status: {label}" ditambah detail, dengan teks port lisan "port 5432", "ports 4222 and 8222", "socket", atau "automatic port"; tombol aksi "{Start|Stop|Retry} {name}"; baris operasi "{judul}, {label state}, {tahap}" ditambah ", {persen} percent".
- Instances: `Table` native (panah memindah pilihan, Return membuka); label baris "{name}, {provider} {versi}, {teks port lisan}, Status: {label}"; tile disembunyikan dari VoiceOver; picker berlabel "Status filter"; pencarian berlabel "Search instances". Tombol di dalam baris tabel tidak dijanjikan bisa dicapai dengan Tab; jalur keyboard untuk aksi adalah tombol utama inspector (⌥⌘I menampilkannya), dan setelah spec 0069 juga menu Instance (Start ⌘R, Stop ⇧⌘R).
- Perintah menu (diperbarui 2026-10-21, spec 0069): perintah Sort Instances By (tujuh baris `sortInstances…`: Name, Provider, Version, Endpoints, Status, lalu Ascending dan Descending) dan Show Inspector atau Hide Inspector ⌥⌘I adalah baris `CommandMap` spec 0069 yang diterbitkan halaman Instances lewat `.publishesCommands(.page(.instances), …)`; tidak ada `InstancesCommands`. Kalau milestone 3 spec ini dibangun sebelum milestone 2 spec 0069, `InstancesCommands` (menu View, aktif hanya saat halaman Instances fokus lewat `FocusedValue`) dibangun dengan penanda `// provisional: F69`, dan langkah 6 (milestone 3) spec 0069 menghapusnya. Halaman menerbitkan `find` dengan `.searchFocused`; sebelum milestone 2 spec 0069, ⌘F memakai perilaku standar `.searchable`.
- Status selalu teks (`StatusLabel`, `ToneBadge`); angka Failed yang merah tetap berlabel "Failed"; simbol tone di item Needs Attention didampingi kata tingkat di label VoiceOver; tidak ada isi yang hanya ada di `.help`.
- Teks Overview, item, dan inspector membungkus tanpa dipotong di lebar konten 640 pt, dan halaman Instances juga di kolom halaman 384 pt saat inspector terbuka di jendela 900 pt (2026-10-21, lihat *Halaman Instances*); file baru tunduk pada test aturan sumber spec 0012.

### Nilai asing

`InstanceOwnership.unrecognized` "Unknown"; `CapacityLevel.unrecognized` item S4 versi level asing; `RecoveryHint.unrecognized` isi O1 versi asing; `OperationKind.unrecognized` judul "Operation". CLI menulis nilai wire `unrecognized`.

### CLI

Modul `apps/cli/src/commands/instance/` (dibuat fitur 14; kalau belum ada, dibuat di sini): `group.rs` (kelompok dan `InstanceCounts`), `resolve.rs` (nama atau UUID ke `InstanceId`; kalau fitur 14 sudah punya resolver, yang itu dipakai), `list.rs`, `inspect.rs`.

- `list`: memanggil `instance.list { limit: 200, before }` sampai `next` null, seperti `stackly runtime list` spec 0018 dan berbeda dengan `operation list` spec 0008 yang berhalaman, karena jumlah instance terbatas dan jumlah serta filter harus dihitung dari semua instance. Tabel di stdout: `NAME`, `PROVIDER` (`provider.name`), `VERSION` (`-` kalau null), `ENDPOINTS` (`<name>=<host>:<port>`, IPv6 dalam kurung siku, `<name>=<host>:auto` untuk port null, `<name>=<socketPath>` untuk socket, `<name>=socket (pending)` untuk `socketPath` null; dipisah `, `), `LIFECYCLE`, `READINESS` (`ready`, `not_ready (<reason>)`, `unknown (<reason>)`, atau `-` kalau reason `not_running`), `HEALTH` (aturan sama), `OWNERSHIP`. Warna (aturan TTY spec 0008): `LIFECYCLE` menurut kelompok (`ready` hijau, `notReady` dan `unknown` kuning, `failed` merah, `stopped` tanpa warna); `READINESS` `ready` hijau, `not_ready` dan `unknown` kuning; `HEALTH` `healthy` hijau, `degraded` dan `unknown` kuning, `unhealthy` merah; `-` tanpa warna. Baris terakhir stdout: `<total> instances: <ready> running, <stopped> stopped` ditambah `, <failed> failed`, `, <notReady> not ready`, dan `, <unknown> unknown` kalau lebih dari 0 (`1 instance:` untuk satu), selalu dari semua instance. Halaman yang gagal di tengah membuat perintah gagal dengan error itu tanpa menulis tabel.
- `--json`: `result` `{ "items": [InstanceSummary], "counts": { "total", "ready", "notReady", "stopped", "failed", "unknown" } }`; `items` sudah disaring `--status`, `counts` dari semua instance.
- `inspect <instance>`: argumen yang cocok dengan bentuk UUID 36 karakter bertanda hubung (huruf besar atau kecil, dinormalkan ke huruf kecil) memanggil `instance.get`; argumen lain dianggap nama dan `resolve.rs` mencari `name` yang sama persis di semua halaman `instance.list`, tanpa hasil gagal `cli.instance_not_found` (kategori `not_found`, exit 10, details `name`, pesan `no instance named <name>`, hint statis `list instances with stackly instance list`). Blok `kv`: `Name`, `Display name`, `ID`, `Provider` (`<name> revision <revision>`), `Manifest` (`sha256:` dan 12 hex pertama), `Software`, `Runtime`, `Version`, `Ownership`, `Initialized` (`yes` atau `no`), `Lifecycle`, `Desired`, `Readiness` (`<state>`, ` (<reason>)`, `: <detail>`, `, checked <umur>`), `Health` (aturan sama), `Process` (`pid <pid>, started <waktu> (<umur>)`, `, adopted`, `, not adoptable`, atau `-`), `Restarts` (`<attempts> of <budget> in <menit>m`, `, next attempt <waktu>`, `, exhausted`), `Last failure` (`<code> at <waktu>`, `, exit code <n>` atau `, signal <n>`, atau `-`), `Blocker`, `Logs` (`available` atau `unavailable`), `Leftover processes` (hanya kalau ada), `Revision`. Lalu tabel `ENDPOINT`, `TRANSPORT`, `ADDRESS`, `PORT MODE`, `BOUND`, lalu `Failure output:` dengan baris `excerpt` berindentasi. Semua teks dari daemon lewat `escape_display` spec 0008.
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
| Umpan balik ditolak atau operasi gagal | pesan kartu "Couldn’t start “{name}”." atau "Couldn’t stop “{name}”.", detail pesan daemon sebagai kalimat (`DaemonText.sentence`), dan kode (2026-10-21) |
| Umpan balik tidak terkirim | `MutationFeedback.notSent` spec 0013 lewat `Feedback.outcome`: "stacklyd is unavailable. Nothing was sent." |
| Umpan balik tidak pasti | `MutationFeedback.outcomeUnknown` spec 0013 lewat `Feedback.outcome`, dengan aksi "Show Operations" |
| Umpan balik dibatalkan | `Feedback.neutral("“{name}” didn’t start: the operation was cancelled.")` atau `Feedback.neutral("“{name}” didn’t stop: the operation was cancelled.")` |
| Pengumuman | "Starting {name}.", "Stopping {name}." |
| Detail status partial | "Refresh failed" |

**Copy kegagalan** (`InstanceFailureCopy`, dari `lastFailure.code`; `lastFailure` null menjadi "It failed."):

| Kode | Teks |
| --- | --- |
| `crash_loop` | "Crash loop after {budget} restarts." |
| `process_exited` | "Exited with code {exitCode}." atau "Its process exited." kalau null |
| `process_signaled` | "Stopped by signal {signal}." atau "Stopped by a signal." kalau null |
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
| `quarantined` (fitur 18, K7) | "macOS quarantined its runtime." |
| `binary_rejected_by_os` (fitur 18, K7) | "macOS refused to run its runtime." |
| kode lain | "Failed with {code}." |

### API surface

| Method | Input | Output | Auth | Error utama |
| --- | --- | --- | --- | --- |
| `storage.get_capacity` (baru, `recovery allowed`) | tidak ada | `CapacityReport` | peer user yang sama (spec 0006) | `storage.capacity_unavailable`, `request.invalid_params` |
| `instance.list` (spec 0010) | `{ limit: 200, before? }` | `{ items: [InstanceSummary], next }` | sama | `request.invalid_params` |
| `instance.get` (spec 0010) | `{ instanceId }` | `InstanceSummary` | sama | `resource.not_found` |
| `instance.start`, `instance.stop` (spec 0010) | `{ instanceId }`, `{ instanceId, overrideLeases: false }` | `InstanceActionAccepted` | sama | tabel spec 0010 |
| `operation.get` (spec 0007) | `{ operationId }` | `OperationDetail` | sama | `resource.not_found` |
| `operation.list`, `runtime.list`, `catalog.list_providers` | lewat store spec 0013, 0017, 0018 | | | |

| API Swift | Bentuk | Pemakai lain |
| --- | --- | --- |
| `InstanceGroup.of`, `InstanceCounts` | fungsi murni | fitur 32 (menu bar), 44 |
| `AttentionRules.items`, `AttentionItem` | fungsi murni | fitur 29 dan 33 menambah aturan |
| `OperationTitle.title` | fungsi murni | fitur 35 |
| `InstanceActionModel`, `InstanceAction.resolve` | `@Observable` main actor, fungsi murni | fitur 21, 26, 32 |
| `ShellNavigator`, `ShellDestination`, `newInstanceAction` | `@Observable` main actor, nilai environment | semua halaman, fitur 20 |
| `CapacityModel` | `@Observable` main actor di `DaemonModel` | fitur 34 |
| `ResourceStore.itemFailures`, `retry(_:)` | tambahan store | semua store |
| `InstanceText.ports`, `UptimeText`, `RelativeAge` | fungsi murni | fitur 21, 32 |

### Model data

Tidak ada tabel, kolom, atau file baru. State GUI hanya di memori (`CapacityModel`, `InstanceActionModel`, `ShellNavigator`), ditambah `@SceneStorage` milik macOS untuk `instances.selection` dan `instances.inspector`. Daemon tidak menyimpan laporan kapasitas; setiap pembacaan memanggil `statfs` baru.

### Value sourcing

| Aksi atau view | Nilai | Sumber |
| --- | --- | --- |
| kelompok, filter, kartu, subjudul | kelompok per instance | `InstanceSummary.lifecycle` dan `readiness.state` lewat `InstanceGroup.of` (data test `groups.json`) |
| kartu Runtimes | angka | `RuntimeStore.count` spec 0018 |
| kartu, subjudul | "sudah termuat" | `ResourceStore.loadedAt` tidak nil |
| Needs Attention | kondisi instance | `lifecycle`, `readiness`, `health`, `blocker`, `restart`, `process.adoptable`, `lastFailure`, `initialized` di `InstanceSummary` |
| Needs Attention | `{time}` I7 | `restart.nextAttemptAt`, diformat jam singkat dengan `now`, `locale`, dan `timeZone` sistem yang diberikan ke `AttentionRules.items` |
| Needs Attention | operasi O1 | `OperationCenter.active` dengan state `needs_recovery`, `recovery.hint`, judul dari `OperationTitle` |
| Needs Attention | O2 dan keadaan menunggu operasi (2026-10-22) | `OperationCenter.active` dan `store.unsupported`, `loadedAt`, dan `error` lewat `OverviewPage.operationsState` (unsupported, ada data, error, belum termuat) menjadi `AttentionOperations` atau nil; fungsi yang sama dipakai `operationsContent` |
| Needs Attention | level dan ukuran disk | `CapacityReport` dari `CapacityModel` |
| kapasitas | `total`, `available` | `statfs` pada root data `RootSet` lewat `Filesystem::volume_space` |
| kapasitas | `level`, `usedPercent` | `stackly_domain::capacity::level` dan `used_percent` dengan konstanta di *Kapasitas disk* |
| kapasitas | `checkedAt` | `Clock::now()` daemon saat pembacaan selesai |
| kapasitas | waktu pemanggilan | tampil Overview, tick 60 detik, perubahan `ShellSync.completedRounds` |
| baris instance | nama tampil dan tile provider | `CatalogStore` entri dengan `ProviderSummary.provider` sama dengan `InstanceSummary.provider.name`; fallback `software` dan tile `gray` dengan mark default spec 0017 |
| baris instance | versi | `InstanceSummary.runtimeVersion` |
| baris instance | teks port | `InstanceSummary.endpoints[].port`, `transport`, `host`, `socketPath` lewat `InstanceText.ports` |
| baris instance | status | `StatusAppearance.instance(lifecycle:readiness:health:)` lewat `StatusAdapter`, dengan detail "Refresh failed" kalau id ada di `itemFailures` |
| inspector | ownership | `InstanceSummary.ownership` (daemon: `manual` sampai fitur 44, diisi langkah 13 spec 0010) |
| inspector | uptime | `now` dikurangi `process.startedAt` lewat `UptimeText` |
| inspector | teks readiness | `ReadinessText` spec 0021 dari `readiness`, tanpa umur; kalau langkah 9 spec 0021 belum di merge, langkah 7 spec ini membangun `ReadinessText` dengan aturan dan lokasi spec 0021, dan langkah 9 spec 0021 memakainya ulang |
| aksi | jenis tombol | `lifecycle`, `initialized`, dan status klik lokal lewat `InstanceAction.resolve` |
| aksi | idempotency key | `MutationPath.newKey()` per klik; key `outcomeUnknown` disimpan bersama `revision` summary dan waktu |
| aksi | status menunggu | `accepted.operation.id`, `OperationCenter.updates(for:)`, `OperationCenter.active` setelah resnapshot, `operation.get` |
| operasi | judul | `OperationTitle` dari `kind`, `resource`, `InstanceStore`, `RuntimeStore` (`installing`) |
| operasi | label tahap, persen | `OperationSummary.stage`; persen `install_runtime` dengan aturan spec 0018 dari `stage` dan `progress` |
| pencarian | field yang dicocokkan | nama, nama tampil, nama tampil provider, `provider.name`, `software`, versi, port |
| urutan | pembanding | tabel kolom *Halaman Instances* |
| partial | item gagal | `ResourceStore.itemFailures` (error dan revision dari refetch yang gagal) |
| navigasi | filter dan pilihan tujuan | `ShellDestination` dari tombol yang diklik |
| New Instance | ada atau tidaknya tombol | `newInstanceAction` di environment (diisi fitur 20) |
| CLI | semua item | semua halaman `instance.list` dengan `limit` 200 |
| CLI | jumlah | `instance::group::of` atas semua item |
| CLI | instance dari nama | `name` sama persis di semua halaman `instance.list` |
| CLI | umur dan waktu | `format_age` dan `format_local` spec 0008 |

### Key invariants

- Kelompok, jumlah, dan filter hanya dihitung dari `lifecycle` dan `readiness.state`; `process` dan PID tidak pernah membuat instance dihitung Running.
- Progress Needs Attention hanya tampil selama sebuah sumber benar benar belum menjawab; sumber yang gagal tanpa data atau unsupported menjadi item (O2 untuk operasi, S4 untuk disk), jadi seksi tidak pernah menunggu selamanya dan tidak pernah menulis "Nothing needs attention." saat operasi unsupported atau gagal tanpa data, atau saat kapasitas belum punya laporan sukses (2026-10-22).
- Tidak ada angka (kartu, subjudul, badge) dan tidak ada "Nothing needs attention." atau "Nothing running." sebelum sumbernya termuat, dan koneksi yang putus tidak pernah mengubah status atau angka (state terakhir tetap, spec 0013).
- Level kapasitas hanya dihitung daemon; GUI dan CLI tidak pernah menghitung ulang ambang.
- GUI tidak pernah membaca filesystem untuk kapasitas, ukuran, atau status.
- `storage.get_capacity` tidak membaca atau menulis registry, tidak membuat operasi, berjalan dalam recovery mode, dan paling banyak menahan satu thread blocking.
- Satu satunya mutasi dari kedua halaman adalah `instance.start` dan `instance.stop` lewat `MutationPath`, selalu dengan gate mutasi dan idempotency key, dan paling banyak satu call per instance berjalan pada satu waktu dari halaman ini.
- Instance retained tidak pernah ada di `InstanceStore`, dari jalur mana pun, termasuk `retry` spec ini (dijaga closure `fetchPage`, `fetchOne`, dan `reduce` langkah 16 spec 0013, K16).
- Error satu instance, satu refetch, atau satu seksi tidak pernah mengosongkan halaman atau mengganti bentuk halaman menjadi failed.
- Fungsi kelompok di Swift dan Rust selalu lulus data test yang sama.

### Security model

- Batas kepercayaan satu akun user (spec 0006): semua method lewat socket privat dengan peer user yang sama; CLI dan GUI tidak menambah jalur akses.
- `storage.get_capacity` hanya memberi angka ukuran volume, tanpa path atau isi file.
- `lastFailure.excerpt` sudah lewat `Redactor` daemon (spec 0010 dan 0011); GUI dan CLI menampilkannya apa adanya (CLI lewat `escape_display`).
- Copy Name dan Copy ID hanya menyalin nama dan UUID, tidak pernah secret.
- Daemon tetap memvalidasi setiap start dan stop walau tombol sudah nonaktif.

### Configuration required

Tidak ada variabel environment, flag, atau credential baru. Test memakai `STACKLY_ROOT`, `STACKLY_DAEMON_BIN`, `SupervisorConfig`, `FakeFilesystem`, dan `FakeDaemonServer` yang sudah ada.

### Critical test scenarios

- Kelompok: semua kasus `groups.json` di Swift dan Rust, termasuk `running` dengan readiness `not_ready` dan proses hidup dihitung `notReady`, `stopping` dihitung `notReady`, dan `retained` menghasilkan nil. Memverifikasi **AC-1**.
- Kartu: store belum termuat tanpa angka, store termuat dengan 0 instance memberi nol, keterangan not ready, angka Failed bertone critical, kartu Runtimes tidak dirender tanpa `RuntimeStore`, klik kartu membuka filter yang jumlah barisnya sama dengan rumus AC-2. Memverifikasi **AC-2**.
- Needs Attention: satu item per aturan dari `list-mixed.json` dan `attention-operations.json`, instance yang cocok dengan beberapa aturan hanya memberi aturan bernomor terkecil, `auto_compensate` tanpa item, disk critical di atas enam instance gagal, batas enam dan Show More, progress sebelum sumber termuat, `id` stabil antar pemanggilan, event `instance.health_changed` dengan `statusSeq` lebih besar memunculkan I6 tanpa snapshot (butuh efek observasi `statusSeq` langkah 16 spec 0013, K16). *Operasi tidak terbaca (2026-10-22):* `OverviewPage.operationsState` untuk setiap kombinasi (unsupported dengan dan tanpa data lama memberi `.unsupported`; data dengan error memberi `.loaded`; error tanpa data memberi `.failed`; tanpa keduanya memberi nil); `OverviewPage.attentionItems` memberi nil hanya untuk `operations` nil, instance yang belum termuat, atau kapasitas yang belum dicoba; `.failed` memberi item instance dan disk ditambah O2 versi error dengan `id` `operations`; `.unsupported` memberi O2 versi unsupported; `.loaded` dengan operasi `needs_recovery` memberi O1 tanpa O2; O2 diurutkan setelah S3 dan S4 dan sebelum I10 di antara item neutral, dan masuk hitungan batas enam; `operationsContent` memberi bentuk seksi yang cocok untuk setiap hasil `operationsState`. Test `ResourceStore` dengan `fetchPage` yang ditahan (continuation) membuktikan bahwa setelah snapshot gagal, refresh yang sedang berjalan tetap memegang `error` lama dan `loadedAt` nil (O2 tetap, bukan progress), dan snapshot yang lalu berhasil mengosongkan `error` dan mengisi `loadedAt` (O2 hilang). Muat ulang pada `.caughtUp` dibuktikan `OperationsUnreadableTests.o2GoesAwayWhenShellSyncLoadsTheFailedStoreAgainOnCaughtUp` dengan `ShellSync`, `OperationCenter`, dan `ResourceStore` nyata: snapshot pertama yang gagal memberi `.failed` (O2), `.caughtUp` memuat ulang dan `operationsState` menjadi `.loaded`, dan `.caughtUp` berikutnya tidak membaca apa pun *(2026-10-22; menggantikan penjagaan review atas panggilan `operations.store.refresh()` di `.task(id: completedRounds)`, yang dihapus di `28cbe678`)*. Memverifikasi **AC-3**.
- Kapasitas: `level` di setiap batas (1 999 999 999 dan 2 000 000 000 tersedia, 4 999 999 999 dan 5 000 000 000 tersedia, 89 dan 90 persen, 79 dan 80 persen), `total` 0 menjadi `no_size`, `available` lebih besar dari `total` dijepit, error `io`, jeda `FakeFilesystem` 6 detik menjadi `timeout` dengan dua call bersamaan yang hanya memakai satu pembacaan, recovery mode tetap menjawab, `mutations()` tidak bertambah; GUI dengan sleep tiruan memanggil ulang setiap 60 detik hanya saat tampil dan tersambung, perubahan `completedRounds` memicu call, reply lama diabaikan, `request.unknown_method` menjadi S4, kegagalan setelah sukses tetap menampilkan item lama. Memverifikasi **AC-4**.
- Seksi Overview: enam instance urut nama dan Show All, empty dengan Browse Catalog, operasi tanpa `needs_recovery`, Show All in Operations hanya di atas lima, judul setiap kind, persen install, Operations failed tetap menampilkan kartu dan Needs Attention. Memverifikasi **AC-5**, **AC-9**.
- Instances: pencarian banyak kata dengan diakritik yang tidak cocok lintas field, setiap pembanding naik dan turun termasuk seri dan nilai kosong, urut Provider sebelum dan sesudah `CatalogStore` termuat, filter dan pencarian bersamaan, no results dengan Clear Filters, kolom tersembunyi di bawah 760 pt, pilihan yang tersaring keluar, Copy Name, permintaan navigasi saat Instances sudah tampil. Memverifikasi **AC-6**.
- Inspector: setiap baris tabel inspector, endpoint dengan port null dan `socketPath` null, banner I1 dengan excerpt per baris, No Selection, keadaan tersimpan. Memverifikasi **AC-7**.
- Aksi: setiap baris tabel aksi termasuk `failed` belum terinisialisasi, klik kedua selama `inFlight` tidak mengirim apa pun, key dipakai ulang setelah `outcomeUnknown` lalu dibuang setelah revision berubah atau 5 menit, `pending` dibersihkan oleh state akhir dan oleh resnapshot tanpa operasi aktif (dengan `operation.get`), umpan balik `rejected`, `notSent`, operasi `failed`, `alreadyInState` tanpa umpan balik, tombol nonaktif saat gate `.blocked`. Memverifikasi **AC-8**.
- State: loading, failed tanpa data, unsupported, stale setelah refresh gagal, partial dari refetch yang gagal dengan detail "Refresh failed" lalu Retry yang berhasil, Retry atas instance yang kini retained (`fetchOne` memberi nil, K16) menghapus item gagal dan barisnya, daemon mati tidak mengubah status. Memverifikasi **AC-9**.
- CLI: golden list (semua, `--status failed`, kosong) dan inspect, baris jumlah sebagai baris terakhir stdout, nama tidak ditemukan exit 10 dengan details `name`, nama yang terdiri dari 32 hex tetap dicari sebagai nama, UUID retained lewat `instance.get`, halaman kedua yang gagal, binary test terhadap `stacklyd` asli dengan instance fake. Memverifikasi **AC-10**, **AC-11**.
- Kontrak: fixture call, reply, dan error `storage.get_capacity`, nilai asing `CapacityLevel` di Rust dan Swift, `InstanceOwnership.unrecognized` tampil "Unknown" di GUI dan `unrecognized` di CLI (fixture default dan nilai asing `ownership` dan `initialized` milik langkah 12 spec 0010), kenaikan minor, `codegen --check`. Memverifikasi **AC-12**.
- Aksesibilitas: label VoiceOver per elemen dari fungsi label murni, menu View hanya aktif di Instances, suite `OverviewSnapshotTests` dan `InstancesSnapshotTests` di atas harness [spec 0068](../0068-fixture-tampilan-cek-visual/index.md) untuk setiap state kedua halaman (inspector terbuka dan tertutup; varian `narrow` 640 pt tanpa teks terpotong, dan `narrowWidth: 384` untuk state Instances dengan inspector terbuka) sebagai bukti tampilan, dengan `#Preview` tetap sebagai alat kerja (2026-10-21), test aturan sumber. *Operasi tidak terbaca (2026-10-22):* `overviewWhoseOperationsFailedToLoad` (`overview-operations-failed`) dan `overviewOfADaemonWithoutOperations` (`overview-operations-unsupported`) memakai input baru supaya O2 ada di enam item pertama tanpa Show More: dua instance `pg17-dev` dan `queue` (`FixtureScenario.instanceCases[0]` dan `[4]`) dan laporan kapasitas `filling` (misalnya `report(.filling, available: 150_000_000_000)`), sehingga Needs Attention menampilkan I1 `queue`, S3, lalu O2, dalam keadaan tertutup (tanpa Show More), dan referensi nya direkam ulang dengan teks `expected` evidence yang menyebut O2. Snapshot baru `overviewWhoseOperationsAreLoading` (`overview-operations-loading`) memakai input yang sama, memuat instance lewat `PageRig.loadInstances()` lalu `capacity.refresh()` tanpa memuat operasi, dan menunjukkan progress di Needs Attention dan seksi Operations sementara kartu dan seksi Instances tampil. Memverifikasi **AC-13**.
- Kinerja: test daemon `instance.list` dengan 100 instance fake (dibuat lewat `instance.create` dengan port Auto tanpa start, di luar waktu ukur) di bawah 500 ms; test Swift dengan `FakeDaemonServer` yang membalas 100 summary sampai `loadedAt` di bawah 1 detik; penghitung recompute tidak naik pada event operasi. Memverifikasi **AC-14**.
- Auth atau permission: tidak ada peran pengguna; akses dibatasi uid socket (spec 0006 dan 0013).

### Cek manual di /check verify

Dicatat sebagai evidence manual spec 0002, di app Debug dengan `stacklyd` asli dan `STACKLY_ROOT` ke folder sementara:

1. Dengan instance fake di kelompok ready, not ready (lewat `readyDelayMs`), stopped, failed (lewat crash loop), dan data in use (lock dipegang proses lain, aturan I3): kartu, subjudul, Needs Attention, dan `stackly instance list` menunjukkan jumlah dan kelompok yang sama (**AC-1**, **AC-2**, **AC-3**, **AC-10**). Kelompok `unknown` hanya dibuktikan lewat fixture.
2. Buat image disk APFS kecil dengan `hdiutil create -size 3g -fs APFS`, mount, dan arahkan `STACKLY_ROOT` ke dalamnya: Overview menampilkan S2 (di bawah 5 GB tersedia); isi sampai kurang dari 2 GB tersedia: S1 dalam 60 detik (**AC-4**).
3. Hanya keyboard (Keyboard navigation menyala): Overview, kartu Failed, Instances, pilih baris, inspector, Stop lewat tombol inspector, urutkan lewat menu View (**AC-13**).
4. VoiceOver: alur yang sama, setiap status terbaca sebagai teks (**AC-13**, PRD AC-027 bagian stop dan periksa).
5. Lebar konten 640 pt: Overview satu kolom tanpa teks terpotong; Instances dengan inspector terbuka menampilkan Name dan Status tanpa menggulir; light, dark, dan Increase Contrast (**AC-13**). Lalu jendela 900 pt dengan inspector terbuka (kolom halaman 384 pt): Instances menampilkan Name, Status, dan aksi tanpa menggulir dan tanpa teks terpotong (ditambah 2026-10-21).
6. Hentikan `stacklyd`: kedua halaman tetap menampilkan state terakhir, tombol aksi nonaktif dengan alasan; jalankan lagi: state disegarkan (**AC-9**).

## Build plan

Pendekatan proyek adalah Journey. Jalur yang dibangun lengkap di fitur ini adalah "setelah instance pertama ada, pengguna membuka Stackly dan langsung tahu apa yang siap, apa yang bermasalah, dan ke mana memperbaikinya, dari GUI dan CLI", termasuk semua state gagal. Kontrak daemon dulu karena CLI dan GUI sama sama membaca fixture nya, lalu CLI dan GUI bisa jalan paralel, lalu bagian yang menunggu fitur 18 dan bukti akhir. Test yang membuktikan ID PRD memakai trait `.evidence` dan guard evidence spec 0002.

**Milestone 1: Kontrak daemon** (needs fitur 10 langkah 12 spec 0010 (DTO instance, termasuk `ownership`, `initialized`, dan `instance.removed`, K4) di `01-foundation.md` dan fitur 6 selesai; tambahan spec 0004 dan 0010 yang dulu diusulkan spec ini sudah di main, diselaraskan 2026-10-20)

1. `Filesystem::volume_space` dan `VolumeSpace` di `stackly-platform`, adapter `statfs` di `stackly-platform-macos`, `FakeFilesystem` dengan volume per awalan path, injeksi error, dan jeda, dan kasus `fs_conformance`; kalau fitur 18 atau fitur 20 sudah membangun helper `statfs`, tabel volume, dan `volume_space` (lihat *Kapasitas disk*), langkah ini hanya menambah yang belum ada. Memenuhi **AC-4**.
2. `stackly_domain::capacity` (level, `used_percent`, konstanta) dan `stackly_core::storage::capacity` dengan unit test batas. Memenuhi **AC-4**.
3. Protokol: modul `storage` (`CapacityReport`, `CapacityLevel`), `storage.get_capacity` di `methods!` dengan `recovery allowed`, `codes::storage` dengan `capacity_unavailable` dan `codes::storage::hint`, fixture wire di `protocols/fixtures/ipc/` dan `index.json`, data test `protocols/fixtures/instance/groups.json`, `list-mixed.json`, dan `attention-operations.json` (summary nya memakai `ownership` dan `initialized` dari langkah 12 spec 0010), kenaikan `ProtocolVersion::CURRENT` ke `CURRENT` di main ditambah 1 saat merge (K4), dan `cargo xtask codegen`. Memenuhi **AC-1**, **AC-12**.
4. `stacklyd`: handler `storage.get_capacity` dengan single flight dan timeout, dan test daemon (level lewat `FakeFilesystem`, `timeout`, recovery mode, tanpa mutasi). Pemetaan `ownership` dan `initialized` milik langkah 13 spec 0010. Memenuhi **AC-4**, **AC-12**.

**Milestone 2: `stackly instance list` dan `inspect`** (after milestone 1; needs fitur 8 build dan fitur 10 langkah 13 spec 0010 (handler instance `stacklyd` dan `install_fake` untuk binary test) di `01-foundation.md`; after fitur 14 hanya kalau fitur 14 sedang mengubah modul `commands/instance`; bisa paralel dengan milestone 3)

5. `group.rs` dengan test data `groups.json`, `resolve.rs`, `list.rs` dan `inspect.rs` dengan output teks dan JSON, `cli.instance_not_found`, golden dari `list-mixed.json` dengan `TZ=UTC`, alias `get` kalau fitur 14 sudah menambahkannya, dan binary test terhadap `stacklyd` asli dengan `install_fake`. Memenuhi **AC-1**, **AC-10**, **AC-11**.

**Milestone 3: Halaman Overview dan Instances** (after milestone 1; needs spec 0013 milestone 4 langkah 16 (`InstanceStore` dengan aturan K16: efek observasi `statusSeq`, reset batas saat `server_restarted`, penyaringan retained di `fetchPage`, `fetchOne`, dan `reduce`, `limit` 200, dan pemetaan `instance.removed`; langkah itu menunggu langkah 12 spec 0010, yang datang setelah milestone 2 fitur 10) dan sisa milestone "Wiring, instances, and operations" fitur 13 (`OperationCenter`, `StatusAdapter`) serta fitur 12 di `ui-macos.md`, dan spec 0017 milestone 3 (`CatalogStore`) di fitur 17; needs fitur 67 milestone 1 dan 3 di `ui-macos.md`, atau membangun komponen itu menurut spec 0067; bisa paralel dengan milestone 2)

6. Model murni dan infrastruktur: `InstanceGroup` (test data), `InstanceCounts`, `InstanceRowModel`, `InstanceSearch`, `InstanceSort`, `InstanceText`, `InstanceAction.resolve`, `InstanceFailureCopy`, `AttentionRules` dan `AttentionItem` (test `list-mixed.json`, `attention-operations.json`, dan fixture kapasitas), `OperationTitle`, `CapacityModel` di `DaemonModel`, `ResourceStore.itemFailures` dan `retry(_:)` (setelah langkah 16 spec 0013 di merge, karena keduanya mengubah `ResourceStore.swift`; retry yang `fetchOne` nya nil menjadi delete), `ShellNavigator` dan `ShellDestination` (tanpa case `.instance` dan `instanceRequest`, yang ditambah langkah 10 spec 0021), dan `newInstanceAction` (kalau langkah 14 milestone 3 spec 0020 belum mendefinisikannya, K17). Penyaringan retained dan `limit` 200 sudah ada dari langkah 16 spec 0013 (K16). Memenuhi **AC-1**, **AC-3**, **AC-4**, **AC-8**, **AC-9**.
7. Halaman Instances: `InstancesPage`, `InstancesTable`, filter, pencarian, pengurutan, kolom sempit, menu konteks, `InstanceInspector` (dengan `ReadinessText` kalau langkah 9 spec 0021 belum ada), `InstanceActionModel` dengan umpan balik lewat `FeedbackCenter`, perintah halaman (penerbitan `CommandMap` spec 0069, atau `InstancesCommands` bertanda `// provisional: F69` menurut aturan di *Aksesibilitas*), empty dan no results, state halaman, penghapusan `ProvisionalInstanceList` dan penanda `// provisional: F22` nya, dan preview light, dark, serta 640 pt dengan inspector terbuka dan tertutup, ditambah 384 pt dengan inspector terbuka (2026-10-21). *Tambahan dari spec 0067, 0068, dan 0069 (2026-10-21):* tabel, inspector, dan aksi dirakit dari `ProviderTile`, `.stacklyInspector`, `InspectorHeader`, `CardSection`, `StacklyAction`, `ActionButton`, `ActionMenuItems`, dan `CopyButton`; pemetaan `startInstance` dan `stopInstance`; suite `InstancesSnapshotTests` kalau milestone 1 spec 0068 sudah di merge, selain itu fitur 68 yang menulisnya (spec 0068 *Key invariants*). Memenuhi **AC-6**, **AC-7**, **AC-8**, **AC-9**, **AC-13**.
8. Halaman Overview: kartu Running, Stopped, Failed, Needs Attention, seksi Instances dan Operations, item kapasitas, state seksi, penggantian `UnbuiltPage(section: .overview)` dan penanda nya, preview, dan bagian Overview dan Instances di `apps/macos/design.md` (tabel Needs Attention dan aturan kartu). *Tambahan dari spec 0068 (2026-10-21):* suite `OverviewSnapshotTests` kalau milestone 1 spec 0068 sudah di merge, selain itu fitur 68 yang menulisnya. Memenuhi **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-9**, **AC-13**.

**Milestone 4: Kartu Runtimes, operasi yang tidak terbaca, dan bukti** (step 9 after spec 0018 langkah 14 (`RuntimeStore`) di fitur 18; step 9a after milestone 3; step 10 after milestone 2, milestone 3, dan langkah 9a)

9. Kartu Runtimes dari `RuntimeStore`, judul dan persen operasi `install_runtime` dari baris `installing`, dan test nya. Memenuhi **AC-2**, **AC-5**.
9a. **Langkah susulan (2026-10-22): Needs Attention saat operasi tidak terbaca.** `AttentionOperations` dan aturan O2 di `AttentionRules` (item `operations`, neutral, kategori operasi, `sortKey` kosong), `OverviewPage.operationsState` yang dipakai `operationsContent` dan `attentionItems`, `OverviewPage.attentionItems` dengan `operations: AttentionOperations?` yang memberi nil hanya selama operasi belum termuat (atau sumber lain menunggu), pengulangan `operations.store.refresh()` di `.task(id: completedRounds)` saat `operationsState` `.failed` *(dihapus 2026-10-22 di `28cbe678`: `ShellSync` kini memuat ulang store yang gagal sekali pada `.caughtUp` (spec 0013, butir `.caughtUp` di *Sinkronisasi snapshot plus cursor*; varian ii di rationale spec 0013) dan snapshot yang berhasil membersihkan `error` dan `unsupported` (`9ee06a25`); test `o2GoesAwayWhenShellSyncLoadsTheFailedStoreAgainOnCaughtUp` menggantikan penjagaan review atas panggilan itu)*, dan doc comment yang ikut berubah (`AttentionItem.id`, `AttentionList`, parameter `operations` di `AttentionRules.items`, dan `OverviewPage.attentionItems`, yang kini tidak lagi menunggu operasi yang gagal). Test: setiap kasus O2 di *Critical test scenarios*, termasuk test `ResourceStore` dengan `fetchPage` yang ditahan, dan pemanggil lama yang ikut diubah (`AttentionRulesTests` yang memberi `[OperationSummary]` dan helper `operationsLoaded:` di `OverviewPageTests`). Baris O2 dan kalimat "Until the instances, the operations (or their failure) …" di bagian Needs Attention `apps/macos/design.md` (operasi yang unsupported atau gagal tanpa data kini menjadi item O2, bukan alasan menunggu). Snapshot: input baru dan rekaman ulang `overview-operations-failed` dan `overview-operations-unsupported`, dan snapshot baru `overview-operations-loading`, menurut *Critical test scenarios* (AC-13). Tidak menyentuh file shell, jadi bisa berjalan di sesi mana pun yang tidak sedang mengubah folder `Overview/`; bisa sebelum atau sesudah langkah 9 (keduanya mengubah `OverviewPage.swift`, jadi tidak di dua sesi bersamaan). Memenuhi **AC-3**, **AC-5**, **AC-13**.
10. Test kinerja (daemon dengan 100 instance fake, Swift dengan `FakeDaemonServer`, penghitung recompute) sebagai evidence NFR-002, lalu evidence manual *Cek manual di /check verify* langkah 1 sampai 6 (keyboard, VoiceOver, 640 pt, image disk kecil, daemon mati). Memenuhi **AC-13**, **AC-14**.

**Sesi paralel yang aman**: setelah milestone 1, milestone 2 (folder `apps/cli/`) dan milestone 3 (folder `apps/macos/`) bisa berjalan di dua sesi karena hanya berbagi fixture yang sudah di commit; langkah 1 dan 2 juga bisa paralel dengan langkah 3. Langkah 6 dan 7 mengubah file shell (`ResourceStore.swift`, `AppShell.swift`, dan `StacklyApp.swift` kalau `InstancesCommands` sementara dibangun), jadi keduanya tidak berjalan bersamaan dengan sesi lain yang mengubah file itu: langkah 16 spec 0013, langkah 2 milestone 1 spec 0016 (K18), langkah 14 milestone 3 spec 0020 (mengubah `AppShell.swift` dan `StacklyApp.swift` dan mungkin mendefinisikan `NewInstanceAction`), atau store fase 02 yang membangun aturan observasi umum. Semua milestone GUI fase 02, termasuk milestone 3 spec ini, menunggu langkah 16 spec 0013, yang menunggu milestone 2 fitur 10 (risiko K18).

## Consequences

**Positive**:

- Angka Overview jujur: instance yang hidup tetapi belum siap terlihat sebagai "not ready", bukan Running, dan setiap instance terhitung di tepat satu kelompok.
- GUI dan CLI memakai satu aturan kelompok yang dijaga data test bersama, jadi `stackly instance list` dan Overview selalu sepakat.
- Peringatan kapasitas punya sumber nyata sekarang, tanpa menunggu metrics fitur 28, dan dihitung sekali di daemon.
- `InstanceActionModel`, `OperationTitle`, `ShellNavigator`, `CapacityModel`, dan `itemFailures` langsung dipakai ulang fitur 20, 21, 26, 32, 34, dan 35.

**Negative / tradeoffs**:

- Aturan kelompok ada di dua bahasa; data test menjaga kesamaannya, tetapi perubahan aturan berarti tiga file (Swift, Rust, data test).
- Kartu Running menampilkan angka siap, berbeda dengan mockup yang menghitung status mentah, dan filter Running memuat juga yang belum siap, jadi angka kartu dan jumlah baris hanya sama kalau keterangan not ready ikut dihitung.
- Ambang kapasitas (80 dan 90 persen, 5 GB, 2 GB) adalah pilihan tanpa data; `f_bavail` di APFS tidak menghitung ruang purgeable, jadi Stackly bisa memperingatkan lebih awal daripada Finder (copy memakai "about" untuk itu).
- Hanya volume root data yang diukur; cache atau log di volume lain tidak diperingatkan sampai ada root yang ditambah.
- Kapasitas di GUI diperbarui per 60 detik, bukan seketika, dan CLI belum punya padanan kapasitas sampai fitur 34.
- Field `ownership` hanya punya satu nilai sampai fitur 44; nilai itu benar hari ini, tetapi namanya bisa tertukar dengan kolom registry `instances.ownership` (asal data) kalau doc comment diabaikan.
- `stackly instance list` mengambil semua halaman, berbeda dengan pola `operation list` spec 0008.
- Sampai fitur 21, 34, dan 35 ada, "membuka instance" hanya memilih baris di Instances, dan item operasi dan disk menunjuk ke halaman placeholder dengan jalan perbaikan di isinya.
- Di tabel native, aksi baris tidak bisa dicapai dengan Tab; pengguna keyboard memakai inspector.

**Neutral**:

- Area protokol `storage`, satu kode error, dan kenaikan minor untuk satu method; kedua field instance ikut minor langkah 12 spec 0010 (K4).
- Folder baru `Overview/` dan isi baru `Instances/` di `StacklyUI`, modul `apps/cli/src/commands/instance/{group,resolve,list,inspect}.rs`, fixture wire baru di `protocols/fixtures/ipc/`, dan data test baru di `protocols/fixtures/instance/`.
- Mockup diperbarui untuk keterangan not ready, kolom Ownership dan Environment yang tidak ada di tabel, dan item kapasitas tanpa "grew this week".

### Usulan update spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead menjadikannya item `architect (update)` atau `/sync`. Semuanya additive. Status per 2026-10-20: usulan spec 0010, 0004, dan 0006 sudah di main; sebagian usulan spec 0013 digantikan keputusan K16 dan K17 (lihat butirnya); usulan spec 0017 dan 0008 masih terbuka.

- **Spec 0010** *(diterapkan di main, update spec 0010 2026-10-20, K4: langkah 12 membangun kedua field, langkah 13 mengisinya, dan baris asumsi spec 0008 kini menyebut `get` sebagai alias opsional `inspect`)*: dua field baru di `InstanceSummary` (dan di `instance.state_changed`): `ownership: InstanceOwnership`, enum terbuka tanpa data (`Manual`, `Unrecognized`) dengan `#[serde(default)]` bernilai `Manual`, diisi `stacklyd` dengan `Manual` selama belum ada keanggotaan environment, dengan doc comment yang membedakannya dari kolom registry `instances.ownership`; dan `initialized: bool` dengan `#[serde(default)]` bernilai true, true jika dan hanya jika `data_initialized_at` aggregate terisi. Fixture untuk keduanya, termasuk summary tanpa kedua field. Baris *Asumsi lintas spec* "fitur 14 membangun `stackly instance ... get`" menjadi "`inspect` dengan alias `get`" (spec 0022).
- **Spec 0004** *(diterapkan di main, update spec 0004 2026-10-20; error nya `PlatformError`, dan `available_space` menjadi provided method di atas `volume_space`)*: `Filesystem::volume_space(path) -> Result<VolumeSpace, FsError>` dengan `VolumeSpace { total, available }`, adapter `statfs` macOS, `FakeFilesystem` (volume per awalan path, error, jeda), dan `fs_conformance`; module baru `capacity` di domain dan `storage` di core. Bisa digabung dengan tambahan `Filesystem` spec 0018 (`available_space` di atas `volume_space`).
- **Spec 0013** *(masih terbuka untuk update spec 0013 setelah branch `f13-wiring` di merge; keputusan K16 dan K17 menggantikan sebagian butir ini: penyaringan retained di `fetchPage`, `fetchOne`, dan `reduce`, `limit` 200, efek observasi `statusSeq`, dan pemetaan `instance.removed` dibangun langkah 16 spec 0013, bukan spec ini; `NewInstanceAction` didefinisikan langkah 6 spec ini atau langkah 14 milestone 3 spec 0020, mana yang lebih dulu; sisanya, yaitu `itemFailures`, `retry`, `ShellNavigator`, dan `CapacityModel`, tetap usulan spec ini)*: `ResourceStore.itemFailures: [Item.ID: ItemFailure]` dengan `ItemFailure { error: ErrorBody, revision: UInt64 }` (diisi saat refetch gagal, dihapus saat item itu berhasil diterapkan dan saat resnapshot selesai) dan `retry(_ id:)` yang mengulang refetch pada revision itu; penyaringan retained, `limit` 200, dan pemetaan `instance.removed` di `InstanceStore` kini milik langkah 16 spec 0013 (K16, catatan di atas); `ShellNavigator`, `ShellDestination`, dan `EnvironmentValues.newInstanceAction` di `Shell/`, dengan `AppShell` tetap memegang `@SceneStorage("shell.section")`; `CapacityModel` di `DaemonModel`. Efek observasi `statusSeq` milik spec 0013 milestone 4 langkah 16 (K16). Baris Follow-up "Fitur 22 ... ganti `UnbuiltPage` dan daftar instance sementara" dijawab AC-2 dan AC-6, dan "kartu operasi Overview milik fitur 22" dijawab AC-5.
- **Spec 0017** *(digantikan spec 0067, 2026-10-21: `ProviderTile` milik spec 0067 dengan signature final `ProviderTile(mark:tile:name:size:)` dan `ProviderTile(_:size:)` dengan `TileSpec`, ukuran `smallSize`, `rowSize`, `mediumSize`, dan `largeSize`)*: signature `ProviderTile(mark: String?, tile: TileColor?, size: CGFloat)` dengan aturan mark default spec 0017 saat `mark` nil dan `gray` saat `tile` nil, dipakai Overview (26 pt), tabel Instances (18 pt), dan inspector (40 pt).
- **Spec 0006** *(diterapkan di main, update spec 0006 2026-10-20: baris area `storage` dan asumsi spec 0022 tercatat, dan aturan hint `codes::<area>::hint` sudah berlaku)*: area `storage` masuk lewat aturan "fitur yang pertama kali memakainya" dan memakai bentuk hint `codes::<area>::hint`.
- **Spec 0008** (catatan saja): `stackly instance list` mengambil semua halaman (seperti `stackly runtime list` spec 0018) karena jumlah instance terbatas, dan `cli.instance_not_found` (details `name`) ditambahkan ke katalog `cli`.

## Follow-up

- [ ] `/architect` update spec 0013 dan 0017 dengan item di *Usulan update spec lain* yang masih terbuka (atau biarkan milestone 3 langkah 6 menambahkannya). Update spec 0010, 0004, dan 0006 sudah di main (2026-10-20). Usulan spec 0017 (`ProviderTile`) digantikan spec 0067 (2026-10-21), jadi yang tersisa hanya spec 0013.
- [ ] Fitur 20: isi `newInstanceAction` di `AppShell` (tombol New Instance di toolbar Overview dan Instances serta aksi kedua "New Instance…" di empty Instances) dan pilih instance baru lewat `ShellNavigator.show(.instances(filter: .all, select: id))`; langkah 14 milestone 3 spec 0020 mendefinisikan environment key nya kalau langkah 6 spec ini belum ada (K17). DTO `instance.removed` datang dari langkah 12 spec 0010, dan `InstanceStore` memetakannya ke delete di langkah 16 spec 0013 (K16).
- [ ] Fitur 21: ganti `openInstance` (`// provisional: F21`) dengan halaman detail, tambah Open di inspector, teks "Double-click a row to open it.", serta baris Data dan Connection di inspector.
- [ ] Fitur 44: kolom Environment dan baris Environment di inspector, field environment pemilik dan nilai baru `InstanceOwnership`, dan seksi Environments di Overview.
- [ ] Fitur 26 dan 47: Restart dan force stop di menu konteks dan inspector, dan stop dengan override lease.
- [ ] Fitur 29: aturan Needs Attention untuk drift config. Fitur 33: banner crash loop memakai `InstanceFailureCopy`.
- [ ] Fitur 34: Diagnostics menampilkan kapasitas dari `CapacityModel` dan menjadi tujuan item S yang berguna; padanan kapasitas di CLI.
- [ ] Fitur 35: halaman Operations memakai `OperationTitle`, menerima pilihan operasi dari Overview, dan mengganti isi O1 (`// provisional: F35`) dengan rujukan ke halaman itu. Item O2 (2026-10-22) juga menuju halaman itu; saat fitur 35 ada, halaman Operations yang gagal dimuat menampilkan Retry nya sendiri, dan isi O2 tidak perlu berubah.
- [x] Spec 0013 (pemiliknya lead ui-macos, 2026-10-22): pertimbangkan `ShellSync` yang mengulang store yang gagal saat `.caughtUp`, dan `ResourceStore` yang mengosongkan `unsupported` saat snapshot berhasil. Kalau keduanya diterapkan, pengulangan `operations.store.refresh()` di `.task(id: completedRounds)` Overview (langkah 9a) bisa dihapus dan O2 versi unsupported hilang setelah daemon diperbarui. Diterapkan di spec 0013 (2026-10-22, butir `.caughtUp` dan aturan snapshot yang berhasil di *Sinkronisasi snapshot plus cursor*, merge `9ee06a25`); pengulangan langkah 9a dihapus di `28cbe678`, dan *Isi, Retry, dan kapan O2 hilang*, *State halaman*, langkah 9a, *Critical test scenarios*, dan [rationale.md](rationale.md) sudah diselaraskan (2026-10-22).
- [ ] `stackly instance start` dan `stop` dengan nama (usulan Follow-up spec 0021, `8660c8ad`, dicatat 2026-10-22; spec ini pemilik resolver `TargetArg`; rincian di bawah disetujui lead 2026-10-22). Kedua perintah masih memakai `InstanceArg` (hanya ID penuh, pesan "expected a whole instance id"), sedangkan `inspect`, `get`, dan `connection` sudah menerima nama atau ID lewat `TargetArg`, padahal journey spec 0021 menulis `stackly instance start <instance> --wait` lalu `stackly instance connection <instance>`. Rekomendasi: satu langkah susulan kecil di milestone 2 spec ini, langkah 5a, hanya di folder `apps/cli/`. Runner up (dari spec 0021): langkah susulan fitur 21, karena journey CLI nya ada di sana.
  - *Perilaku.* UUID 36 karakter bertanda hubung (huruf besar atau kecil) dipakai langsung tanpa baca tambahan, sama dengan sekarang. Argumen lain dicari persis di semua halaman `instance.list` sebelum operasi dikirim; daftar itu tidak memuat instance retained, jadi nama tidak pernah ambigu. Nama yang tidak ditemukan keluar 10 dengan `cli.instance_not_found` sebelum operasi apa pun dimulai, jadi idempotency key belum terpakai (key dibuat di dalam `Context::submit`); daemon yang tidak jalan keluar 6. Selebihnya (`AsyncArgs`, `alreadyInState`, field `instance`, exit code) tetap aturan AC-6 spec 0014. Key yang dipakai ulang setelah nama itu menunjuk instance lain ditolak `operation.idempotency_key_reused` (fingerprint mencakup `instanceId`, spec 0007), bukan menjalankan instance yang salah.
  - *Argumen yang kini keluar 2.* Test `a_bad_instance_id_or_key_exits_2_without_connecting` (`apps/cli/tests/instance/action.rs`) mengharapkan `start orders-db`, `start 0192f3a5`, dan `stop` dengan 34 hex keluar 2 `cli.usage` tanpa koneksi. Setelah langkah 5a ketiganya menjadi pencarian nama: koneksi, lalu exit 10 `cli.instance_not_found` (exit 6 kalau daemon mati). UUID yang salah ketik juga menjadi pencarian nama, sama dengan `inspect` hari ini. Ketiga kasus itu pindah ke test baru yang mengharapkan exit 10 dari daemon skrip yang membalas `instance.list`; kasus `--idempotency-key` dan `--timeout` yang tidak sah dan argumen kosong tetap exit 2 tanpa koneksi.
  - *Instance retained.* Nama instance retained memberi `cli.instance_not_found`, karena `instance.list` tidak memuatnya; UUID nya memberi `supervisor.instance_retained` dari daemon. Diterima, sama dengan `inspect` dan `connection` dengan nama hari ini (AC-11).
  - *File.* `apps/cli/src/commands/instance/action.rs`: `StartArgs` dan `StopArgs` memakai `TargetArg`, lalu `InstanceTarget::resolve` sebelum `cx.submit`. `InstanceArg` di `mod.rs` dan `InstanceIdArg` di `apps/cli/src/args.rs` (beserta unit test nya) dihapus, karena tidak ada pemakai lain. Golden `instance_start` dan `instance_stop` untuk nama, nama yang tidak dikenal, dan UUID huruf besar, dan binary test terhadap `stacklyd` asli dengan `install_fake`.
  - *Help.* `value_name` menjadi `INSTANCE` dengan doc dari `TargetArg` ("The instance name, or its whole ID"). Contoh `after_help` start dan stop di `mod.rs` memakai nama (misalnya `stackly instance start orders-db --wait`), dan satu contoh UUID tetap.
  - *Kontrak.* Tidak perlu kontrak baru: tidak ada method, DTO, kode error, atau kenaikan `ProtocolVersion`; resolver memakai `instance.list` dan `cli.instance_not_found` yang sudah ada.
  - *AC.* Satu AC baru di spec ini, seperti AC-11 yang memperluas `get` tanpa mengubah spec 0014; AC-6 spec 0014 tidak diubah, karena spec 0014 sudah menyebut argumen itu akan menerima nama setelah resolver spec ini ada (*Asumsi lintas spec*, butir Spec 0022, dan *Consequences*, butir "Argumen `<instance-id>` hanya UUID sampai resolver nama spec 0022 ada"). Baris spec 0014 yang menjadi usang (butir *Consequences* itu, dan "setiap kasus `cli.usage` tanpa koneksi" di *Critical test scenarios* untuk argumen instance) dicatat pemilik spec 0014 atau `/sync` saat langkah 5a selesai. Runner up: `<instance-id>` di AC-6 spec 0014 menjadi `<instance>`. Draf teks, **menunggu persetujuan user saat langkah 5a dijadwalkan** (belum masuk *Requirements*):

    ```text
    - **AC-15**: `stackly instance start <instance>` dan `stackly instance stop <instance>` menerima UUID dalam bentuk 36 karakter bertanda hubung, huruf besar atau kecil (dikirim langsung tanpa baca tambahan), atau nama persis untuk argumen lain (dicari lewat semua halaman `instance.list` sebelum operasi dikirim, dengan resolver yang sama dengan AC-11); selebihnya mengikuti AC-6 spec 0014. Nama yang tidak ditemukan keluar 10 dengan `cli.instance_not_found` (details `name`) tanpa memulai operasi dan tanpa memakai idempotency key; UUID instance retained keluar dengan `supervisor.instance_retained` dari daemon; daemon yang tidak jalan keluar 6; `--idempotency-key` atau `--timeout` yang tidak sah dan argumen kosong tetap keluar 2 tanpa koneksi. Golden CLI dan binary test terhadap `stacklyd` asli membuktikan start dan stop dengan nama, nama yang tidak dikenal, dan UUID huruf besar.
    ```
- [ ] Fitur 28: pertimbangkan pertumbuhan data ("grew this week"), kapasitas dari metrics, dan root lain di volume terpisah.
- [x] Fitur 67: ganti `ActionFeedbackBanner` (`// provisional: F67`). Diterapkan (2026-10-21): banner sementara tidak dibuat; umpan balik lewat `FeedbackCenter` spec 0067.
- [ ] Fitur 68: snapshot untuk state kedua halaman. Kini suite `OverviewSnapshotTests` dan `InstancesSnapshotTests` yang ditulis fitur ini di langkah 7 dan 8 (atau fitur 68, menurut urutan merge) (2026-10-21); tetap terbuka sampai suite dibangun.
- [ ] Fitur 69: daftarkan perintah sort, inspector, dan ⌘F di peta perintah. Diputuskan spec 0069 AC-10 dan AC-12 dan aturan di *Aksesibilitas* (2026-10-21); tetap terbuka sampai baris itu built.
- [x] `ui-macos.md` (lewat `/scope`): catat selisih mockup Overview dan Instances: tabel Instances tanpa kolom Ownership (ownership di inspector dan CLI, karena isinya selalu "Manual" sampai fitur 44), kartu Running menghitung yang siap dengan keterangan not ready, kolom dan seksi Environment menunggu fitur 44, New Instance menunggu fitur 20, item disk tanpa pertumbuhan mingguan, dan item drift menunggu fitur 29. Diterapkan di `ui-macos.md` (2026-10-20).
- [ ] `/sync` setelah build: folder `Overview/` dan isi `Instances/` di `apps/macos/AGENTS.md`, area `storage` di tabel area spec 0006, dan perintah `stackly instance list` dan `inspect` di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan hasil cross check ada di [rationale.md](rationale.md).
