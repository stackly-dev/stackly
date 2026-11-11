# 0036. Provider SDK dan test harness: rationale

Catatan keputusan untuk [index.md](index.md). `/develop` tidak perlu membaca file ini.

## Context

Fitur 9 mengunci schema manifest `stackly/v1alpha1` dan sengaja menunda tiga hal ke fitur ini: aturan tentang cara schema itu boleh berubah, validator yang bisa dipakai penulis paket, dan harness yang membuktikan sebuah paket bekerja dari install sampai sehat. Sejak itu schema sudah bertambah berkali kali (probe native, format template baru, blok `connection`, blok `remedies`), selalu dengan janji tidak tertulis bahwa paket lama tetap valid. Janji itu penting karena setiap instance di pin ke salinan paket yang tidak berubah di `<data>/providers/`; kalau engine baru menolak salinan lama, instance pengguna tidak bisa start. Sampai sekarang tidak ada test yang menjaga janji itu, dan satu satunya contoh pengetatan yang aman (`init_port_issues`) ditulis sebagai fungsi lepas tanpa aturan umum.

Pengetahuan tentang cara menjalankan sebuah provider juga ada di tiga tempat. Paket provider memegangnya untuk daemon. Langkah spike di `xtask` memegang salinan tulisan tangan untuk enam provider (ditandai `// provisional: F36`). Dan setiap provider nyata punya `harness.rs` sendiri di `apps/daemon/tests/` yang mengulang persiapan root, pemasangan runtime dari cache, dan pin paket. Fase 07 akan menambah lima provider; tanpa satu jalur bersama, tiap provider menambah recipe spike dan harness test sendiri.

Beberapa batasan repo mempersempit pilihan. CLI tidak pernah menjalankan binary provider dan tidak pernah memanggil core. Proses hanya di spawn lewat port `ProcessSupervision`. Fitur `fixture` core (pemasang runtime fake dan pemasang artefak dari cache) hanya boleh menyala lewat dev dependency. Tidak ada crate yang boleh bergantung pada library `stacklyd`. Paket ber namespace `testing` tidak pernah masuk catalog app, dan `CatalogOptions.allow_testing_namespace` tidak pernah diisi dari environment atau flag. Secret hanya hidup di Keychain, dan sesi pengembangan tidak boleh menyentuh Keychain login asli.

Spec lain menitipkan pertanyaan ke fitur ini: spec 0003 (spike dari paket), spec 0009 (kebijakan additive dan arti folder `tests/`), spec 0010 (fd 3, re exec, foreground), spec 0023 (aturan probe native), spec 0030 (test launchd sebelum `HANDOVER_PROVEN`), dan spec 0014 (AC-029 lengkap). Dua spec yang ditulis bersamaan bergantung pada hasilnya: spec 0037 memakai validator untuk preview import, dan spec 0038 memakai nama perintah untuk panduan kontributor.

User mendelegasikan setiap keputusan ke opsi yang direkomendasikan, jadi tidak ada wawancara; setiap keputusan di bawah diambil architect dan dikirim ke lead untuk `accept`.

## Options considered

### Keputusan 1: penanda versi dan cara schema bertambah

**Option 1: additive di `stackly/v1alpha1`, tanpa field versi kedua, dijaga corpus beku (dipilih).** `apiVersion` tetap satu satunya penanda. Penambahan masuk di tempat, pengetatan ditulis sebagai laporan penulis, dan perubahan yang merusak butuh `apiVersion` baru.

- Pro: sama dengan yang sudah dilakukan sejak fase 02; tidak ada field baru yang harus diisi setiap paket; arah yang penting (engine baru membaca paket lama) dijaga test.
- Kontra: paket yang lebih baru dari build pembacanya ditolak field demi field, bukan dengan satu pesan versi.

**Option 2: field `compatibility.schemaRevision` di manifest.** Setiap penambahan menaikkan angka; paket menyatakan angka minimum; build yang lebih tua menolak dengan satu pesan.

- Pro: pesan yang jelas untuk arah maju; daftar penambahan menjadi ledger bernomor.
- Kontra: setiap cabang validasi fitur baru harus tahu nomornya; paket yang ada harus diisi; manfaatnya baru terasa saat ada registry publik atau downgrade app, keduanya di luar baseline.

**Option 3: `apiVersion` baru untuk setiap penambahan (`v1alpha2`, `v1alpha3`).**

- Pro: satu konsep versi, cocok persis dengan bunyi AC-030.
- Kontra: bertentangan dengan keputusan fase 02 spec 0009 (nama tidak berubah untuk penambahan); engine harus memetakan setiap fitur ke versi; paket yang ada sudah memakai penambahan di bawah nama lama.

### Keputusan 2: di mana validator berjalan

**Option 1: satu fungsi core, dua pintu: xtask tanpa daemon dan method `provider.validate` untuk CLI (dipilih).**

- Pro: hasil selalu sama; kontributor di checkout tidak butuh daemon; pengguna app dan fitur 37 mendapat pemeriksaan tanpa eksekusi; CLI tetap tidak memanggil core.
- Kontra: satu method dan satu DTO baru di kontrak wire, dengan bump minor.

**Option 2: hanya `cargo xtask provider check`.**

- Pro: tanpa perubahan wire.
- Kontra: `stackly provider validate` tidak ada; fitur 37 harus membuat pintu sendiri; pengguna tanpa checkout tidak bisa memeriksa paket.

**Option 3: CLI memanggil core langsung.**

- Pro: tanpa daemon dan tanpa wire.
- Kontra: melanggar arah dependency spec 0001 dan aturan `check-deps`.

### Keputusan 3: di mana harness berjalan

**Option 1: modul test `stacklyd` dengan daemon di dalam proses, dibungkus `cargo xtask provider test` (dipilih).**

- Pro: memakai executor sungguhan lewat `ProcessSupervision`; boleh memakai fitur `fixture` dan `test-support`; sama dengan cara test native provider yang ada; tidak butuh perubahan di binary yang dikirim ke pengguna; secret store test, bukan Keychain.
- Kontra: hanya untuk yang punya checkout; `stackly provider test` tidak ada; example memuat modul dari `tests/` lewat `#[path]`.

**Option 2: `stackly provider test` di CLI yang menjalankan `stacklyd` pribadi di root sementara.**

- Pro: tersedia bagi pengguna app tanpa checkout; sesuai tabel CLI PRD.
- Kontra: binary rilis tidak punya pemasang fixture, jadi paket fake tidak bisa diuji; catalog rilis menolak namespace `testing`; daemon kedua memakai Keychain login; butuh penanda baru di protokol supaya tidak pernah jalan di root asli.

**Option 3: harness di `xtask` yang menggerakkan binary `stackly` dan `stacklyd` hasil build.**

- Pro: setiap langkah adalah perintah produk yang bisa diulang dengan tangan.
- Kontra: binary `stacklyd` tanpa fitur `fixture` tidak bisa memasang runtime fake dan tidak bisa memuat paket `testing` ke catalog, jadi langkah `catalog` dan `form` tidak bisa diuji untuk paket contoh.

### Keputusan 4: bentuk template scaffold

**Option 1: paket contoh yang valid sebagai template (dipilih).** `providers/example/` ikut `provider check`, golden, dan harness; scaffold menyalinnya dengan tiga penggantian baris.

- Pro: template tidak bisa membusuk tanpa ketahuan; hasil scaffold langsung lulus; satu paket menjadi contoh sekaligus bahan AC-034.
- Kontra: satu folder `testing` lagi di `providers/`; penggantian teks bergantung pada tiga baris yang harus tetap ada.

**Option 2: folder template dengan placeholder di `xtask/templates/`.**

- Pro: bebas membentuk isi template.
- Kontra: bukan paket valid, jadi harus dirender dulu untuk diuji, dan mudah tertinggal dari schema.

**Option 3: `stackly provider new` di CLI.**

- Pro: tersedia tanpa checkout.
- Kontra: CLI harus membawa salinan template; alur kontribusi tetap lewat PR ke repo, jadi checkout sudah ada.

### Keputusan 5: cara spike mendapat langkah lifecycle

**Option 1: `PackageRecipe` di xtask yang menjalankan plan dari paket (dipilih).**

- Pro: pengetahuan per provider hanya ada di paket; kontrak, hasil, dan AC spec 0003 tidak berubah; bisa dikerjakan terpisah dari harness.
- Kontra: pelaksana plan kedua yang kecil di luar supervisor; probe closure artefak tetap per provider di xtask.

**Option 2: worker spike menjalankan harness (daemon di dalam proses) sebagai child.**

- Pro: hanya satu executor; spike membuktikan jalur produk.
- Kontra: mengikat alur bukti G0 ke binary example daemon; audit library dan listener harus pindah dari xtask; mengubah AC spec 0003 yang sudah diterima.

**Option 3: spike tidak lagi menjalankan server.**

- Pro: recipe hilang seluruhnya.
- Kontra: menghapus AC-5, AC-6, dan AC-7 spec 0003 (ready, stop, audit runtime, probe closure), yang merupakan isi gate G0.

### Keputusan 6: cara memeriksa kontrak proses

**Option 1: `adoptable` dari daemon ditambah `lsof` dan `ps` dari harness, dua sampel (dipilih).**

- Pro: memakai pengamatan yang sudah ada (`holds_file` spec 0010) dan cara yang sudah dipakai test adopsi PostgreSQL dan NATS; tidak ada method baru.
- Kontra: bergantung pada output `lsof` dan `ps` macOS.

**Option 2: daemon mengekspos pemeriksaan konformitas sebagai method.**

- Pro: tersedia juga untuk doctor.
- Kontra: menambah kontrak wire dan kode supervisor untuk kebutuhan yang hanya dimiliki harness.

## Rationale

**Keputusan 1.** Arah yang bisa merusak data pengguna adalah engine baru yang membaca paket lama yang sudah di pin, bukan sebaliknya. Karena itu usaha diletakkan di sana: corpus beku membuat setiap perubahan engine yang mengubah digest, argv, atau hasil render paket lama gagal di test. Untuk arah sebaliknya, validasi ketat spec 0009 sudah menolak dengan aman dan pesannya sudah menyebut nilai yang dikenal; satu kalimat "butuh Stackly yang lebih baru" baru bernilai saat paket beredar terpisah dari app, yaitu registry publik (Deferred) atau update provider (fitur 59). `schemaRevision` dicatat sebagai runner up dan boleh ditambah saat itu sebagai perubahan additive biasa. Aturan "pengetatan hanya lewat laporan penulis" menjadikan pola `init_port_issues` sebagai aturan umum, supaya tidak ada yang mengetatkan `from_files` dan mematahkan salinan pinned.

**Keputusan 2.** Fitur 37 mensyaratkan paket lokal yang belum dipercaya bisa diperiksa tanpa menjalankan apa pun, dan pemeriksaan itu harus sama dengan yang dipakai maintainer. Satu fungsi core memenuhi keduanya. Method daemon dipilih, bukan CLI yang memanggil core, karena arah dependency spec 0001 dijaga `check-deps`. Method itu read only, jadi mengikuti pola `doctor.run`: bukan operation dan tidak diaudit. Paket rusak dibalas sebagai hasil, bukan error, karena itulah jawaban yang diminta.

**Keputusan 3.** Done when fitur ini meminta provider fake server baru bekerja dari install sampai form. Paket seperti itu ber namespace `testing` dan artefaknya `fixture`, dua hal yang binary rilis sengaja tolak. Hanya kode yang dibangun dengan dev dependency yang boleh membuka keduanya, dan hanya paket `stacklyd` yang boleh memakai library daemon. Jadi harness adalah kode test `stacklyd`, dan pembungkus xtask memberi kontributor satu perintah. Menjalankan daemon di dalam proses juga menjaga dua aturan yang disebut brief: proses provider tetap di spawn lewat `ProcessSupervision`, dan CLI tidak terlibat. Akibatnya `stackly provider test` tidak dibuat; ini dicatat sebagai tradeoff dan diteruskan ke lead karena deskripsi scope menyebut nama itu.

Harness memakai method kontrak sesudah persiapan root supaya paket yang lulus memang bekerja untuk CLI dan GUI, yang hanya punya method itu. Pemasangan runtime tidak lewat `runtime.install`, karena artefak fixture tidak punya URL dan run native tidak boleh mengunduh; installer sendiri tidak bergantung pada provider dan sudah dibuktikan spec 0018.

**Keputusan 4.** Template yang bukan paket valid akan tertinggal dari schema tanpa ada yang gagal. Paket contoh yang ikut semua pemeriksaan tidak bisa begitu. Paket contoh dibuat lebih kecil dari `providers/fake`, yang tugasnya menjadi fixture lengkap untuk fault, bukan bahan bacaan.

**Keputusan 5.** Follow-up spec 0003 meminta harness yang membangun langkah spike dari paket. Yang diduplikasi hari ini adalah pengetahuan per provider (perintah init, argumen start, cara cek ready), dan itulah yang dihapus `PackageRecipe`. Menjalankan plan di xtask memang pelaksana kedua, tetapi ia membaca plan yang sudah tervalidasi, `match` nya exhaustive sehingga primitive baru memaksa pembaruan saat kompilasi, dan ia memakai `probe::run` core untuk readiness. Opsi menjalankan daemon di bawah worker lebih bersih di atas kertas, tetapi mengubah AC spec 0003 yang sudah diterima dan mengikat bukti G0 ke daemon; itu keputusan untuk pemilik spec 0003, bukan untuk fitur Beta ini. Probe closure PostgreSQL dan NATS tetap di xtask karena membuktikan isi artefak, bukan cara menjalankan provider.

**Keputusan 6.** Supervisor sudah memanggil `holds_file` saat readiness pertama dan melaporkan `adoptable`. Harness menambah pengamatan dari luar pada dua titik waktu, karena provider yang menutup fd 3 atau re exec sesudah ready tidak terlihat dari satu sampel. Tiga flag rogue di fake server ada supaya pemeriksaan ini terbukti bisa gagal.

## Keputusan yang diambil architect

Semuanya opsi rekomendasi, untuk di `accept` lead:

1. Tidak ada field versi kedua di manifest; additive tetap di `stackly/v1alpha1`.
2. Corpus paket beku untuk paket `official/`, dengan `cargo xtask provider freeze` dan pemeriksaan di `provider check`.
3. Validator: satu fungsi core, `cargo xtask provider check` tanpa daemon, dan method `provider.validate` untuk `stackly provider validate`.
4. Paket rusak di `provider.validate` adalah balasan sukses dengan `loadable: false`; CLI memakai `cli.provider_invalid` exit 2.
5. `stackly provider test` tidak dibuat; harness adalah `cargo xtask provider test`.
6. Harness hanya menerima namespace `official` dan `testing`.
7. `tests/scenarios.yaml` adalah satu satunya file `tests/` yang dibaca, dengan tiga nilai `expect` dan tanpa secret.
8. Paket contoh `providers/example/` menjadi template scaffold; scaffold hanya di xtask.
9. Langkah `runtime` memasang lewat fungsi fixture dan cache xtask, tanpa unduhan.
10. Kontrak proses diperiksa dengan `adoptable`, `lsof`, dan `ps` pada dua sampel, dengan tiga flag rogue di fake server.
11. Penanda kepemilikan ditambah sebagai API baru `TestRoot`, tanpa mengubah `TestRoot::new()`.
12. Spike memakai `PackageRecipe` untuk provider yang punya paket; recipe MySQL, ClickHouse, dan Valkey tetap sampai fitur 40, 42, dan 43.
13. Test konformitas native tidak mengambil alih bukti BR-025; fitur 39 sampai 43 memutuskannya.
14. Dokumen publik kebijakan ada di `docs/providers/` milik spec 0038; spec ini hanya menulis README paket contoh.
15. `xtask` menyalakan fitur `test-support` core dan `spike/package_recipe.rs` masuk `EXPOSE_ALLOWED`, supaya spike bisa memberi secret fixture ke init paket (dari cross check, gap 9).
16. Fakta beku corpus dihitung satu fungsi core `frozen_facts` dengan *Context beku* sendiri, bukan *Context golden* spec 0009 (dari cross check, gap 12).
17. Fixture form dibangun untuk setiap folder `providers/` dengan fakta runtime dari paket, jadi tiga fixture yang ada ditulis ulang sekali (dari cross check, gap 14).

## Cross check

Dijalankan 2026-11-11 oleh model lain (read only), dengan izin membaca kode untuk memeriksa klaim spec. Hasilnya 21 gap kelengkapan keputusan dan 12 catatan soundness; semuanya diterapkan dengan resolusi yang direkomendasikan, sesuai delegasi keputusan user, dan daftarnya dikirim ke lead bersama permintaan `accept`.

Yang mengubah desain:

- Entri corpus tidak bisa dimuat kalau `frozen.json` ada di dalam folder paket, karena `read_package` menolak entri yang tidak dikenal. Paket kini di subfolder `package/`.
- Shutdown daemon tidak menghentikan instance. Akhir run kini memakai `instance.preview_stop_all` dan `instance.stop_all`, lalu `ReapGuard`.
- Dua sampel proses yang mengapit `restart` selalu gagal, karena restart mengganti PID. Kedua sampel kini di dalam langkah `process`.
- Skenario crash dengan `readyDelayMs` 0 berpacu dengan readiness. Kini `readyDelayMs: 60000`, seperti test crash yang ada.
- Rogue `background` tidak pernah sampai ke langkah `process`, karena proses utama keluar sebelum ready. AC-10 kini mengharapkan gagal di `start` dengan sisa proses tercatat.
- `PackageRecipe` tidak bisa membuat `ResolvedSecrets` atau menulis byte secret di bawah aturan sekarang (keputusan 15), dan tanda tangan `Recipe` (`version_args`, `recipe`) harus berubah.
- Nama skenario kini mengikuti `InstanceName`, karena dipakai sebagai bagian nama instance.
- `--json` di `cargo xtask provider check` dibuang: `PackageCheck` tidak bisa diserialisasi dan pemetaan ke DTO adalah milik `stacklyd`. Bentuk yang bisa dibaca mesin adalah `stackly provider validate --json`.

Yang memperjelas sumber nilai: urutan persiapan root (`Site::open`), bentuk `Catalog::for_tests`, `FakeSecretStore`, nama varian `InitStep`, sumber fakta runtime fixture form, field `connections` yang dibandingkan, batas `limit: 1000` log, dan arti `create_fails`.

Yang mengeraskan harness: pencocokan file lewat device dan inode (bukan teks path, karena root memuat karakter non ASCII), pengecualian PID harness sendiri, test konformitas yang berjalan satu per satu, satu pemilik sinyal, `remove_checked` yang menolak selama masih ada proses di root, dan fuzz target untuk parser skenario. Klaim "tanpa jaringan" dipersempit: cargo tetap mengambil crate di build pertama.
