# 0005. Rationale: data model registry

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: Done when fitur 5 menyentuh tiga hal yang pemiliknya tersebar. "Schema setengah teraplikasi terdeteksi saat daemon start" dimiliki fitur 5, tetapi menampilkan safe recovery state dengan backup yang bisa dipakai restore adalah Done when fitur 48, dan backup sebelum migrasi saat update app adalah fitur 61. "Waktu ditampilkan sesuai locale" baru bisa dibuktikan di CLI (fitur 8) dan GUI (fitur 13). Spec ini membangun deteksi, snapshot, laporan, dan kontrak recovery mode, lalu menyerahkan tampilan dan alur restore ke pemiliknya lewat Follow-up. Spec ini juga membuat tabel untuk entitas yang fiturnya belum punya spec (9, 11, 18, 29, 35, 49); risikonya salah tebak kolom, jadi kolom yang semantiknya milik fitur lain dibuat opsional atau bertipe set terbuka.

Registry adalah satu satunya sumber kebenaran untuk runtime, instance, environment, lease, dan operasi Stackly, dan hanya daemon yang boleh menulisnya (BR-011). PRD bagian 17 mewajibkan 18 entitas logis dengan constraint tertentu, foreign key dan transaksi yang aktif, migrasi berurutan yang diuji dari versi sebelumnya, backup, serta kebijakan forward dan rollback, dan melarang menyalin file SQLite aktif secara sembarangan. Model data adalah bagian paling mahal untuk diulang setelah ada data pengguna.

Spec 0001 sudah mengunci rusqlite dengan fitur `bundled`, satu thread penulis yang menerima perintah dari tokio, jiff untuk waktu UTC, dan UUID v7 yang hanya dibuat daemon. Spec 0001 juga menyerahkan skema, mekanisme migrasi, dan letak port registry ke fitur 5, dan melarang `stackly-testkit` bergantung pada `stackly-core`. Spec 0004 sudah memodelkan aggregate, kode state yang stabil, `ResourceRevision` yang naik tepat 1 per mutasi, path instance yang diturunkan dari ID, dan meminta fitur 5 menyediakan daftar ID `known` untuk `create_instance_layout` serta `RuntimeReferences` untuk `plan_runtime_removal`.

Kekuatan lain yang membentuk keputusan: AC-065 meminta migrasi yang gagal berakhir di safe recovery state tanpa init ulang diam diam dan dengan bukti backup; REQ-048 dan NFR-005 melarang runtime atau instance ganda saat GUI dan CLI mengirim perintah bersamaan, dan melarang SQLite dipakai sebagai mutex selama download atau startup; NFR-011 melarang pertumbuhan registry tanpa batas; PRD 35.2 mengusulkan retensi audit 30 hari; PRD 24.3 mewajibkan backup registry sebelum mutasi schema; fitur 32 meminta perubahan path data hanya berlaku untuk instance baru; AC-040 meminta registry yang dipulihkan di profil uji tetap konsisten. Durability tidak boleh dilemahkan demi demo.

## Options considered

### Keputusan 1: tabel yang dibuat di migrasi pertama

#### Option 1: Semua entitas PRD 17

Delapan belas tabel dengan field inti PRD, foreign key, dan constraint. Kolom yang semantiknya milik fitur tanpa spec dibuat opsional atau set terbuka.

**Pros**:

- Done when terpenuhi apa adanya, dan relasi antar entitas diuji sekali.
- Fitur berikutnya cukup migrasi additive (`ADD COLUMN`), yang murah di SQLite.

**Cons**:

- Sebagian kolom menebak kebutuhan fitur yang belum didesain.

#### Option 2: Hanya entitas yang dimodelkan spec 0004

Runtime, instance, endpoint, environment, demand, operasi, provider revision; sisanya datang bersama fiturnya.

**Pros**:

- Tidak ada tebakan.

**Cons**:

- Done when tidak terpenuhi, dan relasi ke tabel yang datang belakangan butuh rebuild tabel lama.

#### Option 3: Semua, tapi tabel milik fitur lain berupa stub

Tabel tanpa spec hanya berisi ID dan foreign key.

**Pros**:

- Relasi sudah ada tanpa menebak kolom.

**Cons**:

- Constraint PRD untuk tabel itu belum teruji, dan "ada" menjadi klaim kosong.

### Keputusan 2: kebijakan migrasi dan arti rollback

#### Option 1: Maju saja dengan snapshot terverifikasi

Migrasi hanya maju; sebelum migrasi, `VACUUM INTO` membuat snapshot yang diverifikasi. Rollback berarti memasang app lama dan memulihkan snapshot.

**Pros**:

- Satu jalur kode yang diuji; snapshot adalah bukti backup yang diminta AC-065 dan PRD 24.3.

**Cons**:

- Perubahan setelah migrasi hilang saat rollback.

#### Option 2: Down migration per versi

Setiap migrasi punya skrip balik yang diuji.

**Pros**:

- Rollback tanpa kehilangan data selama skrip balik jujur.

**Cons**:

- Biaya test dua kali lipat, dan skrip balik untuk migrasi yang membuang data tidak bisa jujur.

#### Option 3: Maju saja tanpa backup

**Pros**:

- Paling sederhana.

**Cons**:

- Melanggar PRD 17 dan 24.3, dan AC-065 tidak punya bukti backup.

### Keputusan 3: perilaku saat migrasi gagal atau schema tidak cocok

#### Option 1: Recovery mode

Daemon hidup tanpa membaca atau menulis data registry dan tanpa menyentuh proses, hanya menjawab status, doctor, dan laporan recovery; laporan juga ditulis ke file.

**Pros**:

- GUI dan CLI bisa menampilkan state recovery (dibutuhkan fitur 48), dan proses instance yang sedang jalan tidak terganggu.

**Cons**:

- Daemon punya mode kedua yang harus dijaga fitur 6.

#### Option 2: Tolak start

Daemon keluar dengan kode khusus dan baris log.

**Pros**:

- Tidak ada mode kedua.

**Cons**:

- Pengguna hanya melihat daemon mati; fitur 48 harus membaca state dari luar daemon.

### Keputusan 4: durability

#### Option 1: WAL, `synchronous=FULL`, `fullfsync=ON`

**Pros**:

- Setiap commit sampai ke media di macOS, juga saat listrik mati.

**Cons**:

- Setiap commit lebih lambat beberapa milidetik.

#### Option 2: WAL, `synchronous=FULL` tanpa `fullfsync`

**Pros**:

- Lebih cepat dan tetap konsisten setelah crash.

**Cons**:

- `fsync` macOS hanya sampai cache disk, jadi commit terakhir bisa hilang saat listrik mati.

#### Option 3: WAL, `synchronous=NORMAL`

**Pros**:

- Paling cepat.

**Cons**:

- Commit terakhir bisa hilang saat crash OS atau listrik mati; PRD melarang melemahkan durability.

### Keputusan 5: path instance (`instance_paths`)

#### Option 1: Disimpan relatif terhadap root bernama

Satu baris per jenis path dengan nama root (`data`, `logs`, `runtime`) dan path relatif, ditulis saat create dari `DirectoryLayout`.

**Pros**:

- Tetap benar saat registry dipulihkan di profil atau root lain (AC-040, `STACKLY_ROOT`), dan memberi tempat untuk fitur 32.
- Entitas PRD ada sebagai tabel, dengan path unik lintas instance.

**Cons**:

- Duplikat dari hasil layout yang harus dijaga sama.

#### Option 2: Disimpan sebagai path absolut

**Pros**:

- Persis PRD dan paling mudah dibaca.

**Cons**:

- Rusak saat root berpindah, home berganti nama, atau registry dipulihkan di profil lain.

#### Option 3: Diturunkan saja

**Pros**:

- Persis spec 0004, tanpa duplikasi.

**Cons**:

- Entitas PRD tidak ada sebagai tabel, dan fitur 32 butuh migrasi nanti.

### Keputusan 6: keunikan nama instance

#### Option 1: Di antara instance yang tidak retained

**Pros**:

- Remove membebaskan nama, seperti yang diharapkan pengguna; path tetap dari ID sehingga tidak ada overwrite (AC-066).

**Cons**:

- Dua record bisa bernama sama (satu retained), jadi adoption (fitur 57) harus meminta nama baru saat bentrok.

#### Option 2: Di semua instance

**Pros**:

- Nama selalu menunjuk satu record.

**Cons**:

- Pengguna harus purge atau rename instance retained sebelum memakai namanya lagi.

### Keputusan 7: format waktu

#### Option 1: Teks RFC 3339 UTC dengan sembilan digit pecahan

**Pros**:

- Lebar tetap sehingga urutan teks sama dengan urutan waktu, terbaca di `sqlite3`, dan tanpa kehilangan presisi jiff.

**Cons**:

- 30 byte per nilai.

#### Option 2: Integer nanodetik sejak epoch

**Pros**:

- Ringkas dan cepat dibandingkan.

**Cons**:

- Tidak terbaca langsung saat debug, dan mudah tertukar satuan.

#### Option 3: Teks RFC 3339 presisi milidetik

**Pros**:

- Lebih pendek dan umum.

**Cons**:

- Presisi hilang, sehingga perbandingan `observed_at` di domain bisa salah setelah dimuat ulang.

### Keputusan 8: retensi

#### Option 1: Umur dan batas jumlah

**Pros**:

- Mengikuti PRD 35.2 (30 hari) dan tetap terbatas walau aktivitas tinggi (NFR-011).

**Cons**:

- Dua angka per tabel yang harus dijelaskan di Settings nanti.

#### Option 2: Batas jumlah saja

**Pros**:

- Satu angka.

**Cons**:

- Tidak sesuai PRD 35.2.

#### Option 3: Simpan semua sampai fitur 35

**Pros**:

- Tidak menebak kebijakan.

**Cons**:

- Melanggar NFR-011 selama berbulan bulan pengembangan.

### Keputusan 9: letak port registry

#### Option 1: `stackly-domain`, fake di `stackly-testkit`

**Pros**:

- Aturan spec 0001 tetap (testkit tidak bergantung pada core), core diuji dengan fake testkit sesuai AGENTS.md, dan storage tidak perlu bergantung pada core.

**Cons**:

- Domain memuat trait berbentuk persistence, bukan hanya aturan bisnis.

#### Option 2: `stackly-core`, fake di core

**Pros**:

- Lapisan Clean Architecture klasik (port milik use case).

**Cons**:

- Fake harus di modul test core, menyimpang dari aturan fake di testkit, dan test di luar core tidak bisa memakainya.

#### Option 3: Belum ada port

**Pros**:

- Kode paling sedikit sekarang.

**Cons**:

- Fitur 7 dan 20 bisa memutuskan bentuk akses registry sendiri sendiri.

#### Option 4: Crate port baru

**Pros**:

- Lapisan paling bersih.

**Cons**:

- Satu crate dan satu aturan `check-deps` lagi untuk satu trait.

### Keputusan 10: jalur baca

#### Option 1: Satu thread untuk baca dan tulis

**Pros**:

- Baca selalu melihat tulisan terakhir, tanpa pool, dan cek lalu tulis di dalam satu perintah bebas balapan.

**Cons**:

- Baca panjang menunda tulis.

#### Option 2: Penulis ditambah pool baca

**Pros**:

- Baca tidak antre di belakang tulis.

**Cons**:

- Lebih banyak kode dan aturan konsistensi baca setelah tulis, untuk beban yang belum terukur.

### Keputusan 11: readiness dan health di registry

#### Option 1: Hanya di memori

**Pros**:

- Probe berkala tidak memicu tulis `fullfsync`; nilainya pasti basi setelah restart (BR-010).

**Cons**:

- Nilai terakhir sebelum daemon mati tidak bisa dilihat dari registry.

#### Option 2: Disimpan saat berubah

**Pros**:

- Nilai terakhir tercatat.

**Cons**:

- Tetap harus dibaca `unknown`, jadi datanya hanya menambah tulis.

### Keputusan 12: registry hilang tetapi ada data

#### Option 1: Recovery kalau ada sisa

**Pros**:

- Registry yang hilang tidak pernah diganti diam diam (AC-065), dan WAL yatim tidak pernah dipasangkan ke database baru.

**Cons**:

- Pengguna terhenti sampai restore (fitur 48) atau adoption (fitur 57) tersedia.

#### Option 2: Selalu buat baru

**Pros**:

- Lebih mulus.

**Cons**:

- Kehilangan registry tidak terlihat, dan folder instance menjadi orphan tanpa peringatan.

### Keputusan 13: runner migrasi

#### Option 1: Runner sendiri

**Pros**:

- Snapshot, sidik jari, satu transaksi, dan laporan recovery dibuat persis seperti yang dibutuhkan, dalam kira kira 200 baris.

**Cons**:

- Kode yang harus dirawat sendiri.

#### Option 2: `rusqlite_migration`

**Pros**:

- Crate kecil yang sudah dipakai banyak proyek.

**Cons**:

- Berbasis `user_version` saja, tanpa checksum dan sidik jari; backup dan recovery tetap ditulis sendiri.

#### Option 3: `refinery`

**Pros**:

- Tabel riwayat dengan checksum.

**Cons**:

- Framework multi database yang lebih berat; alur recovery tetap harus dibuat sendiri.

### Keputusan 14: transaksi saat beberapa migrasi tertunda

#### Option 1: Satu transaksi untuk semua

**Pros**:

- Registry tidak pernah tertinggal di versi antara yang tidak pernah dipakai app mana pun.

**Cons**:

- Upgrade besar menahan transaksi lebih lama (masih hanya saat start).

#### Option 2: Satu transaksi per migrasi

**Pros**:

- Kemajuan tersimpan per versi.

**Cons**:

- Kegagalan bisa meninggalkan registry di versi antara yang tidak pernah dirilis.

### Keputusan 15: deteksi schema setengah teraplikasi

#### Option 1: Versi, checksum, dan sidik jari schema

**Pros**:

- Menangkap juga tabel atau index yang diubah tangan atau oleh build lama yang salah.

**Cons**:

- Migrasi dibatasi aturan SQL ketat supaya sidik jari tidak bergantung versi SQLite.

#### Option 2: Versi dan checksum saja

**Pros**:

- Lebih sedikit kode.

**Cons**:

- Perubahan schema di luar runner tidak terdeteksi.

### Keputusan 16: fixture registry versi sebelumnya

#### Option 1: Dump SQL teks per versi

**Pros**:

- Diff terbaca dan membekukan bentuk versi lama walau migrasinya diedit.

**Cons**:

- Tidak menguji detail format file biner (header, page size).

#### Option 2: File `.sqlite` biner

**Pros**:

- Persis seperti di disk pengguna.

**Cons**:

- Diff git tidak terbaca.

#### Option 3: Dibangun dari migrasi saat test

**Pros**:

- Tanpa file tambahan.

**Cons**:

- Tidak melindungi dari migrasi lama yang diedit.

### Keputusan 17: kode repository di fitur ini

#### Option 1: Entitas spec 0004 dan query yang diminta

**Pros**:

- Walking skeleton (fitur 14) tinggal merangkai, dan follow up spec 0004 tertutup.

**Cons**:

- Tabel lain belum punya kode sampai fitur pemiliknya.

#### Option 2: Semua 18 tabel

**Pros**:

- Semua siap pakai.

**Cons**:

- Banyak kode menebak kebutuhan fitur tanpa spec.

#### Option 3: Hanya query yang diminta spec 0004

**Pros**:

- Paling sedikit kode.

**Cons**:

- Fitur 7, 14, dan 20 masing masing menulis pemetaan aggregate.

### Keputusan 18: menjaga kode enum di SQLite

#### Option 1: CHECK untuk set tertutup

**Pros**:

- Kode state machine yang stabil dijaga database; daftar yang tumbuh tidak butuh rebuild.

**Cons**:

- Menambah kode ke set tertutup butuh rebuild tabel.

#### Option 2: CHECK untuk semua kode

**Pros**:

- Paling ketat.

**Cons**:

- Setiap `OperationKind` baru berarti rebuild tabel `operations`.

#### Option 3: Tabel lookup dengan foreign key

**Pros**:

- Menambah kode cukup `INSERT`.

**Cons**:

- Banyak tabel kecil dan join untuk nilai yang sudah divalidasi Rust.

## Rationale

**Keamanan data lebih dulu (keputusan 2, 3, 4, 12, 14, 15).** AC-065 dan larangan melemahkan durability menentukan semuanya. Satu transaksi untuk semua migrasi membuat "setengah teraplikasi" hanya mungkin dari luar runner, dan sidik jari schema menangkap kasus itu. Snapshot terverifikasi sebelum migrasi adalah backup yang diminta PRD 24.3 sekaligus jalan rollback yang jujur, jadi down migration tidak perlu. Recovery mode dipilih karena fitur 48 harus menampilkan state recovery dari daemon, dan karena proses instance yang sedang jalan tidak boleh disentuh saat registry tidak bisa dipercaya. Registry yang hilang dengan sisa data diperlakukan sama, karena membuat registry baru di atas WAL yatim atau folder instance adalah bentuk init ulang diam diam. `fullfsync` dipilih karena registry jarang ditulis dan baca tidak terpengaruh, sementara `fsync` biasa di macOS tidak menjamin apa pun saat listrik mati.

**Bentuk skema (keputusan 1, 5, 6, 7, 11, 18).** Semua entitas sekarang, karena relasi adalah bagian yang paling mahal diubah dan kolom tambahan murah. Path disimpan relatif terhadap root bernama karena path absolut patah di skenario restore AC-040 dan di `STACKLY_ROOT`, sedangkan derivasi murni tidak memberi tempat untuk fitur 32. Nama hanya unik di antara instance aktif karena remove harus terasa membebaskan nama, dan path berbasis ID sudah mencegah overwrite. Waktu teks lebar tetap menjaga urutan, keterbacaan, dan presisi jiff sekaligus. Readiness dan health tidak disimpan karena nilainya basi setelah restart dan menulisnya akan menjadi beban `fullfsync` terbesar. CHECK hanya untuk set tertutup karena `OperationKind` dan kode milik fitur lain akan tumbuh.

**Akses (keputusan 9, 10, 13, 16, 17).** Port di domain adalah satu satunya letak yang memenuhi tiga aturan sekaligus: testkit tidak bergantung pada core, fake ada di testkit, dan storage tidak perlu core. Satu thread untuk baca dan tulis membuat cek bertipe dan tulis berada di satu perintah tanpa balapan, sesuai pola satu penulis spec 0001; beban 100 instance jauh di bawah batas NFR-001, dan pool baca bisa ditambah tanpa mengubah API kalau checkpoint performa membuktikan perlu. Runner sendiri dipilih karena bagian yang sulit (snapshot, sidik jari, recovery) tidak disediakan crate migrasi mana pun. Dump SQL teks membekukan versi lama dan tetap bisa dibaca di review. Kode repository untuk entitas spec 0004 menutup kebutuhan walking skeleton tanpa menebak fitur lain.

### Keputusan kecil

| Keputusan | Pilihan | Alasan | Runner up |
| --- | --- | --- | --- |
| Bentuk ID | Teks UUID lowercase dengan CHECK | Terbaca, cocok dengan wire dan manifest | `BLOB` 16 byte (lebih kecil, tidak terbaca) |
| Kunci provider revision dan artefak | `INTEGER` internal plus unique kunci alami | FK pendek; kunci internal tidak pernah keluar crate | FK komposit tiga kolom teks |
| Mode tabel | `STRICT` | SQLite menolak tipe yang salah | Tabel biasa dengan CHECK `typeof` |
| Penjaga konkurensi lintas aggregate | `generation` per registry plus CAS per resource | Satu angka menjaga keputusan yang dibaca dari banyak baris (misalnya konflik endpoint) | Enum precondition per kasus |
| Error constraint | Cek `SELECT` lebih dulu di transaksi yang sama, constraint SQL sebagai penjaga terakhir | Error bertipe tanpa mengurai pesan SQLite | Mengurai pesan constraint |
| Snapshot | `VACUUM INTO` lalu sync, rename, verifikasi | Satu perintah, hasil ringkas dan konsisten | Online Backup API (butuh fitur rusqlite `backup`) |
| Jumlah snapshot | Tiga terbaru, plus yang disebut laporan | Cukup untuk rollback beberapa versi, ukuran kecil | Simpan semua |
| Laporan recovery | File JSON bertanggal di `registry/recovery/`, dedupe dengan laporan terakhir | Bukti tetap ada walau daemon mati | Satu file yang ditimpa |
| Create registry baru | File sementara yang dikunci, rollback journal, lalu publikasi lewat `hard_link` | File `stackly.sqlite` yang ada selalu lengkap, create terputus tidak terlihat sebagai registry rusak, dan dua start bersamaan tidak pernah saling menimpa | Create langsung di tempat, atau `rename` (menimpa diam diam) |
| Urutan cek saat open | Inspeksi lewat koneksi read only, baru open tulis untuk Ready atau migrasi | Pragma WAL dan checkpoint saat tutup mengubah file; keputusan recovery tidak boleh mengubah bukti | Satu koneksi tulis sejak awal |
| Sidik jari yang diharapkan | Konstanta per versi, normalisasi whitespace dan tanda kutip | Registry pengguna dibuat SQLite versi lama; menghitung ulang dengan SQLite baru bisa memberi recovery palsu | Menghitung ulang di memori saat open |
| Seed untuk test | Trait `RegistrySeed` di testkit, koneksi samping di test storage | Tabel tanpa `WriteOp` tetap bisa diuji tanpa membuka API tulis publik | `WriteOp` sementara untuk semua tabel |
| Urutan endpoint dan member | Kolom `position` | Urutan `Vec` di domain terjaga persis | Mengurutkan saat baca dan memaksa domain ikut terurut |
| Aksi on delete | Restrict secara default, cascade untuk baris anak murni, set null untuk riwayat | Data tidak pernah ikut terhapus diam diam | Cascade di mana mana |
| FK ke `operations` | Tidak ada | Riwayat operasi dipangkas retensi tanpa mematahkan instance dan environment | FK dengan set null |
| Nama tabel | Persis PRD 17 (`instance_demands` untuk Lease) | Trace PRD utuh | Nama domain (`leases`) |
| Akses `rusqlite` | Hanya `stackly-storage` | BR-011 dijaga compiler dan `check-deps` | Konvensi saja |
| Kode keluar recovery sebelum fitur 6 | 3, ditandai provisional | Test bisa membedakannya dari error umum (1) | Keluar 0 dengan log |

### Hasil cross check

Cross check read only oleh model lain (Sonnet 5.5) pada 2026-10-18. Engineer memilih menerapkan semua perbaikan yang disarankan. Yang diterapkan:

- Field domain yang hilang (`revision` di Runtime, Environment, Operation; `retained_by` di Instance) masuk Follow-up spec 0004, dan *Pemetaan aggregate* menulis aturan kolom tanpa field domain.
- Registry memegang `Arc<dyn Clock>`; readiness dan health yang dimuat memakai `opened_at`, bukan waktu tiap baca, supaya probe baru tidak dianggap basi.
- `RegistrySeed` untuk tabel tanpa `WriteOp`; modul `files` berbasis `std::fs` untuk operasi file milik storage.
- Kontrak `BatchError` dengan `op_index` opsional, `ResourceRef` bertipe, urutan cek tetap, error bertipe lengkap.
- `InsertInstance` menghitung path sendiri dari ID; test path tidak sama dan tidak bertumpuk.
- Detail recovery: coba ulang migrasi dengan snapshot yang dipakai ulang, nama tanpa menimpa, dedupe tanpa `-shm`, sha256 dicatat setelah koneksi ditutup, pembersihan `.tmp`, daftar `StorageError`, reason `open_failed`.
- `MigrationSet` membawa sidik jarinya sendiri; rebuild di migrasi test menyasar `provider_revisions`.
- Detail retensi (hanya baris yang boleh dihapus, `<`, `Duration`, tanpa generation, per 5.000 baris), kolom `position`, JSON terurut, empat rename di peta field PRD, dua index foreign key.
- Test daemon tidak lagi menulis ke home asli (`--roots-only`), dan kasus versi 999 memakai tambalan header.
- Inspeksi read only sebelum open tulis, probe `BEGIN IMMEDIATE` untuk `registry_locked`, kunci dan `hard_link` untuk create, bukti `registry_missing` diperluas, batas ulang `GenerationConflict`, dan AC-12 dirumuskan ulang.

Dua saran dikoreksi:

- `checkpoint_fullfsync` tidak menambal lubang durability: dokumentasi SQLite menyatakan `fullfsync=ON` sudah memakai F_FULLFSYNC untuk semua sync, termasuk checkpoint. Pragma itu tetap dipasang sebagai pengaman eksplisit.
- Usul menghitung sidik jari yang diharapkan dengan memutar ulang migrasi di memori ditolak (lihat *Keputusan kecil*); sebagai gantinya normalisasi membuang tanda kutip.

Tidak diterapkan: trigger yang menolak `DELETE` pada `provider_revisions`. Immutability di sini berarti isi manifest tidak berubah; revision yang tidak dirujuk mungkin sah dihapus oleh garbage collection (fitur 55, 59), dan foreign key restrict sudah melindungi revision yang dipakai.

## Perubahan 2026-10-18 (rekonsiliasi lintas spec)

Pengecekan silang Assumed lines antara spec 0005, 0006, 0009, 0011, dan 0012 menemukan selisih yang keputusannya milik spec ini. Engineer menyetujui semua perbaikan berikut, sebelum migrasi `0001_registry.sql` pernah dirilis, jadi belum ada data yang perlu dimigrasi.

| Selisih | Perbaikan | Alasan | Runner up |
| --- | --- | --- | --- |
| Kolom `secret_references` (`store_key`, `scope`, `purpose` berpola `code`) tidak bisa menampung model logis spec 0011, dan purpose seperti `admin-password` ditolak CHECK `code` | kolom spec 0011 di migrasi 0001, plus tabel `secret_reference_uses`; AC-1 dan AC-12 menyesuaikan; `scope` PRD dipetakan ke `owner_kind` plus `owner_id` | spec ini memegang skema fisik, spec 0011 memegang model logisnya, dan mengubah migrasi yang belum dirilis lebih murah daripada migrasi v2 | fitur 11 menambah migrasi v2 yang membangun ulang tabel |
| Field secret di `config_values` disebut "ID referensi", padahal spec 0009 dan 0011 menyimpan teks `local:<path>` | teks reference | reference bukan secret dan terbaca di diagnostics; ID saja butuh join untuk setiap render | ID referensi |
| `result` audit berpola `code`, padahal spec 0011 menulis `failed:<kode>` | `result` hanya `succeeded` atau `failed`, kode error di `safe_details.errorCode` | kode wire memuat titik, dan kolom kode yang tertutup tetap mudah difilter | pola `result` dilonggarkan |
| Belum ada spec yang mendefinisikan kode `registry.*` | tabel *Kode error di wire* dan AC-17; CAS memakai `resource.revision_mismatch`, data hilang memakai `resource.not_found`, recovery memakai `registry.recovery_mode` | spec 0006 menyerahkan area `registry` ke spec ini; semua `retryable` false karena kegagalan registry di tengah request tidak menjamin request itu belum berefek | kode generik `registry.error` dengan detail varian |
| `OperationCursor` bernanodetik, wire hanya tiga digit | cursor tidak pernah lewat `Timestamp` wire; spec 0007 mengirimnya sebagai string opaque | baris di milidetik yang sama akan terlewat atau terulang kalau presisi dipotong | membandingkan cursor di milidetik |

Cross check model lain atas update ini menambah, atas persetujuan engineer: trigger `secret_references_owner_fixed`, penghapusan eksplisit baris `secret_reference_uses` saat purge, method baca dan varian `ResourceRef` secret yang dirancang fitur 11 lewat update spec ini sebelum langkah core nya, tanda `allowed_in_recovery` dan percobaan ulang `registry_locked` dari spec 0006, dan aturan bahwa langkah 7 memakai `--print-roots` kalau sudah ada di main.

