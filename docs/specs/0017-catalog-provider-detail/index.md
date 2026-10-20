# 0017. Catalog dan provider detail

**Date**: 2026-10-19
**Status**: In Progress

## Summary

Spec ini menetapkan catalog bawaan Stackly: daftar provider resmi yang ikut terpasang di dalam app (`Stackly.app/Contents/Resources/catalog/`), dibangun dari `providers/catalog.toml` oleh `cargo xtask catalog build`, lalu dibaca daemon sekali saat start tanpa menjalankan kode provider apa pun. Daemon menghitung sendiri status setiap kombinasi versi dan OS (planned, unverified, supported, unsupported, blocked) dan keputusan install untuk Mac ini, menolak install yang tidak didukung lewat satu fungsi gate yang dipanggil `runtime.install` fitur 18, dan memberi GUI serta CLI satu DTO yang sama lewat `catalog.list_providers` dan `catalog.get_provider`. Waktu terbit catalog selalu terlihat dan tidak pernah diklaim lebih baru, karena catalog hanya berubah lewat update aplikasi (fitur 61) atau import lokal (fitur 37), dan setiap perubahan isi wajib dicatat sebagai revision baru. Spec ini juga menulis kontrak pin (menyalin paket provider ke folder data saat instance dibuat) dan baris `provider_sources` yang dibangun fitur 20.

## Requirements

**User stories**:

- Sebagai pengguna yang baru memasang Stackly, saya ingin melihat provider apa saja yang ada beserta versi, platform, lisensi, ukuran unduhan, dan capability nya, supaya saya tahu apa yang bisa saya pasang sebelum mengunduh apa pun.
- Sebagai pengguna di Mac yang tidak didukung sebuah kombinasi, saya ingin label unsupported yang jujur beserta alasannya, dan daemon yang menolak install, supaya saya tidak pernah mendapat runtime yang tidak bisa jalan.
- Sebagai pengguna yang offline atau yang jarang update app, saya ingin melihat kapan catalog terakhir diperbarui, supaya saya tidak salah mengira catalog ini yang paling baru.
- Sebagai penulis script, saya ingin `stackly provider list` dan `inspect` dengan JSON yang stabil dan isi yang sama dengan GUI.
- Sebagai developer fitur 18 dan 20, saya ingin satu read model catalog, satu fungsi gate install, satu enum status, dan satu kontrak pin, supaya resolusi versi, install, dan create instance tidak membaca paket dengan caranya sendiri.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-002):

- **AC-1**: `cargo xtask catalog build --out <dir>` membaca `providers/catalog.toml` dan `providers/catalog-revisions.json` (*Sumber catalog di repo*), memvalidasi setiap paket yang terdaftar dengan `read_package` dan `ValidatedPackage::from_files(.., NameCheck::FolderName)` spec 0009, lalu menulis `<dir>/catalog.json` dan salinan `<dir>/packages/<namespace>/<nama>/` yang hanya berisi file yang dibaca loader (`provider.yaml`, `versions.json`, `schemas/settings.schema.json`, template yang dirujuk). Build gagal (exit 1, satu baris per masalah) untuk: namespace selain `official`, folder paket yang tidak ada, paket yang gagal validasi, `metadata.id` yang berbeda dengan entri, ID ganda di `provider` dan `planned`, artefak dengan `minOs` di atas `APP_MINIMUM_MACOS` (27.0), batas di *Batas catalog* yang terlewati, `published_at` di masa depan, dan revision yang tidak tercatat di ledger atau tercatat dengan digest isi atau `publishedAt` lain. Output yang sama untuk input yang sama sama byte demi byte; folder ditulis ke folder sementara di sebelah `<dir>` lalu di rename, dan `<dir>` yang sudah ada hanya diganti kalau isinya catalog hasil build sebelumnya (ada `catalog.json`), selain itu build menolak. `cargo xtask catalog check` menjalankan validasi yang sama tanpa menulis, dan `--update` mencatat revision baru di ledger sesuai *Ledger revision*. `cargo xtask build-app` (spec 0015) menjalankan `xtask::catalog::build` di dalam proses di slot langkah catalog nya dengan `--out <staging>/Contents/Resources/catalog`, sebelum signing; langkah catalog yang gagal menggagalkan seluruh build tanpa meninggalkan bundle di path keluaran (REQ-033, baris Follow-up spec 0009 tentang `testing/`, klausa catalog AC-1 spec 0015).
- **AC-2**: Daemon menentukan folder catalog sekali saat start: `STACKLY_CATALOG_DIR` kalau di set (path absolut ke folder yang ada; kosong, relatif, atau tidak ada membuat daemon keluar 1 dengan pesan yang menyebut variabel itu), selain itu `AppBundle::resources_dir().join("catalog")` dari spec 0015, selain itu tidak ada catalog. Tanpa catalog daemon tetap start, subsystem `catalog` bernilai `unavailable` dengan detail yang menyebut kedua sumber, dan setiap method `catalog.*` dijawab `catalog.unavailable` reason `not_configured`; daemon tidak pernah crash karena catalog. Memuat catalog hanya membaca lewat port `Filesystem` (`read_file_limited`, `read_dir`, `symlink_metadata`): dengan `FakePlatform`, `mutations()` dan jumlah spawn `FakeProcessSupervision` tidak bertambah, dan tidak ada akses jaringan atau secret (REQ-033: browsing catalog tidak menjalankan kode provider).
- **AC-3**: `catalog.json` didecode ketat (`schema` dicek lebih dulu, field asing ditolak). Index yang tidak ada, bukan file biasa, di atas 256 KiB, gagal decode, ber schema lain, atau berisi ID ganda, ID yang ada di `providers` dan `planned` sekaligus, entri di atas batas, atau entri yang tidak urut ID membuat seluruh catalog `unavailable` dengan reason `missing`, `index_invalid`, atau `unsupported_schema`. Satu entri provider yang gagal (paket tidak valid, `manifestSha256` atau `versionsSha256` berbeda dengan file, `metadata.id` berbeda dengan entri, namespace selain `official`, batas per provider terlewati, atau total byte paket catalog melewati 16 MiB pada entri itu) menjadi entri `invalid` dengan daftar issue (paling banyak 100), sedangkan entri lain tetap dimuat. Status subsystem `catalog` mengikuti *Kesehatan subsystem*. Namespace `testing` hanya diterima kalau `CatalogOptions.allow_testing_namespace` bernilai true, yang hanya bisa dipasang kode test (override `DaemonConfig.catalog` di dalam proses, `Catalog::for_tests`, atau `CatalogBuilder`), tidak pernah lewat variabel environment atau flag.
- **AC-4**: `catalog.list_providers` (tanpa params) membalas `ListProvidersReply { catalog: CatalogInfo, items: Vec<ProviderSummary> }` dengan semua entri dalam *Urutan tampil*, dan setiap field bersumber seperti tabel *Value sourcing*, termasuk `CatalogInfo.host` (target Mac ini). Pada catalog dengan semua batas *Batas catalog* di nilai maksimum, balasan list dan balasan detail terbesar tetap di bawah `CONTROL_FRAME_MAX`, dibuktikan test (hitungan di *Anggaran frame*). Kedua method `catalog.*` bertanda `recovery allowed`, tidak pernah membaca atau menulis registry, dan dijawab dari memori.
- **AC-5**: `catalog.get_provider { provider }` membalas `ProviderDetail`: identitas (ID, software, nama, deskripsi, kategori, provider revision, `manifestSha256`, publisher, trust, sumber), kedelapan capability dalam *Urutan capability* masing masing dengan `supported` (kosong untuk entri planned dan invalid), satu baris per exact version dengan tepat tiga sel `TargetSupport` (`macos`, `linux`, `windows`) dan keputusan install `install` untuk Mac ini, daftar artefak (target, status, ukuran unduhan, ukuran terpasang bila ada, `minOs`, lisensi dan URL lisensinya, sumber, alasan blocked, dependency bila ada), catatan platform, catatan planned, dan issue untuk entri `invalid`. Nilai `supported` setiap capability sama dengan `CapabilitySet` paket yang dipakai `CapabilitySet::require` (BR-016), dibuktikan test per capability untuk paket fake dan NATS. Provider yang tidak ada menghasilkan `catalog.provider_not_found` dengan pesan dan hint statis di *Kode error di wire*; kalau params hanya nama tanpa namespace dan tepat satu entri punya nama itu, `details.suggestion` dan pesannya menyebut ID lengkapnya. Params yang bentuknya salah menghasilkan `request.invalid_params`.
- **AC-6**: Status dihitung daemon dengan *Aturan status* dan tidak pernah oleh client. Setiap baris aturan state entri dan setiap langkah aturan sel punya unit test, termasuk: entri planned tanpa versi; entri yang tidak punya artefak terpasang untuk host kecuali yang `blocked` berstate `blocked`; sel OS host dengan artefak `blocked` menjadi `blocked` dengan catatan dari `versions.json`; artefak `available` menjadi `unverified` reason `untested` sampai fungsi compatibility fitur 18 ada; artefak `fixture` menjadi `unverified` reason `fixture_only` di OS mana pun; OS host tanpa artefak untuk arch host tetapi ada untuk arch lain menjadi `unsupported` reason `no_build_for_arch` beserta arch yang ada; OS tanpa artefak memakai catatan platform (`planned` atau `unsupported`) atau `unsupported` reason `no_build` dengan `arches` kosong; sel OS lain dengan artefak campuran memakai status terbaik. `supported` tidak pernah keluar tanpa compatibility record (BR-025).
- **AC-7**: `catalog::installability(entry, version, host)` mengembalikan `Installable { artifact }` hanya untuk artefak `available` yang target nya persis target host, pada entri provider; selain itu `NotInstallable { reason, available_targets }` dengan reason `provider_planned`, `package_invalid`, `no_build_for_host`, `no_build_for_arch`, `artifact_blocked`, atau `fixture_only` (kodenya sama satu satu dengan `SupportReason`), dan versi yang tidak ada di entri menghasilkan `VersionNotFound`. `CatalogVersion.install` di detail adalah hasil fungsi yang sama untuk host. Pemetaannya ke wire adalah `catalog.not_installable` (kategori `unsupported`, exit 5) dengan details `provider`, `version`, `target`, `availableTargets`, dan `reason`, atau `catalog.version_not_found`. `runtime.install` dan `runtime.resolve` fitur 18 memanggil gate ini tepat setelah resolusi selector dan sebelum operasi dibuat, sehingga install yang tidak didukung ditolak daemon tanpa operasi, bukan hanya disembunyikan UI (PRD AC-002, AC-005, BR-016).
- **AC-8**: `CatalogInfo.revision`, `publishedAt`, dan `contentSha256` sama persis dengan `catalog.json`; `loadedAt` adalah waktu `Clock` saat daemon memuat catalog; `outdated` bernilai true jika dan hanya jika `Clock::now()` dikurangi `publishedAt` lebih dari `CATALOG_OUTDATED_AFTER` (180 hari), dihitung saat request. GUI selalu menulis tanggal terbit di subjudul toolbar setelah snapshot pertama, dan saat `outdated` menambah keterangan stale `.outdated(source:)` spec 0012 dengan `since` sama dengan `publishedAt` dan source "the bundled catalog" atau "the development catalog". CLI menulis baris catalog di *CLI*. Tidak ada teks di GUI, CLI, atau DTO yang menyebut catalog terbaru, up to date, ada catalog yang lebih baru, atau tanggal yang lebih baru dari `publishedAt`, dan tidak ada yang menyentuh jaringan (NFR-014).
- **AC-9**: `stackly provider list [--category <kategori>]` dan `stackly provider inspect <provider>` memanggil kedua method itu dan tidak pernah membaca file catalog. `--json` memberi `result` `{ "catalog": CatalogInfo, "items": [ProviderSummary] }` (dengan `--category` juga menyaring `items`) dan `ProviderDetail`; teks manusia mengikuti *CLI*. Status, reason, catatan, dan keputusan install di CLI dan GUI berasal dari DTO yang sama: golden CLI dan test view model Swift membaca fixture `protocols/fixtures/catalog/` yang sama dan menghasilkan status yang sama per sel, dan ukuran ditulis dengan aturan desimal yang sama di keduanya. Provider yang tidak ada keluar 10, catalog yang tidak tersedia keluar 6, dan daemon yang tidak jalan keluar 6 (REQ-028, spec 0008).
- **AC-10**: Layar Catalog menggantikan `UnbuiltPage(section: .catalog)` dan penanda `// provisional: F17` spec 0013, mengikuti mockup UI dan *Layar Catalog (GUI)*: judul "Catalog" dengan subjudul dari *Copy*, field pencarian "Search providers" di toolbar (cocok dengan nama tampil atau ID, tanpa peduli huruf besar dan diakritik), picker kategori bersegmen (All, Databases, Messaging, Analytics, Cache, Proxy), grid kartu provider (tile, nama, `ToneBadge` status, baris meta, dua baris deskripsi), dan panel detail (header, ID dan kategori, deskripsi, matrix versi per OS dengan catatan alasan di bawahnya, daftar capability dengan yang tidak didukung ditulis "{label} · not supported", baris License, Source, Download, Disk, Requires, Dependencies, Publisher, Revision, dan Trust). Provider planned tampil redup lewat teks sekunder dan tile netral tanpa matrix, capability, atau tombol apa pun, dengan catatan "This provider isn’t available yet. It will appear here once packaging and tests pass." Hasil filter kosong menampilkan "No providers match “{query}”.", dan pilihan yang tersaring pindah ke entri pertama yang terlihat. Halaman memakai `ViewState.resolve` untuk loading, failed, unsupported, empty, stale, dan partial. Build fitur ini tidak menampilkan tombol Get, New Instance, atau Import Provider (milik fitur 18, 20, dan 37).
- **AC-11**: Layar Catalog bisa dipakai penuh dengan keyboard (Tab ke pencarian, picker, setiap kartu, dan isi panel detail; Return atau Space memilih kartu) dan VoiceOver (label kartu "{nama}, {kategori}, {status}"; setiap sel matrix "{OS}, {status}: {alasan}"; setiap capability "{label}, supported" atau "{label}, not supported"; tile disembunyikan dari VoiceOver). Status tidak pernah hanya lewat warna atau hover: label status selalu tertulis, alasan setiap sel juga tertulis di catatan di bawah matrix, dan Differentiate Without Color menampilkan simbol tone. Semua teks, termasuk kartu planned, lolos kontras WCAG AA di light, dark, dan Increase Contrast; mark di dalam tile adalah dekorasi (nama yang sama tertulis di sampingnya) dan dikecualikan. Layar tidak memotong teks di lebar konten minimum 640 pt (NFR-012).
- **AC-12**: `CatalogAppearance` di `StacklyUI` memetakan setiap `SupportStatus` dan `CatalogEntryState`, termasuk `unrecognized`, ke label, tone, simbol, dan detail persis seperti tabel *Tone dan label*, dan setiap enum terbuka lain ke teks di *Nilai asing*, dengan `switch` exhaustive tanpa `default`; tabel itu ditambahkan ke `apps/macos/design.md` sesuai aturan status set baru spec 0012.
- **AC-13**: Setiap kode area `catalog` yang dibangun fitur ini (`catalog.unavailable`, `catalog.provider_not_found`, `catalog.version_not_found`, `catalog.not_installable`) ada di katalog `stackly-protocol::codes::catalog` dengan kategori, `retryable`, kunci `details`, dan `summary` persis seperti *Kode error di wire*, dengan test pemetaan per varian di `stacklyd` (spec 0006 AC-8). Method, DTO, enum terbuka (diakhiri `Unrecognized`), dan kode baru menaikkan minor `ProtocolVersion::CURRENT` di commit yang sama, punya fixture di `protocols/fixtures/` untuk setiap varian `Call` dan `Reply` baru serta nilai asing setiap enum terbuka, dan terbaca di Rust dan Swift.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md), ditambah semua perbaikan cross check yang disetujui lead atas nama user (2026-10-19): catalog sebagai folder data di `Contents/Resources/catalog/` dengan index `catalog.json` hasil `cargo xtask catalog build` dari `providers/catalog.toml` yang dikurasi; revision dan tanggal terbit dicatat di ledger `providers/catalog-revisions.json` yang menolak isi berubah di revision lama; flag outdated setelah 180 hari; entri yang gagal diisolasi; status dan keputusan install dihitung daemon untuk tiga kolom OS tetap; dua method baca `catalog.*`; gate install sebagai fungsi core yang dipanggil `runtime.install`; kontrak pin dan baris `provider_sources` ditulis di spec ini tetapi dibangun fitur 20; tile dari petunjuk di `catalog.toml`; tone menurut arti di spec 0012; namespace catalog hanya `official`; dan provider masuk catalog hanya di commit fitur yang membuktikan lifecycle nya.

Catalog adalah data yang disegel bersama app dan dibaca sekali oleh daemon menjadi read model di memori. Semua status, alasan, dan keputusan install dihitung di daemon; GUI dan CLI hanya menampilkan DTO yang sama.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya decode ketat dengan `deny_unknown_fields`, tidak ada `unwrap()` di luar test, dan kontrol native SwiftUI menggantikan tiruan HTML di mockup.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| REQ-002 (catalog berbasis capability) | AC-4, AC-5, AC-6, AC-9, AC-10 |
| REQ-033 (bagian catalog bawaan) | AC-1, AC-2, AC-3 |
| PAGE-08 (Provider Catalog) | AC-10, AC-11, AC-12 (install dan import di fitur 18 dan 37) |
| BR-016 (unsupported konsisten di GUI, CLI, daemon) | AC-5, AC-7, AC-9 |
| BR-025 (`SUPPORTED` terikat matrix teruji) | AC-6 |
| NFR-014 (catalog stale dengan waktu pembaruan) | AC-8 |
| AC-002 (unsupported berlabel dan ditolak daemon) | AC-6, AC-7, milestone 4 |
| OQ-009 (trust root, signing, revocation) | *Security model*; default PRD dipakai, OQ tetap terbuka untuk public catalog |
| OQ-012 (signing account, registry hosting) | *Security model*: catalog bawaan tidak butuh hosting, segel bundle bergantung pada identity spec 0015 |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| Schema manifest, loader paket, `ValidatedPackage`, `versions.json`, digest paket | spec 0009 (termasuk `versions()` dan empat kode issue loader yang ditambah untuk spec ini, lihat *Update spec lain*) |
| Helper di `Contents/MacOS/`, `AppBundle`, `cargo xtask build-app`, dan slot langkah catalog di `xtask/src/build_app/assemble.rs` (sebelum signing) | spec 0015 |
| Isi slot itu: langkah `xtask::catalog::build` di dalam proses dengan `--out <staging>/Contents/Resources/catalog`, dan test langkah catalog yang sengaja gagal | spec ini (milestone 4 langkah 11) |
| `providers/catalog.toml`, ledger revision, `catalog.json`, `cargo xtask catalog`, read model `Catalog`, `SupportStatus`, aturan status di luar compatibility record, gate `installability`, method dan kode area `catalog`, layar Catalog, `stackly provider list` dan `inspect` | spec ini |
| Kontrak pin dan baris `provider_sources` (ditulis di sini, dibangun dan diverifikasi fitur 20) | *Kontrak untuk fitur 20* |
| Resolusi selector (`@17`, `@stable`), download, verify, extract, commit, compatibility record PRD 32.2 dan fungsi `runtime::compatibility::support_status`, `HostFacts`, field `versions.json` tambahan (`compatibility`, `unpackedSize`, `entryCount`, `dependencies`, `stable`), tombol Get dan progress di kolom aksi matrix, status "{n} installed" di kartu, halaman Runtimes | fitur 18, spec 0018 |
| Isi paket PostgreSQL dan pemindahannya dari planned ke provider | fitur 19 |
| Pemanggil pin (create instance), tombol New Instance di panel detail, form dari settings schema dan capability | fitur 20 |
| Import provider lokal, namespace lain, kind sumber `local`, trust approval, tombol Import Provider, event `catalog.changed` | fitur 37 |
| Update app yang membawa catalog baru | fitur 61 |

### Sumber catalog di repo

`providers/catalog.toml` ditulis tangan (TOML snake_case, schema `stackly.catalog-source/v1`). Tipenya hanya di `xtask/src/catalog/source.rs`, karena daemon tidak pernah membaca file ini. Contoh berikut menunjukkan bentuk setelah fitur 23 memindahkan NATS ke `provider`:

```toml
schema = "stackly.catalog-source/v1"
revision = 4
published_at = 2026-11-02T09:41:00Z

[[provider]]
id = "official/nats"
mark = "Na"
tile = "green"

[[provider.platform_note]]
os = "windows"
status = "planned"
note = "Windows releases exist upstream; per version verification is pending."

[[planned]]
id = "official/mysql"
display_name = "MySQL"
category = "database"
description = "Relational server with isolated datadir, socket and tmpdir per instance. Never bound to an existing system service."
mark = "My"
tile = "teal"
```

| Field | Wajib | Aturan |
| --- | --- | --- |
| `schema` | ya | persis `stackly.catalog-source/v1` |
| `revision` | ya | integer 1 sampai `MAX_SAFE_INTEGER`; dinaikkan maintainer setiap isi catalog berubah (dijaga *Ledger revision*) |
| `published_at` | ya | datetime TOML dengan offset, disimpan UTC; waktu terbit yang sebenarnya (bukan tengah malam buatan), karena keterangan stale menampilkan jam; tidak boleh di masa depan saat build |
| `provider[].id` | ya | `ProviderName` dengan namespace `official`, unik di seluruh file (termasuk `planned`); folder `providers/<nama>/` harus ada dan valid |
| `provider[].mark`, `planned[].mark` | tidak | 1 atau 2 karakter `[A-Za-z0-9]`; default di *Value sourcing* |
| `provider[].tile`, `planned[].tile` | tidak | salah satu `blue`, `cyan`, `teal`, `green`, `yellow`, `orange`, `red`, `brown`, `indigo`, `gray`; default `gray` |
| `provider[].platform_note[]` | tidak | paling banyak satu per `os` (`linux`, `windows`); `status` `planned` atau `unsupported`; `note` 1 sampai 256 byte UTF 8 tanpa karakter kontrol; tidak boleh untuk OS yang punya artefak di `versions.json` |
| `planned[].display_name`, `category`, `description` | ya | aturan yang sama dengan `metadata.displayName`, `category` (tanpa `testing`), dan `description` spec 0009 |
| `planned[].note` | tidak | 1 sampai 256 byte UTF 8 tanpa karakter kontrol |

**Isi awal** yang ditulis build fitur ini: keenam provider wajib PRD 5.2 sebagai `planned` (`official/postgresql`, `official/mysql`, `official/nats`, `official/clickhouse`, `official/valkey`, `official/traefik`) dengan teks dan mark serta tile mockup (PostgreSQL `Pg` blue, MySQL `My` teal, NATS `Na` green, ClickHouse `Ch` yellow, Valkey `Vk` red, Traefik `Tr` cyan), revision 1. Tidak ada entri `provider` sampai fitur yang membuktikan lifecycle nya (lihat *Consequences*). Kandidat PRD 5.3 (MariaDB, RabbitMQ di mockup) tidak masuk sebagai planned.

### Ledger revision

`providers/catalog-revisions.json` ditulis hanya oleh `cargo xtask catalog check --update` (`serde_json` pretty, indent 2, newline di akhir), dengan pola yang sama dengan `protocols/contract-versions.json` spec 0006:

```json
{ "revisions": [ { "revision": 1, "publishedAt": "2026-10-19T14:05:00Z", "contentSha256": "<64 hex>" } ] }
```

- `catalog check` (dan setiap `catalog build`) menghitung *Digest isi*, lalu gagal kalau `revision` di `catalog.toml` tidak ada di ledger, atau ada dengan `contentSha256` atau `publishedAt` lain. Pesannya `catalog content changed: bump revision and published_at in providers/catalog.toml, then run cargo xtask catalog check --update`.
- `--update` menambah entri kalau `revision` belum tercatat; revision itu wajib lebih besar dari yang terbesar di ledger, dan `published_at` nya tidak boleh lebih awal dari entri terakhir.
- `--update --amend` boleh menulis ulang entri revision yang sudah tercatat hanya kalau revision itu tidak ada di `git show main:providers/catalog-revisions.json` (belum masuk `main`), supaya developer di branch bisa mengulang tanpa menaikkan revision setiap kali. Tanpa `--amend`, revision tercatat dengan digest lain selalu ditolak.
- Hook `.githooks/pre-commit` menjalankan `cargo xtask catalog check` kalau file yang di stage menyentuh `providers/` atau `runtimes/inventory.toml` (perubahan inventory membangkitkan ulang `versions.json`).

### Bentuk catalog terpasang

```text
<catalog dir>/                    # Stackly.app/Contents/Resources/catalog/ atau STACKLY_CATALOG_DIR
├── catalog.json                  # ≤ 256 KiB
└── packages/<namespace>/<nama>/  # salinan paket; path selalu diturunkan dari ID
    ├── provider.yaml
    ├── versions.json
    ├── schemas/settings.schema.json
    └── templates/*.tmpl
```

`catalog.json` (schema `stackly.catalog/v1`, JSON camelCase, `serde_json` pretty dengan indent 2 dan satu newline di akhir; tipe di `stackly-core::catalog::index` dengan `deny_unknown_fields`, dipakai xtask untuk menulis dan daemon untuk membaca):

```json
{
  "schema": "stackly.catalog/v1",
  "revision": 4,
  "publishedAt": "2026-11-02T09:41:00Z",
  "contentSha256": "<64 hex>",
  "providers": [
    {
      "id": "official/nats",
      "manifestSha256": "<manifest_sha256 spec 0009>",
      "versionsSha256": "<sha256 byte versions.json>",
      "mark": "Na",
      "tile": "green",
      "platformNotes": [{ "os": "windows", "status": "planned", "note": "..." }]
    }
  ],
  "planned": [
    { "id": "official/mysql", "displayName": "MySQL", "category": "database", "description": "...", "note": null, "mark": "My", "tile": "teal" }
  ]
}
```

- `providers` dan `planned` diurutkan berdasarkan ID. Folder setiap entri provider selalu `packages/<namespace>/<nama>` yang diturunkan loader dari ID, jadi index tidak bisa menunjuk ke luar folder catalog.
- **Digest isi**: `contentSha256` adalah SHA-256 atas serialisasi `serde_json` compact dari `CatalogContent { providers, planned }` (urutan field struct, entri terurut ID). `revision`, `publishedAt`, dan `contentSha256` sendiri tidak ikut. Karena `manifestSha256` dan `versionsSha256` ada di dalamnya, setiap perubahan paket atau `versions.json` mengubah digest ini.
- `versionsSha256` dibutuhkan karena `manifest_sha256` spec 0009 sengaja tidak mencakup `versions.json`.
- Catalog kosong (tanpa entri) valid; testkit memakainya sebagai default (*Lokasi catalog saat start*).

### Batas catalog

Semua dicek build dan loader. Batas teks dalam byte UTF 8.

| Batas | Nilai |
| --- | --- |
| Ukuran `catalog.json` | 256 KiB |
| Entri `providers` / `planned` | 64 / 64 |
| Artefak per provider di `versions.json` | 32 |
| Dependency per artefak (setelah fitur 18 menambah field itu) | 24 |
| Catatan platform per provider | 2 (satu per `linux`, `windows`) |
| `note`, alasan blocked | 256 byte |
| ID artefak, nama dependency | 128 dan 64 byte |
| Lisensi, versi dependency, lisensi dependency | 64 byte |
| URL lisensi | 256 byte |
| Total byte paket catalog (jumlah semua `PackageFiles`) | 16 MiB |
| Issue per entri `invalid` | 100 (batas `PackageError` spec 0009) |
| `ProviderSummary.versions` | 16 pertama, plus `versionCount` |

**Anggaran frame**: satu `ProviderSummary` paling besar sekitar 5 KiB (ID 127 B, nama 256 B, deskripsi 2 KiB, 16 versi kali 64 B, 8 lisensi kali 64 B, sisanya nama field), jadi list 128 entri sekitar 640 KiB. Detail paling besar sekitar 450 KiB (summary 5 KiB; 32 baris versi kali 3 sel dengan catatan 256 B sekitar 115 KiB; 32 artefak kali sekitar 1,5 KiB ditambah 24 dependency kali sekitar 300 B, sekitar 300 KiB; issue hanya untuk entri invalid yang tidak punya artefak). Keduanya di bawah `CONTROL_FRAME_MAX` 1 MiB; test di milestone 1 membangun catalog dengan semua batas penuh dan memeriksa ukuran frame nyatanya.

### Read model di daemon

Modul baru `stackly-core::catalog` (`index`, `load`, `model`, `status`, `install`). Tidak ada dependency crate baru di core; `sha2`, `serde`, dan `serde_json` sudah ada.

```rust
pub enum CatalogLocation { Bundled(PathBuf), Development(PathBuf) }   // path canonical
pub struct CatalogOptions { pub allow_testing_namespace: bool }        // default false; hanya kode test
pub enum CatalogState {
    NotConfigured,
    Unavailable { reason: UnavailableReason, detail: String },        // missing, index_invalid, unsupported_schema
    Loaded(Arc<Catalog>),
}
pub struct Catalog { meta: CatalogMeta, entries: Vec<CatalogEntry> }  // entries dalam urutan tampil
pub struct CatalogMeta {
    pub revision: u64, pub published_at: jiff::Timestamp, pub content_sha256: Sha256Digest,
    pub location: CatalogLocation, pub loaded_at: jiff::Timestamp,
}
pub enum CatalogEntry { Provider(ProviderEntry), Planned(PlannedEntry), Invalid(InvalidEntry) }
pub struct ProviderEntry { pub package: Arc<ValidatedPackage>, pub tile: TileHint, pub notes: Vec<PlatformNote> }
pub struct PlannedEntry { pub id: ProviderName, pub display_name: String, pub category: Category, pub description: String, pub note: Option<String>, pub tile: TileHint }
pub struct InvalidEntry { pub id: ProviderName, pub category: Option<Category>, pub tile: TileHint, pub issues: Vec<Issue>, pub omitted: usize }
pub struct PlatformNote { pub os: Os, pub status: SupportStatus, pub note: String }

pub fn load_catalog(fs: &dyn Filesystem, clock: &dyn Clock, location: CatalogLocation, options: CatalogOptions) -> CatalogState;
impl Catalog {
    pub fn meta(&self) -> &CatalogMeta;
    pub fn entries(&self) -> &[CatalogEntry];
    pub fn entry(&self, id: &ProviderName) -> Option<&CatalogEntry>;
    pub fn find_by_name(&self, name: &str) -> Vec<&ProviderName>;     // untuk hint not found
    pub fn outdated(&self, now: jiff::Timestamp) -> bool;
    #[cfg(feature = "test-support")]
    pub fn for_tests(entries: &[CatalogEntry]) -> Self;                        // meta test tetap, urutkan ulang, tanpa penjaga namespace
    #[cfg(feature = "test-support")]
    pub fn for_tests_with_meta(meta: CatalogMeta, entries: &[CatalogEntry]) -> Self;
}
impl ProviderEntry {
    pub fn artifacts(&self) -> &[VersionArtifact];                     // dari ValidatedPackage::versions() spec 0009
    pub fn versions(&self) -> Vec<&ExactVersion>;                      // urutan kemunculan pertama di versions.json
}
```

- `load_catalog` membaca `catalog.json` lewat `read_file_limited` (256 KiB), memeriksa aturan index (AC-3), lalu setiap entri provider lewat `read_package` dan `from_files(.., NameCheck::FolderName)`, membandingkan `manifestSha256`, SHA-256 byte `versions.json` dari `PackageFiles`, `metadata.id`, namespace, dan batas per provider, sambil menjumlah byte paket (entri yang membuat jumlah melewati 16 MiB, dan semua entri sesudahnya dalam urutan ID, menjadi invalid). Entri yang gagal menjadi `InvalidEntry` dengan issue dari `PackageError` atau satu issue buatan loader, dengan kode `IssueCode` spec 0009 dan `file` serta `location` di *Issue loader*; `category` nya dari decode ketat bagian `metadata` saja (cara yang sama dengan `manifest_software` spec 0009) kalau bisa, selain itu `None`. Fungsi ini tidak pernah menulis, spawn, atau menyelesaikan secret.
- Namespace yang diterima: `official`; `testing` hanya dengan `allow_testing_namespace`. Namespace lain (milik fitur 37) menjadi `namespace_not_allowed`.
- `Category`, `Os`, dan `SupportStatus` adalah tipe domain; `TileHint { mark: String, tile: TileColor }` dan `SupportReason` di `core::catalog::status`.
- `SupportStatus` (baru, `stackly-domain::catalog`, kode snake_case stabil, `#[non_exhaustive]`): `planned`, `unverified`, `experimental`, `supported`, `unsupported`, `blocked` (PRD 5.1). Fitur 18 memakainya ulang untuk runtime terpasang.
- `Catalog::for_tests` (fitur `test-support` core, sama dengan pola `ResolvedSecrets::for_tests`) membangun catalog dari entri yang sudah tervalidasi, termasuk paket `testing/`, untuk test di dalam proses fitur 17, 18, dan 20; `Catalog::for_tests(&[])` adalah catalog kosong yang valid. Meta nya tetap (revision 1, `publishedAt` dan `loadedAt` `2026-10-01T00:00:00Z`, lokasi `Development` dengan path `/stackly-test-catalog`, digest isi dari daftar kosong atau entri yang diberikan); test yang butuh meta lain memakai `for_tests_with_meta`. `DaemonConfig.catalog` menerimanya lewat `CatalogOverride::Preloaded(Catalog)` (daemon membungkusnya dalam `Arc`).
- `stackly_core::catalog::testing::CatalogBuilder` (fitur `test-support` yang sama) menulis folder catalog yang valid di `TestRoot` untuk binary test: memakai tipe `catalog::index`, *Digest isi*, dan `ValidatedPackage::key()` yang sama dengan xtask, menerima paket `testing/` hanya kalau diminta, dan membawa paket fixture `official/linuxonly`, `official/intelonly`, dan `official/blockedhost` (file nya di `crates/core/tests/fixtures/catalog/`). `stackly-testkit` tidak boleh bergantung pada `stackly-core` (spec 0001), jadi builder ini tidak ada di testkit; `stacklyd` dan `stackly-cli` memakainya sebagai dev dependency.
- `CatalogState` disimpan di state daemon sebagai `Arc` dan tidak berubah selama daemon berjalan; fitur 37 yang nanti menambah perubahan saat berjalan.

### Issue loader

Empat kode `IssueCode` spec 0009 yang dibuat `load_catalog` untuk `InvalidEntry.issues`. Daftar `file` validator paket spec 0009 tidak berlaku untuk keempatnya; `file` di sini menyebut file di folder catalog, dan `i` adalah posisi entri di `providers` `catalog.json`, `j` posisi artefak di `versions.json`.

| Kode | `file` | `location` | `reason` (`Issue.reason`) |
| --- | --- | --- | --- |
| `catalog_digest_mismatch` | `catalog.json` | `providers[i].manifestSha256` atau `providers[i].versionsSha256` | kosong (lokasi sudah membedakan) |
| `catalog_id_mismatch` | `provider.yaml` | `metadata.id` | kosong |
| `namespace_not_allowed` | `catalog.json` | `providers[i].id` | kosong |
| `catalog_limit` | `versions.json` untuk batas per artefak, `catalog.json` untuk batas entri | `artifacts` (jumlah artefak), `artifacts[j].dependencies` (jumlah dependency), `artifacts[j].<field>` (panjang teks), `providers[i]` (total byte paket), atau `providers[i].platformNotes` (catatan platform) | `artifacts`, `dependencies`, `text_length`, `total_bytes`, atau `platform_notes` |

`message` mengikuti aturan spec 0009 (lowercase, tanpa titik, tanpa nilai secret), misalnya `manifestSha256 does not match the package files` atau `more than 32 artifacts`.

### Urutan tampil

Entri diurutkan sekali saat load: grup (`Provider` dan `Invalid` lebih dulu, `Planned` terakhir), lalu kategori (`database`, `messaging`, `analytics`, `cache`, `proxy`, `testing`, lalu entri `invalid` tanpa kategori), lalu nama tampil (perbandingan Unicode biasa, tanpa locale), lalu ID. GUI dan CLI memakai urutan ini apa adanya.

### Urutan capability

Daftar `capabilities` di detail dan teks CLI selalu dalam urutan ini, bukan urutan enum domain: `multi_instance`, `persistence`, `reload`, `metrics`, `tls`, `cold_backup`, `online_backup`, `logical_backup`. Entri planned dan invalid mengirim daftar kosong. GUI menampilkan tiga baris dasar tetap sebelum daftar itu (Lifecycle, Readiness probe, Logs, semuanya supported karena setiap paket v1alpha1 wajib punya start, stop, readiness, dan log yang ditangkap supervisor), dan CLI menulis baris yang sama (`baseline: lifecycle, readiness probe, logs`).

### Aturan status

`state` entri (`CatalogEntryState`), dihitung dari gate install atas setiap versi entri itu untuk target host:

| Kondisi (urutan pertama yang cocok) | `state` | `stateReason` |
| --- | --- | --- |
| `Planned` | `planned` | `provider_planned` |
| `Invalid` | `invalid` | `package_invalid` |
| gate `Installable` untuk minimal satu versi | `available` | kosong |
| ada artefak `blocked` dengan target persis host | `blocked` | `artifact_blocked` (catatan dari `blocked.reason` artefak pertama) |
| selain itu | `unsupported` | reason gate versi pertama: `fixture_only`, `no_build_for_arch`, atau `no_build_for_host` |

Sel matrix `TargetSupport` untuk setiap (exact version, OS) dengan OS tetap `macos`, `linux`, `windows`:

1. `A` adalah artefak versi itu untuk OS itu. `arches` selalu berisi arch semua artefak di `A`, urut kode, dan kosong kalau `A` kosong.
2. `A` kosong: kalau ada catatan platform untuk OS itu, status dan catatannya dipakai dengan reason `platform_note`; selain itu `unsupported` dengan reason `no_build`.
3. OS sama dengan OS host: artefak dengan arch host dipilih. Tidak ada: `unsupported`, reason `no_build_for_arch`. Ada: statusnya dari `support_status(artifact, revision, &HostFacts)` fitur 18 (`runtime::compatibility`, dengan `HostFacts` dari `PlatformInfo::host_facts()` yang diusulkan spec 0018). Sampai fitur 18 dibangun, fungsi pengganti `host_cell_status(&VersionArtifact, Target) -> CellStatus` di `catalog::status` (`// provisional: F18`) memberi `blocked` (reason `artifact_blocked`, catatan `blocked.reason`) untuk artefak `blocked`, `unverified` (reason `fixture_only`) untuk `fixture`, dan `unverified` (reason `untested`) untuk `available`.
4. OS bukan OS host: status terbaik di antara artefak `A` menurut urutan `unverified` lalu `blocked`; artefak `available` memberi `unverified` reason `untested`, artefak `fixture` memberi `unverified` reason `fixture_only`, dan kalau semuanya `blocked` hasilnya `blocked` dengan catatan artefak pertama. Fungsi fitur 18 tidak dipanggil untuk OS lain, karena aturannya berpusat pada host.
5. `supported` dan `experimental` hanya datang dari fungsi fitur 18 dengan compatibility record (BR-025).
6. `compatibility.platforms` manifest tidak dipakai: spec 0009 sudah menolak artefak di luar daftar itu, dan platform yang dideklarasikan tanpa artefak untuk sebuah versi tetap tidak punya build.

`minOs` tidak dicek terhadap versi macOS host di fitur ini: build catalog menolak artefak dengan `minOs` di atas 27.0, minimum app, dan perbandingannya memakai urutan `OsVersion` spec 0004 (numerik per komponen, `13.0` lebih kecil dari `27.0`), jadi kondisi itu tidak bisa sampai ke Mac yang didukung. `APP_MINIMUM_MACOS` adalah konstanta `OsVersion` di `xtask::catalog`.

### Gate install

```rust
pub enum Installability<'a> { Installable { artifact: &'a VersionArtifact }, NotInstallable { reason: NotInstallableReason, available_targets: Vec<Target> } }
pub enum NotInstallableReason { ProviderPlanned, PackageInvalid, NoBuildForHost, NoBuildForArch, ArtifactBlocked, FixtureOnly }  // kode sama dengan SupportReason
pub fn installability<'a>(entry: &'a CatalogEntry, version: &ExactVersion, host: Target) -> Result<Installability<'a>, VersionNotFound>;
```

- `Installable` hanya untuk artefak `available` dengan target sama persis dengan host. Tidak ada substitusi arch, OS, atau emulasi (AC-005, AC-074, `native_support` spec 0004). Untuk entri planned dan invalid, versi apa pun menghasilkan `NotInstallable` (`provider_planned`, `package_invalid`), bukan `VersionNotFound`.
- Urutan reason untuk entri provider: artefak host `blocked` berarti `artifact_blocked`; artefak host `fixture` berarti `fixture_only`; tidak ada artefak host tetapi ada artefak di OS host untuk arch lain berarti `no_build_for_arch`; selain itu `no_build_for_host`.
- `available_targets` berisi target artefak `available` versi itu, urut teks `Target`, dan dikirim di detail `availableTargets` dipisah koma (kosong kalau tidak ada).
- Pemanggil: `runtime.install` dan `runtime.resolve` fitur 18, tepat setelah resolusi selector dan sebelum `begin` operasi; test end to end penolakannya (`runtime_install_refuses_an_artifact_for_another_target`) ditulis fitur 18, dan milestone 4 spec ini menunggu test itu lulus. `stacklyd` juga memakai fungsi ini untuk `CatalogVersion.install`.

### Kesehatan subsystem

Subsystem baru `catalog` (konstanta `subsystem::CATALOG`). Statusnya dipilih dengan urutan `unavailable`, lalu `degraded`, lalu `ok`:

| Keadaan | Status | `detail` |
| --- | --- | --- |
| `NotConfigured` | `unavailable` | `no catalog: not inside an app bundle and STACKLY_CATALOG_DIR is unset` |
| `Unavailable` | `unavailable` | `catalog <reason>: <detail>` |
| `Loaded` dengan entri invalid | `degraded` | `<n> catalog providers failed validation` |
| `Loaded` tanpa entri invalid | `ok` | kosong |

Fitur 20 menambah satu bagian `degraded` untuk baris sumber yang belum tertulis (*Kontrak untuk fitur 20*); beberapa bagian detail digabung dengan `; `, dan tulis yang berhasil kemudian mengembalikan status ke nilai di tabel ini. `NotConfigured` sengaja `unavailable`, sehingga daemon development tanpa catalog berstatus `degraded`: tanpa catalog JOURNEY-01 memang tidak bisa jalan.

### Lokasi catalog saat start

- `stacklyd` main membaca `STACKLY_CATALOG_DIR` sekali bersama `STACKLY_ROOT` dan `STACKLY_LOG` ke `StartConfig.catalog_dir` (struct spec 0006 yang juga diperluas spec 0015), dan startup memvalidasinya di sebelah `parse_log_filter`. Nilai kosong, path relatif, path yang tidak ada, atau bukan folder membuat daemon keluar 1 dengan pesan `STACKLY_CATALOG_DIR "<nilai>" is not an absolute path to a folder` (bentuk pesan mengikuti `STACKLY_LOG`). Path canonical nya menjadi `CatalogLocation::Development`.
- Tanpa variabel itu, startup menurunkan `CatalogLocation::Bundled(resources_dir().join("catalog"))` dari `AppBundle` yang sama yang disimpan di `DaemonDeps.bundle` (spec 0015 langkah 4, dihitung dari `StartConfig.current_exe` canonical); tidak ada turunan bundle kedua. Folder yang tidak ada di dalam bundle berarti `Unavailable { reason: missing }`. Cabang ini dibangun hanya di milestone 4 langkah 11; sampai langkah itu masuk, tanpa `STACKLY_CATALOG_DIR` hasilnya selalu `NotConfigured`, dan tempat cabang itu ditandai `// provisional: F17 step 11`.
- Tanpa keduanya (misalnya `cargo run` atau build dari Xcode IDE yang tidak membawa helper), `CatalogState::NotConfigured`.
- Catalog dimuat di slot start setelah registry dibuka (sebelum handler menjawab selain `daemon.starting`); dalam recovery mode registry catalog tetap dimuat.
- `DaemonConfig.catalog: Option<CatalogOverride>` hanya untuk test di dalam proses: `CatalogOverride` (tipe milik spec ini, nama yang sama dipakai spec 0006 dan 0018) bervarian `Location { location: Option<CatalogLocation>, options: CatalogOptions }` atau `Preloaded(Catalog)`. Tanpa override, lokasi selalu dari aturan di atas dengan `CatalogOptions::default()`. Test di dalam proses (`Site` di `apps/daemon/tests/server/common.rs`) memasang `DaemonConfig.catalog = Some(CatalogOverride::Preloaded(Catalog::for_tests(&[])))` secara default, sehingga assert `Ready` yang sudah ada (misalnya di `lifecycle.rs`) tetap berlaku; test yang memeriksa `NotConfigured` memasang `None` secara eksplisit.
- Binary test memakai `STACKLY_CATALOG_DIR` ke catalog yang dibangun `stackly_core::catalog::testing::CatalogBuilder` (fitur `test-support` core, sebagai dev dependency `stacklyd` dan `stackly-cli`) di `TestRoot`. `SpawnedDaemon` memasang catalog kosong yang valid secara default: satu file `catalog.json` tetap di testkit (revision 1, tanpa entri), yang cukup karena loader tidak memeriksa `contentSha256`, sehingga testkit tetap tanpa dependency ke `stackly-core` (spec 0001) dan test daemon yang sudah ada tetap melihat status `ok`.

### API surface

| Method | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- |
| `catalog.list_providers` | allowed | tidak ada | `ListProvidersReply { catalog: CatalogInfo, items: Vec<ProviderSummary> }` | `catalog.unavailable` |
| `catalog.get_provider` | allowed | `GetProviderParams { provider: String }` | `ProviderDetail` | `catalog.unavailable`, `catalog.provider_not_found`, `request.invalid_params` |
| `catalog::installability` (core, bukan method) | | entri, `ExactVersion`, target host | `Installability` | `VersionNotFound` |
| `cargo xtask catalog build --out <dir>` dan `xtask::catalog::build(opts)` | | `providers/` | folder catalog plus ringkasan (jumlah provider, planned, revision) | exit 1 dengan satu baris per masalah |
| `cargo xtask catalog check [--update [--amend]]` | | `providers/` | `ok catalog revision <n> <content_sha256>` | exit 1 |
| `stackly provider list [--category <c>]` | | | tabel atau JSON | exit 6, 10 |
| `stackly provider inspect <provider>` | | | bagian teks atau JSON | exit 2, 6, 10 |

`GetProviderParams::validate`: `provider` 1 sampai 127 byte, hanya `[a-z0-9/-]`, paling banyak satu `/` (`invalid_format`). Bentuk tanpa namespace diterima supaya daemon bisa memberi hint, tetapi tidak pernah dicocokkan sebagai provider.

DTO di `stackly-protocol::catalog` (camelCase, `#[typeshare]`, field opsional `#[serde(default)]`, angka lewat `Revision` dan `Count`, waktu lewat `Timestamp`, tanpa `BTreeMap`):

```rust
pub struct CatalogInfo { revision: Revision, published_at: Timestamp, loaded_at: Timestamp /* untuk diagnostics fitur 34 */,
                         content_sha256: String, source: CatalogSourceKind, trust: TrustLevel,
                         host: HostTarget, outdated: bool, invalid_count: Count }
pub struct HostTarget { os: TargetOs, arch: TargetArch }
pub struct ProviderSummary { provider: String, revision: Revision /* revision catalog */, display_name: String,
                             software: Option<String> /* null untuk entri planned dan invalid; kunci join runtime fitur 18 */,
                             category: Option<ProviderCategory> /* null hanya untuk entri invalid yang metadata nya tidak terbaca */,
                             description: Option<String>, state: CatalogEntryState,
                             state_reason: Option<SupportReason>, state_note: Option<String>, provider_revision: Option<String>,
                             publisher: Option<String>, trust: TrustLevel, versions: Vec<String> /* paling banyak 16 */,
                             version_count: Count, licenses: Vec<String>, mark: String, tile: TileColor }
pub struct ProviderDetail { summary: ProviderSummary, software: Option<String>, manifest_sha256: Option<String>,
                            capabilities: Vec<CapabilityStatus>, versions: Vec<CatalogVersion>,
                            platform_notes: Vec<PlatformNoteDto>, note: Option<String>, issues: Vec<CatalogIssue> }
pub struct CapabilityStatus { capability: ProviderCapability, supported: bool }
pub struct CatalogVersion { version: String, line: String, support: Vec<TargetSupport> /* tepat 3, urut macos, linux, windows */,
                            install: InstallVerdict, artifacts: Vec<CatalogArtifact> }
pub struct InstallVerdict { installable: bool, reason: Option<SupportReason> }
pub struct TargetSupport { os: TargetOs, arches: Vec<TargetArch>, status: SupportStatus, reason: Option<SupportReason>, note: Option<String> }
pub struct CatalogArtifact { id: String, os: TargetOs, arch: TargetArch, status: ArtifactStatus,
                             sha256: Option<String> /* hanya untuk `available`; kunci aksi Get fitur 18 */, min_os: Option<String>,
                             download_size: Option<Count>, installed_size: Option<Count>, license: Option<String>,
                             license_url: Option<String>, source: Option<String>, blocked_reason: Option<String>,
                             dependencies: Option<Vec<ArtifactDependency>> }
pub struct ArtifactDependency { name: String, kind: DependencyKind, version: Option<String>, license: Option<String> }
pub struct PlatformNoteDto { os: TargetOs, status: SupportStatus, note: String }
pub struct CatalogIssue { file: String, location: String, code: String, message: String,
                          reason: Option<String> /* dari Issue.reason, hanya untuk kode yang punya daftar reason */ }
```

Enum terbuka (masing masing diakhiri `Unrecognized`, Swift `.unrecognized` lewat `OpenEnums.swift`): `CatalogSourceKind` (`bundled`, `development`), `TrustLevel` (`official`, `development`), `ProviderCategory` (`database`, `messaging`, `analytics`, `cache`, `proxy`, `testing`), `CatalogEntryState` (`available`, `blocked`, `unsupported`, `planned`, `invalid`), `SupportStatus` (enam nilai domain), `SupportReason` (`untested`, `no_build`, `no_build_for_arch`, `no_build_for_host`, `platform_note`, `artifact_blocked`, `fixture_only`, `provider_planned`, `package_invalid`; fitur 18 menambah reason untuk `supported` dan `experimental`), `TargetOs` (`macos`, `linux`, `windows`), `TargetArch` (`aarch64`, `x86_64`), `ArtifactStatus` (`available`, `blocked`, `fixture`), `ProviderCapability` (`multi_instance`, `persistence`, `reload`, `metrics`, `tls`, `cold_backup`, `online_backup`, `logical_backup`), `DependencyKind` (`bundled`, `own`, `system`), `TileColor` (sepuluh nilai di *Sumber catalog di repo*). `CapabilityStatus` adalah DTO capability yang ditunggu spec 0012 dan 0013; fitur 20 memakainya ulang untuk form dan `UnsupportedContent.reason`.

`sha256` artefak host adalah kunci yang dipakai aksi Get fitur 18 (runtime terpasang, operasi install yang berjalan, dan `expectedSha256`), jadi kolom aksi tidak perlu memanggil `runtime.resolve` per baris. `dependencies` bernilai null selama `versions.json` belum punya field itu (ditambah fitur 18), dan berupa daftar (boleh kosong) setelahnya. `installed_size` dari `unpackedSize` fitur 18, null sebelum itu. Tidak ada event di fitur ini: catalog tidak berubah selama daemon berjalan, dan reconnect setelah restart daemon (gap `server_restarted`) memuat ulang snapshot.

Issue catalog sampai ke wire hanya lewat `ProviderDetail.issues` (DTO `CatalogIssue` milik spec ini), tidak pernah sebagai `ErrorBody.items` ber `kind` `issue`: tidak ada kode `catalog.*` yang membawa `items`. Jadi field `reason` di `CatalogIssue` ikut kenaikan minor fitur ini di langkah 5, sedangkan kunci `reason` di item `issue` `ErrorItem`, perubahan `ItemSpec`, dan minor nya tetap milik spec 0021 milestone 1 langkah 1 (keputusan lead D2).

### Kode error di wire

Area `catalog` milik spec ini (spec 0006). Semua `retryable` false. Pesan dibuat daemon (lowercase tanpa titik; `{x}` diisi dari `details`), dan `summary` katalog adalah bentuk pesan tanpa isian. Hint selalu teks statis dari `codes::catalog::hint(code, details) -> Option<&'static str>`, dipilih dari kode dan nilai `details` (tanpa isian), lalu dipasang `stacklyd` lewat `ErrorBody::with_hint`; ini bentuk hint yang sama untuk semua area menurut aturan *Hint* spec 0006, termasuk test katalog yang memeriksa setiap hint paling banyak 200 karakter tanpa `{`.

| Kode | Kategori | Kapan | Kunci `details` | Pesan | Hint statis |
| --- | --- | --- | --- | --- | --- |
| `catalog.unavailable` | `unavailable` | catalog tidak dikonfigurasi, tidak ada, atau index nya tidak valid | `reason` (`not_configured`, `missing`, `index_invalid`, `unsupported_schema`) | `the provider catalog is not available` | per `reason`: `not_configured` "run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR"; `missing` "reinstall Stackly, its bundled catalog is missing"; `index_invalid` "reinstall Stackly, its bundled catalog is damaged"; `unsupported_schema` "update Stackly, this catalog needs a newer version" |
| `catalog.provider_not_found` | `not_found` | ID tidak ada di catalog | `provider`, `suggestion` (opsional: ID lengkap kalau nama tanpa namespace cocok tepat satu entri) | `no provider {provider} in the catalog`, ditambah `; did you mean {suggestion}` kalau `suggestion` ada | dengan `suggestion` "use the full provider ID, including its namespace"; tanpa itu "run stackly provider list to see the catalog" |
| `catalog.version_not_found` | `not_found` | gate dipanggil dengan versi yang tidak ada di entri | `provider`, `version` | `{provider} has no version {version} in the catalog` | "run stackly provider inspect for this provider to see its versions" |
| `catalog.not_installable` | `unsupported` | gate menolak | `provider`, `version`, `target`, `availableTargets`, `reason` | `{provider} {version} can't be installed on {target}` | per `reason`: `provider_planned` "this provider is not in this release"; `package_invalid` "reinstall Stackly, this provider package failed validation"; `no_build_for_host` "the catalog has no build of this version for this Mac"; `no_build_for_arch` "builds exist only for the targets in availableTargets, and Stackly never runs another architecture"; `artifact_blocked` "this version is blocked in the catalog"; `fixture_only` "test fixtures are never installable"; reason yang ditambah fitur 18 (misalnya `requires_newer_os`) membawa hint nya sendiri |

Kode `catalog.pin_failed` ditambahkan fitur 20 dengan bentuk di *Kontrak untuk fitur 20*.

### CLI

Modul `apps/cli/src/commands/provider.rs` dengan pola command group spec 0008 (AC-12 spec itu); semua teks manusia boleh berubah, JSON stabil.

- `stackly provider list`: satu baris catalog lalu tabel. Baris catalog: `Catalog: official, bundled, revision 4, published 2026-11-02 16:41 +07:00` (waktu lokal), ditambah `, older than 180 days; updating Stackly may bring a newer catalog` saat `outdated`; untuk override, `development` menggantikan `official, bundled`. Kolom: `PROVIDER`, `NAME`, `CATEGORY` (kosong untuk null), `STATE` (nilai wire, dengan reason dalam kurung kalau ada), `VERSIONS` (dipisah koma, ditambah `, +<n> more` kalau `versionCount` lebih dari 16, `-` kalau kosong), `LICENSE` (dipisah `, `). `--category` dengan nilai `database`, `messaging`, `analytics`, `cache`, `proxy`, `testing` menyaring di client, juga untuk `--json`; entri tanpa kategori tidak pernah cocok. Hasil kosong menulis `No providers in category <c>.` dan exit 0.
- `stackly provider inspect <provider>`: bagian identitas (nama, ID, kategori, deskripsi, state dengan reason, publisher, trust dan sumber, provider revision dan `manifestSha256` lengkap), baris `baseline`, capability (satu baris per capability dalam *Urutan capability*, `supported` atau `not supported`), tabel matrix `VERSION`, `MACOS`, `LINUX`, `WINDOWS`, `INSTALL` (status wire plus reason atau catatan dalam kurung; `INSTALL` berisi `yes` atau `no (<reason>)` untuk Mac ini), dan tabel artefak `ARTIFACT`, `TARGET`, `STATUS`, `DOWNLOAD`, `DISK`, `MIN OS`, `LICENSE`, `SOURCE`. Entri planned menulis catatannya; entri invalid menulis issue nya satu per baris.
- Ukuran ditulis desimal SI dengan satu desimal (`6.8 MB`, `48.1 MB`, `1.2 GB`), aturan yang sama dengan `ByteCountFormatter` gaya `.file` di GUI. Golden test berjalan dengan `TZ=UTC`.

### Layar Catalog (GUI)

Folder `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/`: `CatalogStore`, `ProviderDetailModel`, `CatalogPage`, `ProviderCard`, `ProviderDetailPanel`, `VersionMatrix`, `CapabilityList`, `CatalogAppearance`, `CatalogCopy`, dan `ProviderTile`. Kalau fitur 67 sudah menyediakan tile provider, daftar key value, atau banner item, komponen itu yang dipakai dan `ProviderTile` tidak dibuat.

- `CatalogStore`: `ResourceStore<ProviderSummary>` spec 0013 dengan ID `provider`, `revision` dari `ProviderSummary.revision`, `kinds` `["catalog."]` (belum ada event), `fetchPage` memanggil `catalog.list_providers` (satu halaman, tanpa token) dan menyimpan `CatalogInfo` terakhir di `info`, dan `fetchOne` memanggil `catalog.get_provider` lalu mengambil `summary`. Didaftarkan ke `ShellSync` oleh `DaemonModel`. `request.unknown_method` menandai store `unsupported`. Catalog tidak punya event maupun observasi, jadi `CatalogStore` hanya memakai `.upsert` dan resnapshot, dan bukan pemilik aturan umum observasi dengan revision yang sama di `ResourceStore` (keputusan K18: pemiliknya store fase 02 pertama yang mempublikasikan observasi seperti itu, misalnya `RuntimeStore` fitur 18).
- Detail: `ProviderDetailModel` memanggil `DaemonModel.read(.catalogGetProvider)` saat pilihan berubah, menyimpan hasil per (`serverInstanceId`, `provider`, revision catalog), dan membuang cache saat store resnapshot. Gagal memuat menampilkan `FailureContent` di panel dengan Retry.
- Pilihan: entri pertama hasil filter kalau belum ada pilihan atau pilihan tersaring keluar; tanpa entri terlihat, panel menampilkan "No provider selected". Pilihan disimpan `@SceneStorage("catalog.selection")`; filter kategori dan teks pencarian tidak disimpan. Segmen Testing tidak ada; entri `testing` (hanya di test) dan entri tanpa kategori hanya tampil di All.
- Artefak host adalah artefak dengan `os` dan `arch` sama dengan `CatalogInfo.host`; baris License, Source, Download, Disk, dan Requires hanya memakai artefak host (penyaringan tampilan, bukan penurunan status).
- Tata letak: grid kartu `LazyVGrid` `.adaptive(minimum: 220)` di kiri dan panel detail 340 sampai 380 pt di kanan; di lebar konten 640 pt grid menjadi satu kolom dan tidak ada teks yang terpotong (cek di preview lebar sempit spec 0012).
- Kartu: tile (mark putih di atas warna sistem `tile`, dekoratif dan `accessibilityHidden(true)`), nama tampil semibold, `ToneBadge` dari `CatalogAppearance`, baris meta (`{Kategori tunggal} · {lisensi pertama}` untuk provider, "Not in this release" untuk planned, "Failed validation" untuk invalid), dan deskripsi dua baris. Kartu terpilih memakai ring warna accent; kartu planned memakai `textSecondary` untuk nama dan deskripsi serta tile `gray`, tanpa opacity yang menurunkan kontras.
- Panel detail provider: header (tile 52 pt, nama, `{id} · {Kategori}`), deskripsi, matrix (`Grid` native dengan kolom Version, macOS, Linux, Windows; setiap sel `ToneBadge` status dengan `.help` berisi alasan), catatan di bawah matrix (satu baris per kombinasi unik OS, status, dan alasan, memakai *Copy*), daftar capability (tiga baris dasar lalu delapan capability), lalu baris key value: License (lisensi unik artefak host, dengan link `licenseUrl` kalau hanya satu), Source (sumber unik), Download (satu nilai atau "{min} to {max}"), Disk (dari `installedSize` dengan format yang sama, "Not listed" kalau null), Requires ("macOS {minOs terendah} or later on Apple Silicon", dari artefak host), Dependencies ("None declared" kalau null, "None" kalau kosong, selain itu `name version (license)` untuk kind `bundled`), Publisher, Revision (`providerRevision` plus 12 hex pertama `manifestSha256`), dan Trust ("Official, bundled with Stackly" atau "Development catalog").
- Panel detail entri unsupported dan blocked menambah baris alasan di bawah deskripsi (copy `stateReason` dan `stateNote`); entri invalid menampilkan `StateBanner` critical "This provider failed validation" dengan kode `manifest.invalid` dan sampai lima issue; entri planned hanya menampilkan header, deskripsi, dan catatan planned (catatan dari `catalog.toml` kalau ada, selain itu kalimat AC-10).
- Halaman: `ViewState.resolve(unsupported:value:error:lastLoadedAt:isRefreshing:emptyContent:outdated:partial:)` dengan `unsupported` "Catalog Unavailable" dan alasan "This version of stacklyd doesn’t provide the catalog. Update Stackly." saat store `unsupported`; error store menjadi `FailureContent(title: "Catalog Unavailable", message: pesan daemon, code: kode)`; `emptyContent` `.noData` "No Providers" dengan pesan "This catalog has no providers." dan `.noResults(query)` dengan pesan "No providers match “{query}”."; `outdated` `(since: info.publishedAt, source: "the bundled catalog" atau "the development catalog")` hanya saat `info.outdated`; `partial` saat `info.invalidCount > 0` dengan teks "{n} providers failed validation." dan item ID entri invalid (lima pertama lalu "and N more" sesuai `StateBanner` spec 0012).
- Subjudul toolbar tidak ditulis sebelum snapshot pertama.
- Perintah dan shortcut (pencarian ⌘F, pindah bagian) didaftarkan lewat peta perintah fitur 69 kalau sudah ada; tanpa itu `.searchable` memakai perilaku standar macOS.

### Tone dan label

| Nilai | Label GUI | Tone | Catatan |
| --- | --- | --- | --- |
| `SupportStatus.supported` | Tested | `positive` | label mockup; CLI menulis `supported` |
| `SupportStatus.experimental` | Experimental | `caution` | |
| `SupportStatus.unverified` | Unverified | `neutral` | "not tested" di tabel tone spec 0012 |
| `SupportStatus.planned` | Planned | `neutral` | |
| `SupportStatus.unsupported` | Unsupported | `neutral` | bukan kegagalan; label dan alasan yang membawa arti |
| `SupportStatus.blocked` | Blocked | `caution` | prasyarat maintainer, bukan kegagalan pengguna |
| `SupportStatus.unrecognized` | Unrecognized State | `caution` | simbol `questionmark.diamond.fill`, detail "Update Stackly to see this state" (pola spec 0012) |
| `CatalogEntryState.available` | Available | `neutral` | mockup memakai biru; `informative` di spec 0012 khusus pekerjaan yang sedang berjalan |
| `CatalogEntryState.blocked` | Blocked | `caution` | |
| `CatalogEntryState.unsupported` | Unsupported | `neutral` | |
| `CatalogEntryState.planned` | Planned | `neutral` | |
| `CatalogEntryState.invalid` | Invalid | `critical` | paket gagal validasi |
| `CatalogEntryState.unrecognized` | Unrecognized State | `caution` | sama dengan baris `SupportStatus.unrecognized` |

Simbol lain mengikuti simbol tone spec 0012. Fitur 18 menambah "{n} installed" (`positive`) di kartu lewat join di client dengan store runtime miliknya, dengan kunci `ProviderSummary.software == RuntimeSummary.software` (tidak tampil kalau 0 atau kalau `software` null), dan tanda provider di baris Installing halaman Runtimes dengan kunci `InstallingRuntime.provider == ProviderSummary.provider`. Kartu tidak butuh `catalog.get_provider` untuk hitungan itu.

### Nilai asing

Nilai `unrecognized` di enum terbuka lain ditampilkan sebagai teks netral, tidak pernah ditebak menjadi nilai lain: `ProviderCategory` "Other" (hanya di All), `TrustLevel` "Unknown trust", `CatalogSourceKind` "Unknown source", `ProviderCapability` "Unknown capability" (baris dengan nilai `supported` apa adanya), `SupportReason` "Reason not recognized by this version of Stackly.", `TargetOs` dan `TargetArch` "Unknown", `ArtifactStatus` "Unknown", `DependencyKind` "Other", dan `TileColor` sebagai `gray`. CLI menulis nilai wire `unrecognized`.

### Copy

| Konteks | Teks |
| --- | --- |
| Subjudul toolbar | "Official · updated {tanggal}" atau "Development catalog · updated {tanggal}", tanggal gaya `.abbreviated` locale pengguna |
| Reason `untested` | "Not tested on this Mac yet." |
| Reason `no_build` | "No {OS} build in this catalog." |
| Reason `no_build_for_arch` | "No build for {arch}. Builds exist for {arches}." |
| Reason `no_build_for_host` | "No build for macOS on Apple Silicon. Stackly can’t install it on this Mac." |
| Reason `platform_note` | teks catatan apa adanya |
| Reason `artifact_blocked` | "Blocked: {catatan}" |
| Reason `fixture_only` | "Test fixture, not installable." |
| Reason `provider_planned` | "Not in this release." |
| Reason `package_invalid` | "The provider package failed validation." |
| Nama OS | macOS, Linux, Windows; arch `aarch64` "Apple Silicon" (macOS) atau "ARM64", `x86_64` "Intel" (macOS) atau "x86 64" |
| Kategori (segmen, meta) | Databases dan Database, Messaging, Analytics, Cache, Proxy |
| Capability | Multiple instances, Persistent data, Reload without restart, Metrics, TLS, Cold backup, Online backup, Logical backup |
| Baris dasar | Lifecycle, Readiness probe, Logs |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| build | daftar entri, mark, tile, catatan, planned | `providers/catalog.toml` |
| build | `revision`, `publishedAt` | `catalog.toml`, dicek terhadap ledger |
| build | `contentSha256` | *Digest isi*, sama dengan ledger untuk revision itu |
| build | `manifestSha256`, `versionsSha256` | `ValidatedPackage::key()` dan SHA-256 byte `versions.json` dari `PackageFiles` |
| load | lokasi catalog | `STACKLY_CATALOG_DIR`, lalu `AppBundle` spec 0015 |
| `CatalogInfo` | `revision`, `publishedAt`, `contentSha256` | `catalog.json` |
| `CatalogInfo` | `loadedAt` | `Clock::now()` saat `load_catalog` |
| `CatalogInfo` | `outdated` | `Clock::now()` saat request dikurangi `publishedAt`, dibanding `CATALOG_OUTDATED_AFTER` (180 hari) |
| `CatalogInfo` | `source`, `trust` | `CatalogLocation`: `Bundled` berarti `bundled` dan `official`, `Development` berarti `development` dan `development` |
| `CatalogInfo` | `host` | `PlatformInfo::target()` |
| `CatalogInfo` | `invalidCount` | jumlah `InvalidEntry` |
| summary | `provider`, `displayName`, `category`, `description` | manifest (`metadata.id`, `displayName`, `category`, `description`); planned dari `catalog.toml`; invalid: ID dari index, nama tampil sama dengan ID, `category` dari `InvalidEntry.category`, `description` null |
| summary | `revision` | revision catalog |
| summary | `state`, `stateReason`, `stateNote` | *Aturan status* |
| summary | `software` | `ValidatedPackage::software()` (null untuk planned dan invalid) |
| summary | `providerRevision` | `metadata.revision` (null untuk planned dan invalid) |
| summary | `publisher` | "Stackly" untuk namespace `official`; null untuk `testing` |
| summary | `trust` | sama dengan `CatalogInfo.trust` |
| summary | `versions`, `versionCount` | exact version artefak `available` untuk target host, urutan kemunculan pertama di `versions.json`, 16 pertama; jumlah seluruhnya |
| summary | `licenses` | lisensi unik artefak `available` untuk target host, urutan kemunculan |
| summary | `mark` | `catalog.json`; default: dua karakter ASCII huruf atau angka pertama dari nama tampil, yang pertama huruf besar dan yang kedua huruf kecil, atau "?" kalau tidak ada |
| summary | `tile` | `catalog.json`, default `gray` |
| detail | `software`, `manifestSha256` | `ValidatedPackage::software()` dan `key().manifest_sha256` |
| detail | `capabilities` | `ValidatedPackage::provider_revision().capabilities` dalam *Urutan capability* |
| detail | baris matrix | exact version unik di `versions.json`, urutan kemunculan pertama |
| detail | sel matrix | *Aturan status*; target host dari `PlatformInfo::target()` |
| detail | `install` | `installability` untuk versi itu dan target host |
| artefak | `id`, `os`, `arch`, `status`, `minOs`, `license`, `licenseUrl`, `blockedReason` | `versions.json` lewat `ValidatedPackage::versions()` |
| artefak | `sha256` | `ArtifactStatus::Available.sha256` dari `ValidatedPackage::versions()`; null untuk `blocked` dan `fixture` |
| artefak | `downloadSize` | `size` di `versions.json` |
| artefak | `installedSize` | `ArtifactStatus::Available.unpacked.bytes` (field `unpackedSize` fitur 18 di spec 0009), null sebelumnya |
| artefak | `source` | host URL pertama ditambah segmen path pertamanya (misalnya `github.com/nats-io`), tanpa skema |
| artefak | `dependencies` | `ArtifactStatus::Available.dependencies` (field fitur 18 di spec 0009; `None` berarti tidak ada data), null sebelumnya |
| detail | `platformNotes`, `note` | `catalog.json` |
| detail | `issues` | `PackageError` atau issue loader (*Issue loader*); `reason` dari `Issue.reason` |
| gate | target host | `PlatformInfo::target()` |
| gate | `availableTargets` | target artefak `available` versi itu |
| error | pesan dan hint | *Kode error di wire* |
| GUI | label, tone, simbol | *Tone dan label*, *Nilai asing* |
| GUI | teks alasan | *Copy* dari `SupportReason` dan catatan |
| GUI | artefak host | artefak dengan target sama dengan `CatalogInfo.host` |
| GUI | Requires | `minOs` terendah artefak host |
| GUI | tanggal subjudul dan keterangan stale | `CatalogInfo.publishedAt`, `CatalogInfo.source` |
| GUI, CLI | ukuran | `downloadSize` dan `installedSize`, desimal SI satu desimal |
| CLI | baris catalog, tabel | `CatalogInfo`, `ProviderSummary`, `ProviderDetail` |

### Key invariants

- Membaca catalog tidak pernah menulis file, menjalankan proses, menyentuh jaringan, atau menyelesaikan secret.
- Catalog hanya memuat namespace `official`; namespace `testing` tidak pernah ada di catalog yang dibangun xtask dan tidak pernah dimuat daemon kecuali lewat opsi test di dalam proses.
- Setiap entri provider di catalog yang dimuat cocok dengan `manifestSha256` dan `versionsSha256` index nya; isi catalog yang berubah selalu disertai revision baru di ledger.
- Status, reason, dan keputusan install hanya dihitung daemon; GUI dan CLI tidak pernah menurunkan status dari data mentah.
- `supported` tidak pernah tampil tanpa compatibility record; tidak ada substitusi target atau emulasi diam diam.
- `publishedAt` yang ditampilkan selalu tanggal dari catalog terpasang; tidak ada yang mengklaim catalog terbaru atau adanya catalog yang lebih baru.
- Catalog tidak berubah selama satu run daemon.

### Security model

- Ancaman utama: paket atau index yang diubah di dalam bundle, dan catalog palsu yang tampil sebagai official. Pertahanan baseline: segel code signature bundle macOS (spec 0015, signing terakhir dan dari dalam ke luar) ditambah `manifestSha256` dan `versionsSha256` per paket di index yang dicek daemon. Selama OQ-012 belum punya akun Apple Developer, build ditandatangani ad hoc, jadi segel hanya membuktikan "tidak berubah sejak build lokal ini"; pemeriksaan segel saat berjalan milik fitur 60 dan 63. Catalog bawaan tidak butuh hosting registry, jadi bagian hosting OQ-012 tidak menahan fitur ini.
- Catalog dari `STACKLY_CATALOG_DIR` selalu berlabel `development`, tidak pernah `official`, di DTO, GUI, dan CLI.
- Loader menolak symlink, file bukan file biasa, ukuran dan jumlah di atas batas, field asing, dan namespace di luar `official`; semua masukan lain sudah dijaga loader spec 0009. Total byte paket dibatasi 16 MiB supaya catalog tidak bisa menaikkan memori daemon jauh di atas anggaran NFR-003.
- Tidak ada revocation list atau kunci signing catalog di baseline (OQ-009); catalog tidak pernah mengklaim revocation terkini. Sinkronisasi catalog online, trust root publik, dan revocation ada di Deferred (Public registry). Trust approval untuk paket lokal milik fitur 37.
- Semua method `catalog.*` hanya membaca dan tersedia untuk peer yang sudah lolos handshake spec 0006; tidak ada peran baru. Path folder catalog tidak pernah dikirim lewat wire.

### Configuration required

- `STACKLY_CATALOG_DIR`: path absolut ke folder catalog hasil `cargo xtask catalog build`, untuk development dan binary test. Dibaca sekali oleh `stacklyd` saat start; catalog dari sini berlabel `development`. Variabel ini tidak dibaca CLI atau app.

### Critical test scenarios

- Build: `catalog.toml` fixture dengan entri `testing/fake`, entri `community/x`, folder yang hilang, paket rusak, ID berbeda, ID ganda di `provider` dan `planned`, `minOs` 28.0, 65 provider, 33 artefak, `published_at` besok, revision yang tidak tercatat, dan revision tercatat dengan digest lain masing masing exit 1 dengan pesannya; `--update` menambah revision baru dan menolak revision lebih kecil; `--amend` diterima untuk revision yang belum ada di `main` dan ditolak untuk yang sudah ada; build dua kali dari input sama menghasilkan folder yang sama byte demi byte; `--out` yang berisi file lain tanpa `catalog.json` ditolak tanpa dihapus; output tidak memuat `README.md` atau `tests/`; `cargo xtask build-app` dengan langkah catalog yang sengaja gagal (misalnya ledger yang tidak cocok) gagal tanpa meninggalkan bundle di path keluaran, dan bundle yang berhasil memuat `Contents/Resources/catalog/catalog.json` tanpa paket di luar `official`. Memverifikasi **AC-1** (juga klausa catalog AC-1 spec 0015 dan test langkah catalog gagal spec 0015).
- Tanpa efek samping: catalog dua provider di `TestRoot` dimuat dengan `FakePlatform`; `mutations()` dan spawn tetap nol; catalog dimuat walau registry dalam recovery mode. Memverifikasi **AC-2**, **AC-4**.
- Konfigurasi: `STACKLY_CATALOG_DIR` kosong, relatif, dan menunjuk file membuat binary `stacklyd` keluar 1 dengan pesannya; tanpa variabel dan tanpa bundle, `daemon status` menunjukkan subsystem `catalog` `unavailable` dan `catalog.list_providers` menjawab `catalog.unavailable` reason `not_configured` dengan hint nya; `SpawnedDaemon` default melihat `catalog` `ok`. Memverifikasi **AC-2**, **AC-13**.
- Isolasi dan index: satu paket dengan template yang diubah (digest berbeda) dan satu entri `testing/fake` menjadi `invalid`, provider lain tetap `available`, dan subsystem `degraded`; index dengan schema `stackly.catalog/v2`, field asing, ID ganda, ID di dua daftar, dan entri tidak urut masing masing membuat catalog `unavailable`; entri yang melewati 16 MiB total menjadi `invalid` dengan `catalog_limit`; `allow_testing_namespace` dan `Catalog::for_tests` menerima `testing/fake`. Memverifikasi **AC-3**.
- Batas frame: catalog dengan semua batas penuh menghasilkan balasan list dan detail di bawah 1 MiB. Memverifikasi **AC-4**.
- Detail dan BR-016: detail fake dan NATS memuat delapan capability dalam urutan tetap dengan nilai yang sama dengan `CapabilitySet`; `reload` NATS `false` sejalan dengan `reload_plan` `Unsupported`; planned mengirim daftar kosong; `official/postgres` tidak ada menghasilkan `catalog.provider_not_found`; `nats` tanpa namespace memberi `details.suggestion` `official/nats`, pesan `no provider nats in the catalog; did you mean official/nats`, dan hint statis "use the full provider ID, including its namespace"; `Official/NATS` menghasilkan `request.invalid_params`. Memverifikasi **AC-5**.
- Status: satu unit test per baris aturan state dan per langkah aturan sel, dengan fixture `official/linuxonly` (artefak `linux` `x86_64` saja), `official/intelonly` (artefak `macos` `x86_64` saja), `official/blockedhost` (artefak host `blocked`), artefak `fixture`, sel OS lain campuran `blocked` dan `available`, dan catatan Windows `planned`; tidak ada sel `supported` di catalog mana pun sebelum fitur 18. Memverifikasi **AC-6**.
- Gate: `installability` untuk `official/linuxonly` menghasilkan `no_build_for_host` dengan `availableTargets` `linux-x86_64`, untuk `official/intelonly` `no_build_for_arch`, untuk `official/blockedhost` `artifact_blocked`, untuk planned `provider_planned`, untuk versi asing `VersionNotFound`, untuk paket fake (artefak `fixture`) `fixture_only`, dan untuk salinan paket NATS 2.15.1 (artefak `available` `macos-aarch64`) di catalog test `Installable`; `CatalogVersion.install` sama dengan hasil gate di setiap kasus; pemetaan ke `catalog.not_installable` membawa kelima details. Memverifikasi **AC-7**.
- Gate end to end (milestone 4): test fitur 18 `runtime_install_refuses_an_artifact_for_another_target` lulus terhadap catalog fixture, tanpa operasi yang dibuat. Memverifikasi **AC-7** (PRD AC-002, AC-005).
- Kesegaran: `FakeClock` 179 dan 181 hari setelah `publishedAt` memberi `outdated` false dan true; GUI menampilkan keterangan stale dengan tanggal itu dan source sesuai lokasi, CLI menulis frasa `older than 180 days`; pindaian copy GUI dan CLI tidak menemukan "latest", "up to date", atau "newer catalog is available". Memverifikasi **AC-8**.
- CLI: golden teks dan JSON (dengan `TZ=UTC`) untuk list, list `--category messaging`, list `--category proxy` kosong, inspect provider fixture, inspect planned, dan inspect invalid; exit 10 untuk provider yang tidak ada dan 6 untuk catalog tidak tersedia; test Swift membaca fixture yang sama dan menghasilkan status per sel dan teks ukuran yang sama dengan golden. Memverifikasi **AC-9**.
- GUI: test view model untuk filter kategori, pencarian dengan diakritik, pilihan default dan pilihan yang tersaring, hasil kosong, kartu planned, panel unsupported, blocked, dan invalid, artefak host, keterangan stale, partial, dan setiap nilai asing; preview light, dark, 640 pt, Increase Contrast, dan Differentiate Without Color untuk setiap bentuk; evidence manual keyboard dan VoiceOver. Memverifikasi **AC-10**, **AC-11**, **AC-12**.
- Kontrak: test katalog kode, pemetaan per varian, fixture Rust dan Swift untuk setiap `Call` dan `Reply` baru dan nilai asing setiap enum terbuka, dan `codegen --check`. Memverifikasi **AC-13**.

Test yang membuktikan ID PRD (REQ-002, REQ-033, BR-016, BR-025, NFR-014, AC-002) memasang guard evidence `fixture` spec 0002; evidence manual GUI (keyboard, VoiceOver) dicatat `native` dengan status `NOT_RUN` sampai dijalankan.

### Kontrak untuk fitur 20

Bagian ini menjawab baris Follow-up spec 0009 tentang `ValidatedPackage::files()` untuk pin. Isinya dibangun dan diverifikasi fitur 20, bukan fitur ini; architect fitur 20 boleh mengambil kriteria K-1 dan K-2 apa adanya sebagai AC spec nya.

- **K-1** (pin): `catalog::pin_revision` menulis file paket sebuah entri ke `DirectoryLayout::provider_revision_dir(key)` (folder 0700, file 0600 lewat `write_file_atomic`, path dicek `check_owned_path` sebelum IO), membaca ulang dengan `NameCheck::Skip`, dan gagal kalau digest berbeda dengan key. File yang dicakup `manifest_sha256` (`provider.yaml`, settings schema, template yang dirujuk) dibandingkan byte demi byte: sama dilewati, berbeda menghasilkan `catalog.pin_failed` reason `existing_differs` tanpa menimpa. `versions.json` tidak dibandingkan: salinan pertama yang menang, karena digest tidak mencakupnya dan catalog yang lebih baru boleh membawa daftar versi lain untuk revision yang sama. Panggilan kedua untuk revision yang sama tidak menulis apa pun (`mutations()` tetap). Symlink di path atau entri asing di folder itu menghasilkan `catalog.pin_failed` tanpa menimpa atau menghapus apa pun. Delapan thread yang mem pin revision yang sama bersamaan semuanya berhasil dengan isi yang sama. Hasilnya `ProviderRevisionRecord` dengan `source_id` sumber catalog, untuk di commit pemanggil lewat `InsertProviderRevision`. Pin tidak pernah menghapus file, dan kompensasi operasi tidak pernah menghapus folder pinned.
- **K-2** (sumber): setelah registry `Ready` (tidak dalam recovery mode), daemon memastikan tepat satu baris `provider_sources` untuk sumber catalog yang dimuat (tabel di bawah). Restart daemon memakai ulang baris yang sama (ID sama); `last_sync_at` tetap NULL. Kalau baris belum bisa ditulis, subsystem `catalog` mendapat bagian `degraded` `catalog source not recorded: <kode>`, dan daemon mencoba `ensure_source` sekali lagi tepat sebelum pin pertama; kalau tetap gagal, pin ditolak dengan `catalog.pin_failed` reason `source_unrecorded` tanpa satu file pun ditulis.

```rust
pub struct PinnedRevision { pub record: ProviderRevisionRecord, pub wrote_files: bool }
pub fn pin_revision(platform: &Platform, prepared: &PreparedLayout, entry: &ProviderEntry, source: ProviderSourceId) -> Result<PinnedRevision, PinError>;
pub fn ensure_source(registry: &dyn Registry, clock: &dyn Clock, meta: &CatalogMeta) -> Result<ProviderSourceId, RegistryError>;
```

1. `dir = layout.provider_revision_dir(entry.package.key())` (`providers/<namespace>/<nama>/<revision>/<manifest_sha256>/` di root data, spec 0004).
2. Setiap folder dari root `providers/` sampai `dir`, plus `schemas/` dan `templates/`, dibuat dengan `create_dir_private`; `AlreadyExists` diterima hanya kalau path itu folder (bukan symlink), milik user, dan privat (aturan `EnsureDirectories` spec 0009).
3. Setiap file `ValidatedPackage::files()` (urut path) yang belum ada ditulis `write_file_atomic`; yang sudah ada diperlakukan seperti K-1.
4. Folder dibaca ulang dengan `read_package` dan `from_files(.., NameCheck::Skip)`; digest yang berbeda dengan key menjadi `DigestMismatch`.
5. `ProviderRevisionRecord { revision: package.provider_revision(), schema_version: Some("stackly/v1alpha1"), publisher: Some("Stackly"), approval: None, source_id: Some(source) }`. Pemanggil memeriksa `registry.provider_revision(key)` dan memasukkan `InsertProviderRevision` ke batch operasinya kalau belum ada.

- Folder pinned dialamati isi, jadi dua pin bersamaan menulis byte yang sama lewat rename atomik dan tidak butuh lock. Pin tidak mendaftarkan item ke kompensasi spec 0007 dan tidak pernah memanggil `remove_owned_tree`; folder yang tidak dirujuk lagi adalah urusan garbage collection fitur 55 atau 59.
- `PinError` (`#[non_exhaustive]`): `ExistingDiffers { path }`, `DigestMismatch`, `UnsafePath { path }`, `NoSpace`, `Io { op, path }`, `SourceUnrecorded`. Kode wire: `catalog.pin_failed`, kategori `failed`, `retryable` false, details `provider`, `revision`, `reason` (`existing_differs`, `digest_mismatch`, `unsafe_path`, `no_space`, `io`, `source_unrecorded`), pesan `the provider package could not be pinned`, hint "run stackly doctor" (fitur 34).

| Kolom `provider_sources` | Catalog bundle | `STACKLY_CATALOG_DIR` |
| --- | --- | --- |
| `kind` | `bundled` | `development` |
| `location` | `app` (tetap, supaya app yang dipindah tidak membuat baris baru) | path canonical folder override |
| `trust_root` | `app_bundle` | NULL |
| `status` | `active` | `active` |
| `last_sync_at` | NULL (tidak ada sinkronisasi) | NULL |

Kode `kind` dan `status` adalah set terbuka spec 0005; fitur 37 menambah `local` dan status lain.

Tambahan spec lain untuk kontrak ini sudah diterapkan di `main` (2026-10-20) dan dibangun fitur 20:

- **Spec 0005** (sudah di *Tambahan fitur 20* spec itu, tanpa migrasi karena tabel dan kolom sudah ada): `WriteOp::InsertProviderSource(ProviderSourceRecord)` dengan cek `DuplicateResource` untuk ID atau (`kind`, `location`); baca `provider_source_by_location(kind: &str, location: &str) -> Versioned<Option<ProviderSourceRecord>>`; `ProviderRevisionRecord.source_id: Option<ProviderSourceId>` dengan cek `MissingReference`; `ResourceRef::ProviderSource(ProviderSourceId)`; kode di tabel di atas.
- **Spec 0004** (sudah di titik perluasan spec itu): ID `ProviderSourceId` (UUID v7) dan `ProviderSourceRecord { id, kind, location, trust_root, last_sync_at, status, revision, created_at, updated_at }` di modul `registry` domain.

## Build plan

Urutan mengikuti pendekatan Journey proyek. Jalur yang dibuat lengkap lebih dulu adalah "pengguna membuka catalog dan melihat provider dengan status yang jujur", dari repo sampai daemon dengan semua state gagalnya, lalu CLI, lalu GUI, karena itu langkah catalog di JOURNEY-01. Bukti di bundle dan penolakan install end to end terakhir karena menunggu fitur 15 dan 18. Pin dan baris sumber dibangun fitur 20.

**Milestone 1: Catalog dari repo sampai daemon** (prasyarat teks spec 0009 untuk `ValidatedPackage::versions()` dan empat kode issue loader sudah terpenuhi di `main` sejak 1fe59b0; fitur ini membangun kode nya sendiri di langkah 1 dan 3)

1. Domain lebih dulu: value type `OsVersion` dan enum `ArtifactFormat` persis seperti *Model domain* spec 0004 (di tabel titik perluasan spec itu tercatat milik fitur 18; fitur 17 membangunnya lebih dulu karena dibangun lebih awal, dan langkah 6 spec 0018 memakainya tanpa membangun ulang), lalu `SupportStatus` di `stackly-domain::catalog` dengan kode stabil. Setelah itu `ValidatedPackage::versions()`, `VersionCatalog` (dengan `artifacts()` dalam urutan file), `VersionArtifact`, dan `ArtifactStatus` (`Available`, `Blocked`, `Fixture` dengan field dasar spec 0009; field fase 02 fitur 18 menyusul di langkah 1 spec 0018) di `core::provider::versions`, persis seperti *Tipe core* spec 0009. Memenuhi **AC-6**.
2. `core::catalog::index` (tipe `catalog.json`, digest isi) dan xtask `catalog` (`source.rs`, ledger, `build`, `check`, `--update`, `--amend`, `pub fn build` untuk spec 0015), `providers/catalog.toml` dan `providers/catalog-revisions.json` dengan isi awal, dan langkah `catalog check` di `.githooks/pre-commit`. Memenuhi **AC-1**.
3. `core::catalog::load` dan `model`: lokasi, decode ketat, aturan index, batas, isolasi entri, penjaga namespace, urutan tampil, `outdated`, `Catalog::for_tests`; field core `Issue.reason: Option<&'static str>` persis seperti teks spec 0009 (pemakai pertamanya `catalog_limit`; keputusan lead D2); empat kode `IssueCode` loader spec 0009 dengan `file`, `location`, dan `reason` di *Issue loader*, plus test jumlah kode di `issue.rs`; `stackly_core::catalog::testing::CatalogBuilder` dengan fixture `official/linuxonly`, `official/intelonly`, dan `official/blockedhost` (fitur `test-support`), dan file `catalog.json` kosong tetap di testkit sebagai default `SpawnedDaemon`. Memenuhi **AC-2**, **AC-3**, **AC-8**.
4. `core::catalog::status` (aturan status dengan pengganti `// provisional: F18`) dan `core::catalog::install` (gate). Memenuhi **AC-6**, **AC-7**.
5. Protokol: modul `catalog` (DTO, enum terbuka), dua varian `Call` dan `Reply` di tabel `methods!` dengan `recovery allowed`, `GetProviderParams::validate`, `codes::catalog` dengan pesan dan `codes::catalog::hint`, sha256 artefak di `CatalogArtifact`, `software` di `ProviderSummary` (fixture dengan nilai dan dengan null untuk planned), `reason` di `CatalogIssue` (fixture `catalog_limit` dengan reason dan issue lain tanpa reason), fixture `protocols/fixtures/ipc/` dan `protocols/fixtures/catalog/`, kenaikan minor `ProtocolVersion::CURRENT`, dan `cargo xtask codegen`. Memenuhi **AC-13**.
6. `stacklyd`: `StartConfig.catalog_dir` dan validasinya di sebelah `parse_log_filter`, `STACKLY_CATALOG_DIR` dan `NotConfigured` saja (cabang `Bundled` dari `DaemonDeps.bundle` di langkah 11, ditandai `// provisional: F17 step 11`), override `DaemonConfig.catalog` untuk test dengan default `CatalogOverride::Preloaded(Catalog::for_tests(&[]))` di `Site` test di dalam proses, hint lewat `ErrorBody::with_hint`, muat di slot start, subsystem `catalog`, handler kedua method, pemetaan DTO dan error di `map` dengan test per varian, dan test anggaran frame. Memenuhi **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-7**, **AC-8**, **AC-13**.

**Milestone 2: `stackly provider list` dan `inspect`** (needs fitur 8 build, kerangka CLI spec 0008)

7. Command group `provider` dengan output teks dan JSON, `--category`, golden dari fixture catalog dengan `TZ=UTC`, dan binary test terhadap `stacklyd` dengan `STACKLY_CATALOG_DIR`. Memenuhi **AC-9**.

**Milestone 3: Layar Catalog** (needs fitur 13 milestone 4 sampai langkah 16 di `ui-macos.md`, yang menunggu fitur 10 milestone 2 dan langkah 12 nya di `01-foundation.md`; memakai komponen fitur 67 kalau sudah ada)

8. `CatalogAppearance`, `CatalogCopy`, nilai asing, dan tabel *Tone dan label* di `apps/macos/design.md`. Memenuhi **AC-12**.
9. `CatalogStore`, `ProviderDetailModel`, pendaftaran ke `ShellSync`, filter, pencarian, pilihan, dan artefak host, dengan test Swift dari fixture yang sama dengan golden CLI. Memenuhi **AC-9**, **AC-10**.
10. `CatalogPage`, kartu, panel detail, matrix, capability, baris key value, semua bentuk `ViewState`, penggantian `UnbuiltPage(section: .catalog)` dan penanda `// provisional: F17`, preview, dan evidence manual keyboard serta VoiceOver. Memenuhi **AC-10**, **AC-11**.

**Milestone 4: Catalog di bundle dan penolakan install end to end** (needs fitur 15: `AppBundle` dan `cargo xtask build-app` dengan slot langkah catalog di `assemble.rs`; after fitur 18: `runtime.install` dan test end to end nya)

11. Langkah catalog di `xtask/src/build_app/assemble.rs`: panggil `xtask::catalog::build` di dalam proses dengan `--out <staging>/Contents/Resources/catalog` sebelum signing, plus test langkah catalog yang sengaja gagal tanpa meninggalkan bundle; cabang lokasi `Bundled` dari `DaemonDeps.bundle` spec 0015 langkah 4 yang menggantikan penanda `// provisional: F17 step 11` (satu satunya tempat lokasi bundle dibangun); dan pemeriksaan bahwa bundle hasil `cargo xtask build-app` memuat `Contents/Resources/catalog/catalog.json` tanpa paket di luar namespace `official`. Langkah ini juga membuktikan klausa catalog AC-1 spec 0015 dan test langkah catalog gagal nya. Memenuhi **AC-1**, **AC-2**.
12. Pastikan test fitur 18 `runtime_install_refuses_an_artifact_for_another_target` lulus terhadap catalog fixture `official/linuxonly`, lalu catat evidence AC-002. Memenuhi **AC-7**.

**Sesi paralel yang aman**: setelah langkah 1 sampai 5 masuk, langkah 7 (CLI) dan langkah 8 sampai 10 (Swift, memakai fixture dari langkah 5) bisa jalan di dua sesi terpisah karena menyentuh folder berbeda.

## Consequences

**Positive**:

- Satu read model catalog dipakai list, detail, gate install fitur 18, dan pin fitur 20, jadi tidak ada fitur yang membaca paket dengan caranya sendiri.
- Unsupported terlihat dan ditolak di daemon dengan alasan yang sama di GUI dan CLI (BR-016, PRD AC-002), dan keputusan install untuk Mac ini sudah terlihat sebelum fitur 18 ada.
- Isi catalog yang berubah tanpa revision baru tertangkap sebelum commit dan sebelum app dibangun, dan tanggal yang ditampilkan selalu jujur (NFR-014).
- Catalog bekerja penuh offline dan dalam recovery mode registry.
- DTO capability dan enum status siap dipakai ulang fitur 18, 20, dan 37.

**Negative / tradeoffs**:

- Maintainer harus menaikkan `revision` dan `published_at` setiap paket atau `versions.json` berubah; ledger memaksanya, tetapi ini langkah tangan tambahan.
- Sampai fitur 19 dan 23 selesai, catalog bawaan hanya berisi provider planned, dan setelah itu semua versi tampil Unverified sampai fitur 18 membawa compatibility record, berbeda dengan mockup yang menulis Tested.
- Development butuh `cargo xtask catalog build` dan `STACKLY_CATALOG_DIR`; `cargo run -p stacklyd` biasa tidak punya catalog dan berstatus `degraded`.
- Tile dan mark adalah data presentasi di `catalog.toml`, bukan di paket, jadi paket lokal fitur 37 butuh default sendiri.
- Ambang 180 hari adalah pilihan produk tanpa data; terlalu cepat membuat banner yang mengganggu, terlalu lambat membuatnya tidak berguna.
- Build menolak artefak yang butuh macOS lebih baru dari 27.0 sampai fitur 18 membawa cek versi OS saat berjalan.

**Neutral**:

- Area kode `catalog`, subsystem `catalog`, variabel `STACKLY_CATALOG_DIR`, file `providers/catalog.toml` dan `providers/catalog-revisions.json`, perintah `cargo xtask catalog`, dan langkah baru di hook pre commit.
- Kenaikan minor protokol untuk dua method dan DTO nya.
- Tidak ada event catalog sampai fitur 37.

**Aturan masuk catalog** (wajib diikuti fitur 19, 23, dan 40 sampai 43): sebuah provider pindah dari `planned` ke `provider` di `providers/catalog.toml` di commit yang sama dengan commit fitur pemiliknya yang membuktikan lifecycle nya end to end (install, create, start, readiness, stop lewat integration test di Mac ini): fitur 19 untuk PostgreSQL, fitur 23 untuk NATS, fitur 40 sampai 43 untuk Valkey, Traefik, MySQL, dan ClickHouse. Commit itu juga menaikkan `revision` dan `published_at` lalu menjalankan `cargo xtask catalog check --update`. Paket yang sudah ada di `providers/` tetapi belum lulus syarat itu (misalnya Traefik dari fitur 9) tetap planned.

### Update spec lain

Semua usulan spec ini ke spec lain sudah diterapkan di `main` (2026-10-20); daftar ini mencatat apa yang sekarang menjadi teks spec lain.

- **Spec 0009** (diterapkan di `main` 2026-10-20, commit 1fe59b0 dan sesudahnya): `ValidatedPackage::versions()`, `VersionCatalog`, `VersionArtifact`, `ArtifactStatus`, empat kode issue loader catalog, dan field opsional `Issue.reason` (spec 0021). Baris Follow-up spec 0009 tentang namespace `testing/` dan tentang `ValidatedPackage::files()` untuk pin sudah dicentang di sana. `file` dan `location` keempat issue loader, yang dibiarkan spec 0009 sebagai keputusan fitur 17, ada di *Issue loader*.
- **Spec 0004** (diterapkan): modul domain `catalog` dengan `SupportStatus`, `OsVersion`, `ArtifactFormat`, serta tipe registry kontrak fitur 20.
- **Spec 0005** (diterapkan): tambahan registry kontrak fitur 20 (*Tambahan fitur 20* spec itu).
- **Spec 0006** (diterapkan): bentuk hint satu untuk semua area (`codes::<area>::hint`, `ErrorBody::with_hint`, test 200 karakter tanpa `{`), area dan subsystem `catalog`, `STACKLY_CATALOG_DIR` di `StartConfig`, dan muat catalog di slot start.
- **Spec 0001** (diterapkan): `stackly-cli` boleh memakai `stackly-core` sebagai dependency dev dengan `features = ["test-support"]` untuk `CatalogBuilder`, serta `providers/catalog.toml`, `catalog-revisions.json`, dan subcommand `cargo xtask catalog` di peta repo.

## Follow-up

- [ ] Fitur 18: tambah reason `SupportReason` untuk `supported` dan `experimental`, ganti fungsi pengganti `// provisional: F18` di `catalog::status` dengan `runtime::compatibility::support_status`, isi `installedSize` dan `dependencies`, tambah tombol Get, progress, dan status "{n} installed".
- [ ] Fitur 18: setelah `HostFacts` ada, ganti penolakan build untuk `minOs` di atas 27.0 dengan cek saat berjalan (artefak itu menjadi `unsupported` di host yang lebih lama), lalu hapus `APP_MINIMUM_MACOS` dari aturan build.
- [ ] Fitur 19, 23, dan 40 sampai 43: ikuti *Aturan masuk catalog*.
- [ ] Fitur 20: ambil *Kontrak untuk fitur 20* (K-1, K-2, update spec 0004 dan 0005), panggil `pin_revision` di create instance, putuskan revision mana yang dipakai (saran: revision catalog saat create), dan tambah tombol New Instance di panel detail.
- [ ] Fitur 37: namespace lain, kind sumber `local`, publisher dan trust untuk paket lokal, default tile, tombol Import Provider, event `catalog.changed`, dan cara entri lokal bergabung ke read model.
- [ ] Fitur 16 (onboarding, langkah Catalog Trust): pakai `CatalogInfo.source`, `trust`, dan `publishedAt` dari spec ini.
- [x] `ui-macos.md` (lewat `/scope`): catat selisih mockup yang diketahui untuk Catalog: Available netral bukan biru, Unverified bukan Tested sampai fitur 18, kartu planned tanpa opacity, dan MariaDB serta RabbitMQ tidak tampil. Diterapkan di `ui-macos.md` (2026-10-19).
- [ ] `/sync` setelah build: catat `cargo xtask catalog build`, `catalog check`, ledger, `STACKLY_CATALOG_DIR`, dan langkah `cargo xtask catalog check` di hook pre commit (baris Pre commit di bagian Tooling) di root `AGENTS.md`, serta folder `Catalog/` di `apps/macos/AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan hasil cross check ada di [rationale.md](rationale.md).
