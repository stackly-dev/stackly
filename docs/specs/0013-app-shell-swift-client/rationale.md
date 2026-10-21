# 0013. Rationale: app shell SwiftUI dan Swift client

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: fitur 13 menggabungkan dua hal yang biasanya dipisah, yaitu Swift client (transport, koneksi, sinkronisasi) dan shell jendela SwiftUI. Keduanya tetap satu spec karena Done when fitur 13 menyatukan keduanya (status koneksi, reconnect, dan banner offline adalah perilaku client yang terlihat di shell), dan karena gate mutasi spec 0012 butuh satu pemilik yang memegang status koneksi sekaligus mengisi environment UI. Risiko kedua: milestone terakhir bergantung pada kontrak yang belum ditulis (DTO operasi dari spec 0007 yang ditulis paralel, dan DTO instance dari spec 0010 atau build fitur 14). Spec ini menulis bentuk yang dibutuhkan sebagai Assumed dan menaruh pekerjaan yang bergantung padanya di milestone 4, supaya milestone lain tidak menunggu.

Stackly punya satu daemon per user yang memegang registry dan semua proses; GUI hanya client yang bicara lewat socket Unix privat (BR-011, REQ-001). Spec 0006 sudah mengunci wire: frame 4 byte big endian, handshake berversi, envelope error dengan kategori tertutup, event dengan sequence dan cursor di memori, dan resep reconnect "subscribe dulu, lalu snapshot". Spec 0006 juga menulis transport Rust di `stackly-ipc`, tetapi GUI tidak boleh link Rust (spec 0001), jadi Swift butuh transport sendiri yang setara byte per byte, termasuk pemeriksaan path dan uid peer sebelum mengirim apa pun (AC-070). Spec 0004 menurunkan path socket dari temp dir user, `STACKLY_ROOT`, dan hash FNV-1a, dan Follow-up nya meminta GUI menghitung path yang sama dengan bukti fixture bersama.

Dari sisi pengguna, PRD meminta GUI tetap terbuka dan berguna saat daemon tidak terjangkau: status koneksi, retry, dan diagnostics, tanpa menyatakan instance berhenti dan tanpa menjalankan binary sendiri (REQ-024, PRD AC-024). Drift protokol antara GUI dan daemon adalah risiko yang disebut PRD (RISK-09): GUI dan helper bisa berasal dari build berbeda. Scope menambah bentuk shell dari mockup (sidebar dengan grup Library dan System, badge, footer status daemon dengan tombol Settings, toolbar per halaman), banner yang menyebut waktu state terakhir, mutasi yang nonaktif saat offline, jendela yang bisa diubah ukurannya dengan minimum tercatat, dan menutup jendela yang tidak menghentikan daemon atau instance. Spec 0012 sudah menyerahkan ke spec ini: satu banner daemon tidak terjangkau, pengisian `MutationAvailability`, adapter DTO ke enum status UI dengan switch exhaustive, lebar minimum jendela, `SectionIcon` di sidebar, dan apakah `StacklyUI` bergantung pada `StacklyClient`.

Fakta teknis yang membatasi: target macOS 27 dengan Swift 6 strict concurrency; `StacklyClient` sengaja tanpa default isolation `MainActor` dan sejauh ini tanpa dependency pihak ketiga; `StacklyUI` memakai `MainActor` sebagai default. `Network.framework` bisa membuka socket Unix tetapi tidak memberi akses ke file descriptor, sehingga uid peer tidak bisa dibaca lewat `getpeereid`. Panjang path socket macOS dibatasi 104 byte, jadi test dengan socket asli butuh folder pendek. Di macOS 27, text style tidak membesar dengan `dynamicTypeSize` (temuan milestone 1 fitur 12), jadi cek teks besar diganti cek lebar sempit di lebar konten minimum. Fitur 14 (walking skeleton) butuh GUI menampilkan perubahan state instance lewat event, padahal layar Instances milik fitur 22 yang jauh lebih belakang. Tidak ada CI; semua test dan evidence berjalan di Mac ini (spec 0002).

## Options considered

### Keputusan 1: apakah `StacklyUI` bergantung pada `StacklyClient`

#### Option 1: ya, `StacklyUI` → `StacklyClient`

**Pros**:

- Layar dan view model per bagian tetap di `StacklyUI`, sesuai aturan folder per layar di `apps/macos/AGENTS.md`.
- Preview dan fixture fitur 68 memakai DTO kontrak yang sama dengan client, seperti diminta Done when fitur 68.
- Adapter DTO ke enum UI tinggal di sebelah enum UI nya.

**Cons**:

- `swift test` untuk `StacklyUI` ikut membangun client.
- Nama yang sama di dua modul (`HealthStatus`) harus ditulis lengkap di kode `StacklyUI`.

#### Option 2: target baru `StacklyShell`

**Pros**:

- `StacklyUI` tetap design system murni yang cepat dites.

**Cons**:

- Menyimpang dari aturan folder per layar di `StacklyUI`, dan setiap layar journey harus memilih target.
- Satu target tambahan di package dan satu batas lagi yang harus dijaga.

#### Option 3: glue di target app

**Pros**:

- `StacklyUI` tidak tahu protokol sama sekali.

**Cons**:

- Layar tidak bisa di preview dengan DTO asli tanpa app, dan setiap layar butuh model view terpisah yang menggandakan DTO.
- Target app menjadi gemuk dan sulit dites di luar Xcode.

### Keputusan 2: transport socket Unix

#### Option 1: POSIX socket plus `DispatchIO`

**Pros**:

- `getpeereid`, `lstat`, `SO_NOSIGPIPE`, dan `FD_CLOEXEC` langsung tersedia, jadi verifikasi endpoint spec 0006 bisa ditiru persis.
- Tanpa dependency; `DispatchIO` menangani baca tulis tanpa blocking di antrian privat.

**Cons**:

- Kode pointer, errno, dan siklus hidup fd harus ditulis teliti.

#### Option 2: thread khusus dengan baca blocking

**Pros**:

- Paling sederhana dibaca.

**Cons**:

- Jembatan manual ke async/await, dan baca blocking sulit dibatalkan rapi saat koneksi ditutup.

#### Option 3: swift-nio

**Pros**:

- Pipeline framing dan bootstrap socket Unix yang matang.

**Cons**:

- Dependency pihak ketiga pertama di package Swift, besar untuk satu koneksi lokal, dan `getpeereid` tetap harus lewat fd sendiri.

#### Option 4: `Network.framework`

**Pros**:

- API Apple tingkat tinggi dengan async yang rapi.

**Cons**:

- Tidak memberi file descriptor, jadi uid peer tidak bisa diperiksa; melanggar AC-3 spec 0006. Tidak dibawa ke pertanyaan karena gugur di syarat keamanan.

### Keputusan 3: kebijakan reconnect

#### Option 1: jeda 0,5, 1, 2, 4 detik, lalu tiap 5 detik

**Pros**:

- Pulih cepat setelah restart daemon terencana, dan cap 5 detik cocok dengan rebind daemon yang paling lama 12 detik.
- Percobaan langsung saat Reconnect, app aktif, dan Mac bangun menutup celah jeda.

**Cons**:

- Tetap mencoba tiap 5 detik selama daemon dimatikan lama.

#### Option 2: tetap tiap 2 detik

**Pros**:

- Paling sederhana.

**Cons**:

- Terus membangunkan CPU dan mencatat penolakan di log daemon tanpa jeda.

#### Option 3: jeda 1 sampai 30 detik

**Pros**:

- Paling hemat.

**Cons**:

- Pemulihan otomatis bisa terlambat sampai 30 detik.

### Keputusan 4: perilaku saat jendela ditutup

#### Option 1: app tetap hidup dan memberi notice

**Pros**:

- Cocok dengan copy mockup "Stackly is still running" dan dengan menu bar fitur 32 yang butuh proses tetap hidup.
- Koneksi tetap hangat, jadi membuka jendela lagi langsung menampilkan state terbaru.

**Cons**:

- Pengguna yang ingin menutup app harus ⌘Q, dan sampai fitur 32 jalan kembali hanya lewat Dock.
- Notice setelah jendela tertutup butuh panel AppKit kecil.

#### Option 2: app tetap hidup tanpa notice

**Pros**:

- Tidak ada panel buatan sendiri.

**Cons**:

- Peta layar menaruh notice di fitur 13; pengguna tidak tahu app masih hidup.

#### Option 3: app keluar saat jendela ditutup

**Pros**:

- Paling sederhana dan perilaku bawaan scene `Window`.

**Cons**:

- Fitur 32 harus membalik perilaku ini, dan copy mockup tidak berlaku.

### Keputusan 5: tampilan instance sebelum fitur 22

#### Option 1: daftar sementara di halaman Instances

**Pros**:

- Fitur 14 bisa membuktikan "GUI menampilkan perubahan state lewat event" di tempat pengguna mencarinya.
- Fitur 22 mengganti view nya tetapi tetap memakai `InstanceStore`.

**Cons**:

- Kode view yang sengaja dibuang.

#### Option 2: daftar sementara di Overview

**Pros**:

- Halaman pertama yang terlihat.

**Cons**:

- Fitur 22 harus memindahkannya, dan Instances tetap kosong lebih lama.

#### Option 3: tanpa daftar

**Pros**:

- Tidak ada kode sementara.

**Cons**:

- Done when fitur 14 bergantung pada keputusan yang tidak dimiliki siapa pun.

### Keputusan 6: state terakhir setelah app dibuka ulang

#### Option 1: hanya di memori

**Pros**:

- Tidak ada nama instance atau path di cache disk, sejalan dengan spec 0012 (StacklyUI tidak menyimpan state ke disk).
- Tidak ada invalidasi cache dan tidak ada risiko menampilkan state yang sangat lama.

**Cons**:

- Setelah relaunch tanpa daemon, pengguna hanya melihat banner dan placeholder.

#### Option 2: cache snapshot di disk

**Pros**:

- Lebih informatif saat daemon mati sejak sebelum app dibuka.

**Cons**:

- File cache privat baru, aturan privasi, dan state yang bisa berumur hari tetapi tampil seperti nyata.

### Keputusan 7: bukti test perilaku GUI

#### Option 1: Swift Testing plus evidence manual

**Pros**:

- Logika shell (status, presentasi, gate, store) ada di model yang bisa dites Swift Testing dengan trait `.evidence` yang sudah ada.
- Spec 0002 tidak perlu diubah sekarang; XCUITest dan snapshot tetap satu paket di fitur 68.

**Cons**:

- Regresi tampilan shell baru tertangkap saat `/check verify` sampai fitur 68.

#### Option 2: XCUITest sekarang

**Pros**:

- Perilaku jendela dan banner teruji otomatis lebih awal.

**Cons**:

- Butuh helper evidence XCTest di spec 0002 lebih dulu, dan infrastrukturnya tumpang tindih dengan fitur 68.

### Keputusan 8: letak diagnostics koneksi

#### Option 1: seksi Connection di Diagnostics plus aksi banner

**Pros**:

- Satu tempat lengkap yang juga menjadi rumah hasil doctor fitur 34.
- Banner offline memberi jalan langsung lewat Open Diagnostics.

**Cons**:

- Mockup banner hanya punya Reconnect; aksi kedua adalah selisih yang dicatat.

#### Option 2: popover dari footer

**Pros**:

- Dekat dengan status daemon.

**Cons**:

- Popover sempit untuk path, kode, dan petunjuk, dan halaman Diagnostics tidak bisa dibuka saat offline sampai fitur 34.

#### Option 3: keduanya

**Pros**:

- Ringkas dan lengkap sekaligus.

**Cons**:

- Dua tampilan yang harus dijaga tetap sama.

### Keputusan 9: cakupan operation center

#### Option 1: store plus badge sidebar

**Pros**:

- Memberi semua layar satu sumber operasi aktif dan cara mengikuti operasi yang mereka mulai, tanpa mengambil halaman fitur 35.

**Cons**:

- Pengguna belum melihat daftar operasi sampai fitur 35 atau 22.

#### Option 2: ditambah daftar sementara di Operations

**Pros**:

- Store terlihat sebelum fitur 35.

**Cons**:

- Kode view tambahan yang dibuang.

#### Option 3: ditambah popover aktivitas di toolbar

**Pros**:

- Progress terlihat dari halaman mana pun.

**Cons**:

- Tidak ada di mockup, dan bentrok dengan kartu operasi Overview.

### Keputusan 10: halaman yang belum dibangun

#### Option 1: placeholder netral

**Pros**:

- Sidebar lengkap sesuai Done when, dan setiap placeholder jelas ditandai untuk fitur pemiliknya.

**Cons**:

- Pengguna build awal melihat halaman yang belum berisi.

#### Option 2: bagian disembunyikan

**Pros**:

- Tidak ada halaman kosong.

**Cons**:

- Bertentangan dengan Done when fitur 13.

#### Option 3: bentuk unsupported

**Pros**:

- Memakai pola yang sudah ada.

**Cons**:

- Arti unsupported adalah capability yang tidak didukung, bukan fitur yang belum dibangun.

### Keputusan 11: tampilan saat belum pernah ada state

#### Option 1: banner plus placeholder halaman

**Pros**:

- Banner berperilaku sama di setiap keadaan offline; placeholder halaman memberi Reconnect dan Open Diagnostics di tempat konten.
- Sejalan dengan `ViewState.resolve` spec 0012 (tanpa data dan ada error berarti failed).

**Cons**:

- Pesan "tidak tersambung" muncul dua kali di layar.

#### Option 2: placeholder saja

**Pros**:

- Tidak ada pesan ganda.

**Cons**:

- Banner punya dua perilaku, dan halaman tanpa store (placeholder fitur lain) tidak menjelaskan apa pun.

#### Option 3: banner saja, halaman loading

**Pros**:

- Pesan tunggal.

**Cons**:

- Spinner tanpa akhir terasa seperti app macet, dan melanggar urutan `ViewState` spec 0012.

### Keputusan 12: tombol Settings sebelum fitur 32

#### Option 1: scene Settings placeholder

**Pros**:

- Tombol, ⌘, dan menu app sudah benar sejak awal; fitur 32 hanya mengisi isinya.

**Cons**:

- Jendela Settings yang hampir kosong.

#### Option 2: tombol nonaktif dengan alasan

**Pros**:

- Tidak ada jendela kosong.

**Cons**:

- Footer terlihat setengah jadi, dan ⌘, tidak melakukan apa pun.

### Keputusan 13: cara menguji client terhadap daemon

#### Option 1: `stacklyd` asli plus server palsu Swift

**Pros**:

- Membuktikan Swift dan daemon asli benar benar cocok (path socket, handshake, event, restart).
- Kasus yang tidak bisa dipicu daemon asli (major berbeda, frame rusak, uid peer lain, byte yang tertulis) diuji server palsu.

**Cons**:

- Integration test butuh binary `stacklyd` yang sudah dibangun; tanpa binary test `BLOCKED`.

#### Option 2: hanya server palsu

**Pros**:

- Cepat dan tanpa proses Rust.

**Cons**:

- Tidak membuktikan kecocokan dengan daemon nyata, jadi drift lolos sampai dipakai.

#### Option 3: hanya daemon asli

**Pros**:

- Bukti paling kuat.

**Cons**:

- Kasus gagal protokol butuh flag uji khusus di daemon produksi.

### Keputusan 14: ukuran minimum jendela

#### Option 1: 900 × 600 pt

**Pros**:

- Dengan sidebar maksimal 260 pt, kolom konten tidak pernah kurang dari 640 pt, jadi cek lebar sempit spec 0012 tetap 640.
- Muat di layar MacBook terkecil dengan Dock, dan bisa berdampingan dengan editor.

**Cons**:

- Overview dua kolom dan inspector harus menyesuaikan diri di lebar konten 640 pt.

#### Option 2: 1024 × 640 pt

**Pros**:

- Ruang konten lebih lega.

**Cons**:

- Jendela tidak bisa dibuat kecil di samping editor.

#### Option 3: 800 × 560 pt

**Pros**:

- Paling fleksibel.

**Cons**:

- Konten hanya sekitar 560 pt saat sidebar terbuka, jadi cek sempit spec 0012 harus turun.

## Rationale

Semua keputusan memakai opsi yang direkomendasikan, dan engineer mengonfirmasi setiap pilihan.

**Client (keputusan 1, 2, 3, 13).** Kekuatan terbesar adalah AC-3 spec 0006: client tidak boleh menulis satu byte pun sebelum path dan uid peer terbukti milik user yang sama. Itu menggugurkan `Network.framework` dan membuat POSIX socket satu satunya jalan yang tidak menambah dependency, dengan `DispatchIO` sebagai mesin baca tulis yang sudah dipakai Apple sendiri untuk fd. Karena transport ditulis dua kali, bukti kecocokan harus dua lapis: fixture bersama untuk yang bisa diuji lewat data (framing, path socket, DTO, kode error), dan integration test terhadap `stacklyd` asli untuk yang hanya terlihat saat dua proses benar benar bicara (handshake, event, restart). Server palsu menutup sisanya tanpa menambah flag uji ke daemon produksi. Kebijakan reconnect mengikuti angka daemon sendiri: rebind paling lama 12 detik dan restart terencana, sehingga cap 5 detik cukup cepat tanpa memukul daemon yang memang dimatikan. `StacklyUI` bergantung pada `StacklyClient` karena aturan folder per layar dan Done when fitur 68 sama sama mengandaikan layar dan fixture memakai DTO kontrak langsung.

**Kejujuran saat offline (keputusan 6, 8, 11).** PRD AC-024 adalah alasan semua perilaku offline: GUI tidak boleh berbohong tentang instance. Status item hanya berasal dari DTO daemon, koneksi putus hanya menambah banner dan gate, dan waktu state terakhir adalah waktu frame terakhir yang benar benar diterima. State terakhir sengaja hanya di memori: cache disk akan menampilkan data yang bisa berumur hari dengan tampilan yang sama dengan data hidup, dan menambah file privat baru yang harus dijaga. Diagnostics koneksi tinggal di halaman Diagnostics karena hanya halaman itu yang muat path, kode, alasan, dan petunjuk sekaligus, dan fitur 34 nanti melanjutkan halaman yang sama.

**Shell (keputusan 4, 5, 7, 9, 10, 12, 14).** Shell dibuat lengkap sesuai Done when (semua bagian sidebar, footer, Settings) walau sebagian besar halaman belum ada, karena layar journey berikutnya harus bisa menempel ke bingkai yang sudah benar. App tetap hidup setelah jendela ditutup karena fitur 32 membutuhkannya dan copy mockup sudah mengandaikannya; notice hanya muncul sekali per peluncuran supaya tidak mengganggu. Daftar instance sementara ada karena fitur 14, bukan karena shell butuh daftar; ia hidup di halaman yang nanti dimiliki fitur 22 supaya penggantiannya hanya menukar view. Operation center berhenti di store dan badge supaya tidak mengambil halaman fitur 35. Ukuran minimum 900 × 600 pt dipilih dari dua angka yang sudah ada: sidebar mockup 236 pt dan lebar sempit 640 pt spec 0012; dengan sidebar dibatasi 260 pt, konten tidak pernah lebih sempit dari angka yang sudah dites fitur 12. Bukti GUI memakai Swift Testing plus evidence manual karena logika yang bisa salah ada di model, sedangkan XCUITest dan snapshot lebih masuk akal dibangun sekali di fitur 68.

## Keputusan rekomendasi

Keputusan kecil yang diambil saat menulis spec, masing masing dengan alasan satu baris dan alternatif yang ditolak:

- **Satu scene `Window` utama**, bukan `WindowGroup`. Stackly adalah app satu jendela di mockup, dan `Window` memberi satu jendela yang bisa dibuka ulang. Alternatif: `WindowGroup`, yang memberi ⌘N jendela kedua dengan dua shell yang bersaing memegang koneksi yang sama.
- **Satu subscription untuk semua jenis event**, lalu diarahkan di client. Batas 8 subscription per koneksi tetap tersisa untuk log (fitur 27). Alternatif: satu subscription per store, yang menggandakan event gap dan cursor.
- **Snapshot mengganti isi store lalu memutar ulang event yang ditahan**, bukan menggabungkan per item. Aturan revision cukup untuk benar, dan hapus tidak bisa dihidupkan snapshot lama. Alternatif: merge per item dengan revision tertinggi, yang butuh penanda hapus dan rumit untuk item yang hilang dari snapshot.
- **Timeout atau cancel hanya menggagalkan call itu**, dengan set ID yang ditinggalkan yang terbatas supaya balasan telat dibuang tanpa menganggapnya pelanggaran protokol. SwiftUI membatalkan `.task` setiap kali view hilang, jadi menutup koneksi untuk setiap cancel akan mengubah mutasi lain yang sedang berjalan menjadi `outcomeUnknown`. Koneksi hanya ditutup saat handshake atau heartbeat timeout. Alternatif yang ditolak (draf awal): timeout menutup seluruh koneksi.
- **`requestId` dan `idempotencyKey` dari `UUID()` huruf kecil (v4)**. Spec 0006 hanya mengatur format; Foundation tidak punya UUID v7. Alternatif: menulis generator v7 sendiri demi urutan di log, yang tidak dibutuhkan GUI.
- **Tiga hasil gagal (`notSent`, `rejected`, `outcomeUnknown`)**. Umpan balik "Nothing was sent" hanya jujur kalau client tahu tidak ada byte yang tertulis. Alternatif: satu `ErrorBody`, yang memaksa layar menebak dari kode.
- **`MutationPath` hanya menolak lokal saat tidak tersambung**. Saat tersambung, daemon yang memutuskan (termasuk `daemon.starting`), jadi tidak ada `ErrorBody` daemon yang dipalsukan client. Alternatif: menolak lokal untuk setiap `.blocked`, yang harus mengarang kode untuk starting dan recovery.
- **Heartbeat `daemon.get_health` tiap 15 detik (1 detik selama `starting`)**. Spec 0006 tidak punya event untuk perubahan health, daemon yang macet tidak pernah menutup socket, dan tanpa frame berkala `lastKnownAt` bisa berumur jam untuk daemon sehat yang diam. Satu call kecil menjawab ketiganya. Alternatif yang ditolak (draf awal): `get_health` hanya setelah handshake, yang membuat footer tertahan di "starting" dan daemon macet tidak terdeteksi.
- **Status koneksi berubah per kode, bukan per kategori**. `request.unknown_method` juga berkategori `unsupported`, dan error `unauthorized` per call di masa depan tidak boleh memutus koneksi. Alternatif yang ditolak (draf awal): pemetaan per kategori.
- **Presence sebagai fungsi murni dengan status `refreshing`**. Banner, gate, dan pengumuman baru berubah setelah semua store selesai resnapshot, supaya "State refreshed" tidak diumumkan sebelum state benar benar segar dan banner tidak berkedip di setiap percobaan. Alternatif: presence mengikuti `ConnectionState` mentah.
- **Banner critical untuk incompatible, untrusted, recovery, dan misconfigured**, caution hanya untuk tidak terjangkau. Empat keadaan itu tidak pulih sendiri dan butuh tindakan pengguna. Alternatif: semua caution, yang menyamakan masalah keamanan dengan daemon yang sedang restart.
- **`lastKnownAt` adalah waktu frame terakhir yang diterima setelah snapshot selesai**. Selama event mengalir, state benar sampai frame terakhir; daemon yang macet sebelum putus tidak membuat waktu itu terlihat lebih baru dari kenyataan. Alternatif: waktu putus terdeteksi, yang bisa lebih baru dari state sebenarnya.
- **`misconfigured` tidak pernah mencoba connect**. Override yang salah hanya bisa dibetulkan dengan launch ulang. Alternatif: tetap mencoba, yang tidak mungkin berhasil.
- **Jeda 30 detik untuk incompatible dan untrusted**. Keduanya butuh perubahan dari luar, dan setiap percobaan dicatat daemon sebagai koneksi ditolak. Alternatif: jeda yang sama dengan unreachable.
- **Badge sidebar native (`.badge`)**. Aturan fidelity: kontrol native menang atas tiruan HTML, dan warna aksen bukan warna teks kustom (spec 0012). Alternatif: kapsul biru dan angka merah buatan sendiri seperti mockup.
- **Lebar sidebar 200 sampai 260 pt dengan ideal 236 pt**. 236 pt adalah lebar mockup; 260 pt menjaga konten minimal 640 pt di jendela 900 pt. Alternatif: lebar bebas, yang bisa mendorong konten di bawah lebar yang sudah dites.
- **Notice tutup jendela sekali per peluncuran, 4 detik, opak**. Cukup untuk memberi tahu tanpa mengganggu setiap kali, dan tanpa glass kustom (spec 0012). Alternatif: setiap kali ditutup, atau sekali seumur instalasi lewat `UserDefaults`, yang menyimpan state ke disk.
- **Copy Details di seksi Connection**. Laporan bug dan pertanyaan dukungan butuh teks yang bisa ditempel. Alternatif: hanya tampilan, yang memaksa pengguna menyalin baris satu per satu.
- **GUI tidak mengulang validasi kepemilikan dan mode folder override**. Verifikasi endpoint sudah menolak socket yang tidak dipercaya, dan GUI tidak membuat folder. Alternatif: memindahkan `validate_root_override` ke Swift, yang menggandakan aturan tanpa menambah keamanan.
- **`STACKLY_DAEMON_BIN` dengan fallback `target/debug/stacklyd`**. Test Swift tidak menjalankan `cargo build` sendiri, dan binary yang tidak ada dicatat `BLOCKED` dengan jujur. Alternatif: test memanggil cargo, yang lambat dan mengejutkan.
- **Test aturan sumber untuk spawn proses, API jaringan, dan `default:` di adapter**. Tiga invariant yang paling mudah dilanggar diam diam dijaga test, bukan ingatan reviewer. Alternatif: review manual.
- **`SO_NOSIGPIPE` di setiap socket**. Tanpa itu, menulis ke socket yang sudah ditutup daemon mengirim SIGPIPE dan mematikan app. Alternatif: mengabaikan SIGPIPE di seluruh proses, yang mengubah perilaku global app.
- **Jenis event yang dikenal memakai mekanisme fixture `skipped` fitur 6**. Satu sumber untuk tahu event mana yang dikenal Swift. Alternatif: daftar tulis tangan di transport, yang bisa berbeda dengan tipe hasil generate.

Tidak ada tool baru yang dipilih (semua API Apple bawaan), jadi tawaran Agent Skill dan MCP dilewati. Engineer memilih tanpa bagian References.

## Cross check

Sonnet 5.5 membaca draf spec secara read only (tanpa mengubah file) dan membandingkannya dengan spec 0004, 0005, 0006, 0012, scope, dan AGENTS.md. Temuan yang dicek ulang terhadap repo (misalnya milestone 2 sampai 4 fitur 12 belum di main, `StacklyTestSupport` belum product, Rust menghitung hash `as_encoded_bytes`, dan join temp dir default ada di `stackly-platform-macos`) terbukti benar. Engineer memilih menerapkan semua perbaikan yang disarankan pada 2026-10-18:

1. Milestone 2 sampai 4 fitur 12 menjadi prasyarat milestone 3; `StatusAdapter` (AC-16) pindah ke milestone 4 karena butuh enum DTO.
2. Perubahan `Package.swift` ditulis persis: product `StacklyTestSupport`, dan referensi `.product(name:package: "swift")` dari `StacklyUI`.
3. `ContractNames.swift` hasil codegen memberi daftar jenis event dan nama method, karena typeshare tidak menghasilkannya.
4. Dua kode client baru diminta ke spec 0006 (`ipc.not_connected`, `ipc.call_timeout`), dengan `ClientErrorCatalog` Swift yang dites terhadap `error-codes.json` dan pesan tetapnya ditulis.
5. Transisi status koneksi dikunci per kode, dan reply `rejected` tidak pernah mengubah status; handshake yang ditolak `request.*` menjadi `incompatible`.
6. Presence menjadi fungsi murni dengan `refreshing` dan `connecting(slow:)`, supaya banner tidak berkedip, launch tidak menampilkan kegagalan palsu, dan pengumuman datang setelah state segar.
7. Heartbeat `daemon.get_health` menjadi sumber health, deteksi daemon macet, dan batas umur `lastKnownAt`.
8. Recovery dideteksi lewat satu sinyal (`RecoveryReport` di status daemon), dan banner tidak lagi menjanjikan laporan yang milik fitur 48.
9. Selaras dengan spec 0012: banner informative untuk starting, connecting lambat, dan shutting down; setter `MutationAvailability` dicatat Assumed; aksi banner `mutates: false`; placeholder memakai Retry; error presence hanya diteruskan saat belum ada data.
10. Footer memakai simbol tone; `stopping`, health asing, dan `peer_rejected` tanpa alasan punya baris dan teks sendiri.
11. Sinkronisasi dirinci: definisi cursor dan gap, loop berurutan, token generasi, batas ulang, API reducer `ResourceStore`, tombstone, dan pagination token.
12. Operation center: `needs_recovery` di `active` tanpa dihitung badge; badge dan subjudul dari `running` dan `cancel_requested`; pluralisasi "instance".
13. `default_runtime_root` ditaruh di `stackly-platform` (portabel), dan Swift menghitung hash byte mentah `realpath`.
14. Fixture bersama `protocols/fixtures/endpoint/checks.json` mengunci urutan cek endpoint dan kosakata `details.kind` di Rust dan Swift.
15. Timeout atau cancel hanya menggagalkan call itu, dengan set ID yang ditinggalkan; koneksi hanya ditutup saat handshake atau heartbeat timeout.
16. Frame error tanpa `requestId` setelah handshake membuat call tertunda `outcomeUnknown`, bukan `rejected`.
17. Sambungan AppKit dirinci: `ShellRuntime` dengan hook lifecycle sinkron, `WindowOpener`, `MainWindowAccessor`, dan penanda Quit supaya notice tidak muncul saat keluar.
18. Invariant "tanpa disk" mengecualikan restorasi scene milik sistem (`@SceneStorage`).
19. Diagnostics: path socket dibungkus, Copy Details sama persis dengan baris, `lastDaemon`, `lastError`, dan `lastConnectedAt` disimpan, Reconnect nonaktif dengan alasan saat `misconfigured`.
20. Badge Operations disembunyikan saat tidak tersambung; badge Instances tetap dengan label "last known".
21. Mekanik test dan Swift 6: `now` disuntik, regex aturan sumber dengan batas kata, IO blocking di antrian khusus, `DispatchIO` dengan `lowWater` 1 dan resume hanya saat `done`, backpressure lewat penjadwalan baca, cek `sun_path`, multicast status, cek ulang setelah suspensi, `maxInFlight` minimal 1, dan versi bundle yang tidak valid menjadi `misconfigured`.

## Perubahan 2026-10-18 (rekonsiliasi lintas spec, ronde 2)

Disetujui engineer: `ClientInfo.interactive` true untuk GUI; store menerapkan event menurut resep spec 0006 langkah 4 (revision, observasi per jenis, dan `statusSeq` untuk status instance), bukan "revision tidak lebih besar dilewati", supaya readiness dan health tetap hidup tanpa resnapshot; fallback enum terbuka `unrecognized`; `DaemonHealthStatus` menghapus bentrok nama dengan `HealthStatus` instance; laporan recovery dari `DaemonHealth.recovery` (`RegistryRecovery`). `ipc.not_connected` dan `ipc.call_timeout` kini ada di katalog spec 0006.


## Perubahan 2026-10-20 (fase 02, K16 sampai K18)

Update `/architect` setelah branch `claude/f13-wiring-7b3a78` di merge (cdad419), sebelum langkah 16 dibangun. Lead scope 02 menghentikan program di tengah update ini, jadi hanya keputusan lead fondasi K16, K17, dan K18 yang diterapkan; usulan fase 02 lain untuk spec ini belum. Teks saja: ID AC, nomor langkah build, dan baris Status tidak berubah; tambahan per langkah ada di blok *Tambahan langkah 16 dari keputusan K16 dan K17* di bawah *Build plan*.

| Butir | Perubahan | Sumber |
| --- | --- | --- |
| Aturan `statusSeq` | AC-8, *Sinkronisasi snapshot plus cursor*, dan *Key invariants*: kalimat "`.patch` dengan revision yang sama" diganti efek observasi tanpa revision `.observe(id, revision, apply:)` plus `merge` per store; observasi tidak pernah mengubah revision atau tombstone; berlaku juga untuk hasil `fetchOne` dan event yang ditahan; batas di reset saat gap `server_restarted` | K16 (b), risiko K18 "literal statusSeq rule bug" dan "statusSeq resets per daemon run"; 0006 langkah 4 |
| Aturan observasi umum | dicatat belum dibangun; pemiliknya store fase 02 pertama yang mempublikasikan observasi dengan revision yang sama | risiko K18 "generic observation rule" |
| Instance `retained` | disaring di `fetchPage`, `fetchOne` (nil, jadi delete dengan tombstone), dan `reduce` (`.delete`); `count`, badge AC-13, dan subjudul AC-17 tidak menghitungnya; invariant baru; test `offer` dan refetch | K16 (a), risiko K18 "retained filter guard" |
| `limit` 200 | `fetchPage` memanggil `instance.list { limit: 200, before }` | K16; usulan 0022 (P15) |
| `instance.removed` | dipetakan ke `.delete` pada revision event di langkah 16, karena DTO nya ikut langkah 12 spec 0010 yang menjadi prasyarat langkah 16 | K16 (c) dan SEQ 7, K4 |
| Pembagian resep | langkah 12 spec 0010 berhenti di decode dan test kontrak; langkah 16 menjalankan resep lewat `InstanceStore` | K16 SEQ 3 dan 4, risiko K18 "recipe test ownership" |
| AC-17 | ditulis ulang: tanpa aksi selama `newInstanceAction` nil; bila fitur 20 mengisinya, tombol toolbar dan aksi kedua empty state dengan `.mutationGated()` sampai fitur 22; pesan empty state tetap; langkah 16 tanpa aksi dan tanpa `NewInstanceAction` | K17 |
| `LiveShell` dan `assemble` | *Model shell* dan *Shell jendela* mendeskripsikan yang dibangun langkah 15: `LiveShell(runtime:windowWillClose:)`, `assemble -> LiveShellParts`, `live()` satu satunya pintu publik; snippet `AppShell().environment(runtime.model)` dan kalimat accessor diganti | K18 |
| `MainWindowRoot` | hanya baris batas milik spec 0016, berlaku mulai langkah 2 milestone 1 fitur 16; perpindahan accessor, `openWindow`, frame, pengecualian `onboarding.finished`, dan `onboardingStore` ditulis di commit langkah itu | K18; usulan 0016 |
| `rootOverrideActive` | Bool baca saja dari `rootOverride != nil` di `assemble`, tanpa parameter baru, ditambah oleh yang lebih dulu memakainya di antara fitur 15 M3 dan fitur 16 M1 langkah 2 | K18 |
| Urutan file | milestone 3 fitur 22 dan langkah 2 milestone 1 fitur 16 berjalan setelah langkah 16 di merge | K16 SEQ 6, K18 SEQ 4 |

**Keputusan kecil yang diambil update ini.** `.observe` membawa revision event, yang hanya dipakai untuk id yang tidak dipegang: refetch butuh revision supaya aturan tombstone tetap menolak instance yang sudah dihapus (contoh K16 tanpa revision tidak bisa membedakannya). `instance.state_changed` dengan revision yang tidak lebih besar dilewati seluruhnya, karena daemon menaikkan revision di setiap batch yang mengirim event ini, sehingga summary lain pada revision yang sama tidak pernah membawa `statusSeq` yang lebih kecil. Snapshot mengganti item apa adanya tanpa `merge`, karena snapshot selalu lebih baru dari item yang dipegang sebelum snapshot itu dimulai.

**Belum diterapkan (dihentikan lead):** usulan fase 02 lain di inventaris `docs/scope/coordination/02-instalasi-pertama.usulan-fondasi.md` bagian *Spec 0013* (P1 sampai P13, P16, P17, P19, P20, dan butir gabungan 1 sampai 9, 11 sampai 13 yang bukan K16 sampai K18): banner dan seksi agent spec 0015 (AC-7, AC-9, `ConnectionHint`, aturan sumber), registrasi `CatalogStore` dan `RuntimeStore`, `ShellNavigator`, `ShellDestination`, `newInstanceAction` sebagai API shell, `ResourceStore.itemFailures` dan `retry`, `CapacityModel`, `ShellSync.events(kindPrefix:)` spec 0021, `ShellPresentation.status`, `.commands` ⌘N, dan pemecahan Follow-up. Semuanya additive dan masih tertulis di spec pemiliknya.

## Perubahan 2026-10-21 (update gabungan wave 3 ui-macos)

Update `/architect` sesi `arc-f1368-update` setelah verify penuh fitur 13 (`chk-f13-verify`, perbaikan `106cb49`) dan setelah fitur 67, 68, dan 69 dibangun. Prinsipnya: spec menggambarkan shell seperti yang sudah dibangun dan diputuskan. Butir yang sudah ada di kode ditulis seperti yang dibangun dengan rujukan ke spec pemiliknya; butir fase 02 yang belum dibangun ditulis sebagai batas atau catatan *Tambahan fase 02* dengan fitur dan langkah pemiliknya. Langkah Build plan 1 sampai 18 dan penomorannya tidak berubah, dan tidak ada langkah baru; ID AC tetap.

**Perubahan teks AC, disetujui user lewat lead (2026-10-21).** Empat teks AC diubah persis seperti usulan yang dikirim ke lead; satu usulan sengaja tidak mengubah AC.

| AC | Perubahan | Sumber |
| --- | --- | --- |
| AC-7 | "GUI tidak pernah menjalankan `stacklyd` atau binary lain" menjadi "sebagai prosesnya sendiri", dan meminta launchd menjalankan agent terdaftar lewat `SMAppService` (spec 0015) diizinkan; invariant yang sama di *Key invariants* diselaraskan | usulan fondasi butir 6 |
| AC-9 | banner untuk `ipc.endpoint_missing` dan `ipc.connection_refused` mengikuti tabel *Banner per state* spec 0015 mulai fitur 15 milestone 3; footer, gate, dan placeholder halaman tetap | usulan fondasi butir 7, konflik 1 |
| AC-14 | jaminan kolom konten 640 pt berlaku untuk halaman tanpa inspector terbuka; kolom halaman 384 pt (424 pt dengan inspector 240 pt) dengan inspector terbuka di jendela 900 pt | keputusan user 2026-10-21 atas temuan langkah 14 spec 0067 |
| AC-21 | pengumuman transisi ke tidak terjangkau untuk kedua kode itu memakai judul banner agent spec 0015 sebagai ganti "Can’t connect to stacklyd.", supaya satu transisi tetap satu pengumuman | usulan fondasi konflik 1 |
| AC-23 | tidak diubah: aturan `import ServiceManagement` dan API tulis file ditulis di *Aksesibilitas* sebagai milik spec 0015 (AC-11 dan *Instalasi CLI*), supaya tidak ada AC ganda | usulan fondasi butir 11 |

**Perubahan teks lain.**

| Butir | Perubahan | Sumber |
| --- | --- | --- |
| Batas dengan fitur lain | baris fitur 67, 68, 69 menyebut bagian yang mereka bangun di shell; baris baru untuk agent (spec 0015), `MainWindowRoot` dan onboarding (spec 0016), `ShellNavigator`, `ShellDestination`, `newInstanceAction` (spec 0022), dan `CatalogStore`, `RuntimeStore`, `CapacityModel`, `itemFailures`, `retry`, `ShellSync.events(kindPrefix:)`; urutan doctor fitur 34 di bawah seksi agent | usulan fondasi butir 1; spec 0067 baris :72; spec 0069 butir 1; catatan tambahan usulan fondasi |
| Peta package | `Components/`, `Accessibility/`, `Commands/`, `Fixtures/` (dengan `ShellPreviewSamples`), `AppShell+Commands.swift` sebagai yang dibangun; `Agent/` dan `Onboarding/` sebagai tambahan fase 02; `AppDelegate.makeRuntime()` dan `.commands` di target app | spec 0067 :87, spec 0068 (a) dan (d), spec 0069 butir 4 |
| Model data | `ShellStatus` dan `ShellPresentation` seperti di kode (`status`, `footer`, `banner`, `mutationAvailability`, `pageFailure`, `isReconnecting`); pengecualian `onboarding.finished` dan symlink CLI; `lastError` dihapus saat `.connected` | usulan fondasi butir 2, 3, P19; perbaikan verify `106cb49` |
| `ResourceStore` | `itemFailures`, `ItemFailure`, `retry(_:)` sebagai tambahan fitur 22; pengecualian retained tetap lewat closure `InstanceStore` | usulan fondasi butir 4, konflik 6 dan 7 |
| `ShellSync` dan API surface | `events(kindPrefix:)` milik fitur 21 | usulan spec 0021 untuk spec 0013 (Follow-up spec 0021, tercatat "belum diterapkan" di perubahan 2026-10-20) |
| Model shell | hook `mainWindowWillClose(isTerminating:screen:)` dan `systemRetry()` seperti di kode; `ShellBootstrap.fixture` (Debug); tambahan fase 02 untuk `onboardingStore`, `OnboardingModel`, `CommandLineToolModel`, `BackgroundAgentModel`, `CatalogStore`, `RuntimeStore`, `CapacityModel`, dan jadwal `refresh` | usulan fondasi butir 5; spec 0068 (a) |
| Tampilan status daemon | baris agent spec 0015 untuk dua kode itu dengan footer, gate, placeholder, dan kalimat waktu tetap; aksi banner baru `mutates: false`; aturan pengumuman | usulan fondasi butir 7 |
| Shell jendela | snippet scene seperti di kode (`.commands { StacklyCommands(runtime:) }`, `.keyboardShortcut` Show Main Window, `.fixtureWindow()`, `DesignGallery(secrets:)`); `FeedbackCenter` di `LiveShell`; `.feedbackPresenter()`, `.actionHost()` dan urutan modifier; `MainWindowKey` dan `track(_:)`; `AppShell.select(_:)` dan `onReselect`; perintah shell; `stacklyWindowWidth`; tooltip Settings; jalur fixture Debug; tambahan `MainWindowRoot` | spec 0067 *Shell jendela*; spec 0068 (a); spec 0069 butir 2 sampai 5; usulan fondasi butir 8, K18 |
| `.commands` | `StacklyCommands` satu satunya `.commands`, tanpa `CommandGroup` sementara; usulan `.commands` ⌘N bertanda `// provisional: F69` tidak dipakai | spec 0069 butir 4 menggantikan usulan fondasi butir 8 dan P20 |
| Diagnostics koneksi | Copy Details lewat `PlainPasteboard`; satu elemen aksesibilitas terkini per baris; petunjuk hanya untuk masalah yang masih berlaku; tambahan seksi agent dan Command Line serta petunjuk agent | spec 0067 *Diagnostics koneksi*; perbaikan verify `106cb49`; usulan fondasi butir 9 |
| Jalur mutasi | penunjuk ke `Feedback.outcome` spec 0067 | spec 0067 :399 |
| Asumsi lintas spec | penghapusan instance lewat `instance.removed` | usulan fondasi butir 12 |
| Cek manual | cek 1, 2, 5 (ukuran minimum), dan 8 juga dibuktikan spec 0068 AC-14; cek 1 mengikuti baris agent setelah fitur 15 milestone 3 | spec 0068 (c); usulan fondasi butir 7 |
| Build plan | blok *Tambahan dari spec 0067 sampai 0069 dan usulan fase 02* yang menunjuk langkah pemilik setiap tambahan, tanpa langkah baru | prinsip update ini |
| Consequences | bukti GUI manual kini dijawab spec 0068; kolom halaman 384 pt; urutan merge file shell bersama | spec 0068 (b); keputusan user 2026-10-21 |
| Follow-up | fitur 15 diberi jawaban spec 0015 (tanda tangan peer ke fitur 60 dan 63), baris halaman yang belum dibangun dipecah per fitur, baris `/sync` untuk `design.md` | usulan fondasi butir 13; spec 0069 butir 5 |

**Keputusan kecil yang diambil update ini.**

- Butir fase 02 yang belum dibangun ditulis di tempat yang akan diubah fiturnya, dengan penanda *Tambahan fase 02* dan langkah pemiliknya, bukan di satu bagian terpisah. Pembaca bagian itu langsung melihat apa yang akan berubah, dan teks lengkapnya tetap di spec pemilik. Runner up: satu bagian "Fase 02" di akhir desain, yang lebih mudah dihapus nanti tetapi terlewat saat membaca bagian yang terdampak.
- `itemFailures` dihapus lewat callback `didApply` yang sudah ada (`.changed`, `.reloaded`), tanpa hook baru, sesuai konflik 7 usulan fondasi.
- AC-23 tidak diubah: aturan sumber agent sudah punya AC di spec 0015 (AC-11), dan spec 0013 hanya menunjuknya.

**Ditunda atau tidak diterapkan.** Usulan fondasi butir 10 dan K16 sampai K18 sudah diterapkan 2026-10-20 (`99b9b2b`). Usulan `.commands` sementara (butir 8 bagian `.commands`, P20) digantikan spec 0069 butir 4. Tidak ada butir sumber lain yang ditunda.
