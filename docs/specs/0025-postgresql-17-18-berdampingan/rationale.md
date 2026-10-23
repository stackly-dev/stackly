# 0025. Rationale: PostgreSQL 17 dan 18 berdampingan

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: Done when fitur 25 meminta "rebind biasa" ditolak oleh preflight kompatibilitas, padahal operasi rebind adalah fitur 58 (`· needs a decision · GA`, belum punya spec), dan fitur 58 juga mencantumkan REQ-043 dan AC-043 di baris Trace nya. Kalau fitur 25 membangun aksi rebind yang bisa dipanggil pengguna (method, CLI, atau GUI) hanya untuk menolak, fitur ini memutuskan kontrak dan alur fitur 58 (parameter, kebijakan backup, stop terkontrol, idempotency) sebelum fitur itu dirancang, atau meninggalkan perintah setengah jadi yang menolak setiap pasangan termasuk yang kompatibel. Yang sebenarnya dibutuhkan fase 03 adalah jaminan bahwa jalur keputusan rebind menolak mayor lain pada data asli dengan alasan butuh migrasi, dan bahwa keputusan itu sama dengan keputusan start. Spec ini memisahkan keduanya: fitur 25 memiliki pemeriksaan (yang hanya membaca), fitur 58 memiliki aksinya. Bukti AC-043 di fitur 25 karena itu bersifat bagian, sama seperti bukti AC-009 dibagi antara spec 0024 dan spec ini.

Fase 03 adalah nilai inti produk (GOAL-01): `pg17-dev`, `pg17-test`, dan `pg18-dev` jalan bersamaan dengan data, config, log, dan port yang terpisah. Fitur 24 membuktikan dua instance dari runtime yang sama; fitur 25 membuktikan sisi lintas mayor, yaitu JOURNEY-03 di PRD: install runtime baru, create instance baru, buka endpoint baru, bandingkan versi, dan tidak pernah mengubah data directory instance lama sebagai bagian install. REQ-035 meminta PostgreSQL 17 dan 18 dengan tool versi yang cocok dan melarang upgrade mayor otomatis; REQ-043 memisahkan install runtime (tidak mengubah instance) dari rebind (hanya untuk pasangan kompatibel) dan dari migrasi mayor (alur eksplisit); BR-001 mengunci instance ke runtime ID persis; BR-009 melarang binary mayor PostgreSQL lain pada cluster yang ada. PRD 22.1 menegaskan bahwa upgrade 17 ke 18 bukan rebind biasa dan bahwa produk baseline boleh mengarahkan pengguna ke backup, restore, dan panduan migrasi.

Kekuatan yang membatasi desain: hampir semua mekanisme sudah dibangun atau dirancang di spec lain. Preflight data (`ValidatedPackage::preflight`, `dataMarker` `PG_VERSION` tanpa `lineCompatibility`) milik spec 0009 dan 0019, start menolak data mayor lain sebelum spawn lewat `verify` (spec 0010), `Instance::rebind` di domain menolak line yang tidak diizinkan (spec 0004), install runtime adalah operasi `install_runtime` spec 0018 yang hanya menulis `runtimes/` dan registry runtime, dan harness native PostgreSQL (milestone 2 spec 0019) sudah di `main`. Bukti dasar penolakan dua arah sudah dimiliki spec 0019 AC-11, dan install PostgreSQL 17 dan 18 dari cache sudah dimiliki milestone 5 spec 0018. Kode error `supervisor.data_incompatible` hari ini hanya berkata "the data and the runtime are not compatible", tanpa alasan yang bisa ditindaklanjuti. Kontrak protokol sedang dikunci satu per satu oleh merge fase 02, jadi setiap perubahan kontrak tambahan punya biaya koordinasi.

Kalau ini tidak diputuskan, fitur 25 akan membangun ulang bukti yang sudah ada di spec 0019 dan 0024, atau membangun perintah rebind yang mendahului fitur 58, dan kalimat Done when "dengan alasan butuh migrasi" tidak punya tempat yang jelas di produk.

## Options considered

### Keputusan 1: cara membuktikan AC-043 tanpa membangun fitur 58

#### Option 1: Pemeriksaan rebind di core yang hanya membaca, berbagi satu fungsi dengan start, diuji pada data asli

`stackly_core::instance::rebind::check(instance, runtime)` menjalankan urutan pemeriksaan rebind (instance ada, bukan retained, sudah diinisialisasi, runtime target ada, software sama, terpasang, paket utuh, folder privat, lalu kecocokan data) tanpa menulis apa pun, dan menolak dengan kode yang sudah ada. Kecocokan data dipanggil lewat satu fungsi `check_data_compatibility` yang juga dipakai `verify` start. Test core memakai paket PostgreSQL asli dan isi `PG_VERSION` asli; test native memanggilnya pada data cluster 17 dan 18 yang dibuat lewat daemon. Fitur 58 kelak membungkusnya dengan operasi, stop terkontrol, kebijakan backup, dan verifikasi start.

**Pros**:

- Tidak memutuskan apa pun milik fitur 58: tidak ada method, DTO, parameter, atau alur pengguna yang harus ia warisi.
- Start dan rebind tidak bisa berbeda pendapat, karena kecocokan data datang dari satu fungsi.
- Tanpa perubahan kontrak, jadi tidak menambah antrian kontrak fase 02.
- Bukti native nyata: penolakan terjadi pada `PG_VERSION` dari `initdb` asli, dengan registry, paket pinned, dan layout yang sama dengan daemon.

**Cons**:

- Pengguna tidak bisa "mencoba rebind" di fase ini; bagian AC-043 tentang aksi pengguna menunggu fitur 58.
- Kode publik tanpa pemanggil produksi sampai fitur 58, dan bentuknya bisa berubah saat fitur 58 dirancang.
- Test native harus membuka registry dari luar daemon saat daemon berhenti.

#### Option 2: Method IPC `instance.rebind` dan `stackly instance rebind` yang hanya menolak

Fitur 25 menambah method dan perintah rebind lengkap dengan idempotency key, menjalankan precheck, dan menolak pasangan yang tidak kompatibel dengan kode khusus; pasangan kompatibel dijawab "belum didukung" sampai fitur 58.

**Pros**:

- Paling dekat dengan kalimat AC-043 ("pengguna mencoba rebind biasa"): ada perintah yang bisa diketik.
- Fitur 58 tinggal mengisi jalur sukses.

**Cons**:

- Mendahului keputusan fitur 58 (GA, needs a decision): bentuk params (kebijakan backup, target selector), operasi atau bukan, `recovery allowed` atau `refused`, dan pesan arahan ke backup dan restore.
- Rilis Beta membawa perintah yang menolak setiap pasangan, termasuk yang kompatibel, sebuah state produk yang membingungkan.
- Dua kenaikan minor protokol (sekarang dan saat fitur 58 mengubahnya), di tengah antrian kontrak fase 02.

#### Option 3: Method query `instance.check_rebind` yang hanya membaca

Seperti Option 1, tetapi pemeriksaannya juga dibuka lewat method IPC hanya baca (dan `stackly instance rebind --dry-run`), yang membalas verdict.

**Pros**:

- Pengguna dan GUI bisa melihat apakah runtime lain kompatibel, mirip `instance.preflight` milik create.
- Bukti AC-043 bisa lewat CLI asli.

**Cons**:

- Bentuk preflight rebind (daftar baris pemeriksaan seperti `CreatePreflight`, termasuk backup dan stop) adalah keputusan UX fitur 58; mengunci DTO sekarang berisiko diganti.
- Tetap perubahan kontrak dengan biaya codegen, fixture, Swift, dan kenaikan minor.

#### Option 4: Tanpa kode baru, merujuk test domain dan spec 0019

AC-043 dianggap terbukti oleh `runtime_of_another_line_is_rejected_on_existing_data` (domain, fixture) dan spec 0019 AC-11 (start).

**Pros**:

- Nol kode, nol risiko.

**Cons**:

- Test domain tidak membaca penanda data, jadi bukan "ditolak oleh preflight kompatibilitas dari fitur 19" seperti tuntutan Done when.
- Tidak ada jalur keputusan rebind yang teruji untuk dipakai fitur 58; tidak ada alasan butuh migrasi.

### Keputusan 2: tempat alasan "butuh migrasi"

#### Option 1: Hint statis pada `supervisor.data_incompatible` per `outcome`

Fungsi `codes::supervisor::hint` memilih teks dari detail `outcome`; `stacklyd` memasangnya di `ErrorBody` kegagalan operasi lewat fallback hint spec 0006. Teks `incompatible` menyebut bahwa data mayor lain butuh migrasi lewat instance baru serta dump dan restore.

**Pros**:

- Tanpa kode baru dan tanpa perubahan digest kontrak; `ErrorBody.hint` sudah ada di wire.
- Satu teks untuk start dan rebind, karena keduanya memakai kode yang sama.
- Pola yang sudah dipakai area `catalog` dan `storage`.

**Cons**:

- Kode `supervisor.*` dipakai juga oleh jalur rebind, nama area nya sedikit kurang tepat.
- Teks hint bebas berubah tanpa versi kontrak, jadi klien tidak boleh mencocokkan teksnya (memang aturan proyek).

#### Option 2: Kode baru `instance.migration_required`

Outcome `incompatible` mendapat kode sendiri berkategori `conflict`, dengan message yang menyebut migrasi.

**Pros**:

- Klien bisa bereaksi pada kode, bukan pada detail `outcome`.

**Cons**:

- Mengubah kode yang dikembalikan start, jadi mengubah perilaku dan test spec 0010 dan 0019 AC-11.
- Kontrak naik minor tanpa method baru yang membutuhkannya di fase ini.

#### Option 3: Ubah `summary` katalog `supervisor.data_incompatible`

Kalimat ringkasan katalog diganti menjadi menyebut migrasi.

**Pros**:

- Tidak ada fungsi baru.

**Cons**:

- `summary` masuk `protocols/error-codes.json` dan digest kontrak, jadi naik minor.
- Satu kalimat untuk semua outcome, padahal penanda hilang atau berubah bukan soal migrasi.

### Keputusan 3: struktur bukti tiga cluster dan pembagian dengan spec 0024

#### Option 1: Di atas skenario dua PostgreSQL 17 milik spec 0024, runtime 18 lewat `runtime.install` setelah keduanya ada

Spec 0024 memiliki `SentinelCluster` dan `two_postgresql_17_instances`; spec ini menambah runtime 18 lewat operasi install sungguhan saat kedua instance 17 sudah ada (satu berjalan), membuat `pg18-dev`, lalu memeriksa matriks sentinel dengan stop dan start per instance.

**Pros**:

- Tidak ada skenario yang ditulis dua kali; AC-009 ditutup bersama dengan batas yang jelas.
- Install yang dibuktikan adalah install yang dipakai pengguna (operasi `install_runtime`), bukan pintasan fixture, dan terjadi di samping instance yang hidup, persis JOURNEY-03.
- Siklus per instance juga membuktikan lifecycle yang independen: tetangga tetap ready dan membaca sentinel nya sendiri.

**Cons**:

- Build fitur 25 menunggu milestone 4 spec 0024 dan perubahan harness bersama.
- Test lebih lama dari dua cluster terpisah.

#### Option 2: Skenario tiga cluster sendiri di spec ini

Spec ini memasang kedua runtime dan membuat ketiga instance sendiri.

**Pros**:

- Tidak menunggu spec 0024.

**Cons**:

- Menulis ulang penyiapan, helper sentinel, dan keputusan yang sama di dua spec; dua sumber kebenaran untuk isolasi.

#### Option 3: Kedua runtime dipasang di awal lewat `install_cached_artifact`

Penyiapan memasang 17 dan 18 lewat pintasan fixture sebelum ada instance.

**Pros**:

- Penyiapan paling sederhana dan cepat.

**Cons**:

- Tidak membuktikan "install runtime baru tidak mengubah instance lama", karena install terjadi sebelum ada instance dan tidak lewat operasi sungguhan.

### Keputusan 4: cara membuktikan install tidak menyentuh instance

#### Option 1: Snapshot registry, digest pohon, dan identitas proses, di fixture dan native

Sebelum dan sesudah install: field registry yang terlihat di `instance.get`, digest setiap file (dengan mode) di pohon instance dan log instance yang berhenti, serta PID, waktu start, dan restart instance yang berjalan. Fixture memakai instance fake dan `FakeFetcher` (termasuk install yang gagal), native memakai dua PostgreSQL 17 dan artefak 18 dari cache.

**Pros**:

- Mencakup ketiga cara install bisa merusak instance: menulis registry, menulis file, atau mengganggu proses.
- Fixture selalu bisa dijalankan (tanpa artefak), native membuktikannya pada data asli.

**Cons**:

- Pohon instance yang berjalan berubah sendiri (WAL, log), jadi bukti file hanya berlaku untuk instance yang berhenti; instance yang berjalan dibuktikan lewat proses.

#### Option 2: Hanya membandingkan baris registry

**Pros**:

- Paling murah.

**Cons**:

- Tidak menangkap install yang menulis atau menghapus file di folder instance, padahal itu kalimat JOURNEY-03 ("tidak pernah mengubah data directory instance lama").

## Rationale

Option 1 di Keputusan 1 dipilih karena kekuatan paling kuat di Context adalah batas kepemilikan: fitur 58 adalah fitur GA yang masih butuh keputusan, dan Trace nya sendiri memuat AC-043. Membangun perintah rebind di fitur 25, walau hanya untuk menolak, memaksa keputusan params, operasi, dan pesan yang seharusnya diambil bersama kebijakan backup dan stop terkontrol. Yang bisa dan perlu dibuktikan di fase 03 adalah jalur keputusan: bahwa pertanyaan "boleh data ini dijalankan runtime itu" dijawab sama oleh start dan rebind, membaca penanda data asli, dan menolak mayor lain dengan alasan butuh migrasi. Option 1 membuktikan tepat itu, tanpa kontrak baru di tengah antrian kontrak fase 02, dan memberi fitur 58 precheck yang sudah teruji. Kelemahannya (pengguna belum bisa mengetik perintah rebind) jujur dicatat sebagai bukti bagian, dengan sisa AC-043 sebagai Follow-up fitur 58. Option 3 adalah runner up yang masuk akal kalau fitur 58 kelak ingin preflight rebind terlihat di GUI, tetapi keputusan itu milik fitur 58.

Option 1 di Keputusan 2 dipilih karena Done when meminta alasan, bukan kode baru. Hint adalah tempat proyek ini menaruh arahan yang bisa ditindaklanjuti (`catalog::hint`, `storage::hint`), tidak mengubah digest kontrak, dan langsung berlaku untuk start dan rebind karena keduanya memakai kode yang sama. Kode baru (Option 2) akan mengubah perilaku start yang sudah diverifikasi di spec 0019 dan menaikkan kontrak tanpa kebutuhan method.

Option 1 di Keputusan 3 dan 4 dipilih karena JOURNEY-03 adalah urutan, bukan keadaan akhir: mayor kedua dipasang saat instance lama sudah ada dan berjalan. Hanya install sungguhan di samping instance yang hidup yang membuktikan REQ-043 kalimat pertama, dan memakai ulang skenario spec 0024 menjaga satu sumber kebenaran untuk isolasi sentinel.

## Keputusan kecil

| Keputusan | Pilihan | Alasan | Runner up |
| --- | --- | --- | --- |
| Lifecycle yang ditolak `rebind::check` | hanya `retained` dan data yang belum diinisialisasi | `PG_VERSION` stabil saat server berjalan, dan stop terkontrol adalah langkah fitur 58 | menolak selain `stopped` seperti `Instance::rebind` (memaksa fitur 58 stop sebelum bisa menampilkan preflight) |
| Runtime target sama dengan runtime instance | jawaban `Current`, bukan error | pemanggil memutuskan apakah itu no op | error `instance.runtime_unchanged` (kode baru) |
| Paket untuk preflight rebind | paket pinned instance | `dataMarker` dan `lineCompatibility` milik provider revision instance; update provider revision adalah fitur 59 | paket terbaru catalog (mencampur rebind dengan update provider) |
| Folder runtime target | tidak disentuh | validasi executable terjadi saat start (spec 0010) di verifikasi start fitur 58 | memeriksa root canonical runtime (duplikasi start) |
| Tipe error `rebind::check` | `RebindCheckError` yang membungkus `SupervisorError` plus dua varian area `instance` | memakai ulang kode dan details yang ada, tanpa kode baru | `SupervisorError` saja (tidak punya varian runtime target tidak ada dan software beda) |
| Penulisan registry khusus test (AC-4) | `Site::bind_runtime` didefinisikan di spec ini (langkah 6), dipakai bersama dengan langkah 10 spec 0019 kalau spec itu menambah yang sama lebih dulu | langkah 10 spec 0019 hanya menyebut "penulisan registry khusus test" tanpa kontrak, sementara AC-4 dan AC-7 butuh perilaku yang pasti (revision, manifest tidak disentuh, registry ditutup) | bergantung pada helper spec 0019 yang belum didefinisikan |
| Jaringan di test native | platform dengan `FakeFetcher` kosong, catalog dari `providers/` repo, artefak di `downloads_dir()` | install dari cache harus terbukti tanpa jaringan; unduhan yang tidak sengaja membuat test gagal, bukan diam diam lewat | `MacosFetcher` asli (unduhan tak sengaja tidak terdeteksi) |
| Bentuk error `rebind::check` di wire | `RebindCheckError` mengimplementasikan `OperationError` | fitur 58 memakainya langsung, dan hint dipilih dari kode dan details yang sama dengan start | tipe error tanpa trait (fitur 58 harus memetakan ulang) |
| Bukti hint `incompatible` di fixture | runtime fake kedua ber line `0.2` lewat varian `install_fake_runtime` | hint `incompatible` tidak boleh hanya terbukti di native yang bisa `BLOCKED` | hanya `marker_mismatch` di fixture |
| Dua test native, satu fungsi penyiapan | `postgresql_18_beside_17_journey` dan `another_major_is_refused_at_start_and_by_the_rebind_check` di atas `three_postgresql_clusters` | evidence per kelompok AC tetap terpisah, penyiapan tidak ditulis dua kali | satu test panjang (evidence dan kegagalan sulit dibaca) |
| Daemon di test native | in process (`Site::in_process`) | AC-4 dan AC-7 butuh daemon berhenti dan jalan lagi sambil test membuka registry; harness sudah mendukungnya | binary `stacklyd` (lebih dekat ke pengguna, tetapi tidak menambah bukti untuk AC ini) |
| Restart di AC-3 | stop lalu start per instance | restart sebagai satu operasi milik fitur 26; siklus per instance juga membuktikan tetangga tidak terganggu | stop semua lalu start semua |
| Teks hint `incompatible` | "dump and restore" generik | backup dan restore Stackly baru ada di fase 09 | merujuk fitur yang belum ada |
| Port test | `Fixed` dari `allocate_port` | aturan `AGENTS.md`, tidak pernah 5432 | `Auto` (port tidak bisa diperiksa sebelum start) |

## Pertukaran dengan sesi lain

- `arc-f24-spec` (spec 0024), 2026-10-22: sepakat pembagian bukti AC-009. Spec 0024 memiliki dua PostgreSQL 17 dari runtime yang sama (create biasa dan duplicate config), helper `SentinelCluster` di `apps/daemon/tests/postgresql/sentinel.rs` (`new`, `write`, `read`, `system_identifier`, `assert_isolated`, koneksi TCP lewat `Psql` harness), dan `two_postgresql_17_instances(site, api) -> TwoPostgres17 { dev, test, runtime_id }` di `apps/daemon/tests/postgresql/same_runtime.rs`, di milestone 4 langkah 8 dan 9. Evidence spec 0024 menulis AC-009 sebagai bagian dua PostgreSQL 17; spec ini menutup AC-009 dengan mayor 18. Duplicate config dikunci ke runtime sumber, jadi duplicate lintas mayor di luar kedua spec. Lokasi helper dipindah dari `apps/daemon/tests/support/` ke `postgresql/` setelah merge `main` membawa harness PostgreSQL (nama modul `support` sudah dipakai).
- Lead (Scope 03), 2026-10-22: `main` maju dengan milestone 2 spec 0019 dan update spec 0020; worktree spec ini di merge `main` sebelum Build plan ditulis, dan versi protokol tidak disebut dengan angka.
- Lead (Scope 03), 2026-10-22: spec diterima atas delegasi user, dengan rekomendasi gap G1 sampai G5 seperti tertulis dan G6 tanpa perubahan teks Done when scope. Status tetap `Proposed`. Usulan update spec 0010, 0009, 0019, dan 0004 dibawa lead sebagai item `architect (update)`.
- Lead (Scope 03), 2026-10-22, teks diselaraskan 2026-10-23: harness native memuat catalog uji `CatalogOverride::Preloaded(Catalog::for_tests(..))` berisi paket `providers/postgresql`, bukan `CatalogOverride::Location` ke `providers/`, karena folder itu hanya sumber catalog dan daemon membaca `catalog.json` hasil `cargo xtask catalog build`; lead menerima penyimpangan ini saat build (`dev-f25-m3` di log koordinasi Scope 03). Butir *Lingkungan test native* dan langkah 6 kini menyebut harness apa adanya. Baris Keputusan kecil "Jaringan di test native" tetap mencatat pilihan saat spec ditulis: tujuannya (catalog dari paket repo, tanpa jaringan) tidak berubah, hanya cara catalog dimuat. Tidak ada AC atau keputusan yang berubah.

## Cross check

Cross check 2026-10-22 oleh model lain (Sonnet, hanya membaca), 20 temuan (5 high, 9 medium, 6 low). Setiap fakta tentang kode diperiksa ulang sebelum diterapkan.

| No | Temuan | Disposisi |
| --- | --- | --- |
| 1 | `covers` hanya menerima `AC`, `REQ`, `NFR`, `VAL`, `BR` (`crates/evidence/src/rules.rs`), jadi JOURNEY-03 membuat guard panik | diterapkan: JOURNEY-03 pindah ke `expected` (AC-9, AC-10, tabel test) |
| 2 | hint dibatasi 200 karakter (spec 0006 *Hint*), kedua teks lebih panjang | diterapkan: teks dipendekkan (181 dan 192 byte) dan test panjang ditambah |
| 3 | `unknown` tidak bisa dicapai lewat fungsi yang selalu memberi line tercatat | diterapkan: AC-8 menguji empat hasil |
| 4 | digest pohon `pg17-dev` yang berjalan tidak stabil | diterapkan: digest hanya untuk instance yang berhenti, `pg17-dev` lewat identitas proses |
| 5 | penulisan registry khusus test spec 0019 tanpa kontrak | gap, rekomendasi diterapkan: `Site::bind_runtime` didefinisikan di spec ini (G1 sampai G5, diterima lead 2026-10-22) |
| 6 | catalog dan pencegahan jaringan untuk `runtime.install` in process tidak disebut | gap, rekomendasi diterapkan: catalog override `providers/`, `FakeFetcher` kosong, entri di `downloads_dir()` (G1 sampai G5, diterima lead 2026-10-22) |
| 7 | `InstallRuntimeReply` tidak membawa `runtimeId` | diterapkan: ID dari `result.resources[0].id` operasi |
| 8 | tanda tangan `check` di AC beda dengan desain | diterapkan |
| 9 | alasan butuh migrasi di jalur rebind tidak terbukti | gap, rekomendasi diterapkan: `RebindCheckError` mengimplementasikan `OperationError` dan AC-6 serta AC-7 memeriksa hint (G1 sampai G5, diterima lead 2026-10-22) |
| 10 | kode area `instance` di core berupa string literal; `mod prepare` privat | diterapkan |
| 11 | tempat pemasangan hint: fallback spec 0006 milik langkah 11 spec 0018, dan precheck start tidak pernah menolak `data_incompatible` | diterapkan: fallback spec 0006, jalur `catalog_body` tidak diubah |
| 12 | keadaan akhir AC-4 tidak disebut | diterapkan: operasi `failed` di `verify` sebelum `start_begin`, instance tetap `stopped`, `support::serving` kosong, adopsi ditunggu |
| 13 | adopsi lintas restart daemon in process belum pernah dibuktikan | gap, rekomendasi diterapkan: test memeriksa adopsi, `BinaryDaemon` boleh dipakai kalau tidak andal (G1 sampai G5, diterima lead 2026-10-22) |
| 14 | `runtimeVersion` diisi dari versi data | diterapkan: AC-2 menjelaskannya, bukti versi server lewat `SHOW server_version` |
| 15 | Done when scope "rebind biasa ditolak" sementara spec hanya memberi pemeriksaan core | G6, diputuskan lead 2026-10-22: teks Done when scope tidak diubah; sifat bagian AC-043 dicatat di spec dan di `expected` evidence |
| 16 | hint `incompatible` tanpa bukti fixture | gap, rekomendasi diterapkan: kasus runtime fake kedua ber line `0.2` (G1 sampai G5, diterima lead 2026-10-22) |
| 17 | `Site::subject` hanya untuk runtime `Site` | diterapkan |
| 18 | detail harness (bentuk kembalian, `Api` baru per restart, perbandingan registry, folder log yang belum ada) | diterapkan di *Lingkungan test native* |
| 19 | klaim bahwa belum ada test start untuk `data_incompatible` | sebagian keliru: `a_start_checks_the_package_the_folder_the_marker_and_the_runtime_first` sudah menguji `marker_mismatch`; kasus hasil lain ditambah. `rule()` BR-009 tidak ditambah (mengubah wire start di luar kebutuhan Done when) |
| 20 | `check` bisa lebih kecil (tanpa pemeriksaan 7 dan 9); "wajib" mengikat spec fitur 58 | sebagian: "wajib" menjadi "sebaiknya"; pemeriksaan 7 dan 9 dipertahankan karena precheck fitur 58 ditampilkan sebelum stop, dan menolak runtime yang tidak terpasang lebih awal berguna; catatan sesi paralel untuk `startup.rs` ditambah |
