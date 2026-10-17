# 0004. Rationale: domain core dan abstraksi platform

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: fitur 4 menggabungkan beberapa keputusan yang biasanya berdiri sendiri: model domain, tiga state machine, port platform, layout direktori, metadata folder instance, dan fake untuk test. Spec ini tetap satu karena semuanya bertemu di satu jalur yang harus jalan utuh (arahkan Stackly ke root, siapkan folder, buat folder instance, inventarisasi tanpa registry) dan di satu set aturan dependency. Risikonya adalah memodelkan domain sebelum fitur 5 sampai 11 memakainya; karena itu field dibatasi pada yang disebut PRD dan scope, dan bagian yang tumbuh besar nanti (misalnya engine operasi) dipecah lewat update atau spec anak.

Stackly menjanjikan banyak versi dan banyak instance enam provider di satu Mac, dengan satu daemon per user sebagai satu satunya penulis registry dan pengendali proses (basis: PRD bagian 6 dan 15, BR-011). Hampir setiap fitur berikutnya membaca atau mengubah konsep yang sama: runtime, instance, environment, lease, operasi. PRD sudah memberi definisi dan contoh state (bagian 6, 18.2, dan 19.1) serta 25 aturan bisnis (bagian 9), tetapi belum ada satu tempat di kode yang menegakkannya. Tanpa itu, registry (fitur 5), IPC (fitur 6), engine operasi (fitur 7), dan supervisor (fitur 10) masing masing akan menulis enum state dan aturan sendiri, dan perbedaan kecil di antaranya baru ketahuan saat data pengguna sudah ada.

Batas arsitekturnya sudah dikunci spec 0001: `stackly-domain` tidak boleh mengimpor crate Stackly lain, tokio, atau libc; domain, platform, protocol, dan core harus lulus `cargo check` untuk `aarch64-pc-windows-msvc`; `unsafe` hanya di `stackly-platform-macos`; dan `stackly-testkit` hanya dependency dev yang tidak boleh bergantung pada core (basis: spec 0001, AGENTS.md). Scaffold menyisakan satu trait resolver `app_support_dir()` yang ditandai `// provisional: F4`, satu fake resolver di testkit, dan resolver macOS yang memakai `std::env::home_dir()` (basis: `crates/platform/src/lib.rs`, `crates/testkit/src/lib.rs`, `crates/platform-macos/src/lib.rs`). Windows harus siap tanpa harus selesai: capability yang belum dibuat wajib menjawab not supported (basis: REQ-050, NFR-010, AC-050), dan dukungan native tidak boleh diklaim dari artefak macOS atau WSL (basis: AC-074).

PRD bagian 20.1 mengusulkan layout di `~/Library/Application Support/Stackly`, log di `~/Library/Logs`, cache di `~/Library/Caches`, dan socket di "direktori runtime privat" yang pendek, dengan larangan path hardcode (basis: PRD bagian 20.1). Batas path socket Unix di macOS adalah 104 byte termasuk NUL. Path socket PostgreSQL di dalam folder instance, misalnya `/Users/<nama>/Library/Application Support/Stackly/instances/<uuid 36 karakter>/run/.s.PGSQL.54321`, sudah sekitar 120 byte untuk nama user sebelas huruf, jadi masalahnya bukan hanya socket daemon (basis: POSIX `sockaddr_un`, spec 0003 yang memakai `/tmp/sk-<8 hex>` untuk alasan yang sama). Integration test wajib memakai test root ephemeral, tidak pernah folder Application Support asli, dan tanpa state global (basis: AGENTS.md, spec 0002). Folder instance harus tetap bisa dikenali kalau registry hilang, untuk inventaris orphan dan reinstall tanpa `initdb` (basis: REQ-053, AC-053, fitur 57 dan 62).

## Options considered

### Keputusan 1: cakupan trait platform di fitur ini

#### Option 1: Hanya yang dibutuhkan fitur 4, plus daftar capability lengkap

Trait untuk layout direktori, filesystem, jam, identitas user, dan info platform. Proses, secret, endpoint IPC, background agent, dan metrik hanya muncul sebagai capability dengan status not supported sampai fiturnya membangun trait sendiri (basis: spec 0001 *Batas keputusan*, fitur 6, 10, 11, 15, 28).

**Pros**:

- Setiap trait langsung dipakai dan diuji; tidak ada signature tebakan.
- Daftar capability sudah lengkap, jadi AC-050 bisa diuji sekarang.

**Cons**:

- `Platform` akan berubah konstruktornya setiap kali fitur baru menambah trait.

#### Option 2: Semua signature sekarang

Tulis juga trait proses, secret, IPC, dan background agent sekarang (basis: daftar isi `stackly-platform` di spec 0001).

**Pros**:

- Satu tempat untuk melihat seluruh permukaan platform sejak awal.

**Cons**:

- Fitur 6, 10, dan 11 hampir pasti mendesain ulang trait itu setelah menyentuh API nyata (identitas proses majemuk, Keychain terkunci, peer credential socket), jadi pekerjaannya dua kali.

#### Option 3: Resolver direktori saja

Ganti `app_support_dir()` dengan resolver lengkap, tanpa port filesystem.

**Pros**:

- Perubahan paling kecil.

**Cons**:

- Cek containment, kepemilikan root, dan inventaris tidak punya port, jadi core tidak bisa diuji dengan fake (basis: PRD bagian 15.2, "semua mutasi penting harus dapat diuji memakai fake platform").

### Keputusan 2: cara BR-001 sampai BR-025 diwujudkan

#### Option 1: Yang bisa dicek sekarang menjadi kode, sisanya dipetakan

Aturan yang punya input nyata di domain atau core sekarang mendapat fungsi dan test berevidence; aturan lain dipetakan ke fitur pemilik dengan tipe domain yang membuatnya bisa dinyatakan; aturan kebijakan ditulis, bukan dipalsukan sebagai cek (basis: spec 0002, test yang membuktikan ID PRD menulis evidence).

**Pros**:

- Setiap test BR membuktikan sesuatu yang benar benar dijalankan.
- Tabel pemetaan membuat aturan yang belum dicek tetap terlihat.

**Cons**:

- Tujuh aturan tetap tanpa test di fitur ini.

#### Option 2: Kedua puluh lima aturan menjadi kode sekarang

**Pros**:

- Semua BR punya fungsi sejak hari pertama.

**Cons**:

- BR-012, BR-020, dan BR-025 adalah kebijakan, dan BR-022 butuh restore drill yang belum ada; test-nya hanya akan menguji fungsi kosong, dan evidence-nya menyesatkan fitur 65.

#### Option 3: Hanya aturan untuk entitas saat ini

Hanya aturan instance, runtime, dan path; lease, operasi, dan environment menunggu fiturnya.

**Pros**:

- Model lebih kecil.

**Cons**:

- BR-014, BR-015, dan BR-024 adalah logika murni yang paling mudah salah di fitur 31 dan 47; menundanya berarti menulisnya di tengah alur yang lebih besar.

### Keputusan 3: seberapa banyak Operation masuk domain

#### Option 1: Entitas dan tujuh state machine

Domain memegang entitas Operation dan transisi `queued` sampai `needs_recovery`; fitur 7 membangun journal, lock, dan idempotency di atasnya (basis: PRD bagian 18.2, REQ-054).

**Pros**:

- Ketiga state machine lain dan state operasi diuji dengan pola yang sama.
- Registry dan DTO memakai kode state yang sama sejak fitur 5.

**Cons**:

- Fitur 7 mungkin menemukan event yang belum ada, misalnya untuk resume bertahap, dan harus mengubah spec ini.

#### Option 2: Hanya ID dan enum status

**Pros**:

- Fitur 7 bebas mendesain seluruh entitas.

**Cons**:

- Aturan cancel di langkah irreversible dan `needs_recovery` tidak teruji sampai fitur 7, padahal fitur 5 sudah menyimpan status operasi.

### Keputusan 4: cara core menerima port platform

#### Option 1: Trait sync yang object safe, dibundel sebagai `Arc<dyn Trait>`

**Pros**:

- `stacklyd` merangkai satu struct `Platform`; fake dipasang dengan cara yang sama; tipe core tidak membawa parameter generic.
- Cocok dengan larangan tokio di `stackly-platform` (basis: aturan arah dependency spec 0001).

**Cons**:

- Dispatch dinamis, dan IO sync harus dipanggil lewat `spawn_blocking` dari kode async.

#### Option 2: Satu trait `Platform` dengan associated type, core generic

**Pros**:

- Dispatch statis tanpa biaya.

**Cons**:

- Setiap service dan use case core membawa `P: Platform`, dan pesan error compiler memanjang di seluruh daemon.

#### Option 3: Trait async

**Pros**:

- Use case async bisa langsung memanggil port.

**Cons**:

- `async fn` di trait tidak object safe, jadi memaksa generic; dan sebagian besar operasi filesystem tetap blocking di bawahnya.

### Keputusan 5: pembagian owned root di macOS

#### Option 1: Pembagian standar macOS

Data di Application Support, log di `~/Library/Logs`, cache di `~/Library/Caches`, plus runtime root pendek; setiap root adalah owned root (basis: PRD bagian 20.1).

**Pros**:

- Console.app membaca log di tempat yang diharapkan pengguna, dan alat pembersih cache tidak menyentuh data.

**Cons**:

- Cek containment harus menangani empat root, bukan satu.

#### Option 2: Satu root untuk semuanya

**Pros**:

- Cek containment paling sederhana.

**Cons**:

- Log tersembunyi dari Console.app, cache yang boleh dibuang tercampur dengan data yang tidak boleh hilang, dan path socket tetap terlalu panjang.

### Keputusan 6: lokasi runtime folder pendek

#### Option 1: Temp dir per user milik Darwin

`confstr(_CS_DARWIN_USER_TEMP_DIR)` ditambah `stackly`, sekitar 56 byte (basis: API `confstr` Apple, folder 0700 yang dibuat OS per user).

**Pros**:

- Pendek, privat sejak dibuat OS, tanpa nama user di path, dan tidak bisa diserobot user lain.

**Cons**:

- OS bisa mengosongkannya (saat boot, dan file lama secara berkala), jadi daemon harus memeriksa ulang socket.

#### Option 2: `/tmp/stackly-<uid>/`

**Pros**:

- Sangat pendek dan mudah ditebak oleh client mana pun.

**Cons**:

- `/tmp` dipakai bersama, jadi butuh cek kepemilikan ala tmux dan menolak start kalau folder sudah dibuat user lain; dan tetap dibersihkan berkala.

#### Option 3: Di dalam Application Support

**Pros**:

- Tidak pernah dibersihkan OS.

**Cons**:

- Panjangnya bergantung nama user, dan socket PostgreSQL di folder instance sudah melewati 104 byte.

### Keputusan 7: cara mengarahkan bin asli ke root alternatif

#### Option 1: Variabel `STACKLY_ROOT` yang dibaca bin sekali

**Pros**:

- Integration test fitur 6 bisa menjalankan `stacklyd` dan `stackly` asli ke test root; library tetap bebas environment (basis: AGENTS.md, "daemon dan CLI memeriksa env, path, dan config sekali saat start").

**Cons**:

- Variabel yang tertinggal di shell pengguna mengarahkan CLI ke root lain tanpa disadari.

#### Option 2: Flag `--root` saja

**Pros**:

- Selalu eksplisit di command line.

**Cons**:

- Daemon yang dijalankan launchd dan GUI sulit meneruskannya, dan setiap perintah test memanjang.

#### Option 3: Hanya lewat konstruktor

**Pros**:

- Tidak ada permukaan konfigurasi baru.

**Cons**:

- Tidak ada cara menjalankan biner daemon asli ke test root.

### Keputusan 8: layout folder runtime

#### Option 1: Path PRD dengan digest

`runtimes/<software>/<version>/<os>-<arch>/<artifact-sha256>/` (basis: PRD bagian 20.1, id artefak spec 0003 yang berakhiran `macos-aarch64`).

**Pros**:

- Nama folder sudah menjelaskan isinya tanpa registry, dan dua build dari versi yang sama tidak pernah berbagi folder.

**Cons**:

- Path lebih panjang dan bergantung pada validasi komponen versi.

#### Option 2: `runtimes/<runtime-id>/`

**Pros**:

- Pendek dan seragam dengan folder instance.

**Cons**:

- Folder tidak berarti apa apa tanpa registry, yang berlawanan dengan kebutuhan inventaris dan pemulihan.

### Keputusan 9: isi `instance-manifest.json`

#### Option 1: Identitas sampai kompatibilitas data

ID, nama, provider dan revision dengan digest, runtime, line yang menginisialisasi data, waktu dan operasi pembuat, versi Stackly, dan penanda retained.

**Pros**:

- Cukup untuk inventaris, untuk cek BR-009 saat adoption, dan untuk kompensasi berdasarkan operasi pembuat.

**Cons**:

- Sebagian data registry terduplikasi dan bisa berbeda.

#### Option 2: Ditambah endpoint dan config

**Pros**:

- Pemulihan lebih kaya.

**Cons**:

- File harus ditulis ulang di setiap perubahan endpoint atau config, dan peluang berbeda dengan registry naik.

#### Option 3: Minimal

**Pros**:

- Hampir tidak pernah ditulis ulang.

**Cons**:

- Adoption tidak bisa memeriksa kompatibilitas data tanpa registry.

### Keputusan 10: lokasi instance yang di remove (retained)

#### Option 1: Dipindah ke `retained/<id>/`

**Pros**:

- `instances/` hanya berisi instance hidup, dan nama instance bebas dipakai lagi.

**Cons**:

- Remove butuh rename atomik di volume yang sama, dan alat yang mencari folder harus memakai fungsi layout.

#### Option 2: Tetap di tempat, diberi penanda

**Pros**:

- Tidak ada pemindahan.

**Cons**:

- Setiap pemindaian harus menyaring, dan data hidup serta data yang di remove bercampur di satu folder.

### Keputusan 11: kunci pendek folder run per instance

#### Option 1: 8 hex terakhir UUID

**Pros**:

- Diambil dari bagian acak UUID v7, tanpa field baru.

**Cons**:

- Create harus membuat ulang ID saat bentrok, sebelum apa pun disimpan.

#### Option 2: Kunci acak terpisah yang disimpan

**Pros**:

- Tidak bergantung pada ID.

**Cons**:

- Satu field lagi yang harus sinkron di registry dan manifest.

#### Option 3: UUID penuh

**Pros**:

- Tidak ada konsep baru.

**Cons**:

- `<temp>/stackly/i/<36 karakter>/.s.PGSQL.54321` melewati 104 byte.

### Keputusan 12: fake filesystem di testkit

#### Option 1: Filesystem nyata yang dikurung, dengan fault injection

**Pros**:

- Perilaku symlink, canonicalize, dan identitas file sama dengan disk asli; path di luar test root membuat test gagal.

**Cons**:

- Lebih lambat dari fake di memori, dan test kepemilikan hanya di Unix.

#### Option 2: Filesystem di memori

**Pros**:

- Cepat dan sepenuhnya terkendali.

**Cons**:

- Symlink, canonicalize, dan identitas harus ditiru, dan tiruannya bisa berbeda dari APFS.

#### Option 3: Filesystem nyata tanpa kurungan

**Pros**:

- Kode paling sedikit.

**Cons**:

- Path yang salah diam diam keluar dari test root.

### Keputusan 13: bentuk kode state machine

#### Option 1: Enum state, enum event, dan satu tabel `apply`

**Pros**:

- Mudah disimpan, dimuat, dan dikirim lewat IPC, dan bisa diuji menyeluruh per pasangan (basis: state machine berbasis tabel).

**Cons**:

- Transisi yang salah baru gagal saat runtime, bukan saat compile.

#### Option 2: Typestate

**Pros**:

- Transisi yang salah tidak bisa dikompilasi.

**Cons**:

- Sulit disimpan dan dimuat dari registry, dan reconciliation tetap harus menangani state apa pun yang ditemukan.

#### Option 3: Field biasa dengan helper validasi

**Pros**:

- Kode paling sedikit.

**Cons**:

- Transisi tersirat dan tidak bisa diuji sebagai tabel.

### Keputusan 14: Windows di fitur ini

#### Option 1: Semua not supported

**Pros**:

- Jujur dan teruji; tidak ada kode Windows yang tidak bisa dibuktikan.

**Cons**:

- Fase Windows mulai dari nol untuk resolver direktori.

#### Option 2: Folder Windows sekarang

**Pros**:

- Satu bagian port Windows selesai lebih awal.

**Cons**:

- Tidak ada mesin Windows yang bisa membuktikannya, jadi kode itu hanya lulus compile.

### Keputusan 15: cakupan BR-009

#### Option 1: Semua provider, ketat

**Pros**:

- Downgrade dengan mengganti binary dicegah juga untuk MySQL, ClickHouse, dan yang lain; provider yang aman lintas line menyatakannya di manifest.

**Cons**:

- Provider harus menyatakan line yang kompatibel, atau rebind lintas line ditolak.

#### Option 2: PostgreSQL saja

**Pros**:

- Sama persis dengan kalimat PRD.

**Cons**:

- Provider lain tanpa pengaman, padahal REQ-043 dan REQ-055 meminta hal yang sama.

### Keputusan 16: bentuk Lease

#### Option 1: Satu Lease dengan jenis holder

Holder environment, manual, atau startup, sesuai `instance_demands` (basis: PRD bagian 13.2 dan 17).

**Pros**:

- Satu fungsi murni menilai dampak stop dari semua demand.

**Cons**:

- Lease manual dan startup punya aturan session sendiri di dalam satu tipe.

#### Option 2: Lease hanya untuk environment

**Pros**:

- Lease tetap berarti satu hal.

**Cons**:

- Keputusan stop harus melihat dua tempat, lease dan intent di instance.

## Rationale

**Batas dan port (keputusan 1, 4, 12, 14).** Kekuatan yang paling menentukan adalah aturan arah dependency spec 0001 dan kewajiban PRD bagian 15.2 bahwa semua mutasi penting bisa diuji dengan fake platform. Port yang hanya dibuat saat dipakai (keputusan 1) menghindari desain ganda di fitur 6, 10, dan 11, sementara daftar capability lengkap sudah cukup untuk AC-050. Trait sync yang dibundel (keputusan 4) adalah satu satunya bentuk yang object safe tanpa tokio di `stackly-platform`, dan biaya `spawn_blocking` jatuh di daemon yang memang async. Fake di atas disk asli (keputusan 12) dipilih karena bug yang paling mahal di fitur ini justru di perilaku filesystem: symlink, nama yang beda huruf di APFS, dan path yang bocor keluar root. Fake di memori akan menguji tiruan, bukan perilaku itu. Windows tidak mendapat kode yang tidak bisa dibuktikan (keputusan 14), karena REQ-050 meminta siap, bukan selesai.

**Layout (keputusan 5, 6, 7, 8, 10, 11).** Angka 104 byte adalah batas keras yang memaksa dua lapis root: data yang panjang dan tahan lama di `~/Library`, dan socket yang pendek dan sementara di temp dir user. Temp dir Darwin dipilih di atas `/tmp` karena privasi folder itu dijamin OS, bukan oleh pemeriksaan kita sendiri, dan risiko pembersihan berkala bisa ditangani daemon yang memeriksa ulang socket, sedangkan risiko squatting di `/tmp` adalah masalah keamanan. Kunci pendek dari ID (keputusan 11) menjaga path socket instance di bawah anggaran tanpa field baru. `STACKLY_ROOT` (keputusan 7) adalah satu satunya cara yang membuat biner daemon asli bisa diuji ke test root, dan aturan bahwa override tidak boleh menunjuk ke root data default menutup risiko terbesarnya. Path runtime dengan digest dan pemindahan ke `retained/` (keputusan 8 dan 10) mengikuti prinsip yang sama dengan manifest: folder harus menjelaskan dirinya sendiri.

**Domain (keputusan 2, 3, 9, 13, 15, 16).** State machine berbasis tabel (keputusan 13) dipilih karena state hidup di registry dan di IPC, dan reconciliation harus bisa memuat state apa pun; typestate akan memaksa konversi di setiap batas. Operation masuk domain (keputusan 3) supaya fitur 5 menyimpan kode state yang sudah teruji. BR yang bisa dicek menjadi kode (keputusan 2), karena evidence untuk fungsi kosong lebih berbahaya daripada tidak ada evidence. BR-009 diperluas ke semua provider (keputusan 15), karena kerusakan data akibat binary yang salah tidak khas PostgreSQL. Satu Lease untuk semua demand (keputusan 16) membuat BR-014, BR-015, dan BR-024 menjadi satu fungsi murni yang mudah diuji. Manifest berhenti di kompatibilitas data (keputusan 9), karena field yang sering berubah adalah sumber drift.

### Keputusan kecil yang diambil di spec ini

| Hal | Pilihan | Alasan | Runner up |
| --- | --- | --- | --- |
| Home directory macOS | `getpwuid_r(getuid())` | `HOME` bisa diubah environment atau test; database user adalah sumber kebenaran akun | API `sysdir` (`sysdir_start_search_path_enumeration`), lebih banyak kode untuk hasil yang sama |
| Path Library | subpath tetap `Library/Application Support`, `Library/Logs`, `Library/Caches` di bawah home | nama folder itu tetap di macOS; yang dilokalkan hanya tampilannya | `NSFileManager` lewat Objective C, API untuk app, bukan daemon Rust |
| Nama runtime root override | FNV-1a 64 bit, 32 bit bawah, 8 hex | sepuluh baris tanpa dependency, deterministik untuk daemon, CLI, dan GUI; bukan fungsi keamanan | crate `sha2` di platform |
| Newtype ID | makro `uuid_id!` | setiap ID tipe berbeda dengan pola `InstanceId` yang sudah ada, tanpa generic di signature | `Id<T>` dengan marker type |
| JSON manifest | derive serde di domain, `serde_json` di core | spec 0001 mengizinkan serde di domain; encoding dan IO tetap di luar domain | JSON manual di platform |
| Dependency platform | tanpa tokio dan libc, dijaga `check-deps` | crate trait tetap portabel; libc hanya di adapter | tanpa aturan, mengandalkan review |
| Penjaga path hardcode | pindaian `syn` di `check-deps` | menangkap pembacaan environment dan literal path tanpa salah tangkap di doc comment | regex per baris |
| Default umur readiness | 30 detik, `// provisional: F28` | tiga kali interval probe 10 detik | 60 detik (NFR-006 adalah batas waktu startup, bukan kesegaran) |
| Anggaran socket | 103, 15, 76 byte | `sun_path` macOS dan nama socket PostgreSQL | 104 byte (lupa NUL) |
| Folder run instance | tidak dibuat saat create, dipastikan sebelum setiap start | runtime root bisa dikosongkan saat reboot | dibuat saat create |
| Instance retained menahan runtime | ya, sampai purge | data yang ditahan mungkin perlu runtime itu untuk dipulihkan | tidak, runtime bebas dihapus setelah remove |
| Jejak aturan di error | `rule()` mengembalikan ID BR | diagnostics dan evidence terhubung ke PRD | ID hanya di teks pesan |
| Layout log | `logs/daemon/` dan `logs/instances/<id>/` | log daemon terpisah dari log instance | `logs/<id>/` sesuai PRD |
| Root yang terlalu terbuka | ditolak, tidak di `chmod` | Stackly tidak pernah membuat root seperti itu, jadi asalnya pihak lain | `chmod 0700` otomatis |
| Urutan create | manifest ditulis sebelum subfolder | folder sisa crash tetap bisa dikenali lewat `createdByOperation` | manifest terakhir |
| Override di dalam root default | ditolak | melindungi data asli dari test | diizinkan |
| Runtime folder testkit | `/tmp/skt-*` lewat `tempfile` | test root di bawah `TMPDIR` macOS terlalu panjang untuk anggaran, dan prefix `sk-` sudah dipakai spike spec 0003 yang pembersihannya menghapus `/tmp/sk-*` | di dalam test root |
| Batas `DisplayName` | 128 karakter | cukup untuk UI dan tetap terbatas | tanpa batas |
| Operasi terputus yang bisa dilanjutkan | kembali ke `queued`; cancel sesudahnya menjadi `cancel_requested` karena `started_at` terisi | engine fitur 7 mengambilnya lagi tanpa state baru, dan pekerjaan yang sudah jalan tetap dikompensasi | state `resuming` tersendiri |
| Lock satu daemon | `<data>/stacklyd.lock` | runtime root bisa dibersihkan OS saat daemon hidup, dan lock yang hilang membuka jalan bagi daemon kedua | lock di runtime root, di samping socket |
| Init yang gagal | `data_initialized_at` di instance, `start_begin` ditolak sampai terisi | `init_failed` dan crash sama sama berakhir `failed`, padahal hanya yang kedua boleh di start lagi | state lifecycle tambahan di luar daftar PRD |
| Health saat tidak `running` | hanya `unknown`, dan health ikut kedaluwarsa seperti readiness | health `healthy` untuk instance yang berhenti menyesatkan, dan PRD bagian 19.5 meminta kesegaran | health tanpa syarat |
| `STACKLY_ROOT` kosong | ditolak, bin keluar 1 | salah setel tidak boleh diam diam kembali ke data asli | diperlakukan seperti tidak di set |
| Nama file sementara | `.<nama>.tmp-<pid>-<counter>` | unik antar proses tanpa sumber acak tambahan | suffix acak dari `uuid` |
| Adapter macOS di test | `MacosPlatform::with_roots(RootSet)` | integration test tidak meninggalkan folder di temp dir user asli | `MacosPlatform::new` dengan override, yang membuat runtime root di temp dir asli |
| Kesamaan fake dan adapter | suite `fs_conformance` di testkit, dijalankan untuk keduanya | dua implementasi `Filesystem` hanya bisa dipercaya kalau perilakunya diuji sama | tanpa suite bersama |
| Pembacaan manifest | decode `schema` lebih dulu, `O_NOFOLLOW` plus `fstat` | schema baru terbaca sebagai `UnsupportedSchema`, bukan rusak; FIFO tidak pernah membuat inventaris macet | decode penuh langsung |
| Entri `.DS_Store` | dilewati tanpa entri | Finder membuatnya begitu folder dibuka, dan melaporkannya hanya menambah bising | dilaporkan sebagai entri asing |
| Nama enum capability platform | `PlatformCapability` | tidak tertukar dengan `Capability` provider di domain | nama sama di dua crate |

### Cross check

Draf pertama spec ini dibaca ulang oleh model lain (Sonnet 5.5) dengan akses baca saja. Ia memastikan hitungan anggaran socket (103, 15, 76) dan aturan arah dependency sudah benar, lalu menemukan celah keputusan yang akan memaksa builder menebak. Semua perbaikan yang direkomendasikan diterapkan atas persetujuan engineer:

- init yang gagal kini bisa dibedakan dari crash (`data_initialized_at`, `last_failure`, `data.initializedAt` di manifest);
- payload event dan daftar `Machine` ditulis, dan AC-2 dibatasi ke machine runtime dan lifecycle;
- `KeepRunning` membawa `release`, dan stop manual melepas semua lease manual;
- `resolve_port` menerima endpoint;
- precondition aggregate ditambahkan (desired awal, rebind, input revision dan migrasi, `RuntimeReferences`, revision yang tidak naik karena observasi);
- tabel operasi dilengkapi (cancel setelah resume, `queued` ke `failed`, `resume`, resource `manager`, `ProgressError`);
- aturan tepi di core ditambahkan (subfolder standar, `check_owned_path`, runtime root yang dibuat ulang);
- detail inventaris ditetapkan (file biasa, `.DS_Store`, urutan, schema lebih dulu);
- rumusan AC-5 dan `covers` untuk REQ-012, REQ-054, serta REQ-053 diperbaiki;
- nama dan pembersihan file sementara ditetapkan;
- `syn` dan aturan `STACKLY_ROOT` kosong ditetapkan;
- lock daemon dipindah ke root data, dan prefix testkit menjadi `skt-`;
- `MacosPlatform::with_roots` dan suite `fs_conformance` ditambahkan;
- kurungan fake filesystem lewat parent canonical ditetapkan;
- cek tumpuk root canonical ditambahkan;
- health kini terikat lifecycle dan kesegaran.

### Perubahan 2026-10-17

CI ditiadakan atas keputusan pemilik proyek, dan pengembangan fokus di Mac ini dulu. Tidak ada keputusan yang berubah. Yang diganti hanya tempat test berjalan: test core untuk AC-9 sampai AC-11, test AC-13, dan cek portabilitas kini dijalankan manual di Mac ini, bukan di job CI Linux dan macOS. Pro dan kontra keputusan 14 disesuaikan, karena tidak ada lagi test di Linux.

## References

**Project sources**:

- `AGENTS.md`: aturan arah dependency, `unsafe` hanya di adapter macOS, test root dan port per test, env dibaca sekali saat start.
- Spec [0001](../0001-stack-arsitektur/index.md): peta crate, aturan arah dependency, cek portabilitas, dan baris Fitur 4 di *Batas keputusan*.
- Spec [0002](../0002-coding-standards-tooling/index.md): guard evidence, crate `stackly-evidence` sebagai dependency dev domain, dan test yang membuktikan ID PRD.
- Spec [0003](../0003-sumber-packaging-runtime/index.md): pola versi exact, id artefak `<provider>-<version>-macos-aarch64`, dan folder socket pendek `/tmp/sk-<8 hex>` di spike.
- `docs/scope/01-foundation.md`: Done when fitur 4.
- PRD bagian 6, 9 (BR-001 sampai BR-025), 10 (VAL-001, VAL-002, VAL-007, VAL-008), 13.2, 15, 17, 18.2, 19.1 sampai 19.5, 20.1, dan 26; REQ-001, REQ-050, REQ-053; NFR-010; AC-050, AC-052, AC-053, AC-074.
- Kode scaffold: `crates/domain/src/instance_id.rs`, `crates/platform/src/lib.rs`, `crates/platform-macos/src/lib.rs`, `crates/testkit/src/test_root.rs`, `apps/daemon/src/main.rs`.
- Skill `rust-best-practices` (`.claude/skills/rust-best-practices/`): newtype dengan validasi, error dengan thiserror.

**Practices & standards**:

- Ports and adapters (Clean Architecture): domain di tengah, adapter OS di tepi.
- Membuat state yang tidak sah tidak bisa direpresentasikan lewat newtype yang divalidasi di konstruktor.
- State machine berbasis tabel dengan test menyeluruh per pasangan state dan event.
- Batas `sun_path` di `sockaddr_un` (104 byte di macOS, 108 di Linux).
- `confstr(_CS_DARWIN_USER_TEMP_DIR)` untuk temp dir per user di Darwin, dan `getpwuid_r` untuk home dari database user.
- Penulisan atomik: file sementara di folder yang sama, `fsync`, rename, lalu `fsync` folder.
- Identitas file (`st_dev`, `st_ino`) untuk mendeteksi alias path.
- Pemeriksaan kepemilikan folder socket ala tmux (pemilik, mode, bukan symlink).
- Hash FNV-1a 64 bit.
