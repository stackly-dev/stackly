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

#### Option 1: bind tepat di alamat endpoint dengan `SO_REUSEADDR` sebagai error, bind wildcard dan loopback keluarga lain tanpa opsi itu sebagai caution, plus `endpoints_conflict` atas registry (dipilih setelah cross check)

**Pros**:

- Meniru cara server sungguhan bind (PostgreSQL dan server Go memasang `SO_REUSEADDR`), jadi error hanya muncul untuk konflik yang pasti membuat start gagal; sisa `TIME_WAIT` setelah stop tidak lagi menjadi salah alarm.
- Listener wildcard, dual stack, alamat LAN, dan `::1` tetap terlihat sebagai caution, sehingga `localhost` yang bisa sampai ke server lain diperingatkan tanpa memblokir instance yang sebenarnya bisa jalan.
- Port milik instance yang sedang `stopped` tetap dianggap terpakai lewat registry.

**Cons**:

- Hasil bergantung pada perilaku stack jaringan macOS, jadi butuh test native yang mencatatnya; `std::net` tidak bisa dipakai, sehingga core mendapat dependency `socket2`.
- Hanya pemilik Stackly yang bisa dinamai.

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
