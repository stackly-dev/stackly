# 0017. Rationale: catalog dan provider detail

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: scope fitur 17 menyebut "catalog" seolah satu layar, padahal ada tiga hal yang harus diputuskan di satu tempat: bentuk catalog yang ikut di app (data dan cara membangunnya), aturan status yang dipakai bersama install (fitur 18) dan create (fitur 20), dan layar serta perintahnya. Memecahnya menjadi tiga spec akan membuat fitur 18 dan 20 menunggu dua spec untuk satu read model. Spec ini tetap satu, dengan batas yang disepakati langsung dengan architect fitur 15 dan 18 (lihat *Kesepakatan antarsesi*). Risiko kedua: layar fitur ini bergantung pada pekerjaan yang belum dibangun (fitur 15 untuk bundle, fitur 18 untuk compatibility record dan install, fitur 67 untuk komponen dasar). Spec ini menaruh pekerjaan yang bergantung padanya di milestone terpisah, supaya catalog bisa dibangun dan dibuktikan lewat `STACKLY_CATALOG_DIR` lebih dulu.

Stackly memasang provider dari paket deklaratif di `providers/<nama>/` (spec 0009): manifest, settings schema, template, dan `versions.json` yang dibangkitkan dari inventory spec 0003. Loader nya hanya membaca, dan paket uji `testing/fake` hidup di folder yang sama dengan paket asli, dengan aturan bahwa namespace `testing` tidak pernah masuk catalog app. PRD meminta catalog yang menampilkan publisher, revision, kategori, OS dan arch, versi, trust, status kompatibilitas, dependency, kebutuhan disk, dan capability (REQ-002), dengan unsupported yang tidak ditampilkan seolah bisa dipasang dan install yang ditolak daemon, bukan hanya oleh UI (AC-002). Baseline hanya punya catalog bawaan dan import lokal; browsing catalog tidak boleh menjalankan kode provider (REQ-033). Status `SUPPORTED` hanya untuk kombinasi yang lulus matrix teruji (BR-025), dan unsupported harus konsisten di GUI, CLI, dan daemon (BR-016).

Dari sisi waktu, catalog bawaan hanya berubah lewat update app (fitur 61) atau import lokal (fitur 37); sinkronisasi online ada di Deferred. NFR-014 meminta catalog yang stale ditampilkan dengan waktu pembaruan terakhirnya, dan scope meminta catalog tidak mengklaim lebih baru dari itu. Spec 0012 menyiapkan keterangan stale `.outdated(source:)` dengan waktu dari daemon untuk kasus ini. OQ-009 (trust root, signing, revocation) dan OQ-012 (signing account, hosting registry) belum diputuskan pemiliknya; defaultnya bundled official plus import eksplisit, distribusi langsung yang ditandatangani, dan hosting yang bisa ditunda. Mac ini belum punya signing identity, jadi semua build ditandatangani ad hoc (log koordinasi 2026-10-19).

Fakta teknis yang membatasi: helper `stacklyd` dan CLI ada di `Stackly.app/Contents/MacOS/`, dan app dirakit `cargo xtask build-app` dengan signing paling akhir (spec 0015). Registry punya tabel `provider_sources` yang kodenya milik fitur ini dan fitur 37, tetapi port `Registry` belum punya operasi untuk tabel itu (spec 0005). `ValidatedPackage` belum membuka hasil decode `versions.json` (kode fitur 9). Protokol 1.x sudah punya aturan method, enum terbuka, dan batas frame 1 MiB (spec 0006), dan app shell sudah punya `ResourceStore`, `ShellSync`, `UnbuiltPage` untuk Catalog, dan `ViewState` (spec 0012 dan 0013). Mockup menampilkan Catalog dengan filter kategori, pencarian, kartu dengan status, panel detail dengan matrix versi per OS (macOS, Linux, Windows), capability yang tidak didukung, lisensi, sumber, dan ukuran unduhan, serta provider planned yang redup tanpa tombol install. Pendekatan proyek adalah Journey, dan JOURNEY-01 melewati catalog sebelum install runtime.

## Options considered

### Keputusan 1: bentuk dan letak catalog di app

#### Option 1: folder data di `Contents/Resources/catalog/` dengan index `catalog.json` (dipilih)

Build menyalin paket yang dikurasi ke folder di Resources, ditambah index berisi revision, tanggal terbit, digest isi, dan digest per paket. Daemon menemukannya lewat `AppBundle` spec 0015, atau lewat `STACKLY_CATALOG_DIR` untuk development dan test.

**Pros**:

- Loader yang sama dengan repo dan dengan import lokal fitur 37 (`read_package` dari folder), jadi tidak ada jalur kedua.
- Ikut tersegel code signature bundle tanpa kerja tambahan, dan bisa diperiksa dengan Finder atau `ls`.
- Test bisa membangun catalog apa pun di `TestRoot`, termasuk fixture provider untuk platform lain.

**Cons**:

- Daemon harus menemukan folder itu dan menangani kasus tidak ada (Run dari Xcode, `cargo run`).
- Development butuh satu langkah build dan satu variabel environment.

#### Option 2: paket ditanam di binary `stacklyd` saat compile

`build.rs` atau xtask menghasilkan file yang di `include_bytes!` ke daemon, lalu daemon memakai `from_files` langsung tanpa filesystem.

**Pros**:

- Selalu ada, tanpa pencarian path dan tanpa variabel environment.
- Integritas sama dengan integritas binary yang ditandatangani.

**Cons**:

- `build.rs` membaca folder di luar crate, dan perubahan paket memaksa rebuild daemon.
- Binary test tetap butuh override untuk catalog fixture, jadi jalur kedua tetap ada.
- Import lokal fitur 37 tetap butuh jalur folder, jadi ada dua cara memuat paket.

#### Option 3: daemon membaca `providers/` repo atau folder yang disalin apa adanya tanpa index

**Pros**:

- Paling sedikit kode build.

**Cons**:

- Tidak ada revision, tanggal terbit, atau digest untuk NFR-014 dan integritas.
- Paket `testing/` dan paket yang belum siap (Traefik) ikut terbaca kecuali ada daftar.
- Planned tidak punya tempat.

### Keputusan 2: isi catalog

#### Option 1: `providers/catalog.toml` yang dikurasi, dengan entri planned (dipilih)

**Pros**:

- Paket yang validasinya lulus tetapi lifecycle nya belum dibuktikan (Traefik) tidak ikut sebelum fiturnya selesai.
- Planned punya data sendiri (nama, kategori, deskripsi) tanpa paket palsu.
- Tempat yang wajar untuk revision, tanggal terbit, catatan platform, dan petunjuk tile.

**Cons**:

- Satu file lagi yang harus diperbarui setiap provider selesai.

#### Option 2: semua folder non `testing` di `providers/` otomatis masuk, planned ditulis di kode

**Pros**:

- Tidak ada file tambahan.

**Cons**:

- Paket Traefik akan tampil tersedia sebelum fitur 41.
- Daftar planned di Swift atau Rust berarti update app untuk mengubah teks, dan GUI serta CLI bisa berbeda.

#### Option 3: provider planned sebagai paket dengan semua artefak `blocked`

**Pros**:

- Satu bentuk data untuk semua entri.

**Cons**:

- Paket wajib punya lifecycle, endpoint, dan settings yang belum ada; menulisnya palsu melanggar aturan "tanpa metadata fabricated" PRD 21.1.

### Keputusan 3: revision dan waktu terbit catalog

#### Option 1: `revision` dan `published_at` di `catalog.toml`, dijaga ledger digest isi (dipilih)

Draft pertama menyimpan satu `content_sha256` di `catalog.toml`; cross check menunjukkan `--update` bisa menerima digest baru di bawah revision lama, jadi aturannya tidak benar benar dipaksakan. Versi final memakai ledger `providers/catalog-revisions.json` (revision ke digest dan `publishedAt`) dengan `--amend` hanya untuk revision yang belum ada di `main`.

**Pros**:

- Tanggal yang ditampilkan adalah keputusan maintainer yang terlihat di review, bukan kebetulan waktu build.
- Isi yang berubah tanpa revision baru ditolak `catalog check` dan hook pre commit, dengan pola yang sama dengan `contract-versions.json` spec 0006.
- Build deterministik: input sama, output sama.

**Cons**:

- Langkah tangan tambahan setiap paket berubah, plus satu file ledger.

#### Option 2: waktu commit git terakhir yang menyentuh `providers/`

**Pros**:

- Otomatis.

**Cons**:

- Butuh git di build dan checkout bersih; tree kotor tidak punya jawaban yang jujur.
- Revision tetap butuh sumber lain.

#### Option 3: waktu build app

**Pros**:

- Paling mudah.

**Cons**:

- Membuat catalog lama tampak baru setiap build ulang, persis yang dilarang scope ("tidak mengklaim lebih baru").

### Keputusan 4: kapan catalog disebut outdated

#### Option 1: lebih dari 180 hari setelah `publishedAt` (dipilih)

**Pros**:

- Memberi sinyal nyata bahwa app sudah lama tidak di update, dan jalan keluarnya jelas (fitur 61).
- Memakai keterangan `.outdated` spec 0012 yang sudah disiapkan untuk catalog.

**Cons**:

- Angka tanpa data; bisa diubah lewat update spec.

#### Option 2: tidak pernah outdated di baseline, hanya tanggal

**Pros**:

- Tidak ada ambang yang perlu dibela.

**Cons**:

- Pengguna yang tidak membaca subjudul tidak pernah diingatkan, dan jalur `.outdated` spec 0012 tidak terpakai.

#### Option 3: 90 hari

**Pros**:

- Lebih cepat mengingatkan.

**Cons**:

- Siklus rilis app yang wajar sudah mendekati 90 hari, jadi banner akan sering muncul tanpa ada yang bisa dilakukan.

### Keputusan 5: paket yang gagal validasi

#### Option 1: isolasi per entri (dipilih)

**Pros**:

- Satu paket rusak tidak membuat seluruh layar kosong (PRD 11.2: error per item tidak mengosongkan app).
- Kerusakan tetap terlihat dengan issue nya dan subsystem `degraded`.

**Cons**:

- Ada state `invalid` tambahan di DTO dan layar.

#### Option 2: semua atau tidak sama sekali

**Pros**:

- Model lebih sederhana.

**Cons**:

- Satu byte rusak di satu template menghapus seluruh catalog.

### Keputusan 6: siapa menghitung status, dan kolom matrix

#### Option 1: daemon menghitung status untuk tiga kolom OS tetap (dipilih)

**Pros**:

- BR-016: satu sumber status untuk GUI, CLI, dan gate install.
- Tiga kolom sesuai mockup dan menyiapkan AC-074 fase Windows tanpa mengklaim apa pun.
- Catatan platform dari `catalog.toml` memberi perbedaan jujur antara planned dan unsupported per OS (PRD 5.2).

**Cons**:

- Kolom Linux dan Windows di baseline hampir selalu unsupported atau planned.

#### Option 2: client menghitung status dari data artefak mentah

**Pros**:

- DTO lebih kecil.

**Cons**:

- Dua implementasi (Swift dan Rust) untuk satu aturan, dan gate install tetap butuh salinan ketiga di daemon.

#### Option 3: kolom hanya untuk target yang ada di catalog

**Pros**:

- Tidak ada kolom yang selalu kosong.

**Cons**:

- Baseline hanya punya satu kolom, sehingga "matrix versi per OS" di scope dan mockup tidak terpenuhi.

### Keputusan 7: surface IPC

#### Option 1: `catalog.list_providers` dan `catalog.get_provider` tanpa halaman (dipilih)

**Pros**:

- List kecil untuk kartu, detail lengkap hanya untuk yang dipilih.
- Batas catalog (64 provider, 64 planned, 32 artefak per provider, dan batas teks) menjamin balasan di bawah 1 MiB, dibuktikan test, jadi pagination tidak perlu.
- Area `catalog` milik spec ini, tidak bercampur dengan area kode `provider` spec 0009.

**Cons**:

- Batas catalog menjadi kontrak; menaikkannya butuh cek ulang ukuran frame.

#### Option 2: satu `catalog.get` yang mengirim seluruh catalog

**Pros**:

- Satu call.

**Cons**:

- Detail semua provider dikirim walau hanya satu yang dilihat; lebih dekat ke batas frame.

#### Option 3: `provider.list` dan `provider.inspect`, sama dengan nama CLI

**Pros**:

- Nama method sama dengan perintah CLI.

**Cons**:

- Area `provider` milik spec 0009 untuk kode error; method catalog di area itu mencampur dua pemilik.

### Keputusan 8: pemilik primitive pin

#### Option 1: kontrak ditulis di spec ini, dibangun dan diverifikasi fitur 20 (dipilih setelah cross check)

**Pros**:

- Bentuk pin dan baris sumber diputuskan di tempat yang tahu catalog, dan menjawab baris Follow-up spec 0009 tentang `ValidatedPackage::files()`.
- Kode dibangun bersama satu satunya pemanggilnya (create instance), jadi tidak ada kode mati di fitur 17 dan bug seperti perbandingan `versions.json` (cross check S1) ketahuan oleh test yang memakai pemanggil nyata.
- Fitur 17 tidak butuh update spec 0004 dan 0005, sehingga prasyarat di luar fitur 17 tinggal accessor spec 0009.

**Cons**:

- Fitur 20, yang sudah besar dan bertier GA, mendapat pekerjaan tambahan.

#### Option 2: spec ini, dibangun fitur 17 (pilihan draft pertama)

**Pros**:

- Catalog tahu sumber dan file paket, jadi pin adalah batas alami catalog ke folder data.

**Cons**:

- Fitur 17 membangun kode yang baru dipakai fitur 20, dengan tiga update spec lain dan tulis registry di setiap start, padahal Done when fitur 17 tidak membutuhkannya.

#### Option 3: fitur 20 memiliki pin seluruhnya, termasuk desain

**Pros**:

- Kode dibangun bersama pemakainya.

**Cons**:

- Fitur 20 harus tahu bentuk catalog dan baris sumber, dan fitur 37 harus mengulanginya untuk paket lokal.

### Keputusan 9: baris `provider_sources`

Bagian dari kontrak fitur 20 (Keputusan 8); dibangun fitur 20.

#### Option 1: dipastikan sekali saat daemon start (dipilih)

**Pros**:

- Satu penulis, sebelum ada operasi yang butuh `source_id`; tidak ada balapan insert.
- Pengaturan di Settings dan onboarding (fitur 16, 32, 37) punya baris untuk dibaca.

**Cons**:

- Satu tulis registry saat start, dan jalur gagal tulis yang harus terlihat.

#### Option 2: dibuat saat pin pertama di batch operasi

**Pros**:

- Tidak ada tulis saat start.

**Cons**:

- Dua create instance bersamaan bisa berebut insert yang sama.

#### Option 3: tanpa baris, `source_id` NULL

**Pros**:

- Paling sedikit kode.

**Cons**:

- Provider revision tidak tahu asalnya, padahal trust fitur 37 dan revocation nanti butuh itu.

### Keputusan 10: tile dan mark

#### Option 1: petunjuk `mark` dan `tile` di `catalog.toml`, dengan default (dipilih)

**Pros**:

- Tampilan mockup bisa diikuti tanpa menambah field presentasi ke manifest provider.
- Warna sistem bernama, jadi light, dark, dan kontras diurus macOS.

**Cons**:

- Data presentasi di file catalog; paket lokal fitur 37 butuh default sendiri.

#### Option 2: diturunkan dari nama dan kategori

**Pros**:

- Tanpa data tambahan.

**Cons**:

- Tidak bisa menghasilkan mark mockup seperti "Vk" atau warna merek.

#### Option 3: SF Symbol per kategori

**Pros**:

- Konsisten dan native.

**Cons**:

- Semua database tampak sama; menyimpang dari mockup.

### Keputusan 11: tone status catalog

#### Option 1: menurut kolom Arti spec 0012 (dipilih)

**Pros**:

- Aturan spec 0012 untuk status set baru diikuti; biru tetap berarti pekerjaan yang sedang berjalan di seluruh app.

**Cons**:

- "Available" tampil netral, tidak biru seperti mockup.

#### Option 2: warna mockup apa adanya

**Pros**:

- Sama persis dengan mockup.

**Cons**:

- Biru untuk "Available" dan progress install akan berarti dua hal di layar yang sama.

### Keputusan 12: entri planned

#### Option 1: hanya enam provider wajib PRD 5.2 (dipilih)

**Pros**:

- `PLANNED` di PRD 5.1 berarti masuk roadmap; hanya enam provider itu yang benar benar dijanjikan baseline.

**Cons**:

- MariaDB dan RabbitMQ di mockup tidak tampil.

#### Option 2: ikut mockup, termasuk kandidat PRD 5.3

**Pros**:

- Sama dengan mockup.

**Cons**:

- PRD 5.3 menyebut daftar itu backlog, bukan janji; menampilkannya sebagai planned mengklaim lebih dari yang direncanakan.

### Keputusan 13: kapan provider pindah dari planned ke provider

Ditambah dari cross check (S10): draft pertama memasukkan NATS sebagai provider tetapi menahan Traefik, tanpa satu aturan yang menjelaskan bedanya.

#### Option 1: di commit fitur yang membuktikan lifecycle nya end to end (dipilih)

**Pros**:

- Satu aturan yang bisa dicek di review untuk keenam provider: install, create, start, readiness, dan stop lulus integration test di Mac ini sebelum provider tampil tersedia.
- Tidak ada provider yang tampil bisa dipasang sebelum benar benar bisa dijalankan.

**Cons**:

- Catalog bawaan fitur 17 hanya berisi planned sampai fitur 19 dan 23 selesai; test fitur 17 dan 18 memakai catalog fixture.

#### Option 2: saat paketnya lolos validasi spec 0009

**Pros**:

- NATS dan Traefik langsung tersedia.

**Cons**:

- Validasi paket tidak membuktikan lifecycle; pengguna bisa memasang runtime untuk provider yang belum pernah dijalankan.

## Rationale

Kekuatan yang paling menentukan adalah BR-016 dan AC-002: status unsupported harus sama di tiga tempat dan penolakannya terjadi di daemon. Itu yang membuat status dihitung daemon (Keputusan 6), gate install menjadi fungsi core yang dipanggil `runtime.install` (disepakati dengan fitur 18), dan GUI serta CLI hanya menampilkan DTO yang sama, dengan fixture bersama sebagai bukti kesetaraan. Kekuatan kedua adalah kejujuran waktu dan isi (NFR-014, BR-025, "tidak mengklaim lebih baru"): tanggal terbit datang dari keputusan maintainer yang dijaga ledger digest isi (Keputusan 3), `supported` hanya dari compatibility record fitur 18, dan provider yang belum dibuktikan tetap planned (Keputusan 2, 12, dan 13).

Bentuk folder data (Keputusan 1) dipilih karena REQ-033 meminta catalog bawaan dan import lokal hidup berdampingan, dan import lokal pasti datang dari folder. Satu loader untuk keduanya lebih murah dirawat daripada catalog yang ditanam di binary, dan segel bundle spec 0015 memberi integritas yang sama tanpa kerja tambahan. Biaya development (`STACKLY_CATALOG_DIR`) kecil dan sekaligus menjadi jalur test binary. Isolasi per entri (Keputusan 5) mengikuti aturan PRD 11.2 bahwa error satu item tidak mengosongkan layar.

Kontrak pin (Keputusan 8) dan baris sumber (Keputusan 9) ditulis di spec ini karena keduanya adalah batas antara catalog dan folder data, tetapi dibangun fitur 20 karena create instance adalah satu satunya pemanggil (fitur 18 sepakat tidak mem pin) dan Done when fitur 17 tidak membutuhkannya. Pin yang dialamati isi dan tanpa hapus membuat dua create bersamaan aman tanpa lock baru, dan menghindari jalur hapus kedua di luar `remove_owned_tree`. Tone (Keputusan 11) mengikuti spec 0012 walau berbeda dengan mockup, karena scope dan spec menang atas mockup untuk perilaku, dan arti tone adalah aturan lintas layar.

## Keputusan rekomendasi

Keputusan kecil yang diambil langsung dengan opsi rekomendasi (keputusan user 2026-10-19 di log koordinasi: setiap pilihan memakai opsi yang direkomendasikan):

- **Lokasi catalog saat start**: `STACKLY_CATALOG_DIR` dulu, lalu `AppBundle`, lalu tidak ada. Alternatif: tebakan path repo, ditolak karena aturan tanpa path hardcoded spec 0004.
- **Variabel override yang salah**: daemon keluar 1, sama dengan `STACKLY_ROOT` dan `STACKLY_LOG` (aturan fail fast di `AGENTS.md`). Alternatif: catalog `unavailable`, ditolak karena kesalahan konfigurasi development akan tersembunyi.
- **Label catalog override**: `development`, tidak pernah `official`. Alternatif: tetap `official` dengan tanda override, ditolak karena label trust harus jujur.
- **Test seam `testing`**: opsi di `DaemonConfig` saja. Alternatif: variabel environment, ditolak karena bisa dipakai di luar test.
- **Bentuk params detail**: ID lengkap wajib, nama tanpa namespace hanya untuk hint. Alternatif: menerima nama pendek, ditolak karena bisa ambigu setelah fitur 37 menambah namespace lain.
- **Event catalog**: tidak ada sampai fitur 37. Alternatif: `catalog.changed` sekarang, ditolak karena tidak ada pemicunya.
- **`minOs`**: dicek build terhadap minimum app 27.0, bukan saat berjalan. Alternatif: port versi OS di fitur ini, ditolak karena fitur 18 sudah merencanakan `HostFacts`.
- **Sumber artefak**: host plus segmen path pertama. Alternatif: URL lengkap, ditolak karena panjang dan tidak membantu keputusan pengguna.
- **Download dan Disk di panel**: satu nilai atau rentang untuk artefak host. Alternatif: kolom ukuran di matrix, ditolak karena mockup tidak punya kolom itu.
- **Label `supported`**: "Tested" di GUI (copy mockup), `supported` di CLI (nilai wire, aturan spec 0008).
- **Baris capability tetap** (Lifecycle, Readiness probe, Logs): ditampilkan di GUI (mockup) dan sebagai baris `baseline` di CLI supaya keduanya setara, karena setiap paket v1alpha1 wajib punya start, stop, readiness, dan log ditangkap supervisor.
- **Urutan capability**: urutan tampil tetap yang ditulis spec, bukan urutan enum domain. Alternatif: urutan enum, ditolak karena berubah kalau domain menambah varian.
- **Dependencies dan Disk sebelum fitur 18**: "None declared" dan "Not listed". Alternatif: menyembunyikan baris, ditolak karena REQ-002 meminta dependency dan kebutuhan disk terlihat.
- **Kartu planned**: teks sekunder tanpa opacity. Alternatif: opacity 0.6 seperti mockup, ditolak karena menurunkan kontras di bawah WCAG AA.
- **Mark tile**: dekorasi yang dikecualikan dari cek kontras, karena nama yang sama tertulis di sampingnya. Alternatif: warna tile lebih gelap, ditolak karena menyimpang dari warna merek di mockup tanpa menambah informasi.
- **Tombol Import Provider, Get, New Instance**: tidak ditampilkan sampai fitur 37, 18, dan 20. Alternatif: tombol nonaktif dengan alasan, ditolak karena menambah kontrol mati di layar pertama JOURNEY-01.
- **Keputusan install di DTO** (`CatalogVersion.install`): dari gate yang sama dengan `runtime.install`, supaya GUI dan CLI sudah menunjukkan "tidak bisa dipasang di Mac ini" dari daemon sebelum fitur 18 ada. Alternatif: menunggu fitur 18, ditolak karena Done when fitur 17 meminta penolakan di daemon.
- **Entri yang hanya punya artefak host `blocked`**: state `blocked` sendiri. Alternatif: `unsupported` dengan reason blocked, ditolak karena label dan alasan akan bertentangan.
- **Namespace catalog**: hanya `official` (plus `testing` lewat opsi test). Alternatif: semua namespace selain `testing`, ditolak karena trust selalu `official` sedangkan namespace lain milik import lokal fitur 37.
- **Host di DTO** (`CatalogInfo.host`): dikirim daemon supaya GUI bisa menyaring artefak host tanpa menebak target. Alternatif: GUI membaca arch prosesnya sendiri, ditolak karena bisa berbeda dengan daemon.
- **Ukuran**: desimal SI satu desimal di GUI dan CLI. Alternatif: biner (MiB) di CLI, ditolak karena `ByteCountFormatter` gaya file memakai desimal dan keduanya harus sama.
- **`NotConfigured`**: subsystem `unavailable`, dengan catalog kosong default di `SpawnedDaemon` supaya test daemon lama tetap `ok`. Alternatif: `not_built`, ditolak karena arti nilai itu adalah fitur yang belum dibangun.
- **Batas**: artefak per provider 32, dependency per artefak 24, total paket 16 MiB, dan versi di summary 16. Alternatif: batas lebih longgar, ditolak karena anggaran frame 1 MiB dan memori daemon NFR-003 tidak lagi terjamin.

## Kesepakatan antarsesi

- **arc-f18-spec** (2026-10-19): fitur 18 me resolve selector di atas `Catalog::entry` dan `artifacts()` dan tidak pernah membaca `providers/` sendiri; `SupportStatus` di domain dipakai ulang di PAGE-09; `runtime.install` dan `runtime.resolve` memanggil `installability` setelah resolusi dan sebelum `begin`, dengan test end to end di fitur 18; detail `availableTargets` ditambahkan; fitur 18 tidak mem pin dan `artifacts.provider_revision_pk` tetap NULL; compatibility record, `support_status`, `HostFacts` lewat `PlatformInfo::host_facts()`, dan field `versions.json` tambahan (`compatibility`, `unpackedSize`, `entryCount`, `dependencies`, `stable`) milik spec 0018; tombol Get, progress, dan "{n} installed" milik fitur 18.
- **arc-f15-spec** (2026-10-19, draft spec 0015): catalog di `Contents/Resources/catalog/`; helper di `Contents/MacOS/`; `AppBundle::enclosing` dan `resources_dir()` di `stackly-platform` milik spec 0015; `build-app` memanggil `xtask::catalog::build` di dalam proses sebelum signing; build dari Xcode IDE tidak punya catalog dan daemon menanganinya tanpa crash; tidak ada pemeriksaan segel saat berjalan di fitur 15.
- **arc-f18-spec**, permintaan tambahan: test di dalam proses fitur 18 butuh catalog dengan paket `testing/` dan catalog kedua untuk test AC-003 setelah restart. Dijawab dengan `Catalog::for_tests` (fitur `test-support` core) dan `CatalogConfig::Preloaded`, ditambah `CatalogBuilder` dengan `allow_testing_namespace` untuk catalog di `TestRoot`.
- **arc-f19-spec**: diberi tahu cara PostgreSQL masuk catalog (`catalog.toml`, revision, `catalog check --update`, deskripsi manifest sebagai teks kartu); belum ada balasan saat spec ini ditulis. *Aturan masuk catalog* di index menggantikan isi pesan itu: PostgreSQL pindah ke `provider` di commit fitur 19 yang membuktikan lifecycle nya.

## Cross check

Dijalankan 2026-10-19 dengan model lain (Sonnet, read only) atas draft pertama. Semua temuan dikirim ke lead dengan satu rekomendasi masing masing; lead menerima semua rekomendasi atas nama user (aturan di log koordinasi) dan menerima spec. Ringkasan temuan dan perubahannya:

**Gap kelengkapan keputusan**:

- G1, target host tidak ada di DTO: ditambah `CatalogInfo.host`.
- G2, state entri bisa berbunyi "no build" padahal artefak host `blocked` atau `fixture`: state entri diturunkan dari gate, dengan state baru `blocked`.
- G3, aturan sel matrix yang terbuka (campuran arch, isi `arches`, `fixture` di OS lain, `compatibility.platforms`, tanda tangan fungsi pengganti, rumah `SupportReason`): semuanya ditulis di *Aturan status*.
- G4, urutan capability dan tiga baris tetap tidak konsisten: *Urutan capability* tetap, baris `baseline` di CLI, daftar kosong untuk planned dan invalid, baris Requires.
- G5, pesan dan hint kode `catalog.*`: kolom pesan dan hint di *Kode error di wire*.
- G6, komposisi status subsystem dan `NotConfigured` yang membuat daemon development `degraded`: *Kesehatan subsystem*, plus catalog kosong default di `SpawnedDaemon`.
- G7, anggaran frame tidak diturunkan: batas baru dan *Anggaran frame*.
- G8, `unrecognized` hanya untuk tiga enum: tabel *Tone dan label* mengikuti pola spec 0012 dan *Nilai asing* untuk enum lain.
- G9, detail tooling build (`--out` yang sudah ada, folder sementara, `APP_MINIMUM_MACOS`, kapan check berjalan): ditulis di AC-1, *Ledger revision*, dan *Aturan status*; `--update` sekarang hanya menulis ledger JSON, jadi tidak perlu menyunting TOML.
- G10, anomali index: index tidak valid seluruhnya (AC-3).
- G11, sisa GUI dan CLI (subjudul sebelum snapshot, pilihan yang tersaring, item partial, default mark, baris Disk, lisensi ganda di CLI, `--category` dengan JSON, zona waktu golden): ditulis di *CLI*, *Layar Catalog (GUI)*, dan *Value sourcing*.

**Soundness**:

- S1, bug: folder pinned dikunci `manifest_sha256` yang tidak mencakup `versions.json`, jadi pin setelah update catalog gagal `existing_differs`: K-1 hanya membandingkan file yang dicakup digest; `versions.json` salinan pertama yang menang.
- S2, aturan "naikkan revision" tidak dipaksakan: ledger `catalog-revisions.json` dengan `--amend` (Keputusan 3).
- S3, penolakan daemon tidak bisa dibuktikan sebelum fitur 18: milestone 4 tetap `after` fitur 18, dan `CatalogVersion.install` membawa keputusan gate sejak build pertama.
- S4, kalimat CLI yang menyiratkan ada catalog lebih baru, source stale untuk catalog development, dan jam tengah malam buatan: kalimat diganti, source mengikuti lokasi, `published_at` wajib waktu terbit nyata.
- S5, `unrecognized` harus beda dari unknown: lihat G8; `blocked` tetap `caution`.
- S6, kontras mark putih di tile kuning dan cyan: mark dekoratif dan dikecualikan (AC-11).
- S7, MiB di CLI dan MB di GUI: desimal SI di keduanya.
- S8, pin dan `provider_sources` tidak dibutuhkan Done when fitur 17: menjadi *Kontrak untuk fitur 20* (Keputusan 8), milestone pin dihapus, update spec 0004 dan 0005 milik fitur 20.
- S9, namespace dan trust tidak konsisten: hanya `official`.
- S10, NATS tersedia sementara Traefik ditahan: Keputusan 13 dan *Aturan masuk catalog*; isi awal enam planned.
- S11, `minOs` di atas 27.0 menggagalkan build: tetap, dengan Follow-up untuk fitur 18.
- S12, memori paket di daemon: batas total 16 MiB.
- S13, redundansi: field `path` index dibuang, `loadedAt` dipertahankan untuk diagnostics, reason gate sama dengan kode `SupportReason`.
