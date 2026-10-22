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

#### Option 1: bind tepat di alamat endpoint dengan `SO_REUSEADDR` sebagai error, bind wildcard keluarga sama serta loopback dan wildcard keluarga lain, juga dengan `SO_REUSEADDR`, sebagai caution, plus `endpoints_conflict` atas registry (dipilih setelah cross check, diperbaiki 2026-10-22)

Versi cross check memasang bind wildcard dan loopback keluarga lain tanpa `SO_REUSEADDR`. Test native milestone 1 menunjukkan bahwa versi itu membuat sisa `TIME_WAIT` menjadi caution; perbaikannya ada di *Update 2026-10-22* di bawah.

**Pros**:

- Meniru cara server sungguhan bind (PostgreSQL dan server Go memasang `SO_REUSEADDR`), jadi error hanya muncul untuk konflik yang pasti membuat start gagal; sisa `TIME_WAIT` dan koneksi yang masih terbuka setelah stop tidak menjadi salah alarm, baik di langkah 4 maupun di langkah 5 dan 6.
- Listener wildcard, dual stack, `[::]` IPv6 saja, dan `::1` tetap terlihat sebagai caution, sehingga `localhost` yang bisa sampai ke server lain diperingatkan tanpa memblokir instance yang sebenarnya bisa jalan.
- Port milik instance yang sedang `stopped` tetap dianggap terpakai lewat registry.

**Cons**:

- Hasil bergantung pada perilaku stack jaringan macOS, jadi butuh test native yang mencatatnya; `std::net` tidak bisa dipakai, sehingga core mendapat dependency `socket2`.
- Hanya pemilik Stackly yang bisa dinamai.
- Listener di alamat spesifik lain Mac ini (misalnya alamat LAN) tidak terlihat, karena bind dengan `SO_REUSEADDR` hanya bentrok dengan alamat yang persis sama.

#### Option 2: bind tepat di alamat endpoint tanpa `SO_REUSEADDR` sebagai error (pilihan draft pertama)

**Pros**:

- Paling ketat: wildcard dan dual stack langsung error.

**Cons**:

- Salah alarm selama sekitar 30 detik setelah server berhenti (`TIME_WAIT`), dan error untuk listener wildcard yang sebenarnya bisa didampingi server; cross check menunjukkan klaim "start pertama pasti gagal" tidak benar untuk server dengan `SO_REUSEADDR`.

#### Option 3: hanya registry

**Pros**:

- Tanpa IO jaringan.

**Cons**:

- Konflik dengan proses luar baru ketahuan saat start; preflight mockup ("Free on IPv4 and IPv6 loopback") tidak bisa jujur.

### Keputusan 6: titik tak bisa dibalik dan hasil kegagalan create

#### Option 1: tanpa titik tak bisa dibalik; semua langkah `reversible`, termasuk `init`, dengan kompensator `instance_registration` (dipilih setelah cross check)

**Pros**:

- Init yang gagal (setting salah, disk penuh, Keychain ditolak sebelum `init_begin`) tidak meninggalkan instance yang memakan nama dan port; pengguna bisa langsung mencoba lagi tanpa menunggu purge fitur 54 di fase 10.
- Satu aturan untuk gagal, cancel, dan daemon mati: semuanya dikompensasi, jadi tidak butuh varian kebijakan interrupt baru di engine.
- Sah menurut BR-023 dan BR-008: yang dihapus hanya folder, data, baris, dan reference yang dibuat operasi itu, selama operasi itu masih memegang lock key instance sehingga tidak ada start di antaranya.

**Cons**:

- Butuh `WriteOp::DeleteInstanceCreatedBy`, event `instance.removed` supaya daftar di client tidak basi, dan probe lock di kompensator `instance_tree` untuk pembantu init yang tertinggal setelah daemon mati.
- Cancel saat init harus menunggu init selesai.

#### Option 2: `init` sebagai titik tak bisa dibalik, kegagalan ditutup `failed` lewat `InterruptPolicy::CompensateThenClose` (pilihan draft pertama)

**Pros**:

- Jejak kegagalan init tetap terlihat sebagai instance `failed`.

**Cons**:

- Instance `failed` itu memakan nama dan port tanpa jalan keluar sampai fitur 54, dan kegagalan sebelum `init_begin` meninggalkan instance `uninitialized` yang tidak bisa di start atau dihapus (temuan cross check S1 dan G1).

#### Option 3: `register` sebagai titik tak bisa dibalik

**Pros**:

- Tanpa operasi hapus registry baru.

**Cons**:

- Ulangan `GenerationConflict` yang menemukan konflik terjadi setelah titik itu, sehingga reference secret dan folder tertinggal tanpa kompensasi.

### Keputusan 7: Start after creating

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

### Keputusan 8: Start at login

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

### Keputusan 9: perintah CLI setara

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

### Keputusan 10: urutan field di form

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

### Keputusan 11: socket Unix yang merujuk port Automatic

#### Option 1: nama tersimpan sekali (penanda `auto` untuk port Automatic), `socketPath` dihitung saat dibaca dari template paket pinned (dipilih setelah cross check)

**Pros**:

- Tanpa migrasi dan tanpa setter domain atau perubahan batch `spawned`; nilai yang salah tidak pernah tampil.
- Satu sumber kebenaran dengan plan: keduanya merender dari paket pinned dan port efektif.

**Cons**:

- `stacklyd` membaca template dari paket pinned saat membangun summary, jadi butuh cache per provider revision key.

#### Option 2: penanda `auto` yang dirender ulang di batch `spawned` (pilihan draft pertama)

**Pros**:

- Nama di registry selalu nama yang sedang dipakai.

**Cons**:

- Setter domain baru, perubahan spec 0010, dan revision tambahan setiap spawn; client harus mengenali penanda.

#### Option 3: wajib port Fixed untuk endpoint yang dirujuk socket

**Pros**:

- Paling sederhana.

**Cons**:

- PostgreSQL tidak bisa memakai Automatic, padahal mockup memakai Automatic untuk duplicate.

### Keputusan 12: sheet selama operasi create

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

### Keputusan 13: batas ruang disk

#### Option 1: batas tetap 256 MB (dipilih)

**Pros**:

- Sederhana dan mencegah init yang pasti gagal karena disk hampir penuh.

**Cons**:

- Tidak mencerminkan kebutuhan per provider.

#### Option 2: field kebutuhan disk di manifest

**Pros**:

- Akurat per provider.

**Cons**:

- Perubahan schema manifest v1alpha1 dan data yang belum diukur untuk setiap provider.

### Keputusan 14: fuzz target validasi path

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

### Keputusan 15: filesystem yang didukung untuk root data

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

### Keputusan 16: create bernama sama yang bersamaan

#### Option 1: `BusyPolicy::Reject` dengan lock `InstanceName(name)` (dipilih setelah cross check)

**Pros**:

- Yang kalah langsung mendapat `resource.busy` dengan operasi pemenangnya, bukan menunggu init pemenang selama beberapa menit lalu gagal `instance.name_taken`.

**Cons**:

- Berbeda dengan `BusyPolicy::Queue` yang ditulis spec 0010 untuk create.

#### Option 2: `BusyPolicy::Queue` (spec 0010)

**Pros**:

- Tanpa perubahan spec 0010.

**Cons**:

- Pengguna menunggu tanpa hasil sampai pemenangnya selesai.

### Keputusan 17: tempat IO precheck

#### Option 1: `gather` di handler lewat `spawn_blocking` sebelum `begin`, precheck hanya `evaluate` murni (dipilih setelah cross check)

**Pros**:

- Gerbang `begin` engine yang dipakai semua operasi tidak pernah menunggu bind, `statfs`, atau probe Keychain yang antri di belakang dialog Keychain lain.

**Cons**:

- Hasil `gather` bisa basi saat operasi mulai; langkah `validate` mengumpulkan ulang di bawah lock, dan replay membuang hasil itu.

#### Option 2: seluruh `gather` di dalam precheck

**Pros**:

- Fakta paling segar saat diputuskan.

**Cons**:

- IO lambat menahan mutex global `begin`, sehingga satu dialog Keychain bisa menahan start dan stop instance lain.

### Keputusan 18: memberi tahu client saat kompensasi menghapus instance

#### Option 1: event baru `instance.removed { instanceId }` (dipilih setelah cross check)

**Pros**:

- Baris yang sempat muncul setelah `register` hilang lagi di GUI tanpa menunggu snapshot; purge fitur 54 bisa memakai event yang sama.

**Cons**:

- Satu event baru di protokol dan penanganannya di `InstanceStore` (disepakati dengan fitur 22).

#### Option 2: sembunyikan instance yang operasi create nya masih berjalan dari `instance.list`

**Pros**:

- Tanpa event baru.

**Cons**:

- Daftar menyembunyikan state nyata (`initializing`), dan aturan filter itu harus diterapkan di setiap pembaca.

## Rationale

Poros spec ini adalah AC-028: GUI dan CLI harus memberi semantik validasi dan error yang sama. Cara paling murah dan paling sulit rusak untuk menjamin itu adalah satu evaluator murni di core yang dipanggil preflight dan precheck create, dengan IO yang dikumpulkan terpisah oleh `gather` di luar gerbang `begin` (Keputusan 1 dan 17). Karena evaluator murni, semua aturan VAL dan BR bisa diuji dengan tabel kasus tanpa fake, dan karena daemon yang menulis teks preflight, CLI dan GUI juga menampilkan kalimat yang sama. Form bertipe dari daemon (Keputusan 2) dan urutan deklarasi (Keputusan 10) membuat provider baru cukup menulis schema, yang merupakan inti RISK-07 dan AC-029.

Keamanan data dan kemudahan mencoba lagi menentukan bentuk operasi. BR-008 dan AC-008 meminta create tidak pernah menyentuh data yang sudah ada; karena path diturunkan dari ID baru, satu satunya celah adalah folder yatim dengan ID yang sama atau isi yang muncul saat create berjalan, dan keduanya ditolak sebelum init tanpa menghapus apa pun yang bukan milik operasi. Sebaliknya, semua yang dibuat operasi itu sendiri boleh dan harus dibersihkan saat create gagal (BR-023), termasuk data hasil init yang gagal, karena operasi itu masih memegang lock key instance sehingga instance tidak pernah sempat dipakai. Itu sebabnya create tidak punya titik tak bisa dibalik (Keputusan 6): instance setengah jadi yang memakan nama dan port sampai fitur 54 di fase 10 adalah kegagalan pengalaman yang lebih buruk daripada beberapa tambahan kecil di spec 0005 dan 0007.

Untuk port, scope dan BR-005 sudah memutuskan arah: konflik ditolak dan port bebas hanya ditawarkan. Pemeriksaan bertingkat (Keputusan 5) meniru cara server bind, sehingga error hanya untuk konflik yang pasti membuat start gagal dan overlap yang hanya berisiko salah sasaran menjadi peringatan. Pilihan lain (Start after creating di client, Start at login ditunda, socket path yang dihitung saat dibaca, lock nama dengan `Reject`) mengikuti prinsip yang sama: tidak menambah mekanisme baru di engine atau migrasi registry ketika aturan kecil yang eksplisit sudah cukup, dan tidak menampilkan apa pun yang belum benar benar bekerja.

## Kesepakatan antarsesi

- **arc-f22-spec** (spec 0022, 2026-10-20): fitur 22 memiliki empty state Instances dan tata letak toolbar Overview dan Instances; fitur 20 memiliki aksi dan sheet New Instance. Kaitnya nilai environment `newInstanceAction: NewInstanceAction?` yang dipasang fitur 20 di `AppShell`; selama ada, fitur 22 merender tombol toolbar dan aksi kedua empty state, semuanya `.mutationGated()`. Seleksi setelah create lewat `ShellNavigator.show(.instances(filter: .all, select: id))` dari spec 0022. Endpoint socket yang namanya bergantung pada port Automatic tampil "socket, created at start" di GUI dan `<name>=socket (pending)` di CLI selama `socketPath` null. Fitur 22 mengusulkan `InstanceSummary.initialized` untuk instance yang tidak pernah terinisialisasi (setelah keputusan cross check, hanya mungkin untuk create terputus yang kompensasinya tertahan lock); spec ini tidak mengusulkan field setara. Spec 0022 (476204c) memetakan `instance.removed` ke penghapusan di `InstanceStore.reduce` (tombstone di revision event).
- **arc-f23-spec** (spec 0023, 2026-10-20): NATS hanya butuh lima jenis field spec ini (text dengan `serverName` managed, boolean `jetstream`, tiga integer advanced dengan batas) dan tidak punya `socketName` bertoken port. Fitur 23 hanya mengubah probe readiness, jadi fixture `create-form-nats.json` tidak berubah karenanya; AC-13 spec 0023 memakai ulang fixture dan test render AC-21. NATS pindah ke `[[provider]]` di milestone 2 fitur 23, sebelum langkah create GUI fitur 23.

## Cross check

Dijalankan 2026-10-20 oleh model lain (Sonnet, read only) atas draft `7815b6a`, lalu semua rekomendasi diterima lead atas nama user dengan aturan tetap "opsi yang direkomendasikan yang diambil". Setelah itu `main` di merge (`e9d3d42`) dan setiap nama yang diambil dari spec 0015 sampai 0019, 0016, dan 0023 diperiksa ulang: kontrak K-1 dan K-2 tidak berubah, hint memakai `codes::<area>::hint` statis, dan kategori runtime di sheet memakai `ProviderSummary.software`.

| ID | Temuan | Keputusan yang diterapkan |
| --- | --- | --- |
| G1 | Kegagalan setelah titik tak bisa dibalik tetapi sebelum `init_begin` (Keychain ditolak, lock dipegang, daemon mati) meninggalkan instance `uninitialized` tanpa jalan keluar | create tanpa titik tak bisa dibalik; semua langkah `reversible` (AC-11, Keputusan 6) |
| G2 | `bind` atau `copyFrom` ke reference tanpa nilai hanya caution padahal init membutuhkannya; nilai `bind` tidak pernah diperiksa terhadap aturan field | baris error `secret.needs_secret`; langkah `secrets` membaca dan memeriksa nilai `bind` (AC-7) |
| G3 | Pemetaan field error tidak normatif; `PlanError` tanpa field; permintaan endpoint tidak valid tanpa baris; path non settings tidak didefinisikan; `unsupported` dipetakan dua cara | tabel *Field error* dengan `SettingsIssueKind` (usulan spec 0009), path `settings` untuk `PlanError` tanpa field, baris endpoint tambahan, daftar path, `secret.store_unsupported` selalu |
| G4 | `PortProbe` tanpa signature, dan `std::net` diam diam memasang `SO_REUSEADDR` dan `listen` | `PortProbe::try_bind` di atas `socket2` (usulan spec 0001 dan 0010), `CreateDeps` didefinisikan |
| G5 | Siapa yang mem probe kandidat saran dan rentangnya tidak jelas | `gather` menghitung saran, urutan `max(p+1, 1024)` sampai 65535, 64 kandidat (AC-5) |
| G6 | `CommandDraft` tidak didefinisikan; `--provider` dan `--revision` hilang; round trip tidak terdefinisi | `CommandDraft` didefinisikan, flag provider dan revision hanya kalau dikirim, round trip setelah default diisi (AC-18) |
| G7 | `--runtime software@versi` ambigu | versi persis dulu, lalu line unik, selain itu `cli.runtime_ambiguous` dengan kandidat |
| G8 | Build plan tidak mencakup `socketPath`, prasyarat fixture PostgreSQL, fixture NATS, dan `build_form` | langkah 9 untuk `socketPath`, prasyarat fitur 19 milestone 1, fixture NATS dari paket yang ada, `build_form` di langkah 6 |
| G9 | Key start, isi `instance` di JSON `--start`, dan umpan balik start gagal setelah sheet tertutup tidak jelas | key turunan SHA-256, `instance` selalu terisi dengan `--start`, start gagal terlihat di Operation center dan `lastFailure` (AC-18, AC-20) |
| G10 | Port di luar `u16` dan input CLI yang tidak bisa diparse tidak bisa sampai ke daemon | satu aturan client untuk port, tiga aturan `cli.usage` (AC-18, *Sheet*) |
| G11 | Bentuk nilai `value` hanya diperiksa di `put`; `PutTarget::New` tidak membawa aturan field | precheck sinkron `secret.invalid_value` sebagai satu pengecualian AC-2, `PutTarget::New.field` (usulan spec 0011) |
| G12 | 256 MiB ditampilkan 256 MB; detail `not_created` untuk folder yang sudah ada; kalimat AC-2 tentang baris; dua pembacaan jam; sintaks `Endpoint::Ip`; waktu persist port auto | 256 000 000 byte; `already_exists` dan `data_not_empty`; AC-2 diperbaiki; satu pembacaan `Clock` di `layout`; kalimat konstruktor domain; AC-16 menyebut aturan spawn spec 0010 |
| S1 | Instance gagal init memblokir nama dan port sampai fase 10; pembantu init bisa tetap jalan setelah daemon mati | kompensasi penuh untuk init gagal, probe lock di `instance_tree` (Keputusan 6) |
| S2 | IO precheck di dalam gerbang `begin` global bisa tertahan dialog Keychain | `gather` di handler, precheck hanya `evaluate`, probe `Busy` menjadi caution (Keputusan 17, AC-24) |
| S3 | `register` hanya memeriksa ulang nama dan port, jadi dua create provider tanpa `multi_instance` bisa lolos | `register` memeriksa ulang semua aturan bersumber registry (AC-10, AC-15) |
| S4 | Probe tanpa `SO_REUSEADDR` salah alarm untuk `TIME_WAIT` dan listener wildcard | langkah 4 dengan `SO_REUSEADDR`, wildcard dan dual stack menjadi caution, tabel *Hasil yang diharapkan* diperbarui (Keputusan 5) |
| S5 | Penanda `auto` yang ditulis ulang di batch `spawned` rumit dan rawan salah baca | nama disimpan sekali, `socketPath` dihitung saat dibaca (Keputusan 11) |
| S6 | Instance hantu di GUI setelah kompensasi | event `instance.removed` (Keputusan 18) |
| S7 | Create bernama sama menunggu init pemenang lalu gagal | `BusyPolicy::Reject` (Keputusan 16) |
| S8 | `check_data_root_overlap` setelah layout hampir tidak bisa menemukan apa pun | dipertahankan sebagai pemeriksaan berlapis yang murah untuk symlink yang ditanam lebih dulu (AC-6) |

## Penyesuaian dengan update spec 0004 dan 0005 (2026-10-20)

Spec 0004 dan 0005 menerapkan blok usulan spec ini lewat sesi `upd-0004-0005`, dengan dua keputusan yang berbeda dari teks usulan. Spec ini mengikuti keduanya, karena spec pemilik yang menentukan:

- **`DuplicateSocket` masuk BR-004**, bukan rule None, karena baris BR-004 spec 0004 sudah menyatakan dua endpoint Unix dengan nama socket sama konflik. *Endpoint di aggregate* dan butir 2 dan 3 blok spec 0004 disamakan, termasuk `Instance::new` yang mengembalikan `Result<Instance, InvariantError>`.
- **`registry.not_deletable` hanya membawa kunci `details` `reason`** (`created_by_other_operation` atau `active`), dan instance nya dikirim di `ErrorBody.resource` (`ResourceKind` `instance`), mengikuti aturan *Kode error di wire* spec 0005. Tabel *Kode error* dan butir 6 blok spec 0005 disamakan; butir 3 dan 5 ikut memuat nama tipe yang dipilih spec 0005 (`ProviderSourceKind`, `ProviderSourceStatus`, `NotDeletableReason`) dan builder `with_source_id`.

Acceptance criteria dan build plan tidak berubah.

## Penyelarasan 2026-10-20 (keputusan K foundation)

Update `/architect` (sesi `align-0020`) yang menyelaraskan spec ini dengan keputusan lead fondasi K1 sampai K18 dan dengan update spec fondasi di `main` (0001, 0002, 0003, 0004, 0005, 0006, 0009, 0010 di `989e816`, 0011 di `4827440`, dan 0012), dari daftar `docs/scope/coordination/02-instalasi-pertama.penyelarasan-k.md` bagian *0020* dan *Cross-cutting*. ID AC tetap, nomor langkah tetap, dan status tetap Proposed. Keputusan mengikuti rekomendasi:

- **Create sementara (K1)**: langkah 13 spec 0010 membangun create sementara dengan `register` sebagai langkah pertama yang tidak bisa dibalik. Langkah 9 spec ini menggantinya, menghapus penanda `// provisional: F20 until F20 step 9` dan penolakan `provider.revision` kosong, dan memperbarui test spec 0010 dan fitur 14 yang mengharapkan `needs_recovery` setelah init gagal. Usulan baris resume `create_instance` untuk spec 0007 kini menyebut kedua masa itu.
- **`Instance::new` yang fallible (K1)**: preferensi, bukan syarat. Langkah 1 memperbaiki satu titik panggil di `stacklyd` kalau langkah 13 spec 0010 lebih dulu, dan berjalan setelah langkah 2a spec 0010 kalau ditarik lebih dulu.
- **`StageName::PIN` (K1)**: langkah 7, bersama test urutan tahap di commit yang sama.
- **Probe lock di `instance_registration` (K1, K10)**: kompensasi berjalan urut terbalik, jadi baris registry terhapus sebelum `instance_tree` sempat menolak, dan AC-11 bertentangan dengan dirinya sendiri (instance hilang dari registry padahal foldernya masih dipakai pembantu init). Kedua kompensator kini memakai urutan K10 yang sama, dengan detail `instance_locked` dan `lock_probe_failed`. Runner up: kompensator registry menolak saja kalau catatan `instance_tree` gagal, yang tidak bisa karena catatan registry dikompensasi lebih dulu.
- **Port (K2)**: langkah 6 memakai ulang `try_bind` dari langkah 5 spec 0010. Bind di alamat endpoint yang `Denied` atau `Unavailable` kini error `supervisor.endpoint_unavailable` di preflight dan create, sama dengan tahap `endpoints` start, karena port itu pasti gagal saat start; sebelumnya `caution`. Langkah 5 dan 6 tetap `caution`. Runner up: tetap `caution` (opsi yang disebut opsional di catatan `upd-0010`), yang membuat preflight hijau untuk instance yang start nya pasti gagal. AC-16 dan test race memakai listener di alamat yang persis sama, karena listener wildcard tidak lagi menggagalkan start.
- **Kontrak (K4)**: `provider.revision` opsional serta DTO dan fixture `instance.removed` dikirim langkah 12 spec 0010, dan fitur ini yang pertama mengirim event itu. Sembilan kode `instance` sudah terdaftar di tabel spec 0010 dan dibangun langkah 8. Minor protokol mengambil `CURRENT` di main ditambah 1 saat merge.
- **Urutan (K10, K11)**: update spec 0007 sebelum langkah 3; `execute_init` melepas lock sebelum mengembalikan error (langkah 7 spec 0010); langkah 16 fitur 7 (migrasi 0004) sebelum langkah 2 dan langkah 7.
- **Secret (K13, K14, K15)**: `put` dengan `FieldContext` dan `put(Bind)`; `classify_create_failure`, dengan item `needs_secret` hanya untuk reference sumber dan tulisan target yang gagal menjadi `secret.store_*`; `Validate` memanggil `SecretInput::field_errors`, jadi path nya `secrets[<i>].input.content`; NUL keluar dari pengecualian AC-2, daftar precheck AC-7, dan `SecretValueShape`. Bentuk nilai dihitung `secret::redact::value_shape`, fungsi kecil di modul secret yang memakai `char_count` dan `has_control_character`, supaya `instance::create` tidak menyentuh nilai dan tidak ada tempat `expose()` baru. Runner up: memanggil kedua helper `pub(crate)` langsung dari `instance::create`, yang menaruh perhitungan bentuk nilai di luar modul secret.
- **GUI (K16, K17, K18)**: case reduce `instance.removed` ditambah langkah 12 kalau langkah 16 spec 0013 belum memetakannya; `NewInstanceAction` didefinisikan langkah 14 kalau belum ada; tombol di `ProvisionalInstanceList` bertanda `// provisional: F22` hanya saat aksi tidak nil, dengan `.mutationGated()`, setelah langkah 16 spec 0013; milestone 3 menunggu milestone 2 fitur 10 lewat langkah 16 fitur 13.
- **Spec 0012**: status preflight menjadi status set baru di `design.md`, dengan `unrecognized` memakai pola spec 0012 (tone `caution`, `questionmark.diamond.fill`, dibaca "Unrecognized status"); AC-22 memakai cek lebar sempit 560 pt, karena text style di macOS 27 tidak membesar dengan Dynamic Type.
- **Penanda blok usulan**: blok spec 0001, 0009, 0010, dan 0011 ditandai sudah diterapkan, dengan selisih dari keputusan K; blok spec 0007 belum diterapkan, dan butir 3 nya kini memuat aturan K10.

Tidak diterapkan di sini: penyaringan retained dan aturan `statusSeq` di `InstanceStore` (K16, milik langkah 16 spec 0013); `OperationEngine::replay` (K12), karena aturan yang bersumber dari registry sudah berjalan di precheck sesudah pencarian key; dan `instance.name_invalid` lebih awal di langkah 12 spec 0010 (K4, pilihan spec 0010).

## Perbaikan pasca penyelarasan (2026-10-20)

Update `/architect` (sesi `align-0020`) yang menerapkan `docs/scope/coordination/post-alignment/post-0020.md` (4 major, 5 minor) dengan keputusan lead D6 sampai D9 di `post-decisions.md`, yang menang bila teksnya berbeda. ID AC, nomor langkah, dan status tidak berubah.

- **Tipe core dan wire (D8, temuan 1)**: `build_form` mengembalikan `CreateFormModel` core dan `Evaluation.command` adalah `CommandSpec` core; DTO `CommandDraft` tinggal di `stackly_protocol::instance::command` di samping `equivalent_command`, dan `stacklyd` memetakan keduanya di langkah 9. Alasannya aturan spec 0001: core tidak bergantung pada `stackly-protocol`, protocol adalah daun, dan CLI butuh tipe wire untuk round trip.
- **Kode `FieldError` (D7, temuan 2)**: `FieldError` memakai set `FieldCode` tetap spec 0006 dengan teks tetap. Kolom pesan di tabel *Field error* menjadi teks GUI yang disusun klien dari `code` dan batas `FormField`, karena baris preflight `settings` hanya satu untuk semua field dan tidak bisa memuat teks per field; untuk nama, endpoint, dan secret, teks bertopik batas tetap di `detail` baris preflight. Baris "kode di baris evaluator" diganti tabel path dengan `FieldCode` eksplisit, termasuk path runtime dan provider yang sudah ada di daftar path. Runner up: hanya teks tetap tanpa bantuan GUI, yang membuat form kurang jelas dibanding mockup.
- **`fields` di error sinkron (D7, temuan 3)**: `fields` hanya ikut kalau kode baris error pertama mendaftarkan `omitted` dan tanpa `items`; `omitted` ditambah ke `instance.name_invalid` di tabel spec ini, dan pemilik spec 0010 dan 0011 menambahkannya ke kode mereka. Saat `secret.needs_secret` di depan, `fields` tidak dikirim, dan paritas AC-2 membandingkan `fields` hanya untuk kode yang membawanya. Sheet tetap menampilkan field error dari preflight yang dijalankan lagi.
- **Test init gagal spec 0019 (D9, temuan 4 dan 8)**: langkah 9 juga memperbarui skenario init gagal AC-5 spec 0019 menjadi operasi `failed` yang dikompensasi penuh, digerakkan kait test setelah pemeriksaan isi (kait kedua di langkah 7) atau jalur init fake, karena file yang ditanam sebelum init kini ditolak `instance.data_not_empty`.
- **Saluran `fields` precheck (temuan 5)**: precheck menyimpan `Evaluation` di slot yang dipegang handler, dan `stacklyd` memasang `fields` saat `begin` menolak. Runner up: downcast `Rejection::error()`, yang bergantung pada `OperationError` bisa di downcast.
- **Provider revision tanpa kompensator (temuan 6)**: butir 4 baru di blok spec 0007 mencatat pengecualian sengaja untuk `InsertProviderRevision` (immutable, idempoten, dipakai bersama).
- **Cari revision pinned (temuan 7)**: `gather` mendaftar folder `<revision>/` dengan `read_dir`, lalu memastikan satu kandidat dengan `Registry::provider_revision(key)` dan `read_package`, sama dengan jalur disk spec 0010.
- **Serialisasi file shell (temuan 9)**: catatan sesi paralel milestone 3 kini melarang langkah 14 berjalan bersamaan dengan langkah 6 dan 7 spec 0022 dan langkah 2 milestone 1 spec 0016. Daftar di spec 0022 bukan milik spec ini, jadi dicatat sebagai Follow-up untuk lead.
- **Bukti PostgreSQL (D6)**: milestone 4 butuh langkah 9 milestone 3 spec 0019 (PostgreSQL sebagai `[[provider]]` di catalog); test daemon sebelum itu memakai catalog `CatalogBuilder` yang memuat `official/postgresql`.

## Update 2026-10-21 dari spec 0067, 0068, dan 0069

- **Komponen dari spec 0067.** Fitur 67 kini satu satunya pembangun komponen dasar, jadi sheet, checklist preflight, progress, dan perintah setara memakai `StacklySheet`, `CheckList`, `StagedProgress`, dan `ConsoleBlock`. Versi sementara di `CreateInstance/` tidak dibuat lagi, dan tabel status preflight diganti tabel *Status checklist* spec 0067 dengan kata dan simbol yang sama.
- **Copy alasan Create.** Spec 0067 meminta spec ini menulis `disabledReason` Create. Urutannya dari yang paling bisa diperbaiki pengguna: pilih runtime, perbaiki error, jalankan ulang preflight yang gagal, lalu tunggu preflight. Alasan gate tidak diulang karena footer `StacklySheet` sudah menampilkannya. Ini rekomendasi update ini, dilaporkan ke lead.
- **Cancel di mode progress.** Aturan spec 0067 bahwa judul aksi yang dikonfirmasi diakhiri "…" membuat tombol itu "Cancel…"; label tombol konfirmasi "Stop" mengikuti aturan copy spec 0012 (kata kerja dari judul).
- **Umpan balik setelah create.** Mockup tidak punya toast create. Spec 0067 meminta hasil create dikirim ke `FeedbackCenter` oleh layar pemilik sheet, jadi `AppShell` menampilkan "“{name}” created." atau "“{name}” created. Starting it now." Gagal create tetap di dalam sheet, sehingga kartu hanya untuk hasil yang menutup sheet.
- **⌘N.** Tambahan "nonaktif saat jendela utama bukan key window atau onboarding tampil" dari spec 0069 ditulis di *Titik masuk* dan di teks AC-20, yang disetujui user lewat lead (2026-10-21), supaya perilaku itu bisa diverifikasi di fitur 20.
- **Cross check (2026-10-21).** Cross check `Another model` (Sonnet 5.5) atas update ini dan perbaikannya diterima lead atas delegasi user (2026-10-21): `CheckItem.id` dari `PreflightCheck.id` dan "Use {n}" tetap di baris Port; urutan lengkap `disabledReason` dan field action Create (D1); Close menggantikan `Cancel…` saat gate blocked (D2); copy dan pemicu pengumuman preflight (D3); `FocusedValues.newInstanceAction` untuk ⌘N sementara (D4); umpan balik saat start lanjutan gagal (D5); baris perintah setara sebelum dan sesudah preflight (D6); `StateBanner` caution untuk `notSent` dan `outcomeUnknown` di sheet (D7); tombol New Instance sebagai `ActionButton` atas action `newInstance` (D17).

## Update 2026-10-22: sisa `TIME_WAIT` dan probe wildcard

Update `/architect` (sesi `arc-f20-update`, wave 4) atas temuan developer dan checker milestone 1. ID AC dan nomor langkah lama tetap; langkah 6a ditambah, dan teks AC-4 berubah dengan persetujuan user lewat lead (2026-10-22, teks persis seperti usulan).

### Masalah

Tabel *Hasil yang diharapkan* menulis `ok` untuk sisa `TIME_WAIT` di `127.0.0.1:p` setelah server berhenti, tetapi test native `a_time_wait_leftover_on_the_exact_address_is_not_a_conflict` di Mac ini memberi `caution` "In use on another address of this Mac". Langkah 4 (bind `127.0.0.1:p` dengan `SO_REUSEADDR`) memang `Free`; yang gagal adalah langkah 5, bind `0.0.0.0:p` tanpa `SO_REUSEADDR`. Klaim di Keputusan 5 bahwa `TIME_WAIT` tidak lagi menjadi salah alarm hanya benar untuk langkah 4.

### Percobaan di Mac ini

Script Python di scratchpad sesi (tidak di commit), hanya di loopback: `127.0.0.1`, `::1`, dan `fe80::1%lo0` milik `lo0`, tanpa listener di antarmuka jaringan. Setiap skenario memasang satu keadaan socket di port bebas p, lalu menjalankan setiap bind percobaan (bind tanpa `listen`, langsung ditutup). Kolom `r` berarti dengan `SO_REUSEADDR`, `n` tanpa; bind IPv6 memakai `IPV6_V6ONLY`. Untuk endpoint `127.0.0.1:p`:

| Socket lain di port p | `127.0.0.1` r (langkah 4) | `0.0.0.0` n (langkah 5 lama) | `0.0.0.0` r (langkah 5 baru) | `[::1]` n (langkah 6 lama) | `[::1]` r (langkah 6 baru) | `[::]` r (langkah 6 baru) |
| --- | --- | --- | --- | --- | --- | --- |
| TCP `LISTEN` `127.0.0.1:p` | InUse | InUse | Free | Free | Free | Free |
| TCP `LISTEN` `0.0.0.0:p` | Free | InUse | InUse | Free | Free | Free |
| TCP `LISTEN` `[::]:p` dual stack | Free | InUse | InUse | InUse | Free | InUse |
| TCP `LISTEN` `[::]:p` IPv6 saja | Free | Free | Free | InUse | Free | InUse |
| TCP `LISTEN` `[::1]:p` | Free | Free | Free | InUse | InUse | Free |
| TCP `LISTEN` `[fe80::1%lo0]:p` | Free | Free | Free | Free | Free | Free |
| sisa `TIME_WAIT` di `127.0.0.1:p` (server menutup lebih dulu) | Free | InUse | Free | Free | Free | Free |
| koneksi `ESTABLISHED` di `127.0.0.1:p`, listener sudah ditutup | Free | InUse | Free | Free | Free | Free |
| koneksi keluar dengan port lokal `127.0.0.1:p` | Free | InUse | Free | Free | Free | Free |
| sisa `TIME_WAIT` di `[::1]:p` | Free | Free | Free | InUse | Free | Free |
| sisa `TIME_WAIT` di `127.0.0.1:p` dan listener `[::]:p` dual stack | Free | InUse | InUse | InUse | Free | InUse |
| sisa `TIME_WAIT` di `127.0.0.1:p` dan listener `0.0.0.0:p` | Free | InUse | InUse | Free | Free | Free |
| sisa `TIME_WAIT` di `127.0.0.1:p` dan listener `[::1]:p` | Free | InUse | Free | InUse | InUse | Free |
| sisa `TIME_WAIT` di `[::1]:p` dan listener `[::]:p` IPv6 saja | Free | Free | Free | InUse | Free | InUse |
| UDP `127.0.0.1:p` (endpoint TCP) | Free | Free | Free | Free | Free | Free |

Catatan lain dari percobaan yang sama:

- Listener di `[fe80::1%lo0]:p` membuat bind `[::]:p` tanpa `SO_REUSEADDR` `InUse`, tetapi bind `[::]:p` dengan `SO_REUSEADDR` `Free`. Itu padanan IPv6 dari listener di alamat LAN: langkah 5 lama melaporkannya, langkah 5 baru tidak.
- Untuk endpoint `[::1]:p`, tabel yang sama dibaca dengan peran keluarga ditukar (langkah 4 `[::1]` r, langkah 5 `[::]` r, langkah 6 `127.0.0.1` r lalu `0.0.0.0` r): setiap listener yang menerima koneksi ke `::1` atau `localhost` tetap `InUse` di salah satunya, dan sisa `TIME_WAIT` di kedua loopback `Free` di semuanya.
- Endpoint UDP mengikuti aturan yang sama: socket UDP di `127.0.0.1:p` `InUse` di langkah 4, di `0.0.0.0:p` dan `[::]:p` dual stack `InUse` di langkah 5 baru, di `[::]:p` IPv6 saja `InUse` di bind `[::]` r, dan socket UDP terhubung dengan port lokal p hanya `InUse` di bind tanpa `SO_REUSEADDR`.
- Server dengan `SO_REUSEADDR` (bind, `listen`, lalu menerima koneksi) berhasil start di `127.0.0.1:p` selama sisa `TIME_WAIT`, di samping listener `0.0.0.0:p`, dan di `[::1]:p` selama sisa `TIME_WAIT` di `[::1]:p`. Server tanpa `SO_REUSEADDR` di `127.0.0.1:p` selama sisa `TIME_WAIT` gagal `EADDRINUSE`.
- Sisa `TIME_WAIT` menahan bind wildcard lama sekitar 30,4 detik (`net.inet.tcp.msl` 15000 ms, jadi dua kali MSL).
- Kalau client yang menutup lebih dulu, `TIME_WAIT` ada di port client, dan port server bebas untuk semua bind.

Penjelasan singkatnya: saat bind, macOS mencari socket lain yang bentrok di port itu. Dengan `SO_REUSEADDR`, pencarian hanya cocok dengan socket yang belum terhubung dan bind ke alamat yang persis sama (wildcard IPv4 ikut cocok dengan socket dual stack). Tanpa opsi itu, pencarian juga cocok dengan socket mana pun di port p yang alamatnya bertumpuk, termasuk socket yang sudah terhubung. Sisa `TIME_WAIT` adalah socket terhubung yang masih memegang `127.0.0.1:p`, jadi ia hanya tertangkap oleh bind tanpa `SO_REUSEADDR`.

Cross check (di bawah) menjalankan ulang matriks ini dengan pemegang port yang memasang `SO_REUSEADDR`, dan untuk TCP juga `SO_REUSEPORT`, seperti server sungguhan; hasilnya sama persis.

Tidak diamati: socket milik user macOS lain (XNU punya aturan tambahan antar user). Stackly dan server yang ia jalankan memakai user yang sama. Menurut aturan itu, listener wildcard milik user lain bisa membuat langkah 4 `InUse`, sama dengan yang akan dialami server; ini tidak diuji.

### Opsi

#### Option A: semua bind percobaan dengan `SO_REUSEADDR`, alamat wildcard dibind eksplisit (dipilih)

Langkah 5 bind wildcard keluarga yang sama dan langkah 6 bind loopback lalu wildcard keluarga lain, semuanya dengan `BindOptions::like_server`.

**Pros**:

- Sisa `TIME_WAIT`, koneksi yang masih terbuka, dan koneksi keluar dengan port lokal p tidak lagi menjadi caution, di kedua loopback; baris `TIME_WAIT` di tabel tetap `ok`.
- Setiap listener yang bisa menerima koneksi ke A atau ke `localhost` tetap tertangkap, termasuk saat sisa `TIME_WAIT` ada bersamaan (tabel di atas). Overlap IPv4 dan IPv6 serta listener wildcard yang diminta BR-004 dan AC-056 tetap diperiksa.
- Probe menjawab pertanyaan yang sama dengan server: semua bind memakai opsi server, sehingga hasilnya sama dengan yang dialami server saat start.
- Perubahan kecil di `port.rs` (satu bind tambahan di langkah 6), tanpa dependency baru, tanpa method baru di `PortProbe`, dan bentuk `PortFacts` tetap.

**Cons**:

- Listener di alamat spesifik lain Mac ini (alamat LAN) tidak lagi menjadi caution. Listener itu tidak menerima koneksi ke loopback, dan AC-4 sendiri melarang tuple diperlakukan sebagai satu angka global, jadi yang hilang hanya informasi, bukan perlindungan.
- Teks AC-4 berubah (frasa "tanpa `SO_REUSEADDR`" dan daftar listener test), jadi butuh persetujuan user.
- Port bebas kini dibuka empat kali, bukan tiga; pencarian saran paling banyak 256 bind, masih dalam hitungan milidetik.

#### Option B: tabel diubah menjadi `caution` untuk sisa `TIME_WAIT` (dokumen saja)

**Pros**:

- Tanpa perubahan kode, dan teks AC tidak berubah.
- Listener di alamat LAN tetap menjadi caution.

**Cons**:

- Salah alarm sekitar 30 detik setelah setiap server di port itu berhenti, termasuk instance Stackly yang baru dihapus lalu dibuat ulang dengan port yang sama, yang justru jalur coba lagi yang dijaga spec ini.
- Salah alarm juga untuk setiap koneksi terbuka atau koneksi keluar yang port lokalnya p; port fixed di rentang ephemeral macOS (49152 sampai 65535) bisa kena secara acak.
- Teksnya keliru: "In use on another address of this Mac" untuk alamat yang sama tanpa listener, dan sisa `TIME_WAIT` di `[::1]` memberi "clients that connect to localhost may reach that server" padahal tidak ada server.
- Pencarian saran melewati port seperti itu, dan *Consequences* serta temuan S4 yang menjanjikan tanpa salah alarm `TIME_WAIT` harus ditarik.

#### Option C: tetap tanpa `SO_REUSEADDR`, lalu bedakan listener dari socket terhubung dengan bind tambahan

**Pros**:

- Secara prinsip, listener di alamat LAN tetap bisa dilaporkan.

**Cons**:

- Bind saja tidak bisa membedakan listener di alamat spesifik lain dari sisa `TIME_WAIT` saat keduanya ada; kombinasi bind apa pun memberi jawaban salah untuk salah satu keadaan campuran.
- Lebih banyak bind dan aturan yang sulit diuji, tetap dengan salah alarm di kasus campuran.

#### Option D: baca daftar antarmuka jaringan (atau tabel socket OS) lalu bind setiap alamat dengan `SO_REUSEADDR`

**Pros**:

- Listener di alamat LAN terlihat tanpa salah alarm `TIME_WAIT`.

**Cons**:

- Butuh method port baru dan adapter, karena core tidak boleh memanggil API macOS atau libc; atau dependency baru. Tabel socket OS lebih berat lagi dan khusus macOS.
- Alamat berubah bersama jaringan (VPN, DHCP), dan jumlah bind tumbuh menurut jumlah antarmuka dikali kandidat saran.
- Informasi tambahannya tidak mengubah apa pun untuk endpoint loopback.

### Keputusan

Option A. Tugas langkah 5 dan 6 adalah menjawab "apakah client yang dimaksudkan untuk instance ini bisa sampai ke server lain" (BR-004, AC-056). Sebuah socket hanya menerima koneksi baru kalau ia listener di alamat yang persis sama atau di wildcard keluarga itu (dual stack menutup keduanya). Bind A, kedua wildcard, dan loopback keluarga lain dengan opsi server menanyakan tepat hal itu. Socket terhubung, termasuk sisa `TIME_WAIT`, tidak pernah menerima koneksi baru, dan listener di alamat spesifik lain juga tidak menerima koneksi ke loopback, jadi keduanya bukan overlap. Runner up: Option B, yang tidak mengubah kode tetapi meninggalkan salah alarm yang dijanjikan hilang oleh spec ini dan memberi teks yang keliru.

Dampak ke AC dan spec lain:

- AC-4: teks berubah (lihat di bawah); disetujui user lewat lead pada 2026-10-22, teks persis seperti usulan; spec diterima lead dengan opsi A dan langkah 6a.
- AC-5: pencarian saran tidak lagi melewati port yang hanya punya sisa `TIME_WAIT`.
- AC-12 dan AC-16 tidak berubah: langkah 3 (registry) dan langkah 4 (alamat persis) tetap, dan race AC-16 memakai listener di alamat endpoint yang persis sama, yang tetap `supervisor.endpoint_in_use` saat start.
- Spec 0010 tidak berubah: tahap `endpoints` start hanya bind alamat endpoint dengan `like_server` (K2).
- Teks caution langkah 5 diganti menjadi "Also in use on all addresses of this Mac; clients may reach that server", karena langkah 5 kini hanya menangkap listener wildcard; "another address" akan menyesatkan untuk listener yang justru bisa menerima koneksi ke A saat instance ini berhenti.

Teks AC-4 lama (bagian yang berubah): "bind wildcard (`0.0.0.0:p` untuk A IPv4, `[::]:p` dengan `IPV6_V6ONLY` untuk A IPv6) atau bind loopback keluarga lain (`[::1]:p` atau `127.0.0.1:p`) tanpa `SO_REUSEADDR` yang gagal menghasilkan status `caution` dengan keterangan, bukan error. Dengan listener test di `127.0.0.1:p`, `0.0.0.0:p`, `[::]:p` dual stack, `[::1]:p`, sisa socket `TIME_WAIT` di `127.0.0.1:p`, dan UDP di port yang sama,"

Teks AC-4 baru: "bind wildcard keluarga yang sama (`0.0.0.0:p` untuk A IPv4, `[::]:p` dengan `IPV6_V6ONLY` untuk A IPv6) atau bind loopback dan wildcard keluarga lain (`[::1]:p` dan `[::]:p` dengan `IPV6_V6ONLY` untuk A IPv4, `127.0.0.1:p` dan `0.0.0.0:p` untuk A IPv6), semuanya juga dengan `SO_REUSEADDR`, yang gagal menghasilkan status `caution` dengan keterangan, bukan error; socket terhubung yang memakai port p (sisa `TIME_WAIT` atau koneksi yang masih terbuka) tidak menghasilkan caution maupun error. Dengan listener test di `127.0.0.1:p`, `0.0.0.0:p`, `[::]:p` dual stack, `[::]:p` IPv6 saja, `[::1]:p`, sisa socket `TIME_WAIT` di `127.0.0.1:p`, dan UDP di port yang sama,"

Langkah build yang mewujudkannya adalah langkah 6a di milestone 1 (`index.md`, *Build plan*).

### Cross check

Dijalankan 2026-10-22 oleh model lain (Sonnet, read only) atas draft update ini, dengan jawaban tetap brief `Another model`. Verdict: desain sehat, tidak ada listener yang terlewat (setiap listener yang bisa menerima koneksi ke `127.0.0.1:p` atau `localhost` tetap tertangkap, dan hal yang sama berlaku untuk endpoint IPv6), AC-12 dan AC-16 tidak tersentuh, dan tidak ada opsi yang lebih sederhana: bind `[::]` dual stack `Free` di samping pemegang `0.0.0.0`, jadi dua wildcard tidak bisa digabung, dan empat bind adalah minimum. Temuan diterapkan dengan rekomendasi checker sesuai aturan brief (gap keputusan memakai opsi rekomendasi):

| ID | Temuan | Yang diterapkan |
| --- | --- | --- |
| GAP-1 | Teks caution langkah 6 memakai `{alamat}` tanpa definisi, padahal `PortFacts.other_loopback` tidak membedakan bind loopback dari bind wildcard | Teks selalu menyebut loopback keluarga lain (`[::1]:{port}` atau `127.0.0.1:{port}`); *Pemeriksaan port* dan *Copy preflight* |
| GAP-2 | Urutan hasil langkah 5 dan 6 tidak tertulis; `port_notes` sekarang membiarkan `Denied` di langkah 5 menutupi `InUse` di langkah 6 | Urutan teks ditulis (langkah 5 `InUse`, langkah 6 `InUse`, lalu `Denied` atau `Unavailable`, lalu `ok`) dan `port_notes` diubah di langkah 6a. Runner up: mencatat urutan sekarang sebagai sengaja, yang menyembunyikan overlap nyata di balik "couldn’t check" |
| GAP-3 | `Denied` dan `Unavailable` di bind kedua langkah 6 tidak punya test | Dua test di langkah 6a |
| GAP-4 | Tidak ada test untuk endpoint IPv6, padahal validasi paket menerima `defaultHost` `::1` | Satu kasus `::1` lewat `Site::with_provider_edit` di langkah 6a |
| GAP-5 | Detail test: teks `0.0.0.0` di `listeners.rs`, kalimat `expected` evidence, kind dan `#[ignore]` baris baru, dua bentuk koneksi terbuka, alasan `#[ignore]` | Semua disebut di langkah 6a, dengan alasan `#[ignore]` yang tetap |
| GAP-6 | Doc `BindOutcome::InUse` ("exact address and port") salah untuk bind wildcard IPv4 di samping dual stack | Ditambah ke langkah 6a |
| RISK-1 | Pemegang port di matriks tidak memasang `SO_REUSEADDR` atau `SO_REUSEPORT` | Checker menjalankan ulang dengan opsi itu, hasil sama; dicatat di *Percobaan di Mac ini* |
| RISK-2 | Dua probe bersamaan bisa saling melihat socket percobaan | Dicatat di *Consequences* (risiko lama langkah 4, jendela sedikit lebih lebar) |
| NIT-1 | Sel langkah 5 berkata listener wildcard "juga menerima koneksi ke A", padahal server yang bind `A:p` menang selama ia berjalan | Sel dan kalimat penjelasan diperbaiki |

## Update 2026-10-22: usulan spec 0024 dan sumber Copy from

Update `/architect` (sesi `arc-f20-update-2`, wave 4) atas *Usulan update untuk spec lain* dan satu baris Follow-up spec 0024 (fitur 24, duplicate config, milik program Scope 03). ID AC dan nomor langkah tetap; teks AC-20 bertambah satu kalimat dan menunggu persetujuan user lewat lead.

### Usulan yang diterapkan apa adanya

- **Baris *Batas dengan spec lain*** untuk duplicate config: `instance.get_duplicate_form`, `build_form_from_package`, mode duplicate `CreateInstanceModel` dan `NewInstanceSheet`, dan `stackly instance duplicate` milik fitur 24. Konsisten dengan spec ini: duplicate hanya mengisi draft lalu memanggil `instance.preflight` dan `instance.create` tanpa perubahan, jadi kontrak, evaluator, dan operasi create tetap milik spec ini dan tidak berubah.
- **Catatan di butir `build_form`**: `build_form` memanggil `build_form_from_package(package, source, runtime, suggested_name)`, yang juga dipakai form duplicate dengan sumber `Pinned`. Konsisten: isi form create tetap sama dan selalu bersumber catalog, test form spec ini tetap lulus tanpa perubahan (spec 0024 langkah 2), dan signature `build_form` di blok Rust tetap. Atas temuan cross check GAP-4, satu kalimat ditambah sesudah teks usulan: pemisahan isi `build_form` dibangun langkah 2 spec 0024, bukan langkah 6 spec ini, dan baris `source` di *Value sourcing* menyebut bahwa `Pinned` hanya muncul di form duplicate.

### Keputusan 19: sumber daftar Copy from di mode create

#### Masalah

`SecretField` spec 0011 menawarkan Copy from hanya kalau `copyFromOptions` tidak kosong, dan spec 0011 menyebut sheet New Instance menawarkan Generate, Enter, dan Copy from tanpa Bind. Spec ini tidak pernah menyebut dari mana daftar itu diisi. Spec 0024 butuh jawabannya, karena mode duplicate menaruh reference sumber di depan "daftar yang dipakai mode create". Yang perlu diputuskan: siapa yang membangun daftar, reference mana yang masuk, bagaimana ia dibaca manusia, kapan dibaca, dan apa yang terjadi saat pembacaan gagal. Batasannya: daemon tetap satu satunya yang memvalidasi (*Key invariants*), nilai secret tidak pernah ke client, `copyFrom` ke reference yang tidak terdaftar atau tanpa nilai sudah menjadi baris error preflight (AC-7), dan teks sheet tidak boleh terpotong di lebar 560 pt (AC-22), padahal reference instance berbentuk `local:instance/<uuid>/<purpose>`.

#### Option A: client membaca `secret.list` yang sudah ada (dipilih)

`CreateInstanceModel` memanggil `secret.list` spec 0011 bersama method form setiap kali form dimuat, memakai reference yang statusnya `present` atau `unknown` dalam urutan balasan, dan memberi label nama instance pemiliknya lewat `copyFromLabels`.

**Pros**:

- Tanpa method, field, atau fixture kontrak baru: `secret.list`, `ListSecretsReply`, dan `SecretsClient.listSecrets` sudah ada (spec 0011 langkah 9), jadi langkah 8 dan milestone 2 tidak berubah.
- `secret.list` hanya membaca cache dan tidak menyentuh Keychain (spec 0011 *Status dan probe*), jadi daftar tidak pernah tertahan dialog Keychain.
- Form create tetap hanya bergantung pada runtime dan paket; state secret yang berubah sendiri tidak masuk `CreateForm`.
- `SecretRefSummary` sudah membawa `ownerKind`, `ownerId`, dan `purpose`, dan `InstanceStore` sudah ada di sheet, jadi label bisa dibangun tanpa data baru.
- Mode duplicate spec 0024 cukup menaruh reference sumber di depan daftar yang sama.

**Cons**:

- Satu panggilan tambahan per form, dan penyaringan status di model Swift. Penyaringan itu memilih apa yang ditawarkan, bukan aturan validasi: daemon tetap memeriksa setiap `copyFrom`.
- Label butuh parameter `copyFromLabels` di `SecretField`, yang diusulkan spec 0024 untuk spec 0011; update spec 0011 itu harus masuk sebelum langkah 14 spec ini.
- Daftar adalah potret; tidak mengikuti `secret.changed` selama sheet terbuka.

#### Option B: field `copyFromReferences` di `CreateForm`

Daemon menambah daftar reference, beserta label, ke `CreateForm` saat `instance.get_create_form`.

**Pros**:

- Satu panggilan; daemon yang memutuskan isi daftar dan label; `DuplicateForm.form` ikut membawanya.

**Cons**:

- Field kontrak baru di langkah 8, fixture `create-form-*.json` berubah, dan teks AC-1 (isi form dan apa yang dibaca method itu) ikut berubah.
- `instance.get_create_form` mulai membaca tabel secret dan nama instance, padahal isinya sudah tersedia lewat `secret.list` dan `InstanceStore`; dua jalan untuk data yang sama.
- Form yang sebelumnya statis per runtime menjadi bergantung pada state secret yang berubah sendiri, dan `SecretField` tetap butuh parameter label.

#### Option C: tanpa Copy from di mode create

`copyFromOptions` selalu kosong di create; hanya mode duplicate yang memberi reference sumber.

**Pros**:

- Paling sederhana; tidak ada panggilan, penyaringan, atau label tambahan, dan tidak ada ketergantungan pada update spec 0011.

**Cons**:

- Bertentangan dengan spec 0011 (sheet New Instance menawarkan Copy from), dan GUI kehilangan jalan yang CLI punya (`--secret <field>=copy:<ref>`): pengguna yang ingin password sama untuk instance kedua harus mengetiknya ulang.

#### Keputusan

Option A. Kekuatan utamanya ada di batasan yang sudah ada: daemon tetap satu satunya yang memutuskan apakah reference bisa disalin (baris preflight `secret.<field>` dan `put` di tahap `secrets`), jadi daftar di client hanya kemudahan memilih, dan datanya sudah tersedia lewat method baca yang tidak menyentuh Keychain. Runner up: Option B, yang memberi daftar dari daemon tetapi menambah field kontrak dan mengubah AC-1 untuk data yang sudah punya method sendiri.

Rincian yang diputuskan bersama opsi ini, semuanya rekomendasi update ini (dan, untuk yang ditandai, perbaikan cross check yang diterapkan dengan aturan brief):

- **Status yang masuk**: `present` dan `unknown`. `unknown` berarti probe gagal (worker sibuk saat start, error backend, atau store yang tidak didukung), bukan bahwa nilainya hilang; daemon memeriksa lagi saat preflight (`SecretStore::probe`) dan saat `put`, dan memberi baris error kalau reference itu tidak punya nilai *(alasan diperbaiki, RISK-1)*. `missing` dibuang karena pasti ditolak `secret.needs_secret`; `unrecognized` dibuang karena client tidak tahu artinya. Runner up: hanya `present` lewat filter params `status`, yang menyembunyikan reference yang sebenarnya bisa disalin.
- **Prefix**: reference yang segmen pertamanya `stackly` dibuang, karena segmen itu dicadangkan untuk secret internal (spec 0011 *Format secret reference*) *(RISK-2)*.
- **Pemilik**: semua jenis pemilik (instance, environment, user), karena `put` dengan `copyFrom` membaca reference mana pun yang terdaftar dan aturan field diperiksa saat `put` (spec 0011 AC-5 dan langkah 3 *Use case isi atau ganti nilai*) *(rujukan diperbaiki, NIT-2)*.
- **Urutan**: urutan balasan daemon (urut reference), tanpa urutan kedua di client.
- **Label** *(RISK-2)*: "{purpose} of “{nama}”" untuk reference ber `ownerKind` `instance` yang `ownerId` nya ada di `InstanceStore`, dengan purpose yang tanda hubungnya diganti spasi dan huruf pertamanya dibesarkan; reference lain tampil sebagai teks reference. Untuk reference sumber PostgreSQL, hasilnya sama dengan label mode duplicate spec 0024 ("Superuser password of “pg17-dev”"). Label lewat `copyFromLabels`, parameter yang diusulkan spec 0024 untuk spec 0011, sehingga satu mekanisme melayani kedua mode. Runner up: teks reference apa adanya dengan Follow-up label, yang membuat Copy from hampir tidak terpakai (UUID tidak bisa dibedakan) dan berisiko terpotong di 560 pt (AC-22).
- **Waktu baca** *(GAP-2)*: setiap kali model masuk `loadingForm`, satu tugas muat bertoken memanggil method form dan `listSecrets` bersamaan; tugas lama dibatalkan dan balasan token lama dibuang. Model masuk `editing` begitu form kembali, tanpa menunggu daftar, supaya batas `.standard` 10 detik `SecretsClient.listSecrets` tidak pernah menahan sheet; Copy from muncul saat daftar kembali. Form yang dimuat membangun ulang draft, jadi pilihan Copy from lama hilang. Tanpa langganan `secret.changed`. Runner up: menunggu keduanya dengan batas waktu sendiri di model, yang menambah konstanta dan tetap menahan form selama batas itu.
- **Pemilik data di model** *(GAP-3)*: `CreateInstanceModel.copyFromOptions` dan `copyFromLabels`, diteruskan `SchemaFormModel` ke setiap `SecretField`. `store` di balasan tidak dipakai, karena keadaan store tampil di baris preflight.
- **Gagal**: daftar kosong, Copy from tersembunyi, tanpa banner, Create tidak tertahan. Runner up: banner caution, yang menambah state di sheet untuk fitur kemudahan.
- **Mode duplicate** *(GAP-1)*: spec 0024 memanggil `listSecrets` bersama `getDuplicateForm` lewat tugas muat yang sama dan menaruh reference sumber setiap field di depan daftar ini; label sumbernya menang untuk reference itu. Spec 0024 milik Scope 03, jadi penggabungan label itu dicatat sebagai Follow-up untuk lead.
- **Nilai yang melanggar batas field** *(RISK-3)*: preflight tidak pernah membaca nilai, jadi `copyFrom` dari reference yang nilainya di luar `minLength`, `maxLength`, atau `printableOnly` baru gagal `secret.invalid_value` di tahap `secrets`, sebelum folder dibuat, lalu dikompensasi. Dicatat di *Consequences* dan diuji di test secret daemon dan test model.

Dampak ke spec ini: AC-20 bertambah satu kalimat (teks di bawah; AC-21 tetap karena ia hanya tentang render dari DTO, NIT-1); *Sheet New Instance* mendapat butir Copy from, `listSecrets` di `CreateInstanceService`, dan `InstanceStore` di masukan model; *Form dari settings schema*, *Value sourcing*, *Security model*, *Critical test scenarios*, *Consequences*, *Follow-up*, dan langkah 13 dan 14 milestone 3 ikut; prasyarat milestone 3 menyebut fitur 11 langkah 9 dan update spec 0011 untuk `copyFromLabels` sebelum langkah 14. Tidak ada method, field, kode error, atau fixture baru, jadi langkah 8 dan milestone 2 tidak berubah. Spec ini tidak mengubah spec lain; tiga hal untuk pemiliknya ada di *Follow-up* `index.md` (jadwal usulan `copyFromLabels` spec 0011, penggabungan label di spec 0024, dan kalimat `unknown` di spec 0011 *Status dan probe* yang bertentangan dengan AC-3 spec itu).

Teks AC-20 lama (bagian yang berubah): "Create yang hanya aktif saat preflight terakhir `ready` untuk nilai form saat ini dan daemon tersambung. Sheet dibuka dari aksi"

Teks AC-20 baru: "Create yang hanya aktif saat preflight terakhir `ready` untuk nilai form saat ini dan daemon tersambung. Copy from di setiap secret field menawarkan reference dari `secret.list` spec 0011 berstatus `present` atau `unknown` yang segmen pertamanya bukan `stackly`, dalam urutan balasan daemon, dibaca setiap kali form dimuat tanpa menunda form, dengan label "{purpose} of “{nama}”" untuk reference milik instance di `InstanceStore`, dan tersembunyi tanpa menahan Create selama daftar itu belum kembali, kosong, atau gagal dibaca *(ditambah 2026-10-22, Follow-up spec 0024)*. Sheet dibuka dari aksi"

### Cross check

Dijalankan 2026-10-22 oleh model lain (Sonnet, read only) atas draft update ini, dengan jawaban tetap brief `Another model`. Verdict: Option A sehat dan tidak bertentangan dengan aturan inti spec 0011; nama `SecretsClient.listSecrets`, `DaemonLink.secrets`, `SecretRefSummary`, dan `SecretStatus` cocok dengan repo, `SecretField` memang menyembunyikan Copy from saat daftar kosong, `copyFrom` tidak membatasi jenis pemilik, dan `secret.list` tidak menyentuh store. Temuan diterapkan dengan rekomendasi checker sesuai aturan brief (gap keputusan memakai opsi rekomendasi):

| ID | Temuan | Yang diterapkan |
| --- | --- | --- |
| GAP-1 | Mode duplicate memuat lewat `getDuplicateForm`, dan tidak jelas apakah ia memanggil `listSecrets` dan bagaimana reference sumber digabung | Tugas muat memanggil method form mode itu bersama `listSecrets`; reference sumber di depan dengan label sumbernya; penggabungan label sebagai Follow-up spec 0024 |
| GAP-2 | "Tidak menahan form" salah kalau `editing` menunggu keduanya, karena batas `.standard` 10 detik; balasan basi dan reset draft tidak disebut | `editing` tanpa menunggu daftar, tugas muat bertoken yang membuang balasan lama, draft dibangun ulang saat form dimuat |
| GAP-3 | Pemilik `copyFromOptions` di model, `store` yang diabaikan, kasus test, sumber data preview, dan state snapshot tidak disebut | `CreateInstanceModel.copyFromOptions` dan `copyFromLabels` lewat `SchemaFormModel`; `store` tidak dipakai; kasus test lengkap; preview dengan service tiruan; state snapshot Copy from terpilih |
| GAP-4 | Catatan `build_form` terbaca seolah sudah dibangun; baris `source` *Value sourcing* hanya menyebut catalog | Kalimat sesudah teks usulan (signature tetap, pemisahan di langkah 2 spec 0024); baris `source` menyebut `Pinned` hanya di form duplicate |
| RISK-1 | Alasan `unknown` saat Keychain terkunci bertentangan dengan spec 0011 AC-3 (probe di keychain terkunci menjawab `Present` atau `Missing`) | Alasan diganti (probe gagal karena worker sibuk, error backend, atau store tidak didukung); aturannya tetap; kalimat spec 0011 yang bertentangan dicatat sebagai Follow-up |
| RISK-2 | Reference instance berbentuk UUID tidak bisa dibedakan dan berisiko terpotong; prefix `stackly` dicadangkan | Label "{purpose} of “{nama}”" lewat `copyFromLabels` sekarang, bukan Follow-up; prefix `stackly` dibuang |
| RISK-3 | Pelanggaran batas field pada nilai `copyFrom` baru terlihat setelah Create | Dicatat di *Consequences*; test daemon di skenario secret dan test model untuk banner |
| NIT-1 | Kalimat muat lebih cocok di AC-20 daripada AC-21; status "diterapkan" terbaca final padahal teks AC menunggu persetujuan | Kalimat pindah ke AC-20; Follow-up menyebut teks AC-20 menunggu persetujuan |
| NIT-2 | Rujukan spec 0011 kurang tepat | *Status dan probe* untuk cache, AC-5 dan langkah 3 *Use case isi atau ganti nilai* untuk jenis pemilik |

## Update 2026-10-22: dry run init tanpa port `Auto` (usulan spec 0023)

Sumber nya *Usulan update spec lain (2026-10-22)* butir 2 di spec 0023 (commit `73322b32`, merge `d94c4c87`), dibawa lead scope 02 lewat brief `arc-f15-update-4`. Dry run AC-9 memberi port `defaultPort` ke endpoint `Auto` untuk `init_plan` maupun `run_plan`, sedangkan `execute_init` di langkah `init` create memakai `Prepared::context` yang tidak memberi port ke endpoint `Auto` (spec 0010 *Value sourcing*, baris `init`). Jadi paket yang init nya butuh port (NATS `1.0.0`) mendapat preflight `ready`, lalu create gagal `supervisor.config_invalid` di langkah `init` dan dikompensasi. Spec 0023 memperbaiki NATS di paket; butir ini penjaga kedua, dan spec 0009 (AC-21, `init_port_issues`) penjaga pertama untuk paket di repo.

### Keputusan 20: context dry run

- **Option 1: dua context (dipilih).** `init_plan` memakai context yang sama dengan `execute_init` (port `Fixed` apa adanya, `Auto` tanpa port), dan `run_plan` tetap memakai `Fixed` atau `defaultPort`. Kelebihan: preflight memeriksa persis apa yang akan dijalankan langkah `init`, dan `run_plan` tetap terperiksa dengan port yang masuk akal (start memilih port nyata nanti). Kekurangan: dua context dibangun per preflight, dan pemetaan `EndpointUnresolved` dari init butuh keterangan sendiri.
- **Option 2: satu context tanpa port untuk keduanya.** Kekurangan: `run_plan` selalu gagal untuk endpoint `Auto`, karena plan run memang butuh port.
- **Option 3: tetap satu context dengan `defaultPort`, dan andalkan `init_port_issues` spec 0009.** Kekurangan: salinan pinned lama dan paket yang tidak lewat `provider check` (import fitur 37, duplicate dari paket pinned) tidak terjaga, dan preflight tetap tidak memeriksa yang benar benar dijalankan.

### Keputusan kecil

- **Kode dan path.** `PlanError::EndpointUnresolved` sudah dipetakan ke `CreateError::EndpointInvalid { endpoint }` dengan field `endpoints[<i>].port` (`invalid_value`) kalau draft menyebut endpoint itu; pemetaan itu dipakai apa adanya. Draft yang tidak menyebut endpoint (default `Auto`, misalnya `stackly instance create` tanpa `--port`) mendapat error tanpa field issue.
- **Keterangan baris.** Pesan `FieldError` adalah teks tetap per kode (spec 0006), jadi kalimat yang menyebut bahwa provider ini butuh port fixed tidak bisa ditaruh di field. Dipilih: keterangan baris `settings` yang menyebut nama endpoint. Tanpa ini, draft yang tidak menyebut endpoint mendapat "0 settings need attention", karena tidak ada field issue yang dihitung. Runner up: memindahkan error ke baris `endpoint.<n>` endpoint `Auto` itu. Kekurangan nya: baris endpoint dihitung sebelum settings tervalidasi, jadi evaluator harus menulis ulang baris yang sudah selesai, dan urutan baris error pertama (yang menentukan error sinkron create) ikut berubah.
- **Fixture test.** Paket fake yang disunting test (`Site::with_provider_edit`), karena `Site` hanya membaca `providers/`, dan paket fake asli tidak memakai port di init.
- **Urutan.** Milestone baru (5) sesudah fitur 23 langkah 13, supaya test "NATS dengan semua endpoint `Auto` tetap `ready`" berjalan atas `1.0.1`, dan langkah 22 spec 0009 masuk di milestone yang sama.

### Dampak ke spec 0024 (milik scope 03, tidak diedit)

Draft duplicate mengirim setiap endpoint sebagai `auto` (AC-4 spec 0024) dan memakai `instance.preflight` serta `instance.create` tanpa perubahan (AC-6 spec 0024). Teks kedua AC tetap benar. Yang berubah adalah hasil nya untuk sumber yang paket pinned nya butuh port di init: preflight `ready: false` dengan baris `settings` error `instance.endpoint_invalid`, keterangan endpoint, dan field `endpoints[<i>].port` `invalid_value` (draft duplicate menyebut setiap endpoint), sampai pengguna memilih Fixed. Hari ini satu satunya paket seperti itu adalah salinan pinned NATS `1.0.0`.

### Teks AC yang berubah (menunggu persetujuan kamu lewat lead)

AC-9: kalimat dry run. Teks lama dan baru lengkap di catatan bersama `arc-f15-update-4-ac-and-scope.md`. ID tetap.

### Cross check update ini (2026-10-22, model lain: Sonnet 5.5, baca saja)

Temuan untuk spec ini, diisi dengan rekomendasi saya (aturan delegasi user di brief; Keputusan 20 tetap):

| Temuan | Yang diterapkan |
| --- | --- |
| `dry_run` mengembalikan `Option<CreateError>`, dan `EndpointInvalid` hanya membawa nama endpoint, jadi `settings_row` tidak bisa memilih keterangan per sebab | enum privat `DryRunRefusal` (`InitNeedsFixedPort`, `Endpoint`), dipetakan `settings_row` ke kode yang sama; `subject` tetap `None` |
| permintaan `Fixed` yang tidak valid menjadi `None` di context init, jadi terbaca sebagai `Auto` dan memunculkan keterangan "needs a fixed port" di samping error endpoint | `defaultPort` untuk permintaan seperti itu di kedua context; test untuk port 70000 |
| cadangan 49152 di context run tidak tertulis | ditulis, dengan catatan bahwa ia tidak terpakai untuk paket tervalidasi |
| `stackly instance create` tanpa `--port` yang ditolak sinkron tidak mendapat penjelasan, karena teks nya hanya di baris preflight | ringkasan katalog `ENDPOINT_INVALID` mendapat sebab baru dan `codes::instance::hint` mendapat hint statis; keduanya di luar digest kontrak, jadi tanpa kenaikan `ProtocolVersion`; baris `instance.endpoint_invalid` spec 0010 *Kode error* mendapat klausa sebab yang sama. Runner up: kunci `details` `reason` (`request`, `init_needs_port`) dengan hint per reason, lebih tepat tetapi mengubah digest dan menambah kenaikan minor di langkah 17 |

## Update 2026-10-22: register dalam satu transaksi registry (temuan milestone 4)

Sumber nya temuan milestone 4 di `verify.md` (bagian *Commands* dan *Acceptance-criteria coverage*) dan test `apps/daemon/tests/create_perf.rs`, dibawa lead scope 02 lewat brief `arc-f20-update`. Lima create berbeda (nama dan port `Fixed` berbeda) yang dilepas bersamaan setelah dialog Keychain dijawab hampir semuanya berakhir `failed` dengan `registry.generation_conflict` dan dikompensasi penuh: nol atau satu dari lima berhasil, pada load average 66 sampai 101. Tidak ada aturan create yang dilanggar; kelima draft sah.

### Penyebab

Langkah `register` membaca registry beserta `generation` nya (job baca di thread registry), memeriksa ulang aturan di `spawn_blocking`, lalu meng commit batch nya lewat `ctx.commit` dengan `expected_generation` dari baca itu (job tulis kedua di thread yang sama). `generation` naik tepat 1 untuk setiap commit, apa pun isinya (spec 0005): progress operasi, journal, langkah, catatan `created`, reference secret, dan audit. Lima create yang berjalan bersamaan menulis puluhan commit journal per detik, dan di Mac yang ramai jendela antara job baca dan job tulis lebih panjang dari jarak antar commit itu. Jadi hampir setiap percobaan kalah, dan lima percobaan habis dalam beberapa milidetik. Pola yang sama ada di `finalize` engine (`FINALIZE_RETRIES`, dipakai install runtime spec 0018), tetapi jendela nya lebih pendek dan operasi nya lebih jarang.

### Keputusan 21: cara `register` memeriksa ulang aturan registry tepat sebelum menulis

- **Option A: commit terjaga, yaitu baca, periksa ulang, dan tulis dalam satu transaksi registry (dipilih).** Port registry mendapat `commit_guarded(guard)`: guard (closure milik pemanggil) dan op yang ia kembalikan berjalan sebagai satu job di thread registry, di dalam satu `BEGIN IMMEDIATE`. Thread registry memang satu satunya pemegang koneksi, jadi tidak ada commit lain yang bisa masuk di antara baca guard dan tulisan nya. Kelebihan: tidak ada lagi konflik, ulang, atau kebetulan; lima create bersamaan selalu berhasil selama draft nya sah; AC-10 menjadi lebih kuat, karena aturan diperiksa di transaksi yang sama dengan tulisan, bukan "sedekat mungkin" dengan nya; kompensasi tidak berubah (niat dicatat sebelum, konfirmasi sesudah); waktu thread registry per create tidak bertambah, karena baca nya sama dengan yang dulu dijalankan job baca terpisah; dan arti `generation` untuk pemakai lain tidak berubah. Kekurangan: kode milik pemanggil berjalan di thread registry di dalam transaksi tulis, jadi guard yang lambat menahan semua panggilan registry (dijaga aturan "guard itu murni" dan `catch_unwind`); trait `Registry` mendapat satu method yang harus diimplementasikan tiga registry hari ini (`SqliteRegistry`, `FakeRegistry`, dan `FaultyRegistry` yang mendelegasikan), dan satu trait baca kecil (`GuardView`) dengan dua implementasi (view transaksi SQLite dan view state fake); alasan penolakan lewat port sebagai `Box<dyn Any + Send>`.
- **Option B: generation yang hanya naik untuk tulisan yang menyentuh aturan (journal tidak menaikkan nya).** Kelebihan: memperbaiki semua pemakai `expected_generation` sekaligus, dengan sedikit kode di storage. Kekurangan: arti `generation` di spec 0005 ("setiap commit, apa pun isinya") berubah untuk semua pemakai; klasifikasi op per tabel rawan salah, dan salah klasifikasi diam diam melemahkan AC-10 (contoh: `runtime_references` membaca operasi yang belum selesai, jadi `InsertOperation` dan perubahan state operasi tetap harus menaikkan nya); dan tetap optimistik, karena tulisan yang memang menyentuh aturan dari create lain (register, init yang mengubah instance, reference secret kalau klasifikasi nya konservatif) masih bisa menghabiskan lima percobaan untuk lima create bersamaan di Mac yang ramai.
- **Option C: backoff dengan jitter, lebih banyak percobaan, atau batas waktu.** Kelebihan: hanya mengubah `register`, tanpa port baru. Kekurangan: tetap untung untungan; jendela baca dan tulis tetap lebih panjang dari jarak antar commit journal saat beban tinggi; latensi create menjadi tidak bisa ditebak; dan "lima create bersamaan berhasil" hanya bisa dibuat mungkin, tidak bisa dibuktikan, sehingga test nya akan flaky.
- **Option D: serialisasi `register` antar create di core (satu mutex async).** Kekurangan: tidak menghentikan journal operasi lain (termasuk langkah lain create yang sama sama berjalan) menaikkan `generation` di antara baca dan tulis; mengurangi, tidak memperbaiki.
- **Option E: aturan dipindah ke `WriteOp` deklaratif yang diperiksa storage (misalnya "nama bebas", "port bebas").** Kekurangan: aturan port (`endpoints_conflict`, slot alamat dan transport) dan `multi_instance` ada di core; menyalin nya ke storage membuat dua sumber aturan yang bisa berbeda, padahal AC-028 dan AC-10 butuh satu.

**Dipilih A**, karena hanya A yang membuat create bersamaan berhasil secara pasti, sambil menjaga satu sumber aturan (`recheck` di core) dan arti `generation` untuk semua pemakai lain. Runner up: B, kalau nanti banyak pemakai `expected_generation` lain menunjukkan masalah yang sama; B bisa ditambah tanpa membatalkan A.

### Keputusan kecil

- **Bentuk `GuardView`.** Hanya tiga baca yang dibutuhkan `recheck` (`instances`, `runtime`, `provider_revision`), tanpa `Versioned`, karena semuanya satu transaksi. Pemakai berikutnya menambah baca lewat update spec 0005. Runner up: `GuardView` yang memuat semua baca `Registry`; lebih fleksibel, tetapi tiga implementasi harus menulis puluhan method yang belum dipakai.
- **Alasan penolakan.** `GuardDecision::Refuse(Box<dyn Any + Send>)`, di downcast core ke `CreateError`. Runner up: guard menyimpan alasan di slot `Arc<Mutex<Option<CreateError>>>` yang ia tangkap; tanpa `Any`, tetapi alasan dan hasil commit berpisah, dan lupa membaca slot tidak ketahuan compiler.
- **Error baca di guard.** `recheck` tetap memetakan error baca ke `CreateError::DataUnavailable`, jadi kode setiap penolakan sama dengan sebelum perubahan. Runner up: membiarkan error baca menjadi kode registry; lebih jujur, tetapi mengubah kode yang sudah dibangun dan diuji.
- **Panic di guard.** Ditangkap dan menjadi `Io`, supaya satu bug di guard tidak menghentikan thread registry (yang membuat daemon fatal, spec 0005). Tanpa varian `RegistryError` baru, jadi pemetaan kode daemon tidak berubah.
- **Test.** Kait `FaultyRegistry::before_guarded_commit` membuktikan bahwa guard melihat tulisan yang masuk tepat sebelum commit, dan conformance 16 thread membuktikan bahwa commit terjaga tidak pernah bentrok. Test daemon tanpa batas waktu (tiga putaran lima create) menjaga skenario temuan di `cargo test` biasa, terpisah dari pengukuran waktu `create_perf`.
- **Urutan.** Milestone baru (6) setelah langkah 16, tanpa menunggu update spec 0005 dan 0007 atau milestone 3.

### Dampak ke spec lain

- Spec 0005 dan 0007: method dan API baru, teks di blok *Usulan 2026-10-22: commit terjaga* di index.md.
- Spec 0018: tidak ada perubahan sekarang; `finalize` install tetap optimistik dengan lima ulang, dan pemindahan nya ke `commit_guarded` adalah keputusan spec 0007 dan 0018 kalau pengukuran menunjukkan nya.
- Spec 0024 dan 0025 (scope 03, tidak diedit): mendapat perbaikan ini lewat `instance.create` tanpa perubahan teks.
- Spec 0010: commit actor tidak terkena, karena tidak memakai `expected_generation`; satu kata kata di baris `instance.create` *Permukaan IPC* yang masih menyebut `registry.generation_conflict` sebagai kegagalan create (blok usulan spec 0010 butir 1).

### Teks AC yang berubah (menunggu persetujuan kamu lewat lead)

AC-10: kalimat langkah `register` diganti (commit terjaga, tanpa `expected_generation` dan tanpa ulang), dan satu kalimat ditambah di akhir (lima create bersamaan di belakang dialog Keychain semuanya `succeeded`). ID tetap. Sampai disetujui, AC-10 di index.md masih teks lama, dan milestone 6 menunggu persetujuan itu. Teks lengkap nya ada di sini, supaya persetujuan bisa diterapkan dari repo (juga di catatan bersama `arc-f20-update-ac10-and-scope.md`). Awal AC-10 dan kalimat balapan nya tidak berubah.

- **Lama (bagian yang berubah):** "... langkah `register` membaca registry beserta generation nya, memeriksa ulang setiap aturan yang bersumber dari registry (nama, `multi_instance`, runtime ada dan `installed` dengan software sama, langkah 2 dan 3 *Pemeriksaan port*), lalu meng commit `InsertProviderRevision` (kalau belum ada), `InsertInstance`, `InsertConfigRevision`, dan `ReplaceSecretUses` dalam satu batch lewat `ctx.commit` dengan `expected_generation` dari baca itu, dan mengulang paling banyak lima kali saat `GenerationConflict` atau `DuplicateResource` untuk provider revision. Sepuluh create bersamaan ..."
- **Baru:** "... langkah `register` memeriksa ulang setiap aturan yang bersumber dari registry (nama, `multi_instance`, runtime ada dan `installed` dengan software sama, langkah 2 dan 3 *Pemeriksaan port*) dan meng commit `InsertProviderRevision` (kalau belum ada), `InsertInstance`, `InsertConfigRevision`, dan `ReplaceSecretUses` dalam satu transaksi registry lewat `ctx.commit_guarded` (*Commit terjaga di langkah `register`*), sehingga tidak ada commit lain yang masuk di antara pemeriksaan dan tulisan; langkah ini tidak memakai `expected_generation`, tidak mengulang, dan tidak pernah gagal `registry.generation_conflict` *(diubah 2026-10-22, temuan milestone 4)*. Sepuluh create bersamaan ..."
- **Lama (akhir AC-10):** "... Yang kalah tidak meninggalkan folder, reference, atau baris registry."
- **Baru (akhir AC-10):** "... Yang kalah tidak meninggalkan folder, reference, atau baris registry. Lima create dengan nama dan port `Fixed` berbeda yang dilepas bersamaan setelah dialog Keychain dari aksi lain dijawab semuanya berakhir `succeeded` *(ditambah 2026-10-22)*."

### Cross check update ini (2026-10-22, model lain: Sonnet 5.5, baca saja)

Tidak ada gap kelengkapan keputusan yang butuh jawaban user: Keputusan 21 tetap, AC-10 menjadi lebih kuat, AC-11 tidak berubah, dan keenam kutipan teks lama di blok usulan spec 0005 dan 0007 cocok persis. Temuan kelayakan dan konsistensi berikut diperbaiki langsung, tanpa mengubah keputusan:

| Temuan | Yang diterapkan |
| --- | --- |
| langkah 19 menghapus `conflict_instance_inserts` padahal test lama `create_operation` masih memakai nya sampai langkah 20, jadi clippy langkah 19 gagal | penghapusan helper pindah ke langkah 20, di commit yang sama dengan test pengganti; langkah 20 mendapat gerbang clippy, `check-deps`, `cargo check` Windows, dan `supervisor_start`, plus penghapusan import yang tidak terpakai |
| kasus d memakai `fail_commits`, yang dimakan commit apa pun berikutnya (journal dan tahap engine), dan "guard dipanggil tepat sekali" tidak punya cara hitung | `FaultyRegistry::fail_guarded_commits`, `before_guarded_commit` sekali pakai, dan `guard_calls()` lewat closure penghitung, dengan urutan kait, fault, lalu delegasi (aturan 6); `fail_commits` tidak dimakan `commit_guarded` |
| conformance 16 thread menghitung semua instance yang tidak retained, padahal suite berbagi registry yang sudah berisi data | hitungan hanya untuk prefix nama `guard-<run>-` yang baru per run |
| teks basi: butir `validate` dan `register` di *Pemeriksaan port*, baris `expected_generation` di *Value sourcing*, dan baris `instance.create` spec 0010 yang masih menyebut `registry.generation_conflict` | kedua baris spec ini ditulis ulang; blok usulan spec 0010 butir 1; teks lama dan baru AC-10 disalin ke bagian di atas |
| detail yang harus ditebak: `loaded_at` view SQLite, `restored` di view fake, daftar `pub use` di `lib.rs`, `#[non_exhaustive]`, loop terap fake, dan jumlah implementasi `GuardView` | ditulis di *Commit terjaga di langkah `register`* dan langkah 19; `Guarded` dan `GuardDecision` tidak `#[non_exhaustive]`; test daemon dicatat sebagai smoke test, dengan bukti deterministik di conformance dan kasus b dan c |
