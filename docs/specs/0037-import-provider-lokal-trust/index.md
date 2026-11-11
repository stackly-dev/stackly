# 0037. Import provider lokal dan trust

**Date**: 2026-11-11
**Status**: In Progress

## Summary

Spec ini menetapkan cara kamu memasukkan provider dari folder di Mac kamu ke catalog Stackly, dan kapan Stackly boleh menjalankan sesuatu darinya. Import hanya membaca dan menyalin file teks paket, jadi paket yang baru diimpor bisa diperiksa tetapi tidak bisa memasang runtime atau membuat instance sampai kamu menyetujuinya lewat dialog izin yang menyatakan terang bahwa program native berjalan dengan hak akun kamu, tanpa sandbox (pembatas). Persetujuan (approval) mengikat persis byte paket itu, jadi setiap perubahan butuh persetujuan ulang. Mencabut persetujuan, atau revocation (pencabutan) yang dibawa catalog bawaan, hanya menutup install dan instance baru; data, runtime, dan instance yang sudah ada tidak pernah dihapus dan tetap bisa dinyalakan.

## Requirements

**User stories**:

- Sebagai kontributor provider, saya ingin mengimpor folder paket saya ke Stackly sungguhan dan melihatnya di catalog, supaya saya bisa mencoba provider itu sebelum mengirim PR.
- Sebagai pengguna yang menerima paket provider dari orang lain, saya ingin memeriksa metadata, izin, dan sumber unduhannya sebelum ada yang berjalan, supaya saya memutuskan trust dengan informasi yang jujur.
- Sebagai pengguna yang sudah menyetujui sebuah provider, saya ingin revision baru yang berbeda menunggu persetujuan ulang, supaya persetujuan lama tidak diam diam mencakup binary atau izin baru.
- Sebagai pengguna yang mencabut persetujuan, saya ingin data dan instance saya tetap utuh dan tetap bisa dinyalakan, supaya keputusan trust tidak mengunci data saya.
- Sebagai penulis script, saya ingin `stackly provider import`, `approve`, `revoke`, dan `remove` dengan JSON stabil dan perilaku yang sama dengan GUI.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-033):

- **AC-1** (preview, PRD AC-033, REQ-033): `provider.preview_import { path }` membaca folder hanya lewat port `Filesystem` (`symlink_metadata`, `read_dir`, `read_file_limited`) dan port `CodeSigning::quarantine`, memvalidasinya dengan `check_package` spec 0036 (*Kontrak dari spec 0036*) ditambah *Aturan import*, lalu membalas `ImportPreview`: identitas, publisher, tiga digest, ukuran, `permissions`, `findings` validator, `importIssues`, `verdict`, perbandingan dengan import yang sudah ada, dan `changes` terhadap keputusan terakhir. Method ini bukan operasi dan tidak menulis apa pun: dengan `FakePlatform`, `mutations()` dan jumlah spawn `FakeProcessSupervision` tetap nol, `FakeFetcher` tidak menerima permintaan, generation registry tidak berubah, dan tidak ada baris audit. Paket yang tidak valid tetap menghasilkan preview dengan `verdict.importable` false beserta finding dan issue nya; hanya folder yang tidak bisa dibaca sama sekali yang menjadi error `provider.import_unreadable`.
- **AC-2** (aturan import): Sebuah paket bisa diimpor hanya kalau lolos semua *Aturan import*: `check_package` spec 0036 tanpa finding ber severity `error` dari origin `package` atau `authoring` (jadi versi schema yang tidak didukung dan `init_port_issues` menolak import), namespace bukan `official`, `testing`, atau `stackly`, kategori bukan `testing`, setiap URL artefak adalah `HttpsUrl` spec 0018, batas per provider spec 0017 (*Batas catalog*) tidak terlewati, dan batas import (32 provider, 16 MiB total) tidak terlewati. Pelanggaran aturan milik spec ini menjadi `importIssues` dengan kode `IssueCode` di *Aturan import*; finding ber severity `warning` ditampilkan tetapi tidak menolak, dan finding ber origin `scenarios` tidak dipakai import. `path` wajib absolut, UTF-8, paling banyak 1024 byte, tanpa komponen `.` atau `..` (ditolak, tidak dinormalkan), dan menunjuk folder yang bukan symlink. Namespace `testing` dan artefak `fixture` hanya diterima kalau `CatalogOptions.allow_testing_namespace` true, yang hanya bisa dipasang kode test (spec 0017 AC-3).
- **AC-3** (import): `provider.import { idempotencyKey, path, expectedPackageSha256 }` menjalankan operasi `import_provider`: membaca dan memvalidasi ulang folder, menolak dengan `provider.import_changed` kalau `packageSha256` nya berbeda dari `expectedPackageSha256`, menulis salinan ke *Store salinan* (folder 0700, file 0600, dibaca ulang dan digest nya dicek), lalu menulis baris `provider_imports` dan baris sumber `local` lewat `commit_guarded` (guard memeriksa ulang batas import dan revision baris). Sesudahnya entri tampil di `catalog.list_providers` dengan `trust` `local` dan `approval` `pending` (atau keputusan yang masih berlaku menurut *State approval*). Import paket yang `packageSha256` nya sama dengan yang tersimpan, saat salinan store nya masih termuat (entri bukan `invalid`), membalas detail yang sama tanpa operasi, tanpa event, dan tanpa baris audit; kalau entri itu `invalid`, import berjalan sebagai operasi dan menulis ulang salinan store tanpa mengubah keputusan. Folder asal tidak pernah ditulis, diubah atributnya, atau dihapus.
- **AC-4** (belum disetujui berarti tidak berjalan, PRD AC-033, REQ-032): Untuk entri lokal yang `approval` nya bukan `approved`, `runtime.install` dan `runtime.resolve` ditolak sebelum operasi dibuat (juga saat artefaknya sudah terpasang lewat provider lain), dan `instance.create`, `instance.get_create_form`, serta form dan create duplicate ditolak di setiap titik *Gate trust*, dengan `provider.approval_required` (atau `provider.revoked`). Gate membaca ulang baris `provider_imports` dari registry, bukan dari snapshot catalog, dan operasi `install_runtime` serta `create_instance` memeriksanya lagi sebelum eksekusi pertama dan di guard commit. Test membuktikan bahwa sesudah import, preview, list, dan detail, serta sesudah setiap penolakan itu, jumlah spawn `FakeProcessSupervision` nol, `FakeFetcher` tidak menerima permintaan, dan tidak ada folder runtime atau instance yang dibuat. `CatalogVersion.install.installable` false dengan reason `approval_required` atau `trust_revoked`.
- **AC-5** (approval, REQ-032): `provider.approve { idempotencyKey, provider, expectedPackageSha256 }` menjalankan operasi `approve_provider` yang mencatat keputusan `approved` untuk tepat `packageSha256` itu beserta salinan `permissions` nya (`commit_guarded`: guard menolak kalau baris hilang, `packageSha256` berubah (`provider.import_changed`), atau catalog mencabut revision itu (`provider.revoked`)). Sesudahnya `approval` `approved`, gate AC-4 terbuka, dan baris `provider_revisions` dengan kunci (`provider`, `provider_revision`, `manifest_sha256`) yang sama (kalau ada) mendapat `approval` `approved`; baris revision lain provider itu tidak berubah. Approval entri yang sudah `approved` untuk byte yang sama membalas detail tanpa operasi.
- **AC-6** (perubahan butuh approval ulang, PRD AC-032): Import paket dengan `packageSha256` lain untuk provider ID yang sama mengganti entri dan membuat `approval` `pending`, apa pun yang berubah (manifest, settings schema, template, atau hanya `versions.json`), tanpa menyentuh instance, runtime, atau salinan pinned dari revision lama. Preview dan detail mengisi `changes` dengan kode di *Kode perubahan* terhadap `permissions` keputusan terakhir (misalnya `executables`, `downloads`, `download_hosts`, `capabilities`). Mengimpor ulang persis byte yang keputusannya tersimpan mengembalikan keputusan itu (`approved` atau `revoked`), karena keputusan mengikat byte, bukan urutan import.
- **AC-7** (revoke, PRD AC-072, REQ-032): `provider.revoke { idempotencyKey, provider }` menjalankan operasi `revoke_provider` pada entri lokal yang punya approval: keputusan tersimpan menjadi `revoked`, **setiap** baris `provider_revisions` provider itu yang `approval` nya `approved` (semua revision dan digest) menjadi `revoked`, dan gate AC-4 menutup install, create, dan duplicate, termasuk create dari revision pinned mana pun. Revoke berlaku selama ada approval, juga saat entri sedang `pending` karena byte nya sudah berganti. Test membandingkan sebelum dan sesudah: baris `runtimes`, `instances`, dan `artifacts`, isi folder data instance, folder runtime, folder pinned, dan store salinan sama persis; instance yang sudah ada tetap bisa `start`, `stop`, `restart`, dan diubah config nya. Revoke pada entri tanpa approval sama sekali (keputusan bukan `approved` dan tidak ada baris `provider_revisions` yang `approved`) ditolak `provider.not_approved`. Entri `revoked` boleh disetujui lagi lewat AC-5, yang hanya mencakup byte yang sedang tersimpan.
- **AC-8** (revocation dari catalog terpasang, PRD AC-072, PRD 21.4): `catalog.json` boleh membawa daftar `revoked` (*Revocation di catalog*). Revision yang cocok ditolak di gate yang sama dengan `provider.revoked` ber `by` `catalog`, lengkap dengan `since`, `reason`, dan revision catalog; approval nya ditolak; tidak ada yang dihapus dan instance yang ada tetap bisa start. Daftar itu dievaluasi dari catalog yang dimuat dan tidak pernah disalin ke registry. Tidak ada teks di DTO, GUI, atau CLI yang menyebut daftar revocation terkini, terbaru, atau sudah diperiksa; yang tertulis selalu revision dan tanggal terbit catalog terpasang. Tidak ada kode di fitur ini yang menyentuh jaringan, jadi semua perilaku di atas sama saat offline.
- **AC-9** (remove): `provider.remove { idempotencyKey, provider }` menjalankan operasi `remove_provider_import`: baris `provider_imports` dihapus dan entri hilang dari catalog, dan di commit yang sama setiap baris `provider_revisions` provider itu yang `approved` menjadi `revoked`, supaya tidak ada approval yang tertinggal tanpa tempat untuk mencabutnya. Instance, runtime, dan folder pinned tidak disentuh, dan instance yang ada tetap bisa start; membuat instance baru dari provider itu butuh import dan approval lagi. Folder store yang tidak lagi dirujuk dihapus lewat `remove_owned_tree` sesudah commit, masih di bawah lock provider; gagal menghapus hanya dicatat di log.
- **AC-10** (read model dan event): Daemon menyimpan catalog sebagai snapshot yang bisa diganti: entri bawaan ditambah entri lokal, dalam *Urutan tampil* spec 0017. Snapshot diganti atomik sesudah commit setiap operasi provider dan saat start sesudah registry `Ready`; pembaca yang sedang berjalan tetap memegang snapshot lamanya, dan snapshot yang dibangun dari generation registry lebih tua tidak pernah menimpa yang lebih baru. Setiap penggantian sesudah operasi menerbitkan event `catalog.changed`, sesudah commit dan sebelum event selesai operasi; penggantian saat start tidak menerbitkan event. Snapshot hanya untuk tampilan: refresh yang gagal membuat tampilan basi tetapi tidak membuka gate (AC-4). Salinan store yang tidak bisa dimuat (hilang atau rusak) menjadi entri `invalid` dengan issue finding `package` dari `check_package`, dan salinan yang termuat tetapi digest atau kuncinya tidak cocok menjadi entri `invalid` dengan issue `import_digest_mismatch`; entri `invalid` lokal tetap ber `trust` `local` dan tetap bisa di remove atau diimpor ulang, dan entri lain tetap dimuat. Dalam recovery mode registry, entri lokal tidak dimuat dan detail subsystem `catalog` menyebutnya. Catalog dengan semua batas penuh (64 provider, 64 planned, 32 lokal, 64 revocation) tetap menghasilkan balasan list di bawah `CONTROL_FRAME_MAX`, dibuktikan test.
- **AC-11** (label jujur, BR-020, PRD AC-061): Entri lokal selalu ber `trust` `local`, tidak pernah `official`, publisher nya selalu ditulis sebagai belum terverifikasi, dan sel matrix nya tidak pernah `supported` atau `experimental`: compatibility record di paket lokal diabaikan di status, di `runtime.resolve`, dan tidak disalin ke baris `artifacts` saat install. Nama tampil dan status runtime terpasang tidak pernah diambil dari entri lokal kalau ada entri catalog dengan `software` yang sama. Dialog approval GUI dan konfirmasi `stackly provider approve` menampilkan kalimat izin di `protocols/fixtures/provider/approval-statement.txt` persis (test Swift dan golden CLI membaca file yang sama). Test aturan sumber atas copy fitur ini gagal kalau ada frasa "sandboxed", "safe to run", "verified publisher", "trusted publisher", "malware", "up to date", atau "latest". Langkah `/check verify` membaca ulang semua copy terhadap PRD AC-061 dan mencatat hasilnya.
- **AC-12** (VAL-018): Keputusan trust hanya berubah lewat `provider.approve` dan `provider.revoke`. Tidak ada field settings daemon, settings instance, manifest, flag CLI lain, atau variabel environment yang dibaca untuk trust: `settings.update` dengan kunci asing tetap `request.invalid_params`, manifest dengan field asing di `permissions` tetap `unknown_field`, dan daemon yang dijalankan dengan `STACKLY_TRUST=1` atau `STACKLY_APPROVE_ALL=1` di environment tetap memberi `approval` `pending` untuk import baru. `StartConfig` dan `DaemonConfig` tidak punya field trust selain `CatalogOptions.allow_testing_namespace` milik test.
- **AC-13** (quarantine, spec 0003 AC-8, PRD 23): Import membaca file ber atribut `com.apple.quarantine` seperti file lain, menyalin byte nya ke file baru tanpa atribut itu, dan tidak pernah menulis, menghapus, atau mengubah atribut apa pun di folder asal. Preview dan detail melaporkan `quarantined` (true kalau `provider.yaml` di folder asal membawa atribut itu) dan nama agen bila terbaca; atribut yang tidak bisa dibaca berarti `quarantined` null, bukan gagal. Binary provider lokal tetap hanya masuk lewat pipeline install spec 0018, termasuk cek quarantine dan `check_launchable` nya. Tidak ada teks yang menyuruh mematikan Gatekeeper, SIP, atau menghapus quarantine.
- **AC-14** (CLI, REQ-028, PRD AC-028): `stackly provider import <dir> [--dry-run] [--approve] [--yes]`, `stackly provider approve <provider> [--yes]`, `stackly provider revoke <provider> [--yes]`, dan `stackly provider remove <provider> [--yes]` memanggil method yang sama dengan GUI dan tidak pernah membaca file paket sendiri. `--json` memberi `result` DTO di *CLI*; progress ke stderr. `approve`, `revoke`, dan `remove` meminta konfirmasi di TTY; tanpa TTY dan tanpa `--yes` keluar 2 dengan `cli.confirmation_required` tanpa request mutasi (untuk `import --approve` juga sebelum `provider.import` dikirim). `stackly provider list` dan `inspect` menampilkan trust, approval, dan izin entri lokal. Exit code mengikuti kategori: `provider.approval_required` dan `provider.revoked` keluar 4, `provider.import_changed`, `provider.import_limit`, dan `provider.not_approved` keluar 3, `manifest.invalid` dan `provider.import_unreadable` keluar 2, `provider.store_failed` keluar 7, `provider.import_not_found` keluar 10.
- **AC-15** (GUI Catalog, PAGE-08): Toolbar Catalog punya tombol "Import Provider…" yang membuka panel folder lalu sheet "Import Provider" dengan isi di *Layar*; panel detail entri lokal menampilkan banner approval, baris Publisher, Trust, dan Source, seksi Permissions, serta tombol Approve…, Revoke Approval…, dan Remove… menurut state. Approve membuka sheet izin dengan Cancel sebagai tombol default; Revoke dan Remove memakai dialog konfirmasi native dengan Cancel sebagai default. Semua state (loading, gagal baca, paket tidak valid, paket sama, pengganti, pending, approved, revoked oleh user, revoked oleh catalog, entri `invalid`) punya tampilan dan snapshot spec 0068. Kontrol mengikuti Interaction policy di `AGENTS.md`: kontrol pointer native, tanpa shortcut atau identifier test khusus.
- **AC-16** (Settings dan onboarding): Tab Sources mendapat grup "Imported providers" dan tombol "Import Provider…" menurut *Layar*, dan entri `.importProviders` di `SettingsAvailability` menjadi `isBuilt: true`; entri `.communitySources` tetap tidak dibangun dengan pemilik Deferred (public registry). Langkah Catalog Trust onboarding tidak mendapat baris atau toggle baru; subjudul dan satu kalimat catatannya berubah menurut *Copy*.
- **AC-17** (jejak, PRD AC-072): Setiap operasi `import_provider`, `approve_provider`, `revoke_provider`, dan `remove_provider_import` menulis baris audit di commit yang sama lewat mesin operasi (spec 0035) dengan `set_audit_note` berisi `provider`, `providerRevision`, `packageSha256`, `manifestSha256`, dan `changes` (untuk import dan approve), semuanya lewat `Redactor` dengan path home ditulis `~`. `ProviderDetail.local` selalu memuat keputusan terakhir (jenis, waktu, revision, digest), dan penolakan karena revocation catalog membawa revision catalog dan `since`. Preview tidak diaudit.
- **AC-18** (kontrak dan storage): Method, DTO, enum terbuka (diakhiri `Unrecognized`), kode error, dan event `catalog.changed` di *API surface* masuk dalam satu kenaikan major `ProtocolVersion::CURRENT` (nomor major bebas berikutnya, ditetapkan lead saat build), dengan fixture di `protocols/fixtures/` untuk setiap varian `Call`, `Reply`, event, error, dan nilai asing setiap enum terbuka, terbaca di Rust dan Swift, dan seed fuzz dibuat ulang. Method baru di tabel `methods!` bertanda `recovery refused`. Migrasi (nomor bebas berikutnya, ditetapkan lead saat build) membuat `provider_imports`; kolom `provider_imports.decision` dan `provider_revisions.approval` masuk `CODED_COLUMNS`; `ProviderSourceKind` mendapat `local`.
- **AC-19** (create dan pin, follow up spec 0020): `resolve_package`, `select_provider`, dan `build_form` menemukan entri lokal lewat ID lengkapnya seperti entri catalog, dengan sumber baru `PackageSource::LocalImport`. Pin entri lokal menulis `ProviderRevisionRecord` dengan `publisher` namespace paket, `approval` `approved`, `approved_at` waktu keputusan, dan `source_id` dari baris import (`ProviderImportRecord.source_id`). Pemilihan entri dari runtime saat beberapa entri punya `software` yang sama mengikuti *Software yang sama*.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan K1 sampai K21 di [rationale.md](rationale.md): import folder paket yang dibaca daemon dan disalin ke store beralamat isi; namespace bebas selain yang dicadangkan dengan publisher sama dengan namespace dan selalu belum terverifikasi; approval yang mengikat seluruh byte paket (`packageSha256`, termasuk `versions.json`); import dan approval sebagai dua operasi terpisah; gate yang hanya menutup install dan instance baru; revocation catalog yang dievaluasi dari catalog terpasang; tabel `provider_imports`; snapshot catalog yang bisa diganti dengan event `catalog.changed`.

Paket lokal masuk catalog sebagai data yang tidak dipercaya, dan satu satunya hal yang membuat Stackly mau mengunduh atau menjalankan sesuatu darinya adalah keputusan approval pengguna atas persis byte itu.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| REQ-033 (sumber catalog dan provider lokal) | AC-1, AC-2, AC-3, AC-9, AC-10 |
| REQ-032 (trust dan supply chain) | AC-4, AC-5, AC-6, AC-7, AC-8, AC-11, AC-17 |
| REQ-028 (CLI parity) | AC-14 |
| BR-020 (binary terverifikasi tetap berjalan dengan hak user) | AC-11, *Copy* |
| VAL-018 (trust tidak lewat config atau environment) | AC-12 |
| PAGE-08 (Provider Catalog: import provider) | AC-15, AC-16 |
| AC-033 (provider lokal belum dipercaya: bisa diperiksa, tidak berjalan) | AC-1, AC-4 |
| AC-032 (revision baru dengan izin berbeda menunggu approval) | AC-6 untuk provider lokal; untuk provider `official` kebijakannya adalah segel bundle, dan diff izin saat update revision instance milik fitur 59 |
| AC-061 (isolasi dijelaskan dengan benar) | AC-11 |
| AC-072 (revision dan revocation) | AC-7, AC-8, AC-17 |
| OQ-009 (trust root, signing, kesegaran revocation) | *Security model*: default PRD dipakai; OQ tetap terbuka untuk public catalog (Deferred) |
| PRD 21.4, PRD 23 | AC-8, AC-11, AC-13 |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| Schema manifest, `read_package`, `ValidatedPackage`, `manifest_sha256`, `init_port_issues`, `IssueCode` | spec 0009 |
| Kebijakan versi schema, `check_package`, `provider.validate` dan `stackly provider validate`, harness `cargo xtask provider test` | spec 0036 (*Kontrak dari spec 0036*) |
| Catalog bawaan, `catalog.json`, `cargo xtask catalog`, `Catalog`, gate `installability`, `catalog.list_providers`, `catalog.get_provider`, layar Catalog | spec 0017; spec ini menambah entri lokal, revocation, dan field DTO (*Update spec lain*) |
| Pipeline install, `Fetcher`, cek quarantine binary, `check_launchable` | spec 0018; spec ini hanya menambah gate trust sebelum resolusi |
| `resolve_package`, pin saat create | spec 0020; spec ini menambah sumber `LocalImport` dan cek trust |
| Mesin operasi, audit | spec 0007 dan 0035 |
| Panduan kontributor, dokumen publik tentang alur import | spec 0038 (merujuk *CLI* dan *Copy* spec ini; spec ini tidak menulis dokumen publik) |
| Diff izin saat instance pindah provider revision | fitur 59 |
| Registry online, signing, sinkronisasi revocation, sumber community | Deferred (public registry) |
| Garbage collection folder pinned | fitur 55 |

### Istilah

- **Paket lokal**: folder paket provider spec 0009 di luar bundle app, dipilih pengguna.
- **Entri lokal**: paket lokal yang sudah diimpor dan tampil di catalog.
- **`packageSha256`**: digest seluruh isi paket (*Digest paket*). Inilah yang diikat approval.
- **Approval**: state entri lokal, salah satu dari `pending`, `approved`, `revoked` (*State approval*).
- **Permissions**: ringkasan izin yang dihitung dari paket (*Ringkasan izin*); hanya untuk ditampilkan dan dibandingkan.
- **Revocation catalog**: entri `revoked` di catalog bawaan yang menutup sebuah provider revision.

### Kontrak dari spec 0036

Spec 0036 memiliki validator dan kebijakan versi schema. Spec ini hanya memanggilnya (kontrak dari catatan `arc-f36-spec` 2026-11-11; teks yang mengikat ada di spec 0036):

- `stackly_core::provider::check::check_package(fs: &dyn Filesystem, dir: &Path, name_check: NameCheck) -> PackageCheck`, hanya membaca lewat port `Filesystem`, tanpa proses, tanpa secret, tanpa jaringan. Import memanggilnya dengan `NameCheck::Skip` (nama folder asal bebas; identitas dari `metadata.id`).
- `PackageCheck { folder, api_version, package: Option<ValidatedPackage>, findings: Vec<Finding> }`; `Finding { severity: Error | Warning, origin: Package | Authoring | Scenarios, issue: Issue }`. Origin `package` adalah issue `read_package` dan `from_files`; `authoring` adalah pengetatan untuk penulis paket (hari ini `init_port_issues`); `scenarios` adalah masalah `tests/scenarios.yaml`.
- **Aturan spec ini atas hasil itu**: paket bisa diimpor hanya kalau tidak ada finding `error` ber origin `package` atau `authoring`. Finding `scenarios` dibuang (import tidak menyalin atau memakai `tests/`). Finding `warning` diteruskan ke preview; hari ini finding `authoring` selalu `error`, dan `warning` baru muncul untuk `apiVersion` berstatus `deprecated`, yang tetap bisa diimpor. Ini lebih ketat dari `loadable()` dan sama dengan `publishable()` tanpa bagian `scenarios`.
- Versi schema yang tidak didukung adalah finding `package` berkode `unsupported_api_version` dari tabel `SUPPORTED_API_VERSIONS` spec 0036; spec ini tidak punya daftar versi atau kode sendiri.
- DTO finding di wire (`severity`, `origin`, `file`, `location`, `code`, `message`, `reason`) adalah DTO `provider.validate` spec 0036, dipakai ulang oleh `ImportPreview.findings`; di spec ini ia disebut `ValidationFinding` (nama akhirnya mengikuti spec 0036).
- Saat daemon start, salinan store diperiksa ulang dengan `check_package`. Salinan yang tidak `loadable()` menjadi entri `invalid`; finding `authoring` tidak membuat salinan yang sudah tersimpan menjadi `invalid` (aturan yang sama dengan salinan pinned lama). Keputusan approval nya tetap tersimpan, tetapi gate menolak entri `invalid` seperti biasa (`package_invalid`).
- `stackly provider validate` (method `provider.validate`) dan `cargo xtask provider test` tidak butuh import atau approval. Harness spec 0036 hanya menerima namespace `official` dan `testing`; paket ber namespace lain masuk Stackly lewat import spec ini.

### Aturan import

Diperiksa `catalog::local::check_importable(package: &ValidatedPackage, options: CatalogOptions) -> Vec<Issue>` (murni) atas `PackageCheck.package` kalau ada. Setiap baris menghasilkan issue dengan format spec 0009, dikirim sebagai `importIssues`. `init_port_issues` tidak ada di tabel ini karena sudah datang sebagai finding `authoring` dari `check_package`.

| Aturan | Kode `IssueCode` | `file`, `location` |
| --- | --- | --- |
| Namespace `official`, `stackly`, atau (tanpa opsi test) `testing` | `namespace_not_allowed` | `provider.yaml`, `metadata.id` |
| Kategori `testing` tanpa opsi test | `invalid_value` | `provider.yaml`, `metadata.category` |
| URL artefak bukan `HttpsUrl` | `versions_rule` | `versions.json`, `artifacts[j].urls[k]` |
| Batas per provider *Batas catalog* spec 0017 (32 artefak, 24 dependency, panjang teks) | `catalog_limit` dengan `reason` yang sama | seperti *Issue loader* spec 0017, tanpa awalan `providers[i]` |

Batas import bukan issue paket, karena bergantung pada isi registry: paling banyak **32** entri lokal (`LOCAL_PROVIDERS_MAX`) dan **16 MiB** total byte paket lokal (`LOCAL_BYTES_MAX`, dijumlah dari `provider_imports.package_bytes`). Import yang mengganti entri yang ada menghitung selisihnya. Pelanggaran menjadi `verdict.reason` `import_limit` di preview dan `provider.import_limit` di import.

ID yang sama dengan entri catalog bawaan tidak mungkin terjadi, karena catalog hanya memuat namespace `official` (dan `testing` di test). Dengan opsi test, import ID yang sama dengan entri catalog ditolak `namespace_not_allowed`.

### Digest paket

```rust
// stackly_core::catalog::local::digest
// versions_sha256: fungsi `catalog::index::versions_sha256` yang sudah ada (spec 0017), dipakai ulang; paket tanpa versions.json sudah ditolak loader
pub fn package_sha256(files: &PackageFiles) -> Sha256Digest;
```

`package_sha256` = SHA-256 atas teks UTF-8 berisi satu baris `<sha256 file huruf kecil>  <path relatif>\n` per file di `PackageFiles`, diurutkan berdasarkan byte path. Bentuknya sama dengan `manifest_sha256` spec 0009, tetapi mencakup semua file yang dibaca loader, termasuk `versions.json`. Karena `provider.yaml` memuat `metadata.id` dan `metadata.revision`, digest ini juga mengikat ID (dan dengan itu publisher) dan revision.

### Ringkasan izin

```rust
// stackly_core::catalog::local::permissions
pub struct Permissions {
    pub capabilities: Vec<Capability>,            // yang didukung saja, dalam Urutan capability spec 0017
    pub managed_filesystem: Vec<InstanceDir>,     // permissions.managedFilesystem, urut kode
    pub executables: Vec<PermittedExecutable>,    // runtime.executable lalu additionalExecutables urut alias
    pub secrets: Vec<String>,                     // nama field secret yang dikirim ke proses (env, stdin, materializeSecret, config sensitive), urut
    pub endpoints: Vec<PermittedEndpoint>,        // urut nama
    pub downloads: Vec<PermittedDownload>,        // artefak available, urutan versions.json
    pub open_with: Vec<PermittedOpenWith>,        // urutan manifest
}
pub struct PermittedExecutable { pub name: String /* "runtime" atau alias */, pub path: String, pub uses: Vec<ExecutableUse> }
pub enum ExecutableUse { Start, Initialize, Stop, Reload, Probe, VersionCheck, OpenWith }   // urut seperti ini
pub struct PermittedEndpoint { pub name: String, pub transport: String, pub purpose: String, pub default_port: Option<u16> }
pub struct PermittedDownload { pub artifact: String, pub version: String, pub os: Os, pub arch: Arch, pub sha256: Sha256Digest, pub size: u64, pub hosts: Vec<String> /* host unik URL, urut, paling banyak 8 */, pub signing_policy: String }
pub struct PermittedOpenWith { pub name: String, pub kind: String, pub target: Option<String> /* bundle ID pertama, alias executable, atau kosong */ }

pub fn permissions(package: &ValidatedPackage) -> Permissions;                       // murni, deterministik
pub fn permission_changes(decided: &Permissions, decided_revision: &RevisionLabel,
                          current: &Permissions, current_revision: &RevisionLabel) -> Vec<PermissionChange>;
```

- `uses` dihitung dari `Manifest` yang sudah divalidasi: `Start` dari `lifecycle.start`, `Initialize` dari langkah `runExecutable`, `Stop` dari strategi `command`, `Reload` dari aksi `run`, `Probe` dari probe `exec` di readiness atau health, `VersionCheck` dari `runtime.versionCheck` (selalu `runtime`), `OpenWith` dari aksi `terminal`.
- `Permissions` disimpan sebagai JSON `stackly.provider-permissions/v1` (camelCase, urutan field struct) di kolom `decided_permissions`. Untuk paket yang sedang tersimpan ia selalu dihitung ulang dari salinan store, tidak dibaca dari kolom.

**Kode perubahan** (`PermissionChange`, urutan tetap; daftar kosong hanya kalau tidak ada keputusan sebelumnya):

| Kode | Berarti |
| --- | --- |
| `revision` | label `metadata.revision` berbeda |
| `executables` | daftar `executables` berbeda (nama, path, atau `uses`) |
| `downloads` | himpunan (`artifact`, `sha256`) berbeda |
| `download_hosts` | himpunan host unduhan berbeda |
| `capabilities` | daftar capability berbeda |
| `filesystem` | `managed_filesystem` berbeda |
| `secrets` | daftar `secrets` berbeda |
| `endpoints` | daftar endpoint berbeda |
| `open_with` | daftar `open_with` berbeda |
| `content` | `packageSha256` berbeda tetapi tidak satu pun kode di atas berlaku (misalnya isi template) |

### Store salinan

- Folder baru `DirectoryLayout::provider_imports_dir()` = `<data>/provider-imports/` dan `provider_import_dir(package_sha256)` = `<data>/provider-imports/<package_sha256>/` (usulan untuk spec 0004 di *Update spec lain*). Isinya persis file `PackageFiles`.
- Penulisan memakai langkah yang sama dengan pin spec 0017 K-1 (folder `create_dir_private` 0700, file `write_file_atomic` 0600, `check_owned_path` sebelum IO, file yang sudah ada dibandingkan byte demi byte, baca ulang lalu cek digest). Build memisahkan langkah 2 sampai 4 `pin_revision` menjadi fungsi bersama `catalog::store::write_package(platform, prepared, dir, files, mode: StoreMode) -> Result<bool, StoreError>` yang dipakai pin dan import; `StoreError` membawa reason yang sama dengan `PinError` tanpa `SourceUnrecorded`. `StoreMode::Pinned` mempertahankan perilaku pin apa adanya (`versions.json` tidak dibandingkan, salinan pertama menang, baca ulang mencocokkan kunci manifest, tanpa kompensasi). `StoreMode::Import` membandingkan **semua** file termasuk `versions.json`, mencocokkan `package_sha256` saat baca ulang, dan mengembalikan `true` kalau ia yang membuat folder itu supaya pemanggil mendaftarkan kompensasi.
- Folder dialamati isi, jadi dua import bersamaan atas byte yang sama menulis byte yang sama. Operasi `import_provider` mendaftarkan folder yang ia buat sebagai item kompensasi; kalau operasi gagal sebelum commit, folder itu dihapus lewat `remove_owned_tree`.
- Sesudah commit import yang mengganti entri, atau remove, pekerjaan operasi (masih memegang `LockKey::Provider`) membaca ulang `provider_imports` dan menghapus folder lama lewat `remove_owned_tree` hanya kalau tidak ada baris yang merujuk `package_sha256` itu. Gagal menghapus tidak menggagalkan operasi (log `warn` dengan kode error).
- Folder pinned (`<data>/providers/...`) tidak pernah disentuh fitur ini selain lewat pin spec 0020.

### Model data

**Migrasi `NNNN_provider_imports.sql`** (nomor bebas berikutnya, ditetapkan lead saat build). Jenis kolom memakai nama jenis spec 0005.

**`provider_imports`** (satu baris per provider ID yang diimpor)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `provider`* | `TEXT` PK | `ProviderName`, aturan yang sama dengan `provider_revisions.provider` |
| `source_id`* | `uuid` | FK `provider_sources(id)` restrict; selalu baris sumber `local` |
| `provider_revision`* | `TEXT` | `RevisionLabel` |
| `software`* | `name` | |
| `manifest_sha256`*, `versions_sha256`* | `sha256` | |
| `package_sha256`* | `sha256` | unik; nama folder store |
| `package_bytes`* | `INTEGER` | 1 sampai 4194304 |
| `origin_path`* | `TEXT` | 1 sampai 1024 byte; path yang dikirim client, hanya informasi |
| `quarantined` | `bool` | NULL kalau atribut tidak terbaca |
| `quarantine_agent` | `TEXT` | paling banyak 128 byte |
| `imported_at`* | `ts` | waktu commit import terakhir |
| `decision` | `code` | set terbuka: `approved`, `revoked`; NULL kalau belum pernah diputuskan |
| `decided_package_sha256` | `sha256` | byte yang diputuskan |
| `decided_revision` | `TEXT` | `RevisionLabel` saat diputuskan |
| `decided_permissions` | `json` object | `Permissions` saat diputuskan |
| `decided_at` | `ts` | |
| `revision`*, `created_at`*, `updated_at`* | | seperti tabel lain |

`CHECK`: kelima kolom `decision` dan `decided_*` semuanya NULL atau semuanya terisi. Index pada `source_id`.

**`provider_sources`**: tepat satu baris untuk semua import, dibuat saat import pertama: `kind` `local`, `location` `imports` (tetap), `trust_root` NULL, `status` `active`, `last_sync_at` NULL. Tidak ada status baru; revocation melekat pada entri, bukan pada sumber.

**`provider_revisions`**: tidak ada kolom baru. Untuk revision yang dipin dari entri lokal: `publisher` = namespace, `approval` = `approved`, `approved_at` = `decided_at`, `source_id` = sumber `local`. `approval` NULL tetap berarti dipercaya lewat kebijakan sumber (bundled atau development). Approve memperbarui `approval` dan `approved_at` baris dengan kunci (`provider`, `provider_revision`, `manifest_sha256`) kalau ada; revoke dan remove memperbarui setiap baris provider itu yang `approved`. Trigger immutable spec 0005 mengizinkan kedua kolom itu. Approval di tabel ini per `manifest_sha256`, bukan per `package_sha256`: menyetujui paket dengan manifest yang sama tetapi `versions.json` lain juga mencakup salinan pinned yang lebih lama. Itu aman karena salinan pinned tidak pernah memicu unduhan; install selalu lewat entri catalog, yang gate nya `package_sha256`.

**Domain** (`stackly-domain`, usulan untuk spec 0004 dan 0005):

- `ProviderSourceKind::Local` (`local`); `TrustDecision` (`code_enum`, `approved`, `revoked`), dipakai kolom `provider_imports.decision` dan `provider_revisions.approval`. `ProviderRevisionRecord.approval` menjadi `Option<TrustDecision>` (kode asing dibaca lewat aturan set terbuka spec 0005) dan record itu mendapat `approved_at: Option<Timestamp>`, yang sekarang belum ada.
- `ProviderImportRecord { provider, source_id, provider_revision, software, manifest_sha256, versions_sha256, package_sha256, package_bytes, origin_path, quarantined: Option<bool>, quarantine_agent: Option<String>, imported_at, decision: Option<ImportDecision>, revision, created_at, updated_at }` dengan `ImportDecision { decision: TrustDecision, package_sha256, provider_revision, permissions_json: String, at: Timestamp }`.
- `OperationKind`: `import_provider`, `approve_provider`, `revoke_provider`, `remove_provider_import`. `LockKey::Provider(ProviderName)` (teks `provider:<id>`), diambil keempat operasi lewat `extra_locks`. Resource operasinya `OperationResource::Manager` (seperti `update_daemon_settings`), karena `operations.resource_id` dan `audit_events.resource_id` hanya menerima UUID dan provider tidak punya UUID; ID provider dicatat di audit note.
- Port `Registry`: baca `provider_imports() -> Versioned<Vec<ProviderImportRecord>>` (urut `provider`) dan `provider_import(&ProviderName)`; `WriteOp::PutProviderImport(ProviderImportRecord)` (insert atau ganti, revision naik), `WriteOp::DecideProviderImport { provider, decision: ImportDecision }`, `WriteOp::DeleteProviderImport(ProviderName)`, `WriteOp::SetProviderRevisionApproval { key: ProviderRevisionKey, approval: TrustDecision, at: Timestamp }` (nol atau satu baris; nol bukan error), `WriteOp::RevokeProviderApprovals { provider: ProviderName, at: Timestamp }` (setiap baris `provider_revisions` provider itu yang `approved`), dan baca `provider_has_approved_revision(&ProviderName) -> bool`. Semua lewat `commit_guarded` (spec 0020 Keputusan 21).
- `CODED_COLUMNS`: `provider_imports.decision` dan `provider_revisions.approval` dengan `codes_of!(TrustDecision)`.

### State approval

State tidak disimpan; ia dihitung satu fungsi murni domain `approval_state(record: &ProviderImportRecord) -> ApprovalState`:

| Kondisi | State |
| --- | --- |
| `decision` NULL, atau `decided_package_sha256` berbeda dari `package_sha256` | `pending` |
| sama, `decision` `approved` | `approved` |
| sama, `decision` `revoked` | `revoked` |

Transisi:

```text
(tidak ada) --import--> pending
pending   --approve--> approved
approved  --revoke---> revoked
revoked   --approve--> approved
pending (keputusan approved untuk byte lain) --revoke--> pending (keputusan menjadi revoked)
approved | revoked --import byte lain--> pending      (keputusan lama tetap tersimpan untuk changes)
pending   --import byte yang diputuskan--> approved | revoked
(apa pun) --remove--> (tidak ada)
```

Revoke ditolak (`provider.not_approved`) hanya kalau tidak ada approval sama sekali: keputusan bukan `approved` dan `provider_has_approved_revision` false. Satu baris menyimpan satu keputusan: approve atas byte baru menimpa keputusan lama. Remove selalu mencabut approval baris `provider_revisions` provider itu.

### Gate trust

Satu modul core yang dipanggil semua jalur:

```rust
// stackly_core::catalog::trust
pub enum TrustVerdict { Trusted, ApprovalRequired, Revoked(RevokedInfo) }
pub struct RevokedInfo { pub by: RevokedBy /* User | Catalog */, pub since: Timestamp, pub reason: Option<String>, pub catalog_revision: Option<u64> }
pub fn entry_trust(catalog: &Catalog, entry: &ProviderEntry) -> TrustVerdict;                         // tampilan: dari snapshot
pub fn require_entry_trust(catalog: &Catalog, registry: &dyn Registry, entry: &ProviderEntry)
    -> Result<TrustVerdict, RegistryError>;                                                            // gate: baca ulang registry
pub fn pinned_trust(catalog: Option<&Catalog>, key: &ProviderRevisionKey, record: &ProviderRevisionRecord) -> TrustVerdict;
pub fn guard_trust(view: &dyn RegistryView, provider: &ProviderName, package_sha256: &Sha256Digest) -> TrustVerdict;  // di dalam commit_guarded
```

- `entry_trust`: revocation catalog yang cocok dengan kunci entri menang (`Revoked` by `Catalog`); lalu untuk entri lokal `approval_state` atas record di snapshot: `pending` berarti `ApprovalRequired`, `revoked` berarti `Revoked` by `User` dengan `since` = `decided_at`; selain itu `Trusted`. Entri catalog tanpa revocation selalu `Trusted`. Hanya untuk tampilan (list, detail, `CatalogVersion.install`).
- `require_entry_trust`: sama, tetapi untuk entri lokal record nya dibaca ulang dengan `Registry::provider_import(id)`. Baris yang hilang, atau yang `package_sha256` nya berbeda dari entri snapshot, berarti `ApprovalRequired`. Registry yang gagal dibaca menggagalkan request (`internal` atau `unavailable` seperti error registry lain), tidak pernah `Trusted`. Entri catalog tidak membaca registry.
- `pinned_trust`: revocation catalog yang cocok menang; lalu `record.approval` `revoked` berarti `Revoked` by `User` dengan `since` = `approved_at`; selain itu `Trusted`. Catalog yang tidak dimuat berarti tidak ada revocation catalog. Record nya selalu baru dibaca dari registry oleh `find_pinned`.
- `guard_trust`: dipanggil guard `commit_guarded` operasi `create_instance` (untuk sumber `LocalImport`; untuk sumber `Pinned` ber `source_id` sumber `local`, guard membaca `provider_revisions.approval`) dan commit `install_runtime` (untuk seleksi dari entri lokal). Verdict selain `Trusted` menggagalkan operasi dengan `provider.approval_required` atau `provider.revoked`, dan kompensasi biasa operasi itu berjalan. `install_runtime` dari entri lokal juga memanggil `require_entry_trust` tepat sebelum exec check di langkah `validate_binary`, supaya revoke yang masuk selama unduhan mencegah eksekusi pertama. Revoke tidak pernah menunggu unduhan: install dan create tidak mengambil `LockKey::Provider`.
- **Pencocokan revocation catalog**: entri `revoked` cocok dengan sebuah kunci kalau `id` dan `revision` sama dan (`manifestSha256` entri itu null atau sama dengan `manifest_sha256` kunci). `since` (tanggal) menjadi `Timestamp` tengah malam UTC tanggal itu.

| Aksi | Titik gate (fungsi) | `pending` | `revoked` (user atau catalog) |
| --- | --- | --- | --- |
| `catalog.list_providers`, `catalog.get_provider`, preview | tidak ada | boleh | boleh |
| `runtime.install`, `runtime.resolve` | `select()` di `apps/daemon/src/ipc/runtime.rs`, tepat sesudah entri ditemukan dan sebelum `runtime::resolve` dan `install_precheck` (jadi juga saat artefaknya sudah terpasang) | `provider.approval_required` | `provider.revoked` |
| `instance.get_create_form` tanpa `revision` | handler form sesudah `select_provider`, sebelum `build_form` | `provider.approval_required` | `provider.revoked` |
| `instance.create`, form dengan `revision` (entri catalog atau lokal) | `resolve_package`, cabang entri | `provider.approval_required` | `provider.revoked` |
| create atau form dari revision pinned | `resolve_package`, cabang `find_pinned` (`pinned_trust`) | tidak mungkin | `provider.revoked` |
| form dan create duplicate | `gather_duplicate` (`pinned_trust` atas revision instance sumber), lalu `resolve_package` saat create | tidak mungkin | `provider.revoked` |
| commit `create_instance`, exec check dan commit `install_runtime` | `guard_trust`, `require_entry_trust` | operasi gagal | operasi gagal |
| `start`, `stop`, `restart`, `reload`, edit config, rename, ganti endpoint, log, metrics instance yang ada | tidak ada | boleh | boleh |
| runtime terpasang, data, salinan pinned | tidak disentuh | | |

`catalog::installability` dan `provider_installability` mendapat parameter `trust: &TrustVerdict` dan mengembalikan `NotInstallable` dengan reason baru `ApprovalRequired` (`approval_required`) atau `TrustRevoked` (`trust_revoked`) sebelum aturan lain, supaya `CatalogVersion.install` dan `state` entri menampilkannya. Ketiga pemanggilnya menghitung verdict lebih dulu: `catalog::status` dan `apps/daemon/src/map/catalog.rs` dengan `entry_trust`, dan `runtime::resolve` menerima verdict `require_entry_trust` dari `select()`. Handler memetakan kedua reason itu ke `provider.approval_required` dan `provider.revoked`, bukan ke `catalog.not_installable`. `ProviderProblem` dan `FormError` mendapat `ApprovalRequired` dan `Revoked(RevokedInfo)`.

Entri lokal yang tidak `Trusted` ber `state` `blocked` dengan `stateReason` `approval_required` atau `trust_revoked` (baris baru di *Aturan status* spec 0017, tepat sesudah baris `Invalid`).

### Revocation di catalog

Tambahan pada sumber dan index catalog spec 0017 (usulan di *Update spec lain*; dibangun fitur ini):

```toml
# providers/catalog.toml
[[revoked]]
id = "official/nats"
revision = "1.0.0"
manifest_sha256 = "<64 hex>"      # opsional; tanpa ini semua digest revision itu
reason = "Starts with an insecure default; use revision 1.0.1 or later."
since = 2026-11-20
```

```json
"revoked": [ { "id": "official/nats", "revision": "1.0.0", "manifestSha256": null, "reason": "...", "since": "2026-11-20" } ]
```

- Aturan build dan loader: paling banyak 64 entri, urut (`id`, `revision`, `manifestSha256`), unik; `id` `ProviderName` dengan namespace apa pun; `reason` 1 sampai 256 byte tanpa karakter kontrol; `since` tanggal yang tidak di masa depan saat build; entri tidak boleh cocok dengan revision entri `provider` di catalog yang sama (build gagal, loader membuat catalog `index_invalid`).
- `CatalogContent` menjadi `{ providers, planned, revoked }` dengan `revoked` tidak ditulis saat kosong (`skip_serializing_if`), sehingga `contentSha256` setiap revision yang sudah ada di ledger tidak berubah. Menambah entri `revoked` mengubah isi, jadi butuh revision catalog baru di ledger.
- `Catalog::revocation(&self, key: &ProviderRevisionKey) -> Option<&Revocation>`; `Revocation { id, revision, manifest_sha256: Option<Sha256Digest>, reason, since }`.
- Revocation tidak pernah disalin ke registry, tidak mengubah `provider_revisions`, dan tidak diaudit (tidak ada mutasi). Jejaknya adalah revision catalog dan `since` di setiap penolakan dan di `ListProvidersReply.revocations`.
- Kesegaran: daftar ini setua catalog terpasang. `CatalogInfo.publishedAt` dan `outdated` spec 0017 sudah menyampaikan itu; fitur ini tidak menambah klaim lain dan tidak menyentuh jaringan.

### Read model

- `ProviderEntry` mendapat `origin: EntryOrigin`: `Catalog` atau `Local(LocalInfo)` dengan `LocalInfo { record: ProviderImportRecord, permissions: Permissions }`. `InvalidEntry` mendapat `local: Option<ProviderImportRecord>`, supaya entri `invalid` lokal tetap dikenali (trust `local`, `ProviderDetail.local` tanpa `permissions`, tombol Remove).
- `Catalog::with_local(&self, local: Vec<CatalogEntry>) -> Catalog` membuat catalog baru: entri bawaan (dibagi lewat `Arc`) ditambah entri lokal, diurutkan ulang dengan *Urutan tampil*. Entri lokal memakai `TileHint` default: `mark` dua karakter `[A-Za-z0-9]` pertama dari `displayName` (huruf pertama kapital, kedua kecil; satu karakter kalau hanya ada satu; `?` kalau tidak ada), `tile` `gray`, tanpa catatan platform.
- `catalog::local::load(fs, registry, layout, options) -> Vec<CatalogEntry>`: untuk setiap baris `provider_imports`, `check_package(fs, provider_import_dir(package_sha256), NameCheck::Skip)`, lalu cek `package_sha256` dan kunci terhadap baris. Salinan yang tidak `loadable()` menjadi `InvalidEntry` dengan issue finding `package` nya, dan salinan yang digest atau kuncinya tidak cocok menjadi `InvalidEntry` dengan satu issue baru `import_digest_mismatch` (`IssueCode` baru, `file` `provider.yaml`, `location` `metadata.id`, pesan `the stored copy does not match the import record`). Tidak pernah menulis.
- Compatibility record di `versions.json` entri lokal tidak dipakai. Satu accessor `ProviderEntry::trusted_compatibility(&self, artifact: &VersionArtifact) -> &[CompatibilityRecord]` mengembalikan daftar kosong untuk `EntryOrigin::Local`; `catalog::status`, `select()` runtime (`support_status`), dan `InstallInput::new` (`compatibility_input`) memakainya menggantikan `artifact.compatibility()` langsung. Jadi sel host entri lokal `unverified` reason `untested`, dan install dari entri lokal tidak menyimpan record di baris `artifacts`. `minOs` tetap dicek (`requires_newer_os`).
- `catalog_match` dan `DisplayNames` di `apps/daemon/src/ipc/runtime.rs` (nama tampil dan status runtime terpasang per `software`) memilih entri `EntryOrigin::Catalog` lebih dulu; entri lokal dipakai hanya kalau tidak ada entri catalog dengan `software` itu, dan kalau ada beberapa entri lokal, yang ID nya terkecil.
- State daemon: `catalog: OnceLock<Arc<CatalogState>>` diganti `RwLock<Arc<CatalogState>>` (std). `catalog()` mengembalikan `Arc` saat itu; pembaca tidak pernah memegang lock sambil bekerja. `CatalogState::Loaded` selalu berisi catalog gabungan. Entri lokal hanya dilayani saat catalog bawaan `Loaded`; pada `NotConfigured` atau `Unavailable`, method `provider.*` menjawab `catalog.unavailable` seperti method `catalog.*`.
- `apps/daemon/src/catalog.rs` mendapat `refresh_local(core)`: muat entri lokal di `spawn_blocking`, bangun catalog gabungan dari catalog bawaan yang disimpan terpisah (`base: Arc<Catalog>`), ganti isi `RwLock`, terbitkan `catalog.changed`. Dipanggil saat start sesudah registry `Ready` (tanpa event), dan oleh pekerjaan setiap operasi provider sebagai langkah terakhirnya, lewat port `CatalogRefresher` yang disuntikkan ke pekerjaan (bukan oleh handler, yang bisa berhenti menunggu). Urutannya: commit data, ganti snapshot, event `catalog.changed`, operasi selesai (event operasi), balasan. Setiap snapshot membawa generation registry dari `Versioned` yang ia baca, dan penggantian ditolak kalau generation nya lebih kecil dari snapshot yang terpasang. Refresh yang gagal membaca registry membiarkan snapshot lama, menambah bagian `degraded` `local providers not loaded: <kode>` di subsystem `catalog`, dan tidak menggagalkan operasi yang datanya sudah di commit; gate tetap benar karena membaca registry.
- Kesehatan subsystem: entri lokal `invalid` ikut hitungan `<n> catalog providers failed validation`; recovery mode menambah bagian `local providers not loaded: registry recovery`.

### Software yang sama

Beberapa entri boleh punya `software` yang sama (misalnya `official/nats` dan `acme/nats`). Runtime tetap satu per `sha256` artefak (spec 0018), jadi dua provider yang menunjuk artefak yang sama berbagi runtime.

- `instance.create`, form, dan `runtime.install` selalu menerima ID provider lengkap; itu yang menentukan.
- Pemilihan entri dari runtime (`select_provider` di `crates/core/src/instance/create/form.rs`, yang hari ini memberi `ProviderAmbiguous` begitu dua entri punya `software` sama): entri `EntryOrigin::Catalog` dengan `software` itu; kalau tidak ada, satu satunya entri lokal yang `Trusted` dengan `software` itu; selain itu `instance.provider_ambiguous` dengan `details.candidates` (ID dipisah koma, paling banyak 8), dan client meminta pengguna memilih.
- Join "{n} installed" di kartu (spec 0017) tetap lewat `software`, jadi kartu `acme/nats` ikut menghitung runtime NATS yang terpasang.
- Runtime yang dipasang lewat entri lokal yang disetujui adalah runtime biasa: ia tetap terpasang sesudah revoke atau remove dan bisa dipilih instance provider lain ber `software` sama. Approval memang mengizinkan binary itu masuk, dan revocation tidak mencabut yang sudah terpasang (*Security model*); menghapusnya adalah remove runtime (fitur 55).

### API surface

Semua method lewat handshake dan peer user yang sama (spec 0006). Modul DTO baru `stackly-protocol::provider`.

| Method | Jenis | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- | --- |
| `provider.preview_import` | baca, bukan operasi | refused | `PreviewImportParams { path }` | `ImportPreview` | `provider.import_unreadable`, `catalog.unavailable`, `request.invalid_params` |
| `provider.import` | operasi `import_provider` | refused | `ImportProviderParams { idempotencyKey, path, expectedPackageSha256 }` | `ProviderDetail` sesudah operasi selesai, bukan `OperationView`; tanpa operasi kalau byte sama | `provider.import_unreadable`, `manifest.invalid`, `provider.import_changed`, `provider.import_limit`, `provider.store_failed` |
| `provider.approve` | operasi `approve_provider` | refused | `DecideProviderParams { idempotencyKey, provider, expectedPackageSha256 }` | `ProviderDetail` | `provider.import_not_found`, `provider.import_changed`, `provider.revoked`, `catalog.not_installable` reason `package_invalid` |
| `provider.revoke` | operasi `revoke_provider` | refused | `ProviderTargetParams { idempotencyKey, provider }` | `ProviderDetail` | `provider.import_not_found`, `provider.not_approved` |
| `provider.remove` | operasi `remove_provider_import` | refused | `ProviderTargetParams { idempotencyKey, provider }` | `RemovedProvider { provider }` | `provider.import_not_found` |

- Method operasi memakai `replay_operation` lalu precheck lalu `start_operation` (pola `settings.update` spec 0032), dengan resource `OperationResource::Manager`, `extra_locks` `[LockKey::Provider(id)]`, dan `BusyPolicy::Queue`. Request yang diulang dengan `idempotencyKey` yang sama mendapat balasan pertamanya apa adanya (`replay_operation`), juga kalau entri sudah berubah atau dihapus sesudah itu.
- Jalur tanpa operasi (byte sama, sudah `approved`) diputuskan di precheck, sebelum lock. Pekerjaan operasi memeriksa ulang di guard: kalau baris sudah memuat `package_sha256` dan keputusan yang diminta, guard tidak menulis apa pun dan operasi selesai sukses. Jadi delapan import bersamaan atas byte yang sama berakhir dengan satu baris dan delapan balasan sukses.
- `Validate`: `path` menurut AC-2; `provider` menurut `GetProviderParams` spec 0017 tetapi wajib ber namespace; `expectedPackageSha256` 64 hex huruf kecil.
- Keempat operasi tidak resumable dengan kebijakan `Compensate` (baris baru di *Tabel resume per jenis operasi* spec 0007): satu commit lewat `commit_guarded`; hanya `import_provider` yang punya efek di luar registry sebelum commit (folder store, item kompensasi). `OperationSummary.cancel` bernilai `none`.

```rust
pub struct ImportPreview {
    provider: Option<String>, display_name: Option<String>, software: Option<String>,
    category: Option<ProviderCategory>, description: Option<String>,
    provider_revision: Option<String>, publisher: Option<String>,
    manifest_sha256: Option<String>, versions_sha256: Option<String>, package_sha256: Option<String>,
    package_bytes: Count, file_count: Count,
    origin_path: String,                               // home ditulis ~
    quarantined: Option<bool>, quarantine_agent: Option<String>,
    versions: Vec<String> /* paling banyak 16 */, version_count: Count,
    permissions: Option<ProviderPermissions>,          // null kalau paket tidak valid
    findings: Vec<ValidationFinding> /* DTO spec 0036, tanpa origin scenarios, paling banyak 100 */, omitted_findings: Count,
    import_issues: Vec<CatalogIssue> /* dari check_importable */,
    verdict: ImportVerdict,
    existing: Option<ExistingImport>,
    changes: Vec<PermissionChange>,
    revocation: Option<RevocationInfo>,                // revocation catalog untuk kunci paket ini
}
pub struct ImportVerdict { importable: bool, reason: Option<ImportBlockReason> }
pub struct ExistingImport { provider_revision: String, package_sha256: String, approval: ApprovalState, same_package: bool, stored_copy_valid: bool }
pub struct ProviderPermissions { capabilities: Vec<ProviderCapability>, managed_filesystem: Vec<String>,
    executables: Vec<PermissionExecutable>, secrets: Vec<String>, endpoints: Vec<PermissionEndpoint>,
    downloads: Vec<PermissionDownload>, open_with: Vec<PermissionOpenWith> }
pub struct PermissionExecutable { name: String, path: String, uses: Vec<ExecutableUse> }
pub struct PermissionEndpoint { name: String, transport: String, purpose: String, default_port: Option<u16> }
pub struct PermissionDownload { artifact: String, version: String, os: TargetOs, arch: TargetArch, sha256: String, size: Count, hosts: Vec<String>, signing_policy: String }
pub struct PermissionOpenWith { name: String, kind: String, target: Option<String> }
pub struct LocalProviderInfo { imported_at: Timestamp, origin_path: String, quarantined: Option<bool>,
    versions_sha256: String, package_sha256: String, package_bytes: Count,
    decision: Option<TrustDecisionInfo>, permissions: Option<ProviderPermissions> /* null untuk entri invalid */,
    changes: Vec<PermissionChange> /* terhadap keputusan, hanya saat pending dan keputusan ada */ }
pub struct TrustDecisionInfo { decision: ApprovalState /* approved atau revoked */, decided_at: Timestamp, provider_revision: String, package_sha256: String }
pub struct RevocationInfo { by: RevocationSource, since: Timestamp, reason: Option<String>, catalog_revision: Option<Revision> }
pub struct CatalogRevocation { provider: String, provider_revision: String, manifest_sha256: Option<String>, reason: String, since: String /* YYYY-MM-DD */ }
pub struct CatalogChanged { provider: Option<String>, change: CatalogChangeKind }
pub struct RemovedProvider { provider: String }
```

Tambahan pada DTO spec 0017 (semua opsional dengan `#[serde(default)]`):

- `ProviderSummary.approval: Option<ApprovalState>` (null untuk entri catalog).
- `ProviderDetail.local: Option<LocalProviderInfo>` dan `ProviderDetail.revocation: Option<RevocationInfo>` (terisi saat `entry_trust` `Revoked`).
- `CatalogInfo.local_count: Count`; `ListProvidersReply.revocations: Vec<CatalogRevocation>` (paling banyak 64).

Enum terbuka baru atau bertambah (masing masing diakhiri `Unrecognized`): `TrustLevel` mendapat `local`; `SupportReason` mendapat `approval_required` dan `trust_revoked`; baru: `ApprovalState` (`pending`, `approved`, `revoked`), `RevocationSource` (`user`, `catalog`), `ImportBlockReason` (`package_invalid`, `import_limit`), `PermissionChange` (sepuluh kode *Kode perubahan*), `ExecutableUse` (`start`, `initialize`, `stop`, `reload`, `probe`, `version_check`, `open_with`), `CatalogChangeKind` (`imported`, `approved`, `revoked`, `removed`). `OperationKind` wire mendapat empat kode operasi.

**Event** `catalog.changed` (`EventKind::CatalogChanged(CatalogChanged)`): event daemon tanpa resource dan tanpa revision, seperti `settings.changed`; client membaca `catalog.list_providers` lagi. `provider` berisi ID yang berubah. Karena `EventKind` tertutup, ini kenaikan major protokol.

**Anggaran frame**: list paling besar 64 + 64 + 32 = 160 `ProviderSummary` kali sekitar 5 KiB (sekitar 800 KiB) ditambah 64 `CatalogRevocation` kali sekitar 600 B (sekitar 40 KiB), di bawah 1 MiB. `ImportPreview` dan `ProviderDetail` lokal paling besar: 32 `PermissionDownload` kali sekitar 1 KiB, 17 executable, 100 issue kali sekitar 600 B, jauh di bawah batas. Test membangun kasus penuh dan memeriksa ukuran nyata.

### Kode error di wire

Kode baru di area `provider` (area milik spec 0009; kode ini milik spec ini). Semua `retryable` false. Pesan lowercase tanpa titik; hint statis dari `codes::provider::hint(code, details)`, paling banyak 200 karakter.

| Kode | Kategori | Kapan | Kunci `details` | Pesan | Hint statis |
| --- | --- | --- | --- | --- | --- |
| `provider.import_unreadable` | `invalid_input` | folder tidak bisa dibaca | `path` (home sebagai `~`), `reason` (`not_found`, `not_a_directory`, `symlink`, `permission_denied`, `io`) | `the provider folder can't be read` | `permission_denied`: "macOS did not let stacklyd read this folder; move it outside Desktop, Documents, and Downloads, or allow Stackly in Privacy & Security > Files and Folders"; lainnya: "choose the folder that contains provider.yaml" |
| `provider.import_changed` | `conflict` | isi folder atau entri berbeda dari yang dipreview | `provider`, `expected`, `actual` | `the provider package changed since it was previewed` | "preview the package again before you continue" |
| `provider.import_limit` | `conflict` | batas import terlewati | `limit` (`count`, `bytes`), `max` | `too many imported providers` | "remove an imported provider first" |
| `provider.import_not_found` | `not_found` | ID bukan entri lokal | `provider`, `reason` (`not_imported`, `not_local`) | `no imported provider {provider}` | `not_local`: "this provider comes from the catalog and has no approval to change"; lainnya: "run stackly provider list to see imported providers" |
| `provider.approval_required` | `unauthorized` | gate, state `pending` | `provider`, `providerRevision`, `packageSha256` | `{provider} is not approved` | "review it with stackly provider inspect, then run stackly provider approve" |
| `provider.revoked` | `unauthorized` | gate atau approve, verdict `Revoked` | `provider`, `providerRevision`, `by`, `since`, `catalogRevision` (kalau `by` `catalog`) | `{provider} {providerRevision} is revoked` | `user`: "existing instances still start; approve the provider again to allow new installs"; `catalog`: "the catalog bundled with this version of Stackly revokes this revision; existing instances still start" |
| `provider.not_approved` | `conflict` | revoke pada entri tanpa approval sama sekali | `provider`, `approval` | `{provider} has no approval to revoke` | "remove the provider instead if you no longer want it" |
| `provider.store_failed` | `failed` | salinan tidak bisa ditulis | `provider`, `reason` (`existing_differs`, `digest_mismatch`, `unsafe_path`, `no_space`, `io`) | `the provider package could not be stored` | "run stackly doctor" |

Paket tidak valid di `provider.import` memakai `manifest.invalid` spec 0009 dengan `items` issue. `instance.provider_ambiguous` spec 0020 mendapat `details.candidates`.

### CLI

Subcommand baru di grup `stackly provider` (`apps/cli/src/commands/provider/`, yang juga mendapat `validate` dari spec 0036).

| Perintah | Perilaku | `result` `--json` |
| --- | --- | --- |
| `stackly provider import <dir> --dry-run` | path dibuat absolut dari cwd, lalu `provider.preview_import`; mencetak preview; exit 0 kalau `importable`, selain itu `cli.provider_invalid` spec 0036 (exit 2) dengan finding dan issue ke stderr dan `result` tetap terisi di `--json` | `ImportPreview` |
| `stackly provider import <dir> [--approve] [--yes]` | preview, cetak ringkasan ke stderr, lalu `provider.import` dengan `packageSha256` dari preview. Import tidak meminta konfirmasi (tidak menjalankan apa pun). Paket yang tidak `importable` berhenti sesudah preview dengan `cli.provider_invalid`. Dengan `--approve`: syarat konfirmasi (`--yes` atau TTY) dicek sebelum `provider.import` dikirim, lalu lanjut seperti `approve`. Paket yang sama dengan yang tersimpan menulis "Already imported." dan exit 0 | `ProviderDetail` |
| `stackly provider approve <provider> [--yes]` | `catalog.get_provider`, cetak izin dan kalimat izin, konfirmasi `Approve <provider>? [y/N]`, lalu `provider.approve` dengan `local.packageSha256` dari detail yang ditampilkan | `ProviderDetail` |
| `stackly provider revoke <provider> [--yes]` | cetak akibatnya (*Copy*), konfirmasi, `provider.revoke` | `ProviderDetail` |
| `stackly provider remove <provider> [--yes]` | cetak akibatnya, konfirmasi, `provider.remove` | `RemovedProvider` |

- Konfirmasi mengikuti spec 0008: hanya interaktif di TTY; tanpa TTY atau dengan `--json`, tanpa `--yes` gagal `cli.confirmation_required`; jawaban selain `y` atau `yes` gagal `cli.confirmation_declined`.
- `stackly provider list`: kolom baru `TRUST` (`official`, `development`, `local`) dan nilai `STATE` entri lokal memakai `needs approval`, `revoked`, atau state biasa; baris catalog ditambah `, <n> imported` kalau `localCount` lebih dari 0; sesudah tabel, bagian `Revoked by this catalog (revision <n>, published <tanggal>):` dengan satu baris per `revocations` kalau ada.
- `stackly provider inspect <provider>` untuk entri lokal menambah: `publisher: <namespace> (not verified)`, `trust: local, not verified`, `approval: <state>` dengan waktu keputusan, `imported: <waktu> from <path>`, `package: <packageSha256>`, bagian `permissions:` (executables dengan `uses`, downloads dengan host dan digest, filesystem, secrets, endpoints, open with, capabilities), dan `changes since your decision:` kalau ada.
- Teks manusia boleh berubah; JSON stabil. Golden berjalan dengan `TZ=UTC`.

### Layar

Folder `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/`, file baru: `ProviderImportModel`, `ImportProviderSheet`, `ApproveProviderSheet`, `ProviderPermissionsList`, `LocalProviderSection`, `ProviderTrustCopy`. Mockup hanya menggambar tombol "Import Provider…" di toolbar Catalog dan teks toast nya; sheet, dialog, dan seksi di bawah ini tidak ada di mockup, jadi bentuknya ditetapkan di sini dengan komponen spec 0012 dan 0067 (`StacklySheet`, `StateBanner`, `CardSection`, `.labeledContentStyle(.stackly)`, `ToneBadge`).

- **Tombol toolbar** "Import Provider…" di halaman Catalog (posisi mockup). Nonaktif dengan `.help` "Not available while stacklyd is offline." saat daemon tidak terjangkau, dan tidak tampil saat store `unsupported`.
- **Alur import** (`ProviderImportModel`, `@Observable`, `@MainActor`): `NSOpenPanel` (`canChooseDirectories` true, `canChooseFiles` false, satu pilihan, prompt "Choose") → `provider.preview_import` → `ImportProviderSheet`. State model: `choosing`, `loading`, `preview(ImportPreview)`, `unreadable(ErrorBody)`, `importing`, `failed(ErrorBody)`. Sesudah `provider.import` berhasil, sheet tertutup, Catalog memilih entri itu, dan kartu umpan balik spec 0067 menulis "Imported {displayName}. Approve it before you install anything."
- **`ImportProviderSheet`** (judul "Import Provider"): header (tile default, nama, `{id} · revision {providerRevision}`); baris Publisher, Category, Software, Versions, Package (12 hex pertama `packageSha256`), Folder (`originPath`); catatan quarantine saat `quarantined` true; `ProviderPermissionsList`; catatan `existing`; catatan kaki tetap; catatan `revocation` dan `changes` (*Copy*); tombol Cancel dan "Import" (tombol default; nonaktif saat tidak `importable`, atau saat `samePackage` dan `storedCopyValid`; untuk entri `invalid` labelnya "Import Again"). Paket tidak valid: `StateBanner` critical "This package can’t be imported" dengan sampai 20 baris (`file location: message`) dari finding `error` lalu `importIssues`, lalu "and N more". Finding `warning` pada paket yang bisa diimpor tampil sebagai `StateBanner` caution "{n} warnings from the validator" dengan daftar yang sama. `unreadable`: `FailureContent` dengan pesan dan hint daemon dan tombol "Choose Another Folder…".
- **`ProviderPermissionsList`**: grup tetap dalam urutan ini, grup kosong tidak tampil: "Runs" (satu baris per executable: path monospace, lalu label `uses`), "Downloads" (satu baris per host: "{host} · {n} files", lalu disclosure daftar artefak dengan versi, target, 12 hex digest, ukuran), "Writes to" (label folder), "Receives secrets" (nama field), "Listens on" (endpoint dan port default), "Opens" (aksi open with), "Features" (capability).
- **Panel detail entri lokal** (`LocalProviderSection`, disisipkan `ProviderDetailPanel`): banner di bawah header menurut tabel *Banner approval*; baris Publisher "{namespace} (not verified)", Trust "Local, not verified", Source "Imported {tanggal} from {originPath}", Revision seperti spec 0017 ditambah baris Package (12 hex `packageSha256`); seksi "Permissions" (`ProviderPermissionsList`); di bawahnya tombol "Revoke Approval…" (saat `approved`, atau saat `pending` dengan keputusan `approved`) dan "Remove…". Matrix, capability, dan baris lain tetap dari spec 0017.
- **`ApproveProviderSheet`** (judul `Approve “{displayName}”?`): baris identitas, kalimat izin, `ProviderPermissionsList`, `changes` kalau ada, catatan kaki; tombol Cancel (tombol default, juga Escape) dan "Approve". Mengirim `provider.approve` dengan `packageSha256` dari detail yang sedang ditampilkan; `provider.import_changed` menutup sheet dan memuat ulang detail dengan pesan daemon.
- **Revoke dan Remove**: `confirmationDialog` native dengan teks *Copy*, tombol aksi ber role `.destructive`, Cancel sebagai default.
- **Kartu**: baris meta entri lokal "{Kategori} · Local". `ToneBadge` kartu dan header: "Needs Approval" saat `pending`, "Revoked" saat `revoked` atau saat `revocation` ada; saat `approved` badge state biasa spec 0017.
- **`CatalogStore`**: berlangganan `catalog.changed` (kind `catalog.` sudah terdaftar) dan memanggil `refresh()` (snapshot ulang); `ProviderDetailModel` membuang cache seperti biasa. `ProviderSummary.revision` entri lokal tetap revision catalog.
- **Settings › Sources**: grup baru "Imported providers" di bawah "Providers": satu baris per entri lokal ("{displayName}", sub "{id} · {label approval} · imported {tanggal}", tombol "Show in Catalog" yang membuka jendela utama di Catalog dengan entri itu terpilih); kosong: "No imported providers."; tombol "Import Provider…" yang menjalankan `ProviderImportModel` yang sama dengan sheet di jendela Settings. Baris Catalog mendapat "{n} revoked revisions in this catalog" saat `revocations` tidak kosong. Aturan offline spec 0032 AC-24 berlaku.
- **Onboarding**: hanya copy (*Copy*); `CatalogTrustContent` tidak mendapat baris baru.

**Banner approval**:

| Kondisi | Tone | Judul | Pesan | Aksi |
| --- | --- | --- | --- | --- |
| `pending`, tanpa keputusan | caution | "Not approved yet" | "Stackly won’t install or run anything from this provider until you approve it." | "Approve…" |
| `pending`, ada keputusan | caution | "Changed since your approval" (atau "Changed since you revoked it") | "This version differs from revision {decidedRevision} that you decided on {tanggal}: {daftar label changes}." | "Approve…" |
| `revoked` oleh user | caution | "Approval revoked" | "Revoked on {tanggal}. Stackly won’t install runtimes or create instances from this provider. Existing instances and their data are untouched and can still start." | "Approve Again…" |
| revocation catalog | caution | "Revoked by the catalog" | "The catalog in this version of Stackly (revision {n}, published {tanggal}) revokes this revision since {since}: {reason} Existing instances and their data are untouched and can still start." | tidak ada |
| entri `invalid` | critical | banner spec 0017 | | "Remove…" tetap tersedia |

### Tone dan label

| Nilai | Label GUI | Tone |
| --- | --- | --- |
| `TrustLevel.local` | "Local, not verified" | `neutral` |
| `ApprovalState.pending` | "Needs Approval" | `caution` |
| `ApprovalState.approved` | "Approved" | `neutral` (bukan `positive`: approval bukan bukti aman) |
| `ApprovalState.revoked` | "Revoked" | `caution` |
| `ApprovalState.unrecognized` | "Unrecognized State" | `caution`, detail "Update Stackly to see this state" |
| `SupportReason.approval_required` | "Needs your approval." | |
| `SupportReason.trust_revoked` | "Approval revoked." | |
| `RevocationSource.unrecognized`, `PermissionChange.unrecognized`, `ExecutableUse.unrecognized`, `ImportBlockReason.unrecognized`, `CatalogChangeKind.unrecognized` | "Unknown" (teks netral; `CatalogChangeKind` hanya memicu refresh) | |

Tabel ini ditambahkan ke `apps/macos/design.md` (aturan status set baru spec 0012) dan dipetakan `CatalogAppearance` dengan `switch` exhaustive.

### Copy

Semua teks GUI fitur ini ada di `ProviderTrustCopy.swift`; teks CLI di satu modul `copy` grup `provider`.

| Konteks | Teks |
| --- | --- |
| Kalimat izin (`protocols/fixtures/provider/approval-statement.txt`, satu baris) | "This provider runs native programs on your Mac with your user account’s permissions. Stackly does not sandbox them, so they can read and change the same files you can. A matching checksum only shows that a download was not altered. It does not show that the program is safe." |
| Baris identitas sheet approve | "{id} · revision {providerRevision} · from {namespace} (not verified)" |
| Sheet approve, di bawah kalimat izin | "Approve only if you trust where this provider came from." |
| Catatan kaki sheet approve | "This approval covers exactly this version of the provider. Any change needs a new approval." |
| Catatan kaki sheet import | "Importing doesn’t run anything. The provider stays blocked until you approve it." |
| Catatan quarantine | "This folder was downloaded from the internet. Stackly only reads its text files and never changes the folder." |
| `existing`, byte sama | "This exact package is already imported." |
| `existing`, byte sama, `storedCopyValid` false | "Stackly’s copy of this package is damaged. Importing again repairs it and keeps your decision." |
| Sheet import, `changes` tidak kosong | "Differs from the version you decided on: {daftar label changes}." |
| Sheet import, `revocation` ada | "The catalog in this version of Stackly revokes this revision. You can import it to inspect it, but you can’t approve it." |
| `existing`, byte lain | "Replaces imported revision {providerRevision}. The new version needs approval before it can install or run anything." |
| `verdict.reason` `import_limit` | "You’ve reached the limit for imported providers. Remove one first." |
| Dialog revoke, judul dan pesan | `Revoke approval for “{displayName}”?` · "Stackly will stop installing runtimes and creating instances from this provider, including from revisions you approved earlier. Existing instances, their data, and installed runtimes stay as they are, and existing instances can still start." · tombol "Revoke Approval" |
| Dialog remove, judul dan pesan | `Remove “{displayName}” from the catalog?` · "Removing also revokes its approval. Instances created from it, their data, and installed runtimes are not touched, and those instances can still start. To create new instances from it, import and approve it again. The folder you imported from is not touched." · tombol "Remove" |
| Label `uses` | start "starts the server", initialize "runs during setup", stop "runs on stop", reload "runs on reload", probe "runs health checks", version_check "runs once at install", open_with "opens in Terminal" |
| Label `changes` | revision "revision", executables "programs it runs", downloads "files it downloads", download_hosts "download hosts", capabilities "features", filesystem "folders it writes", secrets "secrets it receives", endpoints "endpoints", open_with "apps it opens", content "configuration content" |
| Onboarding, subjudul langkah Catalog Trust (mengganti teks spec 0016) | "Providers describe how to install and run each server. Stackly only installs and runs providers from sources you trust." |
| Onboarding, kalimat tambahan di bawah catatan trust | "You can import a provider from a folder later in Catalog. It stays blocked until you approve it." |
| CLI `approve`, sebelum konfirmasi | baris identitas, bagian `permissions:`, baris kosong, kalimat izin persis, baris kosong, "This approval covers exactly this version of the provider. Any change needs a new approval." |
| CLI `revoke` dan `remove`, sebelum konfirmasi | pesan dialog yang sama |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| preview, import | `path` | input client: `NSOpenPanel` atau argumen CLI yang dibuat absolut dari cwd |
| preview | byte paket, `findings` | `check_package(fs, path, NameCheck::Skip)` lewat port `Filesystem`; finding `scenarios` dibuang |
| preview | `importIssues` | `check_importable` |
| preview | `provider`, `displayName`, `software`, `category`, `description`, `providerRevision` | `ValidatedPackage`; kalau validasi gagal, decode ketat bagian `metadata` saja (cara `manifest_software` spec 0009), selain itu null |
| preview, detail | `publisher` | namespace `metadata.id` |
| preview | `manifestSha256`, `versionsSha256`, `packageSha256` | `ValidatedPackage::key()`, `versions_sha256`, `package_sha256`; untuk paket tidak valid dihitung dari `PackageFiles` kalau terbaca, selain itu null |
| preview | `packageBytes`, `fileCount` | jumlah byte dan jumlah entri `PackageFiles` |
| preview, detail | `originPath` | `path` (preview) atau `provider_imports.origin_path` (detail), dengan awalan home diganti `~` oleh fungsi yang sama dengan `stackly_core::diagnostics` |
| preview | `quarantined`, `quarantineAgent` | `CodeSigning::quarantine(<path>/provider.yaml)`; error menjadi null |
| preview, detail | `permissions` | `permissions(&ValidatedPackage)` |
| preview | `existing` | `Registry::provider_import(id)` dan `approval_state`; `storedCopyValid` = entri itu di snapshot bukan `invalid` |
| preview, detail | `changes` | `permission_changes` atas `decided_permissions` dan `decided_revision` baris itu; kosong tanpa keputusan atau saat state bukan `pending` |
| preview, detail | `revocation` | `Catalog::revocation(key)`; di detail juga keputusan `revoked` user (`since` = `decided_at`) |
| preview | `verdict` | `importable` = tidak ada finding `error` (origin `package` atau `authoring`), `importIssues` kosong, dan batas import tidak terlewati; `reason` `package_invalid` atau `import_limit` |
| import | `expectedPackageSha256` | `ImportPreview.packageSha256` yang ditampilkan client |
| import | `imported_at`, `decided_at` | `Clock::now()` saat commit |
| import | baris sumber | `provider_source_by_location(Local, "imports")`, dibuat di commit yang sama kalau belum ada |
| approve | `expectedPackageSha256` | `ProviderDetail.local.packageSha256` yang ditampilkan client |
| approve | `decided_permissions`, `decided_revision` | dihitung daemon dari salinan store saat operasi, bukan dari client |
| gate | approval | `approval_state` atas `Registry::provider_import(id)` yang baru dibaca (entri lokal), `ProviderRevisionRecord.approval` yang baru dibaca (pinned), atau baris yang sama di dalam guard (`guard_trust`) |
| tampilan | approval | `approval_state(record)` dari snapshot catalog |
| gate | revocation catalog | `Catalog::revocation` dari snapshot yang dimuat |
| `provider.revoked` | `since`, `catalogRevision` | `RevokedInfo` (`decided_at`, `approved_at`, atau `Revocation.since` pada tengah malam UTC; `CatalogMeta.revision`) |
| pin entri lokal | `publisher`, `approval`, `approved_at`, `source_id` | `pin_revision` mendapat parameter `PinMeta { publisher, approval, approved_at }` (untuk entri catalog: `Stackly`, kosong, kosong seperti sekarang) dan `source_id` dari `ProviderImportRecord.source_id`; `SourceRecorder` catalog tidak dipakai untuk entri lokal |
| list | `approval`, `trust` | `approval_state`; `local` untuk `EntryOrigin::Local` |
| list | `localCount`, `revocations` | jumlah entri `Local` di snapshot; `Catalog` |
| list | `mark`, `tile` entri lokal | default di *Read model* |
| audit | detail | `set_audit_note` dari record dan `permission_changes` |
| event | `provider`, `change` | operasi yang memicu refresh |

### Key invariants

- Preview, list, dan detail tidak pernah menulis file, menjalankan proses, menyentuh jaringan, atau menyelesaikan secret, untuk paket lokal seperti untuk catalog bawaan.
- Tidak ada unduhan, exec check, init, atau start yang berasal dari entri lokal kecuali `approval_state` nya `approved` untuk persis `package_sha256` yang tersimpan, dan catalog tidak mencabutnya.
- Byte yang disetujui adalah byte yang dipakai: entri dibaca dari salinan store yang digest nya dicek saat dimuat, bukan dari folder asal.
- Gate tidak pernah bergantung pada snapshot untuk approval: ia membaca registry, dan gagal membaca berarti menolak.
- Setiap perubahan byte paket mengembalikan state ke `pending`, kecuali byte itu persis yang keputusannya tersimpan.
- Entri lokal tidak pernah ber `trust` `official`, tidak pernah `supported`, dan namespace `official`, `stackly`, dan `testing` tidak pernah bisa diimpor di luar test.
- Approval, revoke, remove, dan revocation catalog tidak pernah menghapus atau mengubah runtime, instance, data, atau folder pinned, dan tidak pernah menghalangi start instance yang sudah ada.
- Keputusan trust hanya berubah lewat operasi `approve_provider` dan `revoke_provider`, masing masing dengan baris audit di commit yang sama.
- Folder asal tidak pernah ditulis, dihapus, atau diubah atributnya.
- Tidak ada teks yang mengklaim sandbox, keamanan binary, publisher terverifikasi, atau revocation terkini.

### Security model

- **Siapa**: semua method `provider.*` tersedia untuk peer yang lolos handshake spec 0006 (user OS yang sama); tidak ada peran baru. Proses lain milik user yang sama bisa memanggil `provider.approve` sendiri; itu batas yang sama dengan seluruh Stackly (BR-012) dan pengerasannya milik fitur 71. Dialog izin adalah persetujuan pengguna di client, bukan batas keamanan terhadap proses user yang sama.
- **Ancaman utama**: paket jahat atau ceroboh dari luar (RISK-08). Pertahanan: loader dan validator yang sama dengan catalog (symlink, ukuran, UTF-8, field asing, anggaran YAML), primitive tertutup spec 0009, binary hanya lewat pipeline spec 0018 (HTTPS, digest dan ukuran dipin, extractor sendiri, signing policy, cek quarantine), gate approval sebelum unduhan pertama, dan approval yang mengikat seluruh byte.
- **Residual yang dinyatakan** (BR-020): sesudah disetujui, binary berjalan dengan hak user tanpa sandbox. Checksum di `versions.json` paket lokal ditulis oleh penulis paket itu sendiri, jadi hanya membuktikan unduhan cocok dengan yang ia tunjuk. Kalimat izin mengatakan ini.
- **Label palsu**: namespace cadangan ditolak; trust dihitung dari sumber entri, bukan dari isi paket; publisher selalu berlabel belum terverifikasi; compatibility record lokal diabaikan.
- **TOCTOU**: `expectedPackageSha256` di import dan approve, cek ulang di `commit_guarded`, `LockKey::Provider` per provider, dan untuk install serta create cek trust ulang sebelum exec check dan di guard commit, supaya revoke yang masuk di tengah operasi panjang tetap menang.
- **Runtime yang sudah masuk**: binary yang dipasang lewat provider lokal yang pernah disetujui tetap terpasang sesudah revoke atau remove, dan bisa dipakai instance provider lain ber `software` sama. Revocation menutup pintu masuk, bukan mencabut yang sudah di dalam; pengguna menghapusnya lewat remove runtime (fitur 55). Dialog revoke mengatakan runtime tetap terpasang.
- **OQ-009 di baseline**: trust root provider `official` adalah segel code signature bundle app (spec 0015, 0017); provider lokal dipercaya hanya lewat approval di Mac ini; tidak ada signing paket; revocation hanya dari catalog terpasang dan dari pengguna, setua tanggal terbit catalog, tanpa klaim terkini. Signing, trust root publik, dan revocation online ada di Deferred (public registry); OQ-009 tetap terbuka untuk itu.
- **Privasi**: `originPath` ditulis dengan home sebagai `~` di wire, audit, dan log. Paket provider tidak memuat secret; nama field secret di `permissions` bukan nilai.
- **Residual audit 2026-11 nomor 95** (`read_package` membuka file lewat path) berlaku juga untuk folder asal import; fitur 71 pemiliknya. Dampaknya dibatasi cek digest saat baca ulang salinan store.

### Configuration required

Tidak ada variabel environment baru. Tidak ada kredensial.

### Critical test scenarios

Paket fixture baru di `crates/core/tests/fixtures/packages/`: `acme-demo` (ID `acme/demo`, artefak `available` dengan URL `https://downloads.example.test/...`), `acme-demo-next` (revision dan `versions.json` berbeda), `acme-demo-template` (hanya isi template berbeda), dan dengan opsi test paket `testing/` yang sudah ada untuk skenario yang butuh proses nyata.

- Preview tanpa efek samping: preview `acme-demo` dengan `FakePlatform` dan `FakeFetcher`; mutasi, spawn, permintaan jaringan, generation registry, dan baris audit tetap; isi `ImportPreview` sama dengan fixture. Memverifikasi **AC-1**.
- Aturan: paket ber namespace `official`, `stackly`, `testing`; kategori `testing`; paket dengan port di init; URL `http`; 33 artefak; folder yang berupa symlink; path relatif; path dengan `..`; paket ke 33; total di atas 16 MiB; masing masing dengan issue atau error nya. `allow_testing_namespace` menerima paket `testing/`. Memverifikasi **AC-2**.
- Import: import `acme-demo` menulis store 0700 dan 0600, baris `provider_imports`, satu baris sumber `local`; entri tampil `pending`; import kedua byte sama tanpa operasi; folder diubah di antara preview dan import menghasilkan `provider.import_changed`; gagal tulis store (`FakeFilesystem` `NoSpace`) menghasilkan `provider.store_failed` tanpa baris dan tanpa folder sisa; delapan import bersamaan byte sama berakhir dengan satu baris. Folder asal sama byte dan atributnya sebelum dan sesudah. Memverifikasi **AC-3**.
- Gate tertutup: pada entri `pending`, `runtime.install`, `runtime.resolve`, `instance.create`, `instance.get_create_form` masing masing `provider.approval_required`, tanpa operasi, tanpa spawn, tanpa permintaan `FakeFetcher`, tanpa folder baru. Memverifikasi **AC-4**.
- Approval: approve dengan digest salah `provider.import_changed`; approve benar membuka gate dan `runtime.resolve` berhasil; approve kedua tanpa operasi; approve entri yang dicabut catalog `provider.revoked`. Memverifikasi **AC-5**.
- Perubahan: sesudah approve `acme-demo`, import `acme-demo-next` menjadi `pending` dengan `changes` `revision`, `downloads`; import `acme-demo-template` memberi `content`; paket yang hanya `versions.json` nya berbeda (digest artefak lain, `manifest_sha256` sama) menjadi `pending` dengan `downloads`; import ulang `acme-demo` kembali `approved`. Memverifikasi **AC-6**.
- Revoke: dengan paket `testing/` (opsi test) yang sudah punya runtime fixture dan satu instance, revoke lalu bandingkan registry dan tree; `instance.create` baru dan duplicate ditolak `provider.revoked` by `user`; instance lama `start` dan `stop` berhasil; revoke sesudah import revision baru (state `pending`, keputusan `approved`) tetap berhasil dan mencabut baris pinned revision lama; revoke tanpa approval apa pun `provider.not_approved`; approve lagi membuka gate dan mengembalikan `provider_revisions.approval` hanya untuk kunci yang tersimpan; revoke yang di commit di antara gate dan commit `create_instance` menggagalkan create lewat guard, dan revoke selama unduhan `install_runtime` menggagalkan install sebelum exec check (spawn tetap nol). Memverifikasi **AC-7**, **AC-4**.
- Revocation catalog: `CatalogBuilder` dengan entri `revoked` untuk revision pinned lama dan untuk `acme/demo`; create dari revision pinned ditolak `provider.revoked` by `catalog` dengan `catalogRevision` dan `since`; instance lama tetap start; approve `acme/demo` ditolak; `xtask catalog build` menolak 65 entri, `since` besok, entri ganda, dan entri yang cocok dengan revision provider nya sendiri; ledger revision lama tetap cocok saat `revoked` kosong; pindaian copy tidak menemukan klaim kesegaran. Memverifikasi **AC-8**.
- Remove: remove menghapus baris dan folder store, entri hilang, instance dan folder pinned tetap, `provider_revisions.approval` provider itu menjadi `revoked` dan duplicate ditolak sampai import dan approve ulang; urutan import A, B, A tidak menghapus folder A yang dirujuk lagi; remove ID catalog `provider.import_not_found` reason `not_local`. Memverifikasi **AC-9**.
- Read model: event `catalog.changed` terbit sekali per operasi dengan `change` yang benar; pembaca yang memegang snapshot lama tidak terpengaruh; restart daemon memuat entri lokal tanpa event; folder store yang dihapus menjadi entri `invalid` dengan finding `package`, dan yang diubah di luar Stackly menjadi `invalid` dengan `import_digest_mismatch`, keduanya tetap `local`, dengan entri lain tetap, dan import ulang byte yang sama memperbaikinya dengan keputusan utuh; refresh yang gagal (registry fake error) meninggalkan snapshot lama tetapi gate tetap menolak sesudah revoke; snapshot ber generation lebih tua tidak menimpa; recovery mode tanpa entri lokal; catalog penuh di bawah 1 MiB. Memverifikasi **AC-10**.
- Kejujuran: entri lokal dengan compatibility record `pass` tetap `unverified` di catalog dan di `runtime.resolve`, dan baris `artifacts` sesudah install dari entri itu tanpa record; `runtime.get` dan nama tampil runtime NATS tetap dari `official/nats` saat `acme/nats` ikut terdaftar; `trust` selalu `local`; test copy membandingkan kalimat izin dengan fixture dan memindai frasa terlarang. Memverifikasi **AC-11**.
- VAL-018: `settings.update` dengan kunci `trust`; manifest dengan `permissions.trusted: true` (`unknown_field`); daemon dengan `STACKLY_TRUST=1` dan `STACKLY_APPROVE_ALL=1`. Memverifikasi **AC-12**.
- Quarantine: `FakeCodeSigning` yang memberi `QuarantineInfo` membuat `quarantined` true dan import tetap berhasil; yang gagal membaca memberi null; `mutations()` tidak memuat operasi atribut. Test native menulis `com.apple.quarantine` ke salinan paket di `TestRoot`, mengimpornya, dan memeriksa atribut asal tetap ada serta salinan store tanpa atribut. Memverifikasi **AC-13**.
- CLI: golden teks dan JSON untuk `import --dry-run` (valid dan tidak valid), `import`, `import --approve --yes`, `approve`, `revoke`, `remove`, `list`, dan `inspect` entri lokal di setiap state; `approve` tanpa TTY tanpa `--yes` keluar 2 tanpa request mutasi; exit code per kode. Memverifikasi **AC-14**.
- GUI: test `ProviderImportModel` untuk setiap state; test view model panel detail per state approval; test bahwa `ApproveProviderSheet` mengirim digest yang ditampilkan; snapshot spec 0068 untuk scenario `catalog-import-preview`, `catalog-import-invalid`, `catalog-import-replaces`, `catalog-local-pending`, `catalog-local-changed`, `catalog-local-approved`, `catalog-local-revoked`, `catalog-local-catalog-revoked`, `catalog-approve-sheet`, `settings-sources-imported`, light dan dark. Memverifikasi **AC-15**, **AC-16**.
- Audit: setiap operasi menghasilkan satu baris dengan detail yang benar dan `~` untuk home; preview tanpa baris; `audit.list` menampilkannya. Memverifikasi **AC-17**.
- Kontrak: test katalog kode dan pemetaan per varian, fixture Rust dan Swift, `codegen --check`, test schema `CODED_COLUMNS`, test migrasi dari seed versi sebelumnya. Memverifikasi **AC-18**.
- Create dan pin: create dari entri lokal `approved` (paket `testing/` dengan opsi test) menulis `provider_revisions` dengan publisher, approval, dan sumber yang benar; dua entri lokal dengan `software` sama tanpa entri official memberi `instance.provider_ambiguous` dengan `candidates`; dengan entri official, form dari runtime tetap memilih official; form default dan form duplicate untuk entri `pending` ditolak. Memverifikasi **AC-19**.

Test yang membuktikan ID PRD memasang guard evidence `fixture` spec 0002. Evidence `native` yang dicatat `NOT_RUN` sampai dijalankan (langkah manual di `verify.md` saat build): import dari folder di Downloads oleh daemon LaunchAgent (izin Files and Folders macOS), folder hasil unduhan browser yang benar benar ber quarantine, alur GUI lengkap, dan perilaku dengan jaringan mati.

### Jawaban Follow-up spec lain

| Asal | Pertanyaan | Jawaban |
| --- | --- | --- |
| Spec 0017 | Namespace lain | apa pun selain `official`, `testing`, `stackly` (*Aturan import*) |
| Spec 0017 | Kind sumber `local` | satu baris `provider_sources` `local` dengan `location` `imports` (*Model data*) |
| Spec 0017 | Publisher dan trust paket lokal | publisher = namespace, selalu belum terverifikasi; `TrustLevel.local`; approval per entri |
| Spec 0017 | Default tile | *Read model* |
| Spec 0017 | Tombol Import Provider | *Layar* |
| Spec 0017 | Event `catalog.changed` | *API surface*; major protokol |
| Spec 0017 | Entri lokal di read model; `CatalogState` berubah saat berjalan | *Read model* |
| Spec 0009 | `init_port_issues` saat import | ditolak: finding `authoring` ber severity `error` dari `check_package` menolak import (K11) |
| Spec 0009 | `read_package` dipakai import | ya, lewat `check_package` spec 0036 dengan `NameCheck::Skip` |
| Spec 0009 | Peringatan hak user | kalimat izin di *Copy*, AC-11 |
| Spec 0016 | Langkah Catalog Trust | tanpa baris lokal atau community, tanpa trust per provider; hanya copy (K14) |
| Spec 0005 | `publisher`, `approval`, `ProviderSourceKind`, status | *Model data*; tidak ada status sumber baru |
| Spec 0020 | `provider` memilih di antara entri ber software sama | *Software yang sama* |
| Spec 0032 AC-13 | Baris Sources untuk import | *Layar*; `.communitySources` tetap tidak dibangun |
| Spec 0004 | BR-020 | AC-11, *Security model* |
| Spec 0003 | Quarantine paket lokal | AC-13 (K13) |

## Build plan

Urutan mengikuti pendekatan Journey: jalur "kontributor mengimpor paket, memeriksanya, menyetujuinya, lalu memasang dan membuat instance" dibangun lengkap dari daemon sampai setiap state gagalnya, lalu CLI, lalu GUI. Protokol naik major satu kali, di langkah 4, dengan seluruh kontrak fitur ini.

**Milestone 1: Paket lokal sampai catalog** (needs feature 36: spec 0036 step 1 (`provider::check::check_package` dan `Finding`) untuk langkah 2, 3, dan 5; spec 0036 step 3 (DTO finding `provider.validate`) untuk langkah 4)

1. Domain dan storage: `ProviderSourceKind::Local`, `TrustDecision`, `ProviderImportRecord`, `ImportDecision`, `approval_state`, empat `OperationKind` dan baris tabel resume nya, `LockKey::Provider`; migrasi `provider_imports`; baca dan `WriteOp` di port `Registry` dan `stackly-storage`; `CODED_COLUMNS`; `DirectoryLayout::provider_imports_dir` dan `provider_import_dir`; fake registry testkit. Memenuhi **AC-18**, **AC-3**.
2. Core `catalog::local`: `package_sha256`, `permissions`, `permission_changes`, `check_importable` dan aturan atas `PackageCheck`, batas import, dan `catalog::store::write_package` (dipisah dari `pin_revision` tanpa mengubah perilaku pin), dengan paket fixture `acme-*`. Memenuhi **AC-1**, **AC-2**, **AC-6**.
3. Core read model dan gate: `EntryOrigin` dan `InvalidEntry.local`, `Catalog::with_local`, tile default, `catalog::local::load` dengan `import_digest_mismatch`, `trusted_compatibility`, `catalog::trust` (`entry_trust`, `require_entry_trust`, `pinned_trust`, `guard_trust`), parameter `trust` dan reason `ApprovalRequired` serta `TrustRevoked` di `installability` dan `provider_installability` beserta ketiga pemanggilnya. Memenuhi **AC-4**, **AC-10**, **AC-11**.
4. Protokol: modul `provider`, lima method (`recovery refused`), tambahan DTO catalog, enum terbuka, `EventKind::CatalogChanged`, kode `provider.*` dengan hint, `details.candidates`, fixture `protocols/fixtures/` (termasuk `provider/approval-statement.txt`), kenaikan major `ProtocolVersion::CURRENT`, `cargo xtask codegen`, `cargo xtask fuzz seed`. Memenuhi **AC-18**.
5. `stacklyd`: `RwLock` catalog dengan generation, port `CatalogRefresher`, `refresh_local` saat start dan sebagai langkah terakhir operasi, event, handler `provider.preview_import`, `provider.import`, dan `provider.remove` dengan operasi, kompensasi folder store, dan `set_audit_note`; pemetaan DTO dan error dengan test per varian; kesehatan subsystem; test anggaran frame; test quarantine dengan `FakeCodeSigning` dan test native atribut. Memenuhi **AC-1**, **AC-3**, **AC-9**, **AC-10**, **AC-13**, **AC-17**.

**Milestone 2: Approval, revocation, dan gate** (after milestone 1)

6. Operasi `approve_provider` dan `revoke_provider` dengan `commit_guarded`, `SetProviderRevisionApproval` dan `RevokeProviderApprovals` (juga di `remove_provider_import`), audit, dan test VAL-018. Memenuhi **AC-5**, **AC-6**, **AC-7**, **AC-12**, **AC-17**.
7. Gate di setiap titik tabel *Gate trust*: `select()` runtime, handler form sesudah `select_provider`, `resolve_package` (cabang entri dan `find_pinned`), `gather_duplicate`, guard commit `create_instance`, serta exec check dan commit `install_runtime`; `PackageSource::LocalImport`; `PinMeta` dan `approved_at` di record pin; aturan *Software yang sama* di `select_provider`, `catalog_match`, dan `DisplayNames`; `InstallInput` tanpa compatibility record lokal; dan test bahwa penolakan tidak membuat operasi, spawn, atau unduhan. Memenuhi **AC-4**, **AC-7**, **AC-19**.
8. Revocation catalog: `[[revoked]]` di `xtask/src/catalog/source.rs`, `revoked` di `core::catalog::index` dengan `skip_serializing_if`, aturan build dan loader, `Catalog::revocation`, penyambungan ke `catalog::trust`, `ListProvidersReply.revocations`, dan `CatalogBuilder` dengan revocation. Memenuhi **AC-8**.

**Milestone 3: `stackly provider import`, `approve`, `revoke`, `remove`** (after langkah 4 untuk golden dari fixture; binary test needs milestone 2; after spec 0036 step 4 (subcommand `validate`), supaya kedua fitur tidak mengubah `provider/mod.rs` bersamaan)

9. Subcommand, konfirmasi, kolom `TRUST`, bagian inspect entri lokal, golden teks dan JSON, exit code, dan binary test terhadap `stacklyd`. Memenuhi **AC-14**, **AC-11**.

**Milestone 4: Import dan approval di app** (after langkah 4; evidence manual needs milestone 2)

10. `ProviderTrustCopy`, tambahan `CatalogAppearance` dan tabel di `apps/macos/design.md`, `ProviderPermissionsList`, `LocalProviderSection`, badge dan meta kartu, `CatalogStore` berlangganan `catalog.changed`, test copy terhadap fixture kalimat izin dan frasa terlarang. Memenuhi **AC-11**, **AC-15**.
11. `ProviderImportModel`, tombol toolbar, panel folder, `ImportProviderSheet`, `ApproveProviderSheet`, dialog revoke dan remove, kartu umpan balik, dan scenario snapshot spec 0068. Memenuhi **AC-15**.
12. Settings › Sources (grup, tombol, hitungan revocation, `SettingsAvailability`), copy onboarding di `OnboardingCopy.swift` dan test copy nya, scenario `settings-sources-imported`. Memenuhi **AC-16**.
13. Evidence: guard `fixture` untuk ID PRD di *Trace PRD*, record `native` `NOT_RUN` untuk empat langkah manual, dan bagian `### UI / manual` di `verify.md`. Memenuhi **AC-8**, **AC-13**, **AC-15**.

**Sesi paralel yang aman**: sesudah langkah 1 sampai 4 masuk, tiga sesi bisa berjalan tanpa file beririsan: daemon dan core (langkah 5 sampai 8, `crates/core`, `crates/storage`, `apps/daemon`, `xtask/src/catalog`), CLI (langkah 9, `apps/cli`, golden dari fixture langkah 4; binary test menunggu langkah 7), dan SwiftUI (langkah 10 sampai 12, `apps/macos`, memakai fixture langkah 4). Langkah 1 sampai 4 berurutan di satu sesi karena berbagi `crates/domain`, `crates/core/src/catalog`, dan `crates/protocol`. Langkah 13 menunggu semuanya.

## Consequences

**Positive**:

- Kontributor bisa mencoba provider di Stackly sungguhan tanpa membangun ulang app, lewat loader, gate, dan pipeline install yang sama dengan provider resmi.
- Aturan trust satu kalimat: yang berjalan adalah persis byte yang kamu setujui. AC-032 terpenuhi tanpa daftar "perubahan material".
- Revocation dan approval tertunda tidak pernah mengunci atau menghapus data.
- Catalog kini bisa berubah saat daemon berjalan, lewat satu jalur (`refresh_local`) dan satu event, yang bisa dipakai ulang fitur 61.

**Negative / tradeoffs**:

- Kenaikan major protokol untuk `catalog.changed`; app, CLI, dan daemon harus dari build yang sama (sudah begitu sejak 3.0).
- Setiap edit paket butuh import dan approval ulang; iterasi kontributor di app nyata lebih lambat daripada harness spec 0036.
- Daemon membaca folder pilihan pengguna, jadi macOS bisa menolak folder terlindung; pengguna harus memindahkan folder atau memberi izin.
- Provider yang sudah dicabut masih bisa berjalan di instance lama. Ini disengaja dan dinyatakan di dialog, tetapi bukan penghentian.
- Paket untuk namespace `official` tidak bisa diimpor apa adanya; jalur coba nya catalog development.
- Proses lain milik user yang sama bisa memanggil `provider.approve`; approval bukan batas keamanan terhadapnya (fitur 71).
- Satu baris menyimpan satu keputusan, jadi riwayat keputusan lama hanya ada di audit (dengan retensi spec 0035).
- Runtime yang masuk lewat provider lokal yang kemudian dicabut tetap terpasang dan bisa dipakai provider lain ber `software` sama.
- Remove juga mencabut approval, jadi duplicate instance dari provider yang sudah di remove butuh import dan approve ulang.

**Neutral**:

- Satu migrasi, satu tabel baru, empat jenis operasi, satu `LockKey`, satu `IssueCode`, dan satu folder data baru.
- `catalog.json` mendapat field opsional `revoked`; ledger revision lama tetap sah.
- Sumber community, signing, dan revocation online tetap Deferred; `TrustLevel` belum punya nilai untuk reviewed community.

### Update spec lain

Usulan untuk lead (architect spec ini tidak mengubah spec lain):

- **Spec 0017**: *Batas dengan spec lain* dan Follow-up fitur 37 dijawab di sini; `CatalogState` kini berubah saat berjalan (invariant "Catalog tidak berubah selama satu run daemon" menjadi "catalog bawaan tidak berubah; entri lokal berubah lewat operasi provider"); tambahan `revoked` di `catalog.toml`, `catalog.json`, `CatalogContent`, dan aturan build; `TrustLevel.local`, `SupportReason` baru, `ProviderSummary.approval`, `ProviderDetail.local` dan `revocation`, `CatalogInfo.localCount`, `ListProvidersReply.revocations`; baris baru *Aturan status*; kolom `TRUST` di CLI; batas 32 entri lokal di *Anggaran frame*.
- **Spec 0009**: `IssueCode` `import_digest_mismatch`; kode error `provider.*` baru di area milik spec itu; Follow-up `init_port_issues` dijawab (ditolak).
- **Spec 0005**: tabel `provider_imports`, `TrustDecision`, `ProviderSourceKind::Local`, `ProviderRevisionRecord.approved_at` dan `approval` bertipe, baca dan `WriteOp` baru, dua kolom baru di `CODED_COLUMNS`.
- **Spec 0004**: `provider_imports_dir`, `provider_import_dir`; BR-020 kini dipenuhi fitur 37 (dan 66).
- **Spec 0007**: empat baris baru di *Tabel resume per jenis operasi* (tidak resumable, `Compensate`, resource `Manager`); `LockKey::Provider`.
- **Spec 0020**: `PackageSource::LocalImport`, `ProviderProblem` dan `FormError` `ApprovalRequired` dan `Revoked`, gate di handler form, `resolve_package`, dan `gather_duplicate`, cek trust di guard commit create, `PinMeta`, `details.candidates`, dan aturan *Software yang sama* di `select_provider`.
- **Spec 0018**: gate trust di `select()` sebelum resolusi, cek ulang sebelum exec check dan di commit install, `trusted_compatibility` di `support_status` dan `InstallInput`, serta urutan `catalog_match` dan `DisplayNames`.
- **Spec 0016**: subjudul dan satu kalimat baru di tabel *Copy*; Follow-up fitur 37 dijawab.
- **Spec 0032**: grup "Imported providers" dan tombol di tabel *Baris Settings*; `.communitySources` berpemilik Deferred.
- **`docs/scope/ui-macos.md`**: sheet import, sheet approval, dan seksi Permissions tidak ada di mockup dan ditetapkan spec ini.

## Follow-up

- [x] *Update spec lain*, butir spec 0017 dan spec 0007: diterapkan di spec 0017 (2026-11-11, *Tambahan fitur 37*) dan di spec 0007 (2026-11-11, baris `LockKey`, butir *Operasi provider*, dan empat baris tabel resume). Butir lain di *Update spec lain* (spec 0009, 0005, 0004, 0020, 0018, 0016, 0032, dan `ui-macos.md`) menunggu `/sync` akhir wave.
- [ ] Begitu spec 0036 diterima: cocokkan *Kontrak dari spec 0036* dengan teks akhirnya (nama DTO finding, dan nomor langkah 1, 3, dan 4 yang dipakai prasyarat milestone 1 dan 3).
- [ ] Lead: spec 0036 menaikkan minor protokol (`provider.validate`) dan spec ini menaikkan major; tetapkan urutan nomornya saat build.
- [ ] Fitur 59: tampilkan diff `Permissions` (fungsi `permission_changes` spec ini) saat instance pindah provider revision, termasuk untuk provider `official`.
- [ ] Fitur 34 (doctor): pertimbangkan temuan `provider_revoked` untuk instance yang provider revision nya dicabut, dengan fix berupa navigasi ke Catalog.
- [ ] Fitur 55: garbage collection folder pinned dan sisa folder `provider-imports/` yang gagal dihapus; pertimbangkan mencatat provider yang memasang sebuah runtime supaya runtime dari provider yang dicabut bisa ditandai.
- [ ] Fitur 61: update app yang membawa catalog baru memanggil jalur refresh yang sama dan menerbitkan `catalog.changed`.
- [ ] Fitur 71: pembatasan pemanggil `provider.approve` terhadap proses lain milik user yang sama.
- [ ] Deferred (public registry): sumber community, signing paket, trust root publik, revocation online, dan nilai `TrustLevel` untuk reviewed community; menutup OQ-009.
- [ ] Import dari arsip (`tar.gz`, `zip`) dan artefak dari file lokal (`file:`) tidak ada di baseline; nilai lagi kalau kontributor memintanya.
- [ ] Spec 0038: bagian "Try your provider locally" merujuk *CLI* dan kalimat izin di *Copy* spec ini.

## Rationale

Alasan dan opsi yang ditimbang: lihat [rationale.md](rationale.md).
