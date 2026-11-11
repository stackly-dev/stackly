# 0037. Import provider lokal dan trust: alasan

Catatan keputusan untuk [index.md](index.md). `/develop` tidak membaca file ini.

## Context

Sampai fitur 35, satu satunya provider yang bisa dipakai Stackly adalah provider `official` yang ikut tersegel di dalam app (spec 0017). Kontributor JOURNEY-07 yang menulis provider baru bisa memvalidasi dan mengujinya lewat harness fitur 36 di test root sementara, tetapi belum punya cara mencobanya di Stackly sungguhan tanpa membangun ulang app. REQ-033 meminta sumber kedua di baseline: folder atau paket lokal yang diimpor secara eksplisit, dengan syarat browsing catalog tidak pernah menjalankan kode provider.

Begitu paket dari luar bundle bisa masuk, trust menjadi keputusan yang nyata. Provider Stackly memang deklaratif (primitive tertutup, tanpa shell, tanpa hook; spec 0009), tetapi manifest tetap menentukan binary mana yang diunduh dan dijalankan, dengan argumen apa, dan binary itu berjalan dengan hak user tanpa sandbox (BR-020, PRD 21.4). Checksum hanya membuktikan byte nya tidak berubah, bukan bahwa programnya aman. REQ-032 karena itu meminta approval yang terikat ke identitas paket, revision, digest, sumber atau publisher, dan capability, yang tidak ikut terbawa saat isi paket berubah (AC-032), dan revocation yang memblokir instalasi baru tanpa menghapus data (AC-072). VAL-018 menutup jalan pintas: trust tidak boleh berubah lewat settings atau variabel environment.

Batas yang sudah ada di repo membentuk ruang geraknya:

- Loader paket spec 0009 hanya membaca empat jenis file teks, menolak symlink dan entri asing, dan `manifest_sha256` sengaja tidak mencakup `versions.json`. Padahal `versions.json` yang menentukan URL dan digest binary, jadi perubahan "executable berbeda" bisa terjadi tanpa mengubah `manifest_sha256`.
- `CatalogState` spec 0017 tidak berubah selama daemon berjalan, catalog hanya menerima namespace `official`, dan `TrustLevel` hanya `official` dan `development`. Kolom `publisher`, `approval`, dan `approved_at` di `provider_revisions` (spec 0005) sudah disediakan tetapi belum dipakai.
- Runtime dipasang hanya lewat pipeline spec 0018 (HTTPS, digest dan ukuran yang dipin, extractor sendiri, kebijakan signing, cek quarantine, `check_launchable`). Tidak ada jalur lain yang membuat binary bisa dijalankan.
- Setiap mutasi adalah operasi `OperationEngine` dengan baris audit di commit yang sama (spec 0007 dan 0035). `EventKind` tertutup, jadi event baru adalah kenaikan major protokol (spec 0030).
- OQ-009 (trust root, signing, kesegaran revocation) belum diputuskan pemiliknya. Default PRD: bundled official ditambah import lokal eksplisit, tanpa script bebas. Tidak ada kunci signing, tidak ada registry online, dan tidak ada akun Apple Developer (OQ-012).
- Quarantine macOS (spec 0003 dan 0018) tidak pernah dihapus atau dimatikan oleh Stackly, dan PRD 23 melarang instruksi untuk mematikannya secara global.

Kalau ini tidak diputuskan, kontributor tidak punya jalur jujur untuk mencoba providernya, dan lima provider resmi berikutnya (scope 07) tidak dibangun lewat jalur yang sama dengan kontributor. Kalau diputuskan dengan longgar, satu folder yang salah pilih bisa menjalankan binary sembarang dengan label yang terlihat resmi.

## Options considered

### K1: bentuk yang diimpor

- **Opsi 1 (dipilih): folder paket saja.** Bentuk yang sama dengan `providers/<nama>/` di repo dan dengan catalog bawaan, dibaca `read_package` yang sudah ada. Pro: tidak ada parser arsip baru di jalur masukan yang tidak dipercaya; satu loader untuk semua sumber. Kontra: paket yang dibagikan sebagai `.zip` harus dibuka dulu oleh pengguna (Finder melakukannya, dan hasilnya membawa quarantine yang ditangani K13).
- **Opsi 2: folder dan arsip `tar.gz` atau `zip`.** Extractor aman spec 0018 dipakai untuk membuka arsip ke staging. Pro: satu file mudah dibagikan. Kontra: extractor itu dirancang untuk artefak runtime yang digest nya sudah dipin; di sini arsipnya belum dipercaya sama sekali, jadi permukaan serangan parser bertambah persis di titik paling lemah, dan butuh aturan staging serta pembersihan baru.
- **Opsi 3: arsip saja.** Kontra: kontributor yang sedang mengedit folder harus mengemas ulang setiap kali.

### K2: siapa yang membaca folder

- **Opsi 1 (dipilih): daemon membaca path absolut yang dikirim client.** Preseden: `storage.set_location` (spec 0032) dan `diagnostics.export_bundle` (spec 0034). Pro: satu jalur baca (`Filesystem` port, `read_package`), CLI dan GUI identik, byte yang divalidasi adalah byte yang disimpan. Kontra: daemon bisa ditolak macOS saat membaca folder terlindung (Desktop, Documents, Downloads); ditangani dengan kode `provider.import_unreadable` dan hint, dan dicek manual.
- **Opsi 2: client membaca dan mengirim byte.** Pro: memakai izin folder milik app atau Terminal. Kontra: paket boleh 4 MiB sedangkan frame kontrol 1 MiB, jadi butuh protokol potongan baru; dua implementasi baca (Rust CLI dan Swift) untuk masukan yang tidak dipercaya.

### K3: salin atau rujuk di tempat

- **Opsi 1 (dipilih): salin ke store milik Stackly yang dialamati isi.** Byte yang disetujui adalah byte yang dipakai; folder asal boleh berubah, pindah, atau hilang. Kontra: kontributor harus mengimpor ulang setelah mengedit (satu perintah).
- **Opsi 2: rujuk folder asal, baca ulang setiap start.** Pro: edit langsung terlihat. Kontra: isi bisa berubah sesudah approval, jadi setiap pemakaian harus memeriksa ulang digest; daemon saat login bisa gagal membaca folder terlindung atau volume yang dicabut; "apa yang saya setujui" tidak lagi punya jawaban tetap.

### K4: namespace dan publisher

- **Opsi 1 (dipilih): namespace apa pun selain yang dicadangkan (`official`, `testing`, `stackly`); publisher adalah namespace itu, selalu berlabel belum terverifikasi.** Pro: tanpa perubahan schema manifest; trust melekat pada sumber, bukan pada nama, jadi paket lokal tidak pernah bisa tampil `official`. Kontra: paket yang ditulis untuk masuk repo resmi (`official/redis`) harus diimpor dengan ID lain, atau dicoba lewat catalog development (`STACKLY_CATALOG_DIR`).
- **Opsi 2: wajib namespace `local/`.** Pro: paling sederhana dibaca. Kontra: semua paket lokal berbagi satu "publisher" yang tidak berarti, dan ID harus diubah lagi saat paket dibagikan.
- **Opsi 3: field manifest baru `metadata.publisher`.** Pro: nama publisher yang enak dibaca. Kontra: klaim yang tidak bisa diverifikasi tanpa signing (OQ-009), menambah schema milik spec 0009 dan kebijakan versi spec 0036, dan mengubah `manifest_sha256` semua paket yang memakainya.

### K5: apa yang diikat approval

- **Opsi 1 (dipilih): seluruh byte paket, lewat `packageSha256` yang mencakup `versions.json`.** Setiap perubahan butuh approval ulang; ringkasan izin dan daftar perubahan hanya untuk menjelaskan. Pro: aturan satu kalimat, tidak ada celah "perubahan yang dianggap tidak material", dan menutup lubang `versions.json` yang tidak dicakup `manifest_sha256`. Kontra: mengubah satu komentar template pun butuh approval ulang.
- **Opsi 2: `manifest_sha256` saja.** Kontra: URL dan digest binary bisa diganti tanpa approval ulang, bertentangan langsung dengan AC-032.
- **Opsi 3: ringkasan izin; approval terbawa kalau ringkasan sama.** Pro: iterasi lebih mulus. Kontra: template config dan argumen bisa mengubah perilaku server tanpa mengubah ringkasan, jadi Stackly harus memutuskan apa yang "material", dan keputusan itu sendiri menjadi permukaan serangan.

### K6: import dan approval

- **Opsi 1 (dipilih): dua langkah. Import membuat entri `pending` yang bisa diperiksa di catalog; approval adalah aksi terpisah dengan dialog izin.** Memenuhi "bisa diperiksa, tetapi tidak berjalan" (AC-033) dengan state yang nyata dan bisa diuji. CLI boleh merangkai keduanya dengan `--approve`, tetap lewat dua method yang sama. Kontra: satu klik lebih banyak.
- **Opsi 2: import selalu sekaligus approval.** Kontra: tidak ada state "diimpor tetapi belum dipercaya", jadi pemeriksaan hanya bisa terjadi di sheet sementara.
- **Opsi 3: approval diam diam saat import.** Ditolak: melanggar REQ-032.

### K7: apa yang diblokir

- **Opsi 1 (dipilih): `pending`, `revoked`, dan revocation catalog memblokir install runtime dan pembuatan instance baru (termasuk duplicate dan create dari revision pinned). Instance yang sudah ada tetap bisa start, stop, dan diubah.** Sesuai REQ-032 dan AC-072 ("memblokir instalasi baru tanpa menghapus data"), dan pengguna tetap bisa menyalakan instance untuk mengambil datanya. Kontra: binary dari provider yang sudah dicabut masih bisa berjalan di instance lama; ini dinyatakan terang di dialog revoke.
- **Opsi 2: revoke juga memblokir start.** Pro: lebih ketat. Kontra: data pengguna terkunci di balik keputusan trust, dan revocation dari catalog yang salah bisa mematikan stack kerja tanpa jalan keluar offline; bertentangan dengan PRD 21.4.

### K8: revocation dari catalog terpasang

- **Opsi 1 (dipilih): daftar `revoked` di `catalog.json` bawaan, dievaluasi dari catalog yang dimuat setiap kali gate dipanggil, tidak disalin ke registry.** Pro: tidak ada mutasi saat start, tidak ada state yang bisa basi, dan kesegarannya persis sama dengan tanggal terbit catalog yang sudah selalu ditampilkan. Kontra: jejaknya ada di catalog (revision dan tanggal), bukan baris audit.
- **Opsi 2: daemon menyalin revocation ke `provider_revisions.approval` saat start.** Pro: satu tempat membaca status. Kontra: mutasi di luar operasi pengguna, perlu operasi daemon dan audit sendiri, dan catalog yang kemudian mencabut revocation meninggalkan state lama.
- **Opsi 3: tanpa revocation catalog; hanya artefak `blocked`.** Kontra: tidak bisa mencabut provider revision lama yang sudah dipin, dan Done when fitur ini menyebut revocation dari catalog terpasang.

### K9: model data

- **Opsi 1 (dipilih): tabel baru `provider_imports` (satu baris per provider ID yang diimpor) dan satu baris `provider_sources` ber kind `local`.** Pro: state import punya rumah sendiri, `provider_revisions` tetap immutable kecuali kolom approval, dan folder asal hanya informasi. Kontra: satu migrasi.
- **Opsi 2: satu baris sumber per folder asal, state di `provider_revisions`.** Kontra: `provider_revisions` baru ditulis saat pin (fitur 20), jadi entri `pending` tidak punya baris; `versions_sha256` tidak punya kolom; dan path folder menjadi identitas padahal bukan.
- **Opsi 3: file JSON di samping store.** Kontra: di luar transaksi registry dan `commit_guarded`, tanpa audit di commit yang sama.

### K10: masuk ke read model

- **Opsi 1 (dipilih): snapshot gabungan (catalog bawaan yang tetap ditambah entri lokal) yang diganti atomik setelah setiap operasi provider, plus event `catalog.changed`.** Pro: `catalog.list_providers`, gate install, dan resolusi create tetap membaca satu tempat; `CatalogStore` spec 0017 sudah menyiapkan kind `catalog.`. Kontra: `EventKind` tertutup, jadi ini kenaikan major protokol.
- **Opsi 2: entri lokal baru terlihat setelah restart daemon.** Kontra: import yang tidak terlihat sampai restart bukan alur yang bisa dipakai.
- **Opsi 3: client memuat ulang catalog saat melihat event operasi jenis provider selesai.** Pro: tanpa major. Kontra: setiap client harus tahu daftar jenis operasi yang mengubah catalog, persis jenis tabel di client yang dilarang spec 0035, dan client yang berlangganan hanya `catalog.` tidak pernah tahu.

### K11: `init_port_issues` saat import

- **Opsi 1 (dipilih): tolak.** `check_package` spec 0036 melaporkannya sebagai finding `authoring` ber severity `error`, jadi `cargo xtask provider check` dan `stackly provider validate` sudah gagal untuk paket itu; import mengikuti hasil yang sama. Paket baru tidak punya alasan warisan seperti salinan pinned lama. Kontra: paket lama yang masih memakai port di init harus diperbaiki dulu.
- **Opsi 2: peringatkan saja (rekomendasi `arc-f36-spec`: tolak hanya yang tidak `loadable`, tampilkan finding `authoring` sebagai peringatan sebelum approval).** Pro: import menerima apa pun yang bisa dimuat. Kontra: validator berkata gagal sedangkan import berkata boleh, dan paket lolos import lalu create dengan port `Auto` ditolak belakangan di preflight spec 0020, jauh dari tempat penulis bisa memperbaikinya.

### K12: compatibility record di paket lokal

- **Opsi 1 (dipilih): diabaikan; sel entri lokal paling tinggi `unverified`.** `supported` berarti diuji oleh project (BR-025); record di paket lokal hanya klaim penulisnya. Kontra: kontributor tidak melihat label Tested untuk hasil ujinya sendiri.
- **Opsi 2: dipakai apa adanya.** Kontra: paket lokal bisa menampilkan Tested tanpa evidence.

### K13: quarantine pada paket lokal

- **Opsi 1 (dipilih): hanya dibaca dan dilaporkan.** Daemon membaca byte, menulis salinan baru tanpa atribut, tidak pernah menyentuh atribut di folder asal, dan preview menyebut bahwa folder itu diunduh dari internet. Binary tetap hanya datang lewat pipeline spec 0018 dengan cek quarantine nya sendiri. Kontra: tidak ada.
- **Opsi 2: tolak folder ber quarantine.** Kontra: mendorong pengguna menghapus atribut itu sendiri, persis yang dilarang PRD 23.
- **Opsi 3: hapus atribut saat import.** Ditolak: melanggar spec 0003 AC-8.

### K14 sampai K21: keputusan kecil

- **K14 Onboarding**: tidak ada baris atau toggle baru di langkah Catalog Trust; hanya subjudul yang diperbaiki supaya tetap benar setelah entri lokal yang belum disetujui bisa tampil. Alternatif: baris "Local providers" di onboarding, ditolak karena saat onboarding pertama daftar itu selalu kosong dan trust per provider punya rumah di Catalog.
- **K15 Settings Sources**: grup "Imported providers" yang hanya menampilkan dan menavigasi, plus tombol Import Provider…; aksi approve, revoke, dan remove hanya di panel detail Catalog. Alternatif: aksi lengkap di Settings, ditolak karena dua tempat untuk aksi berisiko.
- **K16 OQ-009 di baseline**: trust root provider official adalah segel bundle app; provider lokal dipercaya hanya lewat approval eksplisit di Mac ini; tidak ada signing; kesegaran revocation sama dengan tanggal terbit catalog terpasang dan tidak pernah diklaim terkini. OQ-009 tetap terbuka untuk public catalog (Deferred).
- **K17 Software yang sama di beberapa entri**: `provider` (ID lengkap) yang menentukan; tanpa itu entri `official` menang, lalu satu satunya entri lokal yang `approved`; selain itu `instance.provider_ambiguous`. Alternatif: entri terakhir diimpor menang, ditolak karena diam diam mengganti provider resmi.
- **K18 Tile default**: mark dua karakter pertama `[A-Za-z0-9]` dari nama tampil, tile `gray`. Alternatif: warna dari hash ID, ditolak karena warna tile dipakai sebagai identitas provider resmi.
- **K19 Batas**: 32 provider impor dan 16 MiB total, supaya balasan list tetap di bawah 1 MiB dan memori daemon tetap di anggaran NFR-003.
- **K20 Kata kerja**: `approve` dan `revoke` di wire, CLI, dan GUI, mengikuti kata scope ("approval"). Alternatif: "trust" di GUI, ditolak supaya dokumen fitur 38 memakai satu kata.
- **K21 Remove**: termasuk di fitur ini, karena tanpa itu paket coba coba menetap di catalog selamanya. Remove tidak menyentuh instance, runtime, salinan pinned, atau data, tetapi juga mencabut approval revision pinned provider itu. Alternatif: remove membiarkan approval pinned, ditolak karena sesudah entri hilang tidak ada lagi tempat untuk mencabutnya (temuan cross check 3).
- **K22 Sumber kebenaran gate**: gate membaca baris `provider_imports` dari registry setiap kali, dan operasi panjang memeriksa ulang di guard commit dan sebelum eksekusi pertama; snapshot catalog hanya untuk tampilan. Alternatif: gate membaca snapshot, ditolak karena refresh yang gagal atau terlambat membuat gate terbuka sesudah revoke (temuan cross check 2 dan 7). Alternatif lain: install dan create mengambil lock provider, ditolak karena revoke jadi menunggu unduhan.
- **K23 Cakupan revoke**: seluruh provider ID (keputusan tersimpan dan setiap revision pinned yang `approved`), dan boleh selama ada approval. Alternatif: hanya kunci yang sedang tersimpan, ditolak karena revision pinned lama tetap bisa dipakai membuat instance sesudah "revoke" (temuan cross check 3).
- **K24 Resource operasi**: `Manager` dengan `LockKey::Provider`, karena kolom resource operasi dan audit hanya menerima UUID dan provider tidak punya UUID. Alternatif: jenis resource `provider` baru dengan migrasi pembangunan ulang tabel `operations` dan `audit_events`, ditolak karena mahal untuk empat operasi singkat (temuan cross check 1).
- **K25 Runtime yang dipasang lewat provider lokal**: tetap runtime biasa sesudah revoke atau remove, dan bisa dipakai provider lain ber software sama; dinyatakan sebagai residual. Alternatif: mencatat provider pemasang di baris runtime dan memblokirnya, ditolak untuk baseline karena mengubah model runtime spec 0018 dan PRD meminta revocation tidak menghapus yang sudah ada; dicatat di Follow-up fitur 55.

## Rationale

Dua gaya menarik desain ini ke arah yang sama: paket lokal adalah masukan yang sama sekali tidak dipercaya, dan Stackly tidak punya signing untuk memverifikasi siapa pun (OQ-009). Karena itu semua pilihan mengambil bentuk yang paling sedikit memberi ruang tafsir. Yang diimpor adalah folder yang dibaca loader yang sudah difuzz, bukan arsip baru (K1). Daemon menyalin byte nya ke store sendiri sehingga "yang disetujui" punya arti tetap (K3). Approval mengikat seluruh byte, termasuk `versions.json` yang lolos dari `manifest_sha256`, sehingga AC-032 terpenuhi tanpa Stackly harus menilai perubahan mana yang material (K5). Publisher adalah namespace yang selalu berlabel belum terverifikasi, karena hanya itu yang jujur tanpa signing (K4, K16).

Sisi lain dari REQ-032 adalah tidak merugikan pengguna: revocation dan approval yang tertunda hanya menutup pintu masuk kode baru (install dan create), tidak pernah menghapus atau mengunci data (K7), dan revocation dari catalog tidak disimpan sehingga tidak ada yang bisa basi atau diklaim lebih segar dari tanggal catalog (K8). Preview tidak menjalankan apa pun karena loader spec 0009 memang tidak bisa, dan binary hanya bisa masuk lewat pipeline spec 0018, jadi quarantine tidak perlu aturan baru selain "jangan disentuh" (K13).

Biaya terbesar yang diterima adalah kenaikan major protokol untuk `catalog.changed` (K10). Itu dipilih karena alternatifnya memindahkan pengetahuan tentang jenis operasi ke setiap client, dan karena app, CLI, dan daemon selalu dirilis bersama di satu bundle, sehingga major baru tidak memutus siapa pun di lapangan.

## Koordinasi dengan sesi paralel

- `arc-f36-spec` (spec 0036): kontrak validator diambil dari catatan bersama `0036-kontrak-untuk-0037-dan-0038.md` (2026-11-11): `check_package`, `PackageCheck`, `Finding` dengan severity dan origin, method `provider.validate`, tanpa `IssueCode` baru, tanpa field versi baru di manifest. Harness hanya menerima namespace `official` dan `testing`, jadi paket ber namespace lain hanya bisa dicoba lewat import spec ini. Satu beda pendapat tercatat di K11.
- `arc-f38-spec` (spec 0038): menerima kontrak alur import, perintah CLI, istilah trust, dan kalimat izin lewat catatan bersama `0037-kontrak-untuk-0038.md`. Spec ini tidak menulis dokumen publik.

## Cross check

Cross check dijalankan model lain (Opus, read only) pada 2026-11-11 atas draf pertama, dengan akses baca ke repo. Putusannya: inti trust benar (approval mengikat digest seluruh paket, daemon membaca dan menyalin sendiri, gate sebelum unduhan pertama, namespace cadangan ditolak), tetapi belum siap dibangun sampai lima celah berat ditutup. Semua 16 temuan diterapkan dengan perbaikan yang direkomendasikan (user mendelegasikan keputusan ke opsi rekomendasi; daftar ini dikirim ke lead bersama permintaan accept):

1. Kolom resource operasi dan audit hanya menerima UUID, jadi `ResourceRef` kind `provider` tidak bisa disimpan. Perbaikan: resource `Manager` dan `LockKey::Provider` (K24).
2. Gate membaca snapshot, jadi terbuka saat refresh gagal atau terlambat. Perbaikan: gate membaca registry; snapshot hanya tampilan dan penggantiannya monoton menurut generation (K22).
3. Revoke hanya mengenai kunci yang tersimpan, dan remove meninggalkan approval pinned. Perbaikan: revoke seluruh provider ID, boleh selama ada approval; remove ikut mencabut (K23, K21).
4. Form default dan duplicate tidak lewat `resolve_package`, dan `select_provider` menjadi ambigu begitu dua entri ber software sama. Perbaikan: titik gate disebut per fungsi; aturan official lebih dulu ditaruh di `select_provider`.
5. Entri lokal bisa memberi nama tampil dan status "Tested" ke runtime resmi lewat `catalog_match`, `DisplayNames`, `support_status`, dan record yang dibekukan ke baris `artifacts`. Perbaikan: `trusted_compatibility` dan urutan entri catalog lebih dulu.
6. Runtime bersama tidak diputuskan. Perbaikan: gate menolak sebelum `install_precheck`; runtime dari provider yang dicabut dinyatakan residual (K25).
7. Revoke bisa masuk di tengah install atau create. Perbaikan: cek ulang di guard commit dan sebelum exec check.
8. Approval pinned per `manifest_sha256`, dan penulis store tidak sama dengan pin. Perbaikan: dinyatakan terang; `StoreMode`.
9. `ProviderRevisionRecord` belum punya `approved_at`, `PUBLISHER` hard coded, dan `SourceRecorder` hanya tahu sumber catalog. Perbaikan: field baru, `PinMeta`, `source_id` dari baris import.
10. Entri `invalid` tidak bisa diperbaiki dengan import ulang. Perbaikan: jalur tanpa operasi hanya saat salinan store termuat; `storedCopyValid`.
11. Replay dan konkurensi tidak ditetapkan. Perbaikan: balasan pertama apa adanya, `BusyPolicy::Queue`, cek ulang di guard.
12. Urutan sesudah commit, pemanggil refresh, dan event saat start. Perbaikan: urutan ditetapkan, port `CatalogRefresher`, tanpa event saat start, hapus folder di bawah lock dengan cek rujukan.
13. Entri `invalid` lokal tidak dikenali sebagai lokal. Perbaikan: `InvalidEntry.local`.
14. `installability` tidak punya catalog untuk `entry_trust`. Perbaikan: parameter `trust` dan daftar tiga pemanggil.
15. Folder store yang hilang punya dua kemungkinan issue. Perbaikan: finding `package` kalau tidak termuat, `import_digest_mismatch` kalau termuat tetapi tidak cocok.
16. Detail CLI dan GUI: konfirmasi `import --approve` sebelum import, `cli.provider_invalid` untuk dry run, exit code `provider.store_failed`, copy `revocation` dan `changes` di sheet import, tombol Import untuk perbaikan.

Klaim yang salah dan sudah dibetulkan: lokasi `versions_sha256` (`catalog::index`), "langkah yang sama dengan pin" (pin tidak membandingkan `versions.json`), dan kontradiksi event saat start.

Saran yang tidak diambil: mengecilkan `PermissionChange` menjadi empat kode. Sepuluh kode dipertahankan karena murah (satu fungsi murni) dan dialog approval ulang menjadi lebih jelas; approval ulang tetap wajib apa pun kodenya.

## Bahan yang dibaca

- `docs/scope/06-menambah-provider.md` (fitur 37), `prd.md` baris REQ-028, REQ-032, REQ-033, BR-020, VAL-018, PAGE-08, AC-032, AC-033, AC-061, AC-072, OQ-009, bagian 21.4 dan 23.
- Spec 0003, 0004, 0005, 0006, 0007, 0009, 0016, 0017, 0018, 0020, 0030, 0032, 0034, 0035 (bagian yang dirujuk di `index.md`).
- `apps/macos/AGENTS.md`, `docs/scope/ui-macos.md`, `docs/mockups/Stackly.dc.html` (tombol "Import Provider…" di toolbar Catalog dan teks toast nya; tidak ada sheet import atau dialog izin di mockup).
- Kode di `main` pada 2026-11-11: `crates/core/src/catalog/`, `crates/core/src/provider/`, `crates/storage/`, `crates/protocol/src/`, `apps/cli/src/commands/`, `apps/macos/Packages/StacklyUI/`.
