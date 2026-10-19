# 0020. Rationale: create instance

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: scope fitur 20 terdengar seperti satu sheet, padahal isinya lima hal yang saling mengunci: form dari settings schema, preflight, operasi create yang aman, pin paket provider dari catalog, dan perintah CLI yang setara. Memecahnya menjadi beberapa spec akan membuat aturan validasi tersebar di tiga tempat (GUI, CLI, daemon), dan justru itu yang dilarang AC-028. Spec ini tetap satu dengan satu evaluator sebagai poros. Risiko kedua: hampir semua bahan fitur ini belum dibangun (supervisor fitur 10, secret fitur 11, catalog fitur 17, runtime fitur 18, PostgreSQL fitur 19, komponen fitur 67), dan enam spec lain butuh tambahan kecil. Build plan menaruh pekerjaan di atas fake lebih dulu supaya bisa jalan begitu prasyarat core ada, dan bukti PostgreSQL nyata paling akhir. Ketiga: PAGE-04 menyebut autostart, tetapi tabel layar di `ui-macos.md` menaruh Start at login di fitur 31; spec ini mengikuti `ui-macos.md` dan hanya membangun Start after creating.

Pengguna Stackly membuat instance setelah memasang runtime (JOURNEY-01). PRD meminta create yang menghasilkan ID stabil, nama, provider revision, runtime ID, credential reference, config revision, endpoint, direktori, dan startup policy, dan yang hanya menginisialisasi target baru yang tervalidasi tanpa menimpa data (REQ-008, BR-008, AC-008). Endpoint harus divalidasi sebagai tuple alamat, transport, dan port dengan wildcard dan IPv4 serta IPv6 diperiksa, port fixed tidak pernah diganti diam diam, dan port otomatis baru ditampilkan setelah bind (REQ-010, BR-004, BR-005, VAL-004 sampai VAL-006, AC-055, AC-056). Path tidak boleh keluar dari owned root atau bertumpuk dengan data instance lain (VAL-007, BR-002), dan data directory harus sesuai owner, permission, filesystem, ruang, runtime, dan lock (VAL-008). Form harus berasal dari schema dengan kelas field VAL-012, sehingga provider baru tidak butuh perubahan SwiftUI (REQ-029, AC-029). Alurnya harus bisa diselesaikan dengan keyboard dan VoiceOver (AC-027, NFR-012), dan CLI `--json` harus memberi semantik validasi dan error yang sama dengan GUI (AC-028).

Fondasi yang sudah diputuskan: spec 0004 punya `InstanceName`, `Endpoint`, `endpoints_conflict`, `resolve_port`, `create_instance_layout`, `check_owned_path`, dan `check_data_root_overlap`, dengan validasi endpoint di `Instance::new` sengaja ditinggalkan untuk fitur ini. Spec 0005 tidak memeriksa konflik endpoint lintas instance dan meminta pemanggil memakai `expected_generation`. Spec 0007 memberi engine operasi dengan kompensasi sebelum titik tak bisa dibalik dan menyerahkan perilaku setelah titik itu untuk `create_instance` kepada fitur ini. Spec 0009 mem parse settings schema menjadi model bertipe tetapi menyimpannya dalam peta terurut nama. Spec 0010 sudah punya `instance.create` minimal yang mencari paket yang sudah pinned dan menjalankan `execute_init`, tanpa preflight, tanpa form, dan dengan nama socket Unix yang diasumsikan literal. Spec 0011 menyimpan secret lewat `SecretService::put` dengan reference yang diturunkan dari ID instance. Spec 0017 menulis kontrak pin (K-1) dan baris sumber catalog (K-2) untuk dibangun di sini. Spec 0019 memakai `socketName` yang memuat token port (`.s.PGSQL.${endpoint.client.port}`) dan field password `printableOnly`.

Fakta yang membatasi: semua mutasi lewat daemon (spec 0001), GUI hanya klien IPC tanpa Rust, protokol punya aturan enum terbuka dan tanpa `u64` atau `BTreeMap` di DTO (spec 0006), dan registry punya satu generation untuk seluruh database sehingga commit progress operasi apa pun bisa membuat commit lain dengan `expected_generation` gagal. Mockup menampilkan sheet New Instance dengan filter kategori, runtime, nama, port Fixed atau Automatic dengan tombol saran, Start after creating, Start at login, enam baris preflight, perintah CLI dengan Copy, Cancel, dan Create. Pendekatan proyek adalah Journey.

## Options considered

### Keputusan 1: permukaan IPC untuk form dan preflight

#### Option 1: dua method baca baru di atas satu evaluator yang juga menjadi precheck create (dipilih)

`instance.get_create_form` membangun form dari paket dan runtime; `instance.preflight` menjalankan evaluasi yang sama dengan precheck `instance.create` tanpa menulis apa pun.

**Pros**:

- Satu fungsi `evaluate` untuk GUI, `--dry-run`, dan create, jadi kode error dan field error sama secara struktural (AC-028).
- Preflight tidak butuh idempotency key, tidak membuat operasi, dan tidak pernah menerima nilai secret.
- Error sinkron create tetap ada untuk klien yang tidak memanggil preflight (script).

**Cons**:

- Dua method dan beberapa DTO baru di protokol.
- Precheck berjalan di gerbang `begin` engine, jadi IO nya harus dibatasi (bind percobaan dan probe Keychain tanpa dialog).

#### Option 2: flag `dryRun` di `instance.create`

**Pros**:

- Satu method saja.

**Cons**:

- Method pemula operasi wajib idempotency key dan pencarian key; dry run berulang per ketikan akan mengotori atau berbenturan dengan aturan itu.
- Params create membawa nilai secret, jadi preflight per ketikan akan mengirim password berulang kali lewat IPC.

#### Option 3: validasi di client, daemon hanya saat create

**Pros**:

- Tidak ada round trip saat mengetik.

**Cons**:

- Aturan pattern, konflik port, filesystem, dan registry hanya bisa dicek daemon; GUI dan CLI akan punya dua salinan aturan yang berbeda.
- Melanggar prinsip spec 0009 bahwa pattern dievaluasi daemon.

### Keputusan 2: bentuk form di wire

#### Option 1: DTO `FormField` bertipe yang dibangun daemon dari `SettingsSchema` (dipilih)

**Pros**:

- Swift tidak perlu mem parse JSON Schema; jenis, batas, default, dan kelas VAL-012 sudah bertipe.
- Jenis field baru bisa ditambah lewat enum terbuka dengan perilaku `unrecognized` yang aman.

**Cons**:

- Butuh newtype integer bertanda (`SignedSafeInteger`) dan enum nilai settings di protokol.

#### Option 2: kirim isi `settings.schema.json` mentah

**Pros**:

- Tanpa DTO baru untuk field.

**Cons**:

- Swift harus menafsirkan dialek terbatas spec 0009 sendiri, salinan kedua dari aturan yang sama.

#### Option 3: perluas `catalog.get_provider` dengan schema

**Pros**:

- Memakai method yang sudah ada.

**Cons**:

- Catalog tidak tahu runtime dan registry (nama saran, sumber pinned), dan detail catalog menjadi jauh lebih besar untuk layar yang tidak butuh schema.

### Keputusan 3: revision provider yang dipakai instance baru

#### Option 1: revision catalog yang dimuat saat create, di pin saat itu (dipilih, saran spec 0017)

**Pros**:

- Instance baru selalu mendapat paket terbaru yang dibawa app, dan instance lama tetap pada revision nya (BR-001, BR-017).
- Pin terjadi tepat sekali per revision dan alamatnya berdasarkan isi, jadi aman dipanggil bersamaan.

**Cons**:

- Dua instance yang dibuat sebelum dan sesudah update app bisa memakai revision berbeda; ini disengaja tetapi perlu terlihat (fitur 59).

#### Option 2: revision pinned terbaru di registry

**Pros**:

- Konsisten antar instance.

**Cons**:

- Revision di registry tidak punya urutan yang bisa dipercaya (`RevisionLabel` hanya label), dan perbaikan paket di app baru tidak pernah sampai ke instance baru.

#### Option 3: revision wajib dikirim client

**Pros**:

- Eksplisit.

**Cons**:

- GUI dan CLI harus mencari revision dulu; tidak menambah keamanan karena daemon tetap memeriksa.

### Keputusan 4: port fixed yang konflik saat create

#### Option 1: tolak dengan saran port, saran hanya dipakai lewat aksi pengguna (dipilih)

**Pros**:

- Sesuai BR-005 dan scope ("menawarkan port bebas tanpa menggantinya otomatis").
- Connection string yang ditulis pengguna tidak pernah salah diam diam.

**Cons**:

- Pengguna harus menekan "Use {n}" atau mengetik port lain.

#### Option 2: peringatan saja, create tetap jalan

**Pros**:

- Bisa membuat instance untuk dipakai nanti walau port sedang dipakai.

**Cons**:

- Start pertama pasti gagal, dan "Start after creating" yang aktif secara default akan langsung gagal.

#### Option 3: isi port bebas berikutnya saat sheet dibuka (perilaku mockup dari Catalog)

**Pros**:

- Lebih sedikit klik.

**Cons**:

- Angka yang dipilih Stackly tampil sebagai port fixed, yang berarti port pilihan manager di luar mode Automatic.

### Keputusan 5: cara memeriksa port

#### Option 1: bind percobaan tepat di alamat endpoint sebagai error, bind wildcard dan loopback keluarga lain sebagai caution, plus `endpoints_conflict` atas registry (dipilih)

**Pros**:

- Menangkap listener wildcard dan dual stack di langkah utama lewat semantik bind BSD, dan memperingatkan `localhost` yang bisa sampai ke server lain lewat `::1` tanpa memblokir instance yang sebenarnya bisa jalan.
- Port milik instance yang sedang `stopped` tetap dianggap terpakai.

**Cons**:

- Hasil bergantung pada perilaku stack jaringan macOS, jadi butuh test native yang mencatatnya.
- Hanya pemilik Stackly yang bisa dinamai.

#### Option 2: hanya registry

**Pros**:

- Tanpa IO jaringan.

**Cons**:

- Konflik dengan proses luar baru ketahuan saat start; preflight mockup ("Free on IPv4 and IPv6 loopback") tidak bisa jujur.

#### Option 3: semua kegagalan bind sebagai error

**Pros**:

- Paling ketat.

**Cons**:

- Listener di alamat LAN atau di `::1` memblokir instance yang sebenarnya bisa bind di `127.0.0.1`.

### Keputusan 6: urutan langkah dan titik tak bisa dibalik

#### Option 1: `init` sebagai titik tak bisa dibalik, dengan kompensator `instance_registration` untuk baris registry (dipilih)

**Pros**:

- Konflik yang baru terlihat saat commit `register` (nama, port, generation) gagal sebelum titik itu, jadi kompensasi membersihkan semuanya.
- Daemon yang mati di antara `register` dan `init` dikompensasi otomatis tanpa baris instance yatim.

**Cons**:

- Butuh `WriteOp::DeleteUninitializedInstance` baru di spec 0005 dan kompensator kustom pertama.

#### Option 2: `register` sebagai titik tak bisa dibalik

**Pros**:

- Tanpa operasi hapus registry baru.

**Cons**:

- Ulangan `GenerationConflict` yang menemukan konflik terjadi setelah titik itu, sehingga reference secret dan folder tertinggal tanpa kompensasi.

### Keputusan 7: hasil kegagalan setelah titik tak bisa dibalik

#### Option 1: `InterruptPolicy::CompensateThenClose`, operasi `failed` dan instance `failed` (dipilih)

**Pros**:

- Init yang gagal adalah hasil wajar (setting salah, disk penuh), dan instance `failed` sudah punya jalan keluar yang jelas (purge fitur 54).
- Lock instance tidak ditahan operasi `needs_recovery`, jadi purge nanti tidak perlu abandon lebih dulu.

**Cons**:

- Varian kebijakan baru di domain dan engine.

#### Option 2: `needs_recovery` sesuai aturan umum engine

**Pros**:

- Tanpa perubahan engine.

**Cons**:

- Setiap init gagal menjadi operasi yang butuh abandon manual, lalu purge, dan selama itu instance terkunci.

### Keputusan 8: Start after creating

#### Option 1: client mengirim `instance.start` setelah create `succeeded` (dipilih)

**Pros**:

- Tidak ada semantik engine baru; start memakai jalur dan kode error yang sama dengan Start biasa.
- CLI dan GUI melakukan hal yang sama (`--start`).

**Cons**:

- App yang ditutup di antara create dan start meninggalkan instance `stopped`.

#### Option 2: daemon merangkai operasi start setelah create

**Pros**:

- Tetap jalan walau client putus.

**Cons**:

- Engine harus bisa memulai operasi dari operasi lain dengan key turunan, dan client tidak punya cara mengetahui operasi start tanpa method baru.

### Keputusan 9: Start at login

#### Option 1: ditunda ke fitur 31, instance dibuat `manual` (dipilih)

**Pros**:

- Sesuai pembagian layar di `ui-macos.md`; tidak ada toggle yang tampil seolah bekerja sebelum login trigger ada.

**Cons**:

- PAGE-04 autostart baru lengkap setelah fitur 31.

#### Option 2: simpan `startup` sekarang

**Pros**:

- Field registry sudah ada.

**Cons**:

- Pilihan yang tersimpan tanpa efek apa pun sampai fitur 31 menyesatkan pengguna GUI dan CLI.

### Keputusan 10: perintah CLI setara

#### Option 1: satu fungsi murni di `stackly-protocol`, dipanggil daemon dan diuji CLI dengan round trip (dipilih)

**Pros**:

- Satu implementasi; GUI hanya menampilkan string.
- Test round trip menjamin perintah yang disalin benar benar diparse CLI menjadi draft yang sama.

**Cons**:

- Crate protokol memegang satu fungsi format yang bukan DTO.

#### Option 2: builder di Swift dengan fixture bersama

**Pros**:

- Tanpa round trip ke daemon.

**Cons**:

- Dua implementasi syntax flag (Swift dan Rust) yang harus dijaga sama.

### Keputusan 11: urutan field di form

#### Option 1: urutan deklarasi `properties` di schema (dipilih)

**Pros**:

- Penulis provider mengatur urutan form tanpa kode Swift, sejalan dengan AC-029.

**Cons**:

- Butuh perubahan kecil di decode spec 0009, karena model saat ini menyimpan field dalam peta terurut nama.

#### Option 2: urut nama key

**Pros**:

- Tanpa perubahan spec 0009.

**Cons**:

- Urutan form tidak bisa dikendalikan penulis provider (misalnya password muncul di tengah).

### Keputusan 12: socket Unix yang merujuk port Automatic

#### Option 1: penanda `auto` di nama yang tersimpan, dirender ulang di batch `spawned`, path null sebelum bind (dipilih)

**Pros**:

- Tanpa migrasi; kolom `socket_name` tetap wajib dan charset nya tetap.
- Nilai yang salah tidak pernah tampil, dan plan tetap merender dari paket.

**Cons**:

- Satu setter domain baru dan satu aturan tambahan di spec 0010.

#### Option 2: wajib port Fixed untuk endpoint yang dirujuk socket

**Pros**:

- Paling sederhana.

**Cons**:

- PostgreSQL tidak bisa memakai Automatic, padahal mockup memakai Automatic untuk duplicate.

#### Option 3: socket opsional di registry

**Pros**:

- Model paling jujur.

**Cons**:

- Mengubah CHECK SQLite berarti membangun ulang tabel, yang dihindari aturan migrasi spec 0005.

### Keputusan 13: sheet selama operasi create

#### Option 1: tetap terbuka dengan progress tahap, tutup saat sukses (dipilih)

**Pros**:

- Kegagalan (Keychain terkunci, init gagal) tampil di konteks form yang sama dan bisa langsung diperbaiki.
- Baris secret bisa menunjukkan hasil `put` (Follow-up spec 0011).

**Cons**:

- Berbeda dengan mockup yang langsung menutup sheet.

#### Option 2: tutup langsung dan tampilkan progress di halaman instance

**Pros**:

- Sesuai mockup.

**Cons**:

- Error harus dicari di Operations, dan instance yang gagal sebelum `register` tidak punya halaman.

### Keputusan 14: batas ruang disk

#### Option 1: batas tetap 256 MiB (dipilih)

**Pros**:

- Sederhana dan mencegah init yang pasti gagal karena disk hampir penuh.

**Cons**:

- Tidak mencerminkan kebutuhan per provider.

#### Option 2: field kebutuhan disk di manifest

**Pros**:

- Akurat per provider.

**Cons**:

- Perubahan schema manifest v1alpha1 dan data yang belum diukur untuk setiap provider.

### Keputusan 15: fuzz target validasi path

#### Option 1: harness atas `check_owned_path` dan `check_data_root_overlap` dengan pohon fixture nyata (dipilih)

**Pros**:

- Menguji fungsi yang benar benar menjaga VAL-007 untuk semua tulisan create, termasuk symlink, loop, dan Unicode.

**Cons**:

- Butuh filesystem nyata, jadi target menyiapkan folder sementara sekali.

#### Option 2: fuzz decode params create

**Pros**:

- Tanpa filesystem.

**Cons**:

- Decode params sudah dicakup fuzz framing spec 0006, dan params create tidak memuat path.

### Keputusan 16: filesystem yang didukung untuk root data

#### Option 1: hanya volume lokal `apfs` atau `hfs`, lewat `Filesystem::volume_facts` (dipilih)

**Pros**:

- Permission, `flock`, dan rename atomik yang diandalkan spec 0004 dan 0010 berlaku di sana.

**Cons**:

- Root data di volume exFAT atau jaringan (lewat `STACKLY_ROOT`) ditolak untuk instance baru.

#### Option 2: tanpa pemeriksaan

**Pros**:

- Tanpa port baru.

**Cons**:

- VAL-008 tidak terpenuhi, dan lock instance di filesystem jaringan bisa tidak berarti apa apa.

## Rationale

Poros spec ini adalah AC-028: GUI dan CLI harus memberi semantik validasi dan error yang sama. Cara paling murah dan paling sulit rusak untuk menjamin itu adalah satu evaluator murni di core yang dipanggil preflight dan precheck create, dengan IO yang dikumpulkan terpisah (`gather`). Karena evaluator murni, semua aturan VAL dan BR bisa diuji dengan tabel kasus tanpa fake, dan karena daemon yang menulis teks preflight, CLI dan GUI juga menampilkan kalimat yang sama. Form bertipe dari daemon (Keputusan 2) dan urutan deklarasi (Keputusan 11) membuat provider baru cukup menulis schema, yang merupakan inti RISK-07 dan AC-029.

Keamanan data menentukan bentuk operasi. BR-008 dan AC-008 meminta create tidak pernah menyentuh data yang sudah ada; karena path diturunkan dari ID baru, satu satunya celah adalah folder yatim dengan ID yang sama atau isi yang muncul saat create berjalan, dan keduanya ditolak sebelum init tanpa menghapus apa pun yang bukan milik operasi. Titik tak bisa dibalik ditaruh di awal `init` (Keputusan 6) karena registry hanya punya satu generation, jadi konflik yang baru terlihat saat commit harus bisa dikompensasi; kegagalan init ditutup `failed` (Keputusan 7) karena instance `failed` sudah merupakan state yang jelas dan punya jalan keluar.

Untuk port, scope dan BR-005 sudah memutuskan arah: konflik ditolak dan port bebas hanya ditawarkan. Pemeriksaan bertingkat (Keputusan 5) membedakan konflik yang pasti membuat start gagal dari overlap yang hanya berisiko salah sasaran, supaya preflight jujur tanpa memblokir kasus yang sebenarnya aman. Pilihan lain (Start after creating di client, Start at login ditunda, penanda `auto` untuk socket) mengikuti prinsip yang sama: tidak menambah mekanisme baru di engine atau migrasi registry ketika aturan kecil yang eksplisit sudah cukup, dan tidak menampilkan apa pun yang belum benar benar bekerja.

## Kesepakatan antarsesi

- **arc-f22-spec** (spec 0022, 2026-10-20): fitur 22 memiliki empty state Instances dan tata letak toolbar Overview dan Instances; fitur 20 memiliki aksi dan sheet New Instance. Kaitnya nilai environment `newInstanceAction: NewInstanceAction?` yang dipasang fitur 20 di `AppShell`; selama ada, fitur 22 merender tombol toolbar dan aksi kedua empty state, semuanya `.mutationGated()`. Seleksi setelah create lewat `ShellNavigator.show(.instances(filter: .all, select: id))` dari spec 0022. Endpoint socket yang namanya bergantung pada port Automatic tampil "socket, created at start" di GUI dan `<name>=socket (pending)` di CLI selama `socketPath` null. Fitur 22 mengusulkan `InstanceSummary.initialized` untuk instance yang tidak pernah terinisialisasi; spec ini tidak mengusulkan field setara.
- **arc-f23-spec** (spec 0023, 2026-10-20): NATS hanya butuh lima jenis field spec ini (text dengan `serverName` managed, boolean `jetstream`, tiga integer advanced dengan batas) dan tidak punya `socketName` bertoken port. Fitur 23 hanya mengubah probe readiness, jadi fixture `create-form-nats.json` tidak berubah karenanya; AC-13 spec 0023 memakai ulang fixture dan test render AC-21. NATS pindah ke `[[provider]]` di milestone 2 fitur 23, sebelum langkah create GUI fitur 23.

## Cross check

Diisi setelah cross check model lain.
