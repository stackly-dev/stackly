# 0006. Rationale: protokol IPC dan daemon

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: fitur 6 menggabungkan beberapa keputusan yang biasanya berdiri sendiri: kontrak wire, envelope error, versi, event, batas, transport socket, siklus hidup daemon, log, dan toolchain fuzzing. Spec ini tetap satu karena semuanya bertemu di satu jalur yang harus jalan utuh (client tersambung dengan aman ke satu daemon lalu melihat statusnya) dan karena spec 0005, 0009, dan 0011 ditulis paralel sambil mengasumsikan envelope dan kategorinya. Memecahnya menjadi beberapa spec akan membuat spec paralel itu bergantung pada beberapa dokumen yang belum ada. Bagian yang nanti tumbuh besar (misalnya transport Windows) masuk lewat update atau spec anak.

Stackly punya satu daemon per user yang menjadi satu satunya penulis registry dan pengendali proses; CLI dan GUI hanya client (BR-011, REQ-001). Spec 0001 sudah mengunci encoding dan framing (JSON camelCase, panjang 4 byte big endian, batas 1 MiB untuk frame kontrol dan 64 KiB untuk batch log, enum berdata `type`/`content`, typeshare dan schemars, socket Unix privat, tanpa TCP), tetapi sengaja menyerahkan ke fitur 6 bentuk pesan, handshake, envelope error, pemetaan tipe asing di typeshare, integer 64 bit, enum yang tidak dikenal, adapter IO untuk framing, dan sink log daemon. Spec 0004 menyediakan layout direktori dengan `daemon_socket()` di runtime root pendek 0700 dan `daemon_lock()` di root data, plus capability `ipc_endpoint` yang masih not supported.

PRD bagian 18 meminta handshake yang membawa major dan minor, versi aplikasi, capability, jenis client, identitas instance server, schema revision, dan cursor event; major yang tidak cocok ditolak dan minor harus additive. VAL-020 meminta setiap error punya kode stabil, pesan aman, konteks field dan resource, retryability, dan hint. VAL-017 dan NFR-004 meminta batas ukuran, kedalaman, method tidak dikenal, operasi bersamaan, dan subscription, dengan semua antrian berbatas dan kehilangan yang terlihat. AC-047, AC-070, dan AC-079 meminta peer dari user lain dan endpoint palsu ditolak sebelum efek samping atau sebelum credential dikirim. AC-071 meminta client dengan minor lama tetap jalan dan cursor yang kedaluwarsa memicu snapshot. PRD bagian 12 mengusulkan exit code CLI 2 sampai 9 yang harus diturunkan dari kategori error (fitur 8). PRD bagian 23 menempatkan framing IPC di daftar fuzzing prioritas.

Tiga batas praktis membentuk ruang keputusan. Pertama, typeshare 1.13.4 menolak `u64` dan `i64` biasa, menulis `BTreeMap` sebagai tipe Swift yang tidak ada, dan tidak memberi jalur `unknown` untuk tag enum yang tidak dikenal (lihat *Bukti* di bawah). Kedua, toolchain proyek dipin di Rust stable 1.99.1, sedangkan cargo-fuzz butuh nightly; pilihan fuzzing mengikat fitur 9, 18, 20, 54, dan 63. Ketiga, `stackly-platform` dilarang memakai tokio atau libc, dan `unsafe` hanya boleh di `stackly-platform-macos`, padahal transport socket butuh IO async dan kredensial peer. Kalau semua ini tidak diputuskan sekarang, spec 0005, 0007 sampai 0011, dan 0013 masing masing akan menebak bentuk error, revision, dan event, dan perbedaannya baru ketahuan saat GUI dan daemon dari build berbeda bicara satu sama lain (RISK-09).

## Options considered

Setiap keputusan di bawah ditanyakan ke engineer satu per satu dengan satu pilihan yang direkomendasikan; pilihan pertama di setiap keputusan adalah yang dipilih.

### Keputusan 1: penomoran versi protokol

#### Option 1: Mulai 1.0 dengan aturan ketat

Protokol nyata pertama lahir sebagai 1.0. Setiap perubahan yang merusak menaikkan major, termasuk sebelum rilis stabil; minor hanya untuk tambahan yang aman.

**Pros**:

- Handshake jujur sejak hari pertama: hanya major yang dibandingkan, dan semua perubahan merusak tertangkap.
- AC-071 (client minor lama tetap jalan) bisa dibuktikan sekarang.

**Cons**:

- Major bisa naik beberapa kali sebelum aplikasi stabil.

#### Option 2: Tetap 0.x, minor harus sama

Major 0 selama pengembangan, dan minor diperlakukan sebagai pemutus sehingga handshake menuntut minor sama.

**Pros**:

- Jelas memberi tanda bahwa protokol belum stabil.

**Cons**:

- Kompatibilitas minor tidak bisa dibuktikan sampai naik ke 1.0, dan setiap tambahan kecil memutus CLI dan GUI dari build lain.

#### Option 3: 0.x dengan aturan seperti 1.x

Angka 0 hanya label; aturan major dan minor sama dengan Option 1.

**Pros**:

- Tanda belum stabil tetap ada tanpa kehilangan kompatibilitas minor.

**Cons**:

- Naik ke 1.0 saat stabil menjadi pemutusan buatan yang menolak semua client lama tanpa ada perubahan wire.

### Keputusan 2: skema penamaan kode error

#### Option 1: `area.alasan` snake_case

Contoh `instance.name_taken`, `ipc.frame_too_large`. String di wire, katalog di `stackly-protocol`.

**Pros**:

- Spec paralel tidak bentrok karena setiap area punya pemilik; terbaca di log; selaras dengan kode state snake_case spec 0004.

**Cons**:

- Kode lebih panjang dari angka, dan katalog harus dijaga test.

#### Option 2: snake_case datar

Contoh `name_taken`.

**Pros**:

- Pendek.

**Cons**:

- Rawan bentrok antar fitur, dan pemilik kode tidak terlihat.

#### Option 3: Kode angka

Contoh `E1203`.

**Pros**:

- Ringkas dan netral bahasa.

**Cons**:

- Tidak terbaca tanpa tabel, dan rentang nomor mudah tabrakan antar sesi paralel.

#### Option 4: SCREAMING_SNAKE

Contoh `INSTANCE_NAME_TAKEN`.

**Pros**:

- Dikenal dari gRPC dan AWS.

**Cons**:

- Tidak cocok dengan nilai snake_case lain di wire.

### Keputusan 3: set kategori error

#### Option 1: Sepuluh kategori selaras PRD bagian 12

`invalid_input`, `not_found`, `conflict`, `unauthorized`, `unsupported`, `unavailable`, `failed`, `timeout`, `cancelled`, `internal`.

**Pros**:

- Delapan kategori langsung cocok dengan exit code PRD; GUI bisa membedakan resource hilang dari input salah, dan bug dari kegagalan biasa.

**Cons**:

- Dua kategori butuh exit code yang belum ada di PRD, dan fitur 8 harus memutuskannya.

#### Option 2: Delapan kategori persis PRD

**Pros**:

- Pemetaan exit code satu banding satu tanpa keputusan tambahan.

**Cons**:

- `not_found` dan bug harus disamarkan menjadi `invalid_input` dan `failed`.

#### Option 3: Enam belas kategori gaya gRPC

**Pros**:

- Lengkap dan dikenal luas.

**Cons**:

- Banyak yang tidak terpakai di app lokal, dan pemetaan ke exit code PRD jadi kabur.

### Keputusan 4: autostart daemon dari CLI sebelum fitur 15

#### Option 1: Tidak pernah

CLI melaporkan daemon tidak jalan dengan kategori `unavailable` dan petunjuk.

**Pros**:

- Tidak ada daemon yang lahir dari environment shell, tidak ada jalur kode yang dibuang di fitur 15, dan exit 6 mudah didiagnosis.

**Cons**:

- Sebelum fitur 15, developer menjalankan `stacklyd` sendiri.

#### Option 2: Hanya mode development

Autostart kalau `STACKLY_DEV_AUTOSTART=1`.

**Pros**:

- Praktis saat mencoba.

**Cons**:

- Daemon mewarisi environment shell (misalnya `STACKLY_ROOT`), dan kodenya dibuang di fitur 15.

#### Option 3: Selalu autostart

**Pros**:

- Paling nyaman.

**Cons**:

- Bersaing dengan SMAppService di fitur 15 dan menyembunyikan penyebab daemon tidak jalan.

### Keputusan 5: daya tahan cursor event

#### Option 1: Di memori, per run daemon

Cursor berisi `serverInstanceId` dan sequence; ring buffer event terbaru untuk reconnect singkat; gap untuk cursor dari run lain atau yang tergeser.

**Pros**:

- Tidak ada tulisan disk per event; registry tidak tumbuh; mudah diuji.

**Cons**:

- Setiap restart daemon memaksa snapshot penuh.

#### Option 2: Disimpan di registry

**Pros**:

- Cursor bertahan lintas restart.

**Cons**:

- Setiap event menjadi tulisan SQLite, registry tumbuh (NFR-011), dan fitur 5 masuk jalur panas.

#### Option 3: Tanpa cursor

**Pros**:

- Paling sederhana.

**Cons**:

- Melanggar Done when (snapshot plus cursor) dan membebani NFR-002 saat reconnect sering.

### Keputusan 6: kebijakan overflow subscriber

#### Option 1: Buang lalu kirim penanda gap

**Pros**:

- Producer tidak pernah tertahan; kehilangan eksplisit dan terhitung; cocok dengan perilaku lagged `tokio::sync::broadcast`.

**Cons**:

- Client lambat harus mengambil snapshot ulang.

#### Option 2: Putuskan subscriber lambat

**Pros**:

- Server lebih sederhana.

**Cons**:

- Request lain di koneksi yang sama ikut gagal, dan client lambat terus putus sambung.

#### Option 3: Tahan producer

**Pros**:

- Tidak ada event hilang.

**Cons**:

- Satu GUI yang macet bisa menahan daemon (melanggar NFR-004).

### Keputusan 7: angka batas

#### Option 1: Set angka usulan

Buffer event 1024, ring 4096, 64 koneksi, 32 request per koneksi, 8 subscription, kedalaman 32, handshake 5 detik, frame terhenti 30 detik.

**Pros**:

- Cukup untuk GUI dengan banyak layar ditambah beberapa CLI; memori tetap kecil (ring 4096 event kira kira 2 MiB).

**Cons**:

- Angka belum diukur di mesin baseline.

#### Option 2: Lebih ketat

**Pros**:

- Memori lebih kecil.

**Cons**:

- GUI dan CLI paralel lebih cepat kena batas.

#### Option 3: Lebih longgar

**Pros**:

- Jarang kena batas.

**Cons**:

- Memori idle lebih besar terhadap anggaran NFR-003.

### Keputusan 8: log file daemon

#### Option 1: 5 kali 20 MiB, penulis sendiri

**Pros**:

- Rotasi berdasarkan ukuran sesuai PRD bagian 35.2, penulisan tidak pernah menahan daemon, tanpa crate baru, dan bisa dipakai ulang fitur 27.

**Cons**:

- Kode rotasi dan kasus disk penuh harus dirawat sendiri.

#### Option 2: 5 kali 20 MiB dengan crate `file-rotate`

**Pros**:

- Lebih sedikit kode rotasi.

**Cons**:

- Dependency tambahan, dan penulisan yang tidak menahan daemon tetap ditulis sendiri.

#### Option 3: Rotasi harian `tracing-appender`

**Pros**:

- Crate resmi ekosistem tracing.

**Cons**:

- Hanya berdasarkan waktu, jadi ukuran sehari tidak terbatas.

### Keputusan 9: integer 64 bit

#### Option 1: Newtype transparan yang dipetakan

**Pros**:

- Tetap angka JSON; tipe Rust jelas (`Revision`, `Sequence`, `Count`); typeshare menerimanya.

**Cons**:

- Batas 2^53 − 1 harus dijaga decode, dan setiap newtype baru butuh baris mapping.

#### Option 2: Kirim sebagai string

**Pros**:

- Aman untuk semua bahasa tanpa batas.

**Cons**:

- Tidak bisa dibandingkan sebagai angka di JSON, dan Swift mengubahnya sendiri.

#### Option 3: Batasi ke `u32`

**Pros**:

- Paling sederhana untuk typeshare.

**Cons**:

- Revision dan sequence bisa habis pada daemon yang hidup lama.

### Keputusan 10: UUID dan waktu di Swift

#### Option 1: `UUID` dan `Date`, presisi milidetik

**Pros**:

- Tipe Swift bermakna; nilai kembali sama setelah encode dan decode; satu strategi tanggal di `StacklyJSON`.

**Cons**:

- Presisi di bawah milidetik hilang di wire.

#### Option 2: `UUID` dan `Date`, presisi nanodetik

**Pros**:

- Tidak ada pemotongan di Rust.

**Cons**:

- `Date` Swift tidak bisa memegang nanodetik, jadi fixture tidak bisa dibandingkan persis.

#### Option 3: Semua tetap `String`

**Pros**:

- Tidak ada jebakan decoder.

**Cons**:

- Setiap pemakai parse sendiri, dan tipe kehilangan makna.

### Keputusan 11: nilai enum yang tidak dikenal

#### Option 1: Enum terbuka dengan `unknown`

**Pros**:

- Nilai baru di minor tidak menggagalkan decode; event baru dilewati; enum berdata tetap sederhana.

**Cons**:

- Varian `Unknown` di Rust, file Swift tambahan dari xtask, dan UI wajib menangani `unknown`.

#### Option 2: Semua enum tertutup

**Pros**:

- Tanpa codegen tambahan.

**Cons**:

- Hampir setiap fitur menaikkan major.

#### Option 3: Daemon menurunkan nilai per minor client

**Pros**:

- Client tidak perlu toleran.

**Cons**:

- Setiap varian baru butuh aturan downgrade di daemon.

### Keputusan 12: bentuk pesan di wire

#### Option 1: Seragam `type`/`content`

**Pros**:

- Satu aturan enum spec 0001 tanpa pengecualian; mudah untuk generator C# nanti.

**Cons**:

- Sedikit lebih bersarang dan kurang mirip contoh PRD.

#### Option 2: `method`/`params` seperti PRD

**Pros**:

- Lebih mudah dibaca di log; typeshare mendukung nama tag kustom.

**Cons**:

- Pengecualian aturan enum spec 0001.

#### Option 3: JSON-RPC 2.0

**Pros**:

- Format standar yang dikenal.

**Cons**:

- `params` dan `result` tidak bertipe per method di typeshare, dan bentuk error nya bentrok dengan VAL-020.

### Keputusan 13: letak transport socket

#### Option 1: Crate baru `stackly-ipc`

**Pros**:

- Satu implementasi untuk daemon, CLI, dan test; tanpa `unsafe` (tokio `peer_cred`, std `File::try_lock`); codec tetap murni di `stackly-protocol`.

**Cons**:

- Satu crate lagi di peta spec 0001 dan aturan `check-deps` baru.

#### Option 2: Port `IpcEndpoint` di `stackly-platform`

**Pros**:

- Pola port yang sama dengan spec 0004.

**Cons**:

- Tokio dilarang di `stackly-platform`, jadi transport async tetap ditulis di tempat lain, dan `getpeereid` lewat `libc` menambah `unsafe`.

#### Option 3: Langsung di `stacklyd` dan CLI

**Pros**:

- Struktur paling sedikit.

**Cons**:

- Kode framing async dua kali, dan test daemon butuh client ketiga.

### Keputusan 14: toolchain fuzzing

#### Option 1: cargo-fuzz dengan nightly yang dipin

**Pros**:

- libFuzzer dengan panduan coverage, cepat untuk target kecil; nightly terkurung di `fuzz/`; regresi tetap jalan di stable.

**Cons**:

- Toolchain kedua yang harus dipasang dan dinaikkan dengan sengaja.

#### Option 2: cargo-afl di stable

**Pros**:

- Tanpa nightly.

**Cons**:

- Butuh AFL++ terpasang dan setelan shared memory macOS dengan sudo.

#### Option 3: bolero

**Pros**:

- Harness yang sama jalan sebagai property test di stable.

**Cons**:

- Crate tambahan, dan engine libFuzzer nya tetap butuh nightly.

#### Option 4: Property test saja

**Pros**:

- Stable penuh.

**Cons**:

- Bukan fuzzing dengan panduan coverage; tidak memenuhi Done when dan PRD bagian 23.

### Keputusan 15: schema revision di handshake

#### Option 1: Digest kontrak sebagai warning

**Pros**:

- Kelalaian menaikkan minor langsung terlihat tanpa memutus build yang kompatibel.

**Cons**:

- Dua file hasil generate lagi yang harus dijaga `--check`.

#### Option 2: Digest yang menolak

**Pros**:

- Paling ketat.

**Cons**:

- CLI dan GUI dari build berbeda tidak bisa bicara walau kompatibel.

#### Option 3: Tanpa digest

**Pros**:

- Paling sederhana.

**Cons**:

- Drift baru ketahuan saat decode gagal.

### Keputusan 16: verifikasi endpoint oleh client

#### Option 1: Cek path lalu cek peer

**Pros**:

- Dua lapis; pesan error jelas untuk socket yang salah pemilik; tidak ada byte yang terkirim ke socket palsu.

**Cons**:

- Ada jendela kecil antara cek path dan connect, yang ditutup oleh cek peer sesudahnya.

#### Option 2: Cek peer saja

**Pros**:

- Cukup sebagai jaminan identitas.

**Cons**:

- Tanpa pertahanan berlapis, dan diagnosis kurang jelas.

#### Option 3: Ditambah tanda tangan kode

**Pros**:

- Melindungi dari proses palsu dengan uid yang sama.

**Cons**:

- Di luar BR-012 dan butuh identity signing dari fitur 15.

### Keputusan 17: penolakan peer dan major berbeda

#### Option 1: Kirim error lalu tutup

**Pros**:

- Client tahu sebabnya; sesuai AC-047.

**Cons**:

- Memberi tahu proses lain bahwa ada daemon Stackly, padahal proses itu sudah melewati folder 0700.

#### Option 2: Tutup tanpa jawaban

**Pros**:

- Tidak memberi informasi apa pun.

**Cons**:

- Client tidak bisa membedakan penolakan dari crash, dan AC-047 meminta error aman.

### Keputusan 18: daemon kedua

#### Option 1: Exit 3 sambil menyebut pemilik

**Pros**:

- Fitur 15 dan doctor bisa membedakan "sudah ada daemon" dari "daemon rusak".

**Cons**:

- Lock file harus membawa isi yang ditulis dengan hati hati di handle yang memegang lock.

#### Option 2: Exit 1 untuk semua kegagalan

**Pros**:

- Sederhana.

**Cons**:

- Penyebab tidak bisa dibedakan.

### Keputusan 19: path field di error params

#### Option 1: `serde_path_to_error`

**Pros**:

- Path field yang tepat dari decode biasa, satu crate kecil dari penulis serde_json.

**Cons**:

- Satu dependency lagi di `stackly-protocol`.

#### Option 2: Pesan serde_json saja

**Pros**:

- Tanpa crate baru.

**Cons**:

- Hanya baris dan kolom, jadi GUI tidak bisa menyorot field.

#### Option 3: Validasi manual per method

**Pros**:

- Path selalu tepat.

**Cons**:

- Kode berulang di setiap method fitur lain.

### Keputusan 20: frame yang tidak valid

#### Option 1: Framing rusak menutup, payload rusak dijawab

**Pros**:

- Sinkronisasi hanya diputus saat memang hilang; bug kecil client tidak membuat GUI putus sambung.

**Cons**:

- Client yang terus mengirim payload rusak tetap tersambung (dibatasi oleh batas request berjalan).

#### Option 2: Error apa pun menutup

**Pros**:

- Lebih ketat.

**Cons**:

- Satu bug kecil di client membuat koneksi terus putus.

### Keputusan 21: mendeteksi socket yang hilang

#### Option 1: Cek berkala setiap 10 detik

**Pros**:

- Sederhana, tanpa `unsafe`, jendela tanpa socket paling lama 10 detik.

**Cons**:

- Ada jeda sampai 10 detik.

#### Option 2: Pantau lewat kqueue

**Pros**:

- Reaksi langsung.

**Cons**:

- Butuh `unsafe` atau crate `notify`, dan tetap perlu cek berkala sebagai cadangan.

#### Option 3: Cek hanya saat start

**Pros**:

- Paling sederhana.

**Cons**:

- CLI kehilangan socket setelah daemon lama hidup, risiko yang sudah dicatat spec 0004.

## Rationale

**Versi, kategori, dan kode (keputusan 1 sampai 3).** Kekuatan terbesar di sini adalah bahwa CLI dan GUI dari build yang berbeda pasti bertemu daemon yang berbeda versinya (CLI lewat symlink yang tidak ikut terupdate, GUI yang dibuka saat helper sudah baru). Handshake hanya bisa melindungi kalau setiap perubahan merusak menaikkan sesuatu yang dibandingkan handshake, jadi aturan major ketat sejak 1.0 adalah satu satunya pilihan yang jujur dan tetap membiarkan AC-071 dibuktikan sekarang. Kategori dipilih sebagai "apa yang sebaiknya dilakukan client", sehingga exit code, tombol GUI, dan keputusan retry diturunkan dari satu tempat; `not_found` dan `internal` dipertahankan karena keduanya meminta reaksi yang berbeda dari `invalid_input` dan `failed`. Kode `area.alasan` dipilih terutama karena spec ditulis paralel: area memberi setiap spec ruang nama sendiri, dan katalog dengan test membuat bentrok ketahuan saat build, bukan saat runtime.

**Event (keputusan 5 sampai 7).** Event di Stackly adalah petunjuk perubahan, bukan sumber kebenaran: snapshot dengan revision dari registry adalah kebenarannya. Karena itu cursor tidak perlu bertahan lintas restart, dan menyimpannya di SQLite hanya akan memindahkan beban ke jalur tulis registry. Buang dan tandai gap adalah satu satunya kebijakan overflow yang memenuhi NFR-004 (tidak ada antrian yang tumbuh dan tidak ada producer yang tertahan) sambil tetap jujur ke client; `tokio::sync::broadcast` sudah punya perilaku lagged yang persis itu, jadi tidak perlu antrian per subscriber buatan sendiri. Angka batas dipilih untuk skala PRD (100 instance terdaftar, 10 berjalan, satu GUI dan beberapa CLI) dan sengaja dibuat konstanta yang dikirim di handshake, supaya fitur 64 bisa menyetelnya tanpa mengubah wire.

**Wire dan Swift (keputusan 9 sampai 12).** Uji langsung typeshare 1.13.4 menunjukkan bahwa `u64` hanya bisa lewat newtype yang dipetakan, `BTreeMap` harus dihindari, dan enum berdata tidak punya jalur `unknown`. Dari situ aturan wire dibentuk: newtype untuk setiap `u64`, `Vec` berisi pasangan untuk peta, enum terbuka hanya untuk enum tanpa data (yang bisa diberi `init(from:)` lewat extension, terbukti dengan `swiftc`), dan event baru dilewati lewat `EventProbe` karena event adalah satu satunya enum berdata yang datang tanpa diminta. Milidetik dipilih karena urutan memakai sequence, bukan waktu, dan karena `Date` Swift tidak bisa memegang nanodetik; dengan begitu fixture bisa dibandingkan persis. Bentuk `type`/`content` dipertahankan karena satu aturan tanpa pengecualian lebih murah untuk tiga bahasa daripada sedikit kenyamanan membaca log.

**Transport dan keamanan (keputusan 13, 16 sampai 18, 20, 21).** Tokio sudah menyediakan `peer_cred()` di macOS dan std sudah punya `File::try_lock()`, jadi transport bisa dibuat tanpa `unsafe` sama sekali; menaruhnya di port `stackly-platform` justru akan menambah `unsafe` dan tetap butuh pembungkus async di tempat lain. Crate kecil `stackly-ipc` yang hanya bergantung pada protokol membuat daemon, CLI, dan test memakai satu implementasi, dan membuat pemeriksaan peer tidak mungkin terlewat karena listener sendiri yang menolak. Verifikasi dua lapis di client mengikuti PRD bagian 18.1 yang meminta permission file dan pemeriksaan peer diuji, bukan hanya mengandalkan nama socket. Lock diambil setelah `prepare_layout` karena fungsi itu sudah tahan terhadap start bersamaan dan tidak mengubah apa pun pada layout yang ada, dan karena lock file butuh root data yang sudah ada.

**Log (keputusan 8).** PRD bagian 35.2 dan Done when fitur 27 memakai rotasi berdasarkan ukuran, dan NFR-004 meminta kehilangan yang terlihat. Tidak ada crate yang memberi keduanya sekaligus tanpa tetap menulis antrian sendiri, jadi penulis kecil buatan sendiri lebih murah dirawat daripada menggabungkan dua crate.

**Fuzzing (keputusan 14).** cargo-fuzz adalah alat yang paling umum di ekosistem Rust untuk fuzzing parser, dan memberi panduan coverage yang dibutuhkan PRD bagian 23. Nightly dikurung di `fuzz/` dengan pin bertanggal di xtask, sehingga toolchain stable proyek tidak berubah dan input yang pernah crash tetap diuji di stable lewat test regresi. Aturan yang sama dipakai fitur 9, 18, 20, 54, dan 63 supaya hanya ada satu cara menjalankan fuzz di repo ini.

**Digest kontrak (keputusan 15).** PRD meminta schema revision di handshake. Digest atas schema dan katalog adalah cara paling murah untuk mendeteksi kelalaian menaikkan minor, dan menjadikannya warning (bukan penolakan) menjaga agar build yang sebenarnya kompatibel tetap bisa bicara.

### Keputusan kecil

| Keputusan | Pilihan | Alasan | Runner up |
| --- | --- | --- | --- |
| Format `requestId` | 1 sampai 64 byte `[A-Za-z0-9._:-]`, dipilih client; Rust memakai UUID v7 | client lain (test, C#) bebas memilih format, tetapi log tetap aman dibaca | wajib UUID |
| Versi di setiap request | tidak; versi hanya di handshake | versi berlaku per koneksi | `protocol` di setiap request seperti contoh PRD |
| Frame dari client | hanya `Request` | client tidak perlu jenis frame lain; cancel dan unsubscribe adalah method | enum `ClientFrame` |
| Decode request | bertahap di `stackly-protocol`: kepala dengan `content` mentah, lalu params per method, lalu `Validate` | method tidak dikenal dan params salah bisa dibedakan, path field tetap benar walau `content` datang sebelum `type`, dan logikanya murni serta bisa di fuzz | `#[serde(other)]` pada `Call` |
| Teks error params | set kode field tetap dan teks tetap, tanpa teks serde | teks serde bisa memuat nilai input, termasuk secret | meneruskan pesan serde |
| Field opsional | `#[serde(default)]`, ditulis `null` | field yang hilang dari pengirim lama terbaca sebagai kosong | `skip_serializing_if` |
| Pesan error | bahasa Inggris, huruf kecil, tanpa titik, maksimal 200 karakter | sama dengan gaya error Rust di AGENTS.md; GUI memakai kode untuk teks lokal | pesan berbahasa Indonesia |
| `retryable` | selalu dari katalog | satu kode selalu satu perilaku retry | diisi di tempat error dibuat |
| `hint` | string opsional | cukup untuk CLI sekarang; kode hint bisa ditambah nanti sebagai minor | kode hint terstruktur |
| `details` | `Vec<ErrorDetail>` | deterministik dan aman untuk typeshare | `HashMap` |
| Urutan start | `prepare_layout`, lalu lock, lalu log file, lalu socket dengan status `starting`, lalu slot start, lalu `ready` | `prepare_layout` sudah tahan race dan tidak mengubah layout yang ada; log file milik pemegang lock; client bisa melihat `starting` selama migrasi dan rekonsiliasi nanti | lock lebih dulu dengan fungsi baru untuk membuat root data; socket baru dibuka setelah siap |
| Mode socket | bind di `<path>.tmp`, `chmod` 0600, lalu `rename` | `bind` di std dan tokio sekaligus `listen`, jadi `chmod` sebelum `listen` tidak mungkin tanpa `unsafe` atau crate baru; rename membuat path akhir tidak pernah ada dengan mode lain | `socket2` untuk bind, `chmod`, lalu `listen` terpisah |
| Urutan berhenti | unlink dan buang listener, publish, drain, flush, tutup | client baru langsung tahu daemon berhenti, dan `daemon.shutting_down` sempat terkirim sebelum koneksi ditutup | drain dulu baru unlink |
| Test di dalam proses | `stacklyd` library plus bin, trait `Handlers`, `Daemon::start` | handler lambat, ribuan event, dan drain tidak bisa dipicu lewat binary yang semua method nya instan | method khusus test di protokol |
| Test root lama | `stacklyd --print-roots` | test home directory tanpa override tidak boleh menjalankan daemon terhadap Application Support asli | memindahkan test ke `platform-macos` dengan mengubah `HOME` di proses (state global) |
| Catatan versi | `protocols/contract-versions.json` dengan `--amend` | kelalaian menaikkan versi tertangkap saat codegen, bukan hanya sebagai warning saat berjalan | hanya warning `contractDrift` |
| Isi lock file | JSON pid, `serverInstanceId`, `startedAt`, versi, ditulis lewat handle yang memegang lock | daemon kedua bisa menyebut pemilik walau socket sudah hilang | lock file kosong dan pemilik ditanya lewat socket |
| Socket lama | dihapus hanya kalau berjenis socket dan lock sudah dipegang | tidak pernah menghapus file milik pengguna | selalu menolak start |
| Event bus | satu `Mutex` untuk sequence, ring, dan broadcast | urutan ring dan broadcast selalu sama, dan replay tanpa celah mudah dibuktikan | ring tanpa lock dengan atomik |
| Filter subscribe | jenis lengkap atau awalan area, plus satu resource | cukup untuk `--wait` di fitur 8 dan layar per resource | ekspresi filter bebas |
| Waktu habis | handshake 5 detik, frame terhenti 30 detik, tulis 30 detik, berhenti 5 detik | cukup longgar untuk mesin yang sibuk, cukup ketat untuk melepas sumber daya | satu batas waktu untuk semua |
| `STACKLY_LOG` | filter tracing, default `info`, ke stderr dan file | debugging tanpa build ulang | `RUST_LOG` |
| stderr | selalu ditulis, oleh thread penulis yang sama dengan file | developer melihat log di terminal; pipe stderr yang tersumbat tidak pernah menahan worker tokio | hanya kalau stderr adalah TTY; tulis langsung dari subscriber |
| Client Rust untuk CLI | async dengan tokio runtime `current_thread` | satu implementasi `stackly-ipc` dipakai daemon, CLI, dan test | client sync terpisah |
| Bin di test | `CARGO_BIN_EXE_stacklyd` untuk test daemon; `cargo_bin` dengan folder target terpisah hanya untuk test CLI | test CLI butuh `stacklyd` dari paket lain tanpa berebut lock build | wajib `cargo build` manual sebelum test |
| Runtime root di test | `SpawnedDaemon` menghapus runtime root milik run itu setelah berhenti | tidak meninggalkan folder di temp dir user; folder nya diturunkan dari root test yang unik | variabel env khusus test untuk runtime root |
| Bukti dua user | uid harapan yang disuntik ke `stackly-ipc`, plus evidence manual dengan akun kedua | jalur kode asli teruji tanpa root; skenario nyata tetap dicatat jujur | flag test di binary daemon |
| Tanpa TCP | test `lsof` plus pindaian `check-deps` | bukti saat berjalan dan pencegahan saat build | hanya review |
| Kinerja | p95 500 panggilan `get_info` paling lama 20 ms | jauh di bawah 250 ms NFR-001, jadi transport tidak pernah menjadi penyebab | tanpa test kinerja sampai fitur 64 |
| Durasi fuzz untuk Done when | 5 menit per target | cukup untuk menemukan bug dangkal di codec kecil tanpa menahan pengembangan | 60 detik |
| `max_len` fuzz | 65 536 byte | input kecil lebih cepat; jalur `TooLarge` tetap teruji lewat header | 1 MiB plus 4 |
| Swift di fitur ini | tipe hasil generate, `StacklyJSON`, dan test fixture; transport di fitur 13 | kontrak bisa diuji sekarang tanpa mendahului desain fitur 13 | transport minimal sekarang |

### Bukti: uji typeshare 1.13.4 dan Swift (2026-10-18)

Diuji di Mac ini dengan `typeshare-cli` 1.13.4 dan `swiftc` dari Xcode 27, di folder sementara.

| Kasus | Hasil |
| --- | --- |
| Field `u64` biasa | ditolak: `Unsupported type: "u64"` |
| `u64` dan `i64` lewat `[swift.type_mappings]` | tetap ditolak; mapping hanya berlaku untuk nama tipe kustom |
| Newtype `#[serde(transparent)] struct Sequence(u64)` tanpa `#[typeshare]`, dipetakan `Sequence = "UInt64"` | diterima, menjadi `UInt64` |
| `#[typeshare(serialized_as = "Int64")]` pada field `i64` | diterima, tetapi string diperlakukan sebagai nama tipe kustom (cara kerja tidak resmi) |
| `Uuid` dan `Timestamp` dipetakan ke `UUID` dan `Date` | diterima |
| `BTreeMap<String, String>` | ditulis `BTreeMap<String, String>`, tipe yang tidak ada di Swift |
| `HashMap<String, String>` | menjadi `[String: String]` |
| Enum berdata dengan `tag = "method"`, `content = "params"` | didukung |
| Varian `#[serde(rename = "daemon.get_info")]` | case `daemonGetInfo` dengan raw value `daemon.get_info` |
| Enum berdata dengan `#[serde(other)] Unknown` | Swift punya `case unknown` dengan raw `Unknown`, tetapi tag yang tidak dikenal tetap melempar `DecodingError`; tidak ada jalur ke `unknown` |
| Enum tanpa data dengan `#[serde(other)] Unknown` dan `rename_all = "snake_case"` | `enum X: String, Codable` dengan `case unknown` |
| Extension `init(from:)` pada enum tanpa data itu | dikompilasi dengan Swift 6; `"paused"` terbaca `.unknown`, `"needs_recovery"` terbaca `.needsRecovery` |
| Strategi tanggal `ISO8601FormatStyle(includingFractionalSeconds: true)` dengan cadangan tanpa pecahan | `2026-10-18T03:04:05.123Z` dan `2026-10-18T10:04:05+07:00` terbaca benar |

## Cross check

Sonnet 5.5 membaca draf spec ini secara read only pada 2026-10-18, dengan fokus utama pada nilai yang sumbernya belum disebut dan keputusan yang masih harus ditebak builder. Engineer menyetujui semua usulan perbaikan, dan semuanya sudah masuk ke index.md.

**Celah keputusan yang ditutup**:

1. Validasi params per method lewat trait `Validate` dengan set kode field tetap; teks error serde tidak pernah diteruskan karena bisa memuat nilai input.
2. Kegagalan apa pun sebelum handshake diterima mengirim error lalu menutup; batas 5 detik berlaku sampai frame pertama lengkap.
3. Bagian yang dibekukan lintas major (amplop request, `protocol` di handshake, amplop frame error, field inti `ErrorBody`) plus `handshake_major` yang longgar, supaya client major lain selalu mendapat `protocol.major_mismatch`.
4. `stacklyd` menjadi library plus bin dengan trait `Handlers` dan `Daemon::start`, supaya test event, batas request, dan drain bisa ditulis.
5. `stacklyd --print-roots`, karena test home directory yang lama akan menjalankan daemon terhadap Application Support asli.
6. `verify_path` dan `verify_peer` terpisah dengan `ConnectPolicy`, plus seam `PeerCredSource`, supaya kedua cek bisa diuji dengan satu uid.
7. Root dilaporkan dalam bentuk resolved di semua tempat, sama dengan `socketPath`.
8. Nilai `ResourceKind`, bentuk `ResourceRef.id`, alasan `endpoint_untrusted`, dan nama serta `detail` subsystem ditulis lengkap.
9. Aturan revision: event resource wajib membawa revision, revision naik per resource, event penghapusan membawa revision terakhir ditambah 1.
10. `ipc.reply_too_large` dan kewajiban berhalaman untuk method list milik spec lain.
11. `DecodedFrame` dengan `SkippedEvent` sebagai hasil decode frame daemon, diikuti transport Swift.
12. Gap dan `daemon.shutting_down` lolos filter; `subscriptionId` mulai 1 dan tidak dipakai ulang; penerus event tidak dihitung sebagai request berjalan.
13. Level log, kebijakan tulis gagal, pemasangan sink lewat pesan kendali, dan aturan setiap counter.
14. Blok `client` di `--json` `daemon status`, exit 0 untuk status kesehatan apa pun, dan perilaku saat `get_health` gagal.
15. Status `starting` yang terlihat, slot start untuk fitur 5, 10, dan 30, dan kode `daemon.starting`.
16. Bunyi aturan major dan minor untuk varian `Reply` diperjelas, siapa yang menaikkan versi, `contract-versions.json` dengan `--amend`, merge file hasil generate lewat codegen ulang, dan katalog per area.
17. Kunci `details` per kode di katalog, pemotongan pesan di 200 karakter, `ipc.io_error`, batas buffer decoder yang tepat dengan `READ_CHUNK`, dan `tokio::time::Instant` untuk jam test.
18. `syn` dan `proc-macro2` di xtask, dependency dev `stackly-ipc`, `NCSA` di `deny.toml` bila perlu, dan `cargo xtask fuzz seed`.

**Perbaikan kebenaran teknis**: `bind` di std dan tokio sekaligus `listen`, jadi mode socket diatur lewat nama sementara, `chmod`, dan `rename`; urutan berhenti dibalik (unlink lebih dulu, lalu publish, drain, dan flush) supaya `daemon.shutting_down` sempat terkirim dan client baru tidak menggantung; stderr ditulis oleh thread penulis yang sama dengan file; params didecode dari `content` mentah karena `serde_path_to_error` kehilangan path pada enum adjacently tagged kalau `content` datang sebelum `type`; digest dihitung dari bentuk ternormalisasi tanpa teks dokumentasi; client membaca frame error yang tertunda saat tulisnya gagal; penulisan penolakan di accept tidak menahan loop; accept loop memegang listener dan menjalankan pemeriksaan socket; bunyi waktu rebind di AC-11 disesuaikan; `ipc.connection_closed` tidak retryable karena request mungkin sudah berjalan; test log, `lsof`, dan publish dibuat struktural; Milestone 3 kini menunggu Milestone 1, sehingga Milestone 2, 3, dan 5 berjalan paralel setelahnya.

## Perubahan 2026-10-18 (rekonsiliasi lintas spec)

Pengecekan silang Assumed lines antara spec 0005, 0006, 0009, 0011, dan 0012 menemukan selisih yang keputusannya milik spec ini. Engineer menyetujui semua perbaikan berikut. Semuanya tambahan minor, kecuali dua ganti nama yang masih murah karena protokol 1.0 belum dirilis.

| Selisih | Perbaikan | Alasan | Runner up |
| --- | --- | --- | --- |
| `details` hanya pasangan string, padahal spec 0011 butuh daftar reference untuk needs secret dan spec 0009 butuh daftar issue manifest | field opsional `ErrorBody.items: Vec<ErrorItem { kind, fields }>` berbasis string, dengan `kind`, kunci, dan batas per kode di katalog | cross check model lain menunjukkan enum berdata terbuka tidak bisa didecode aman (serde tidak menangkap tag asing, Swift hasil typeshare melempar error dan seluruh frame hilang); item berbasis string selalu terbaca dan amplop yang dibekukan tidak berubah | enum `ErrorData` terbuka dengan probe |
| Recovery mode spec 0005 belum ada di daemon | AC-18: daemon tetap hidup, `registry` `unavailable`, `registry.recovery_mode` untuk method tanpa tanda `allowed_in_recovery`, `DaemonInfo.recovery` (`RecoveryInfo`), `registry_locked` dicoba ulang tiga kali, prune saat start lalu setiap 24 jam | kontrak recovery 0005 menuntut daemon menjawab status dan doctor tanpa menyentuh registry; tanda per method membuat doctor (fitur 34) masuk tanpa mengubah aturan; kunci dari tool lain biasanya sementara; 24 jam cukup untuk retensi 30 hari | daemon keluar 3 (bentrok dengan kode lock) |
| Resep snapshot plus cursor membuang event observasi karena revision nya tidak naik | event berrevision sama hanya membawa observasi dengan waktu observasinya; client menerapkannya per jenis observasi kalau waktunya sama atau lebih baru; pemilik observasi mengirim event saat nilai efektif menjadi basi | observasi memang tidak menaikkan revision (spec 0004, 0011); per jenis supaya health tidak tertolak readiness; waktu sama diterima karena wire hanya tiga digit; tanpa event basi, client yang hanya membaca event tidak pernah melihat `ready` kedaluwarsa | observasi ikut menaikkan revision (merusak CAS registry) |
| Spec 0011 butuh jenis klien dan `interactive` untuk memilih dialog Keychain | `ClientKind` `cli` dan `gui`, `ClientInfo.interactive` opsional dengan default false, dan `RequestContext` untuk setiap handler (AC-19) | default false membuat client lama tidak pernah memicu dialog, jadi aman sebagai minor | field per request di amplop (bagian yang dibekukan) |
| Fallback enum terbuka bernama `unknown`, padahal `unknown` state asli di spec 0004 dan 0011 | fallback menjadi `Unrecognized` (`unrecognized`) | satu kata tidak boleh punya dua arti di wire dan UI | fallback berbeda hanya untuk enum yang punya state `unknown` |
| `HealthStatus` daemon bentrok nama dengan health instance di spec 0012 | `DaemonHealthStatus` | nama generik lebih cocok untuk health instance yang nanti dikirim spec 0010 | spec 0012 yang mengganti nama |

Catatan build fitur 6 milestone 1 (disetujui engineer di sesi build itu): `Call` dan `Reply` ditulis tangan dengan `#[typeshare]`, karena typeshare 1.13.4 tidak melihat item hasil makro `macro_rules!`; tabel `methods!` dengan `match` menyeluruh menggantikan "satu makro menurunkan `Call` dan `Reply`", sehingga varian yang lupa didaftarkan tetap gagal dikompilasi. Fungsi yang gagal dengan `ErrorBody` mengembalikan `Box<ErrorBody>` supaya lint `result_large_err` lulus. Wire tidak berubah. Dari pass rekonsiliasi yang sama: recovery sementara spec 0005 keluar 1, bukan 3, sehingga kode 3 hanya berarti lock.

## Perubahan 2026-10-18, ronde 2 (rekonsiliasi dengan spec 0007, 0008, 0010, 0013)

Disetujui engineer: laporan recovery dipindah ke `DaemonHealth.recovery` dengan tipe `RegistryRecovery` (heartbeat GUI membaca health setiap 15 detik, dan nama `RecoveryInfo` sudah dipakai spec 0007); `statusSeq` spec 0010 menjadi pengecualian resmi di resep snapshot plus cursor; urutan slot start registry, `recover`, rekonsiliasi, `ready`, `start()`, dan shutdown yang menunggu engine operasi 10 detik lalu menghentikan supervisor tanpa menyinyal instance; area `instance` milik spec 0010; enum status instance di wire tertutup; `Client::call_detailed`, `ipc.not_connected`, dan `ipc.call_timeout` (dipakai CLI dan GUI dengan arti yang sama); `ContractNames.swift` dan evaluator murni `evaluate_path` dengan fixture endpoint bersama; `ResourceKind` `manager` dan `secret`; exit code `not_found` 10 dan `internal` 1 dari spec 0008; area `cli` dikecualikan dari digest kontrak karena kodenya tidak pernah lewat wire.


## Perubahan 2026-10-19 (protokol 1.4, temuan review fitur 6)

Review model segar atas fitur 6 (`docs/reviews/2026-10-19-claude-f6-closeout-938837.md`) menemukan celah kontrak yang tidak bisa diperbaiki sesi penutup fitur 6 karena sesi itu tidak menyentuh `crates/protocol`. Sesi kontrak wave 3 memperbaikinya dalam satu kenaikan minor, 1.3 ke 1.4. Engineer memilih kedua keputusan yang ditanyakan (baris pertama dan kedua); sisanya keputusan teknis yang direkomendasikan.

| Selisih | Perbaikan | Alasan | Runner up |
| --- | --- | --- | --- |
| `ErrorBody.retryable` wajib di Rust dan Swift, tetapi bagian yang dibekukan lintas major hanya `code`, `category`, `message`, dan `details`, jadi daemon major 2 yang membuangnya merusak pesan `protocol.major_mismatch` untuk client 1.x | `retryable` masuk daftar bagian yang dibekukan; wire dan kode tidak berubah | tidak ada perubahan wire, Swift tetap `retryable: Bool`, dan app Swift yang sedang dirangkai fitur 13 tidak terganggu; `retryable` adalah informasi keamanan inti amplop, jadi wajar kalau tidak pernah hilang | `#[serde(default)]` di `retryable`: typeshare lalu menulis `Bool?` di Swift, dan setiap pembaca harus membuka opsionalnya |
| `request.invalid_params` tanpa batas `fields`: sekitar 10 500 `kinds` yang salah membuat frame error di atas 1 MiB, lalu daemon gagal encode dan memutus koneksi tanpa jawaban (melanggar AC-7) | `ErrorBody::with_fields` menyimpan 32 field pertama (`FIELD_ERRORS_MAX`) dan menghitung sisanya di `details.omitted`; `FieldError::new` memotong `path` di 256 karakter (`FIELD_PATH_MAX`); `Validate` subscribe hanya memeriksa 32 entri pertama | batas di `ErrorBody` berlaku untuk setiap aturan `Validate` sekarang dan nanti, bukan hanya subscribe; `omitted` sudah dipakai `items` untuk arti yang sama, jadi client cukup punya satu cara menampilkan "dan N lagi"; batas `path` menjaga ukuran walau params nanti punya map dengan kunci panjang | membuang field berlebih tanpa hitungan (client tidak tahu daftarnya terpotong) |
| `Timestamp::from_jiff` panic dari `9999-12-30T22:00:00.001Z` sampai `jiff::Timestamp::MAX`, karena jumlah milidetik di sana di luar rentang `from_millisecond` | memotong hanya nanodetik lewat `jiff::Timestamp::new(detik, nanodetik yang dipotong ke milidetik)`, ditambah test di `MIN`, `MAX`, dan sebelum epoch | detik yang sudah dipegang jiff tidak pernah keluar rentang, dan tanda nanodetik mengikuti detik, jadi pemotongan selalu menuju epoch | `round(Unit::Millisecond)` dengan mode trunc (juga mengembalikan `Result`, tanpa kelebihan) |
| Derive serde membaca struct dari array posisional, jadi `["id", null, {...}]` diterima sebagai request | request, `call`, params, frame daemon, dan `content` frame wajib objek JSON, dicek dari byte pertama sebelum decode; params bukan objek menjadi `request.invalid_params` di path `.` | pemeriksaan satu byte murah dan tidak mengubah jalur decode; client Rust dan Swift selalu menulis objek | deserializer pembungkus yang menolak array di setiap tingkat (lebih lengkap, tetapi ratusan baris; dicatat sebagai Follow-up) |
| Tidak ada fuzz target untuk `decode_server_frame` di client (target itu akan menemukan panic timestamp) | `ipc_server_frame_decode` dengan seed dari setiap fixture `ServerFrame`; harness `ipc_request_decode` juga memeriksa ukuran frame error | jalur client sama terpapar input dari socket seperti jalur daemon | property test di stable saja |
| Digest tidak melihat `CONTROL_FRAME_MAX`, `MAX_JSON_DEPTH`, `REQUEST_ID_MAX`, `SUBSCRIBE_KINDS_MAX`, dan batas lain, jadi menurunkan batas lolos `--check` padahal itu kenaikan major | `limits::CONTRACT_LIMITS`, daftar batas bernama camelCase, masuk digest sebagai objek nama ke nilai | daftar di samping konstanta membuat penambah batas melihat aturannya; objek membuat urutan daftar tidak berarti | membaca konstanta dari sumber Rust dengan `syn` di xtask (rapuh terhadap ekspresi konstanta) |
| Normalisasi digest mempertahankan urutan `required` dan `oneOf`, jadi menyusun ulang field atau varian menggeser digest walau wire sama | `required`, `enum`, `type`, dan cabang `oneOf` serta `allOf` diurutkan; `anyOf` dan `prefixItems` tidak | `oneOf` berarti tepat satu cabang cocok dan `allOf` berarti semua cabang cocok, jadi urutannya tidak bermakna; `anyOf` dipakai serde untuk enum untagged yang dicoba berurutan, dan urutan `prefixItems` adalah posisi tuple | mengurutkan semua daftar schema (bisa menyembunyikan perubahan urutan untagged yang nyata) |
| `IpcError::Io` dengan `op` `read` atau `write` dipetakan ke `ipc.io_error` yang retryable, padahal request mungkin sudah terkirim | `read` dan `write` dipetakan ke `ipc.connection_closed` (tidak retryable); ringkasan katalog kedua kode diperluas | `retryable` true hanya boleh untuk kode yang menjamin request tidak berefek | kode baru `ipc.connection_failed` (menambah kode tanpa arti baru bagi client) |

Nit yang ditunda: Swift membaca `sequence` dan `revision` sebagai `UInt64` biasa (Follow-up), karena perbaikannya mengubah API Swift yang sedang dirangkai fitur 13 di wave yang sama.

Kelas versi: memberi batas pada `fields` dan menolak array posisional dihitung minor, bukan "menurunkan batas" yang major. `fields` tanpa batas tidak pernah benar benar bisa dikirim di atas sekitar 10 500 entri (frame nya gagal dan koneksi putus), jadi pembaca lama tidak kehilangan apa pun yang pernah sampai; array posisional tidak pernah ditulis client Rust maupun Swift. Aturan ini sekarang tertulis di *Versi dan kompatibilitas*.

Cross check model lain (Sonnet) atas update spec ini menemukan: batas yang belum terdaftar (segmen reference secret dan batas bawah `limit` operasi, kini terdaftar plus test pindai sumber), aturan arah yang terbalik untuk batas yang dijamin pengirim (kini dibedakan, dan `--check` hanya memaksa versi baru), arti `omitted` sebagai batas bawah dan larangan `items` di kode yang membawa `fields` (kini tertulis dan diperiksa setiap kali field ditambah), AC-10 yang belum menyebut aturan objek di client dan waktu terakhir jiff (kini disebut), test wire daemon untuk AC-7 (kini ada), dan invariant ukuran frame yang terlalu luas (kini hanya untuk penolakan decode, sisanya Follow-up untuk daemon). Test versi literal (`current_version_is_one_point_four`, `protocolIsOnePointFour`) tetap ada sebagai tanda sengaja bahwa versi naik, walau fixture sudah dijaga sama dengan `CURRENT`.

## Perubahan 2026-10-20 (fase 02)

Update `architect (update)` dari inventaris lead scope 02 (`docs/scope/coordination/02-instalasi-pertama.usulan-fondasi.md`, bagian *Spec 0006*), setelah fitur 6 selesai dan sesi Contract 1.4 merge. Semua perubahan additive atau klarifikasi: tidak ada AC yang diganti nomornya, tidak ada langkah Build plan yang berubah, dan teks ini sendiri tidak mengubah kontrak, jadi `ProtocolVersion::CURRENT` tidak naik karenanya. Setiap spec sumber menaikkan minor di build nya sendiri, dan cabang paralel diselesaikan dengan aturan *Merge* yang sudah ada.

| Item | Perubahan | Sumber |
| --- | --- | --- |
| Hint | satu bentuk untuk semua area: `codes::<area>::hint(code, details) -> Option<&'static str>`, teks statis, dipasang `stacklyd` lewat `ErrorBody::with_hint`, juga untuk `FailureError` operasi yang hint tersimpannya kosong | spec 0017 (*Usulan update spec lain*, *Kode error di wire*), spec 0018 (*Usulan additive untuk spec lain*, *Kode error*), keputusan lead 2026-10-20 |
| Batas hint 200 karakter | dinyatakan sebagai properti katalog yang dicek test katalog per area, bukan entri `CONTRACT_LIMITS`; `with_hint` tidak memotong | spec 0018, resolusi 6 inventaris |
| Tabel area | `catalog` (spec 0017), `runtime` (spec 0018), `storage` (spec 0022) dipindah dari baris "fitur yang pertama kali memakainya" ke baris pemilik | spec 0017, 0018, 0022 |
| Kode `daemon.restart_unavailable` | tabel baru "kode di area spec ini milik spec lain", pemilik spec 0015 | spec 0015 *Usulan update untuk spec lain*, resolusi 7 |
| `ShutdownReason::RestartRequested` | `requested` berarti `daemon.shutdown` mode `stop`; AC-11 dan urutan berhenti langkah 2 dan 7 mengikuti pemicunya (exit 0 atau 75) | spec 0015 AC-6 dan *Method `daemon.shutdown` dan exit code* |
| `daemon.shutdown`, `DaemonInfo.launch` | baris API surface, field opsional `DaemonInfo`, selalu dilayani di slot start | spec 0015 |
| Exit 75 | tabel exit code `stacklyd` | spec 0015 |
| `StartConfig`, `DaemonDeps`, `DaemonConfig`, launch record, catalog di slot start, subsystem `catalog`, `STACKLY_CATALOG_DIR`, `XPC_SERVICE_NAME` | *Daemon* (pengantar, langkah 1, 8, 8a, 11, subsystem) dan *Configuration required* (implisit di spec sumber) | spec 0015 *Konteks launch dan launch record di daemon*, spec 0017 *Lokasi catalog saat start* dan K-2 |
| Subsystem baru | aturan bahwa subsystem baru ditambah fitur pemakai pertama, dengan `catalog` tercantum; kedua opsi resolusi 2 dipakai karena keduanya additive | spec 0017, resolusi 2 |
| Hint CLI daemon status | `start it with stacklyd` diganti penunjuk ke *Langkah pemulihan CLI* spec 0015 | spec 0015 |
| `SignedSafeInteger` | aturan wire dan mapping typeshare `Int64`, memakai ulang `MAX_SAFE_INTEGER` tanpa konstanta baru sehingga `CONTRACT_LIMITS` tidak berubah | spec 0020 (implisit; spec itu menyatakan tidak ada update untuk 0006), resolusi 5 |
| Security model | verifikasi tanda tangan peer menunjuk spec 0015 (ditunda ke fitur 60 dan 63) | spec 0015 *Verifikasi tanda tangan peer* |
| *Asumsi ke spec lain* | tambahan spec 0015, 0017, 0018, 0020, dan 0022 dicatat dengan cara yang sama dengan spec lain | inventaris, pengantar bagian *Spec 0006* |

Tidak diterapkan: item 10 inventaris (daftar pemakai *Fuzzing* ditambah fitur 19 dan 23), karena ditandai opsional dan tidak diusulkan. Follow-up spec ini untuk fitur 15 (verifikasi tanda tangan dan `stackly daemon start`) sudah terjawab spec 0015 dan dicentang lead lewat `/sync`, bukan di sini.
