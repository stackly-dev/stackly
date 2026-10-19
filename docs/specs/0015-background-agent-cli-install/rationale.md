# 0015. Background agent dan instalasi CLI: rationale

Catatan keputusan untuk [index.md](index.md). `/develop` membaca index; file ini menyimpan alasannya.

## Context

> ⚠️ Premise note: scope meminta tiga state OS (`requiresApproval`, `disabled`, `notRegistered`) terlihat di GUI dan CLI, tetapi `SMAppService.Status` hanya punya `notRegistered`, `enabled`, `requiresApproval`, dan `notFound`, dan cek web tidak menemukan cara publik untuk membedakan "belum pernah diizinkan" dari "diizinkan lalu dimatikan". `disabled` karena itu harus diturunkan dari bukti milik Stackly sendiri, dan spec ini memilih launch record (Keputusan 5). Scope juga menagih "helper ditandatangani dengan identity development nyata", padahal Mac ini punya 0 signing identity (OQ-012); spec ini lengkap, tetapi Done when baru bisa ditutup setelah identity ada, dan bagian itu dikumpulkan di milestone 4 sebagai `BLOCKED`. Personal Team dari Apple ID gratis kemungkinan cukup untuk sertifikat Apple Development lokal; itu keputusan release owner, bukan spec ini.

Sampai fitur 6, `stacklyd` hanya bisa dijalankan dengan tangan, dan CLI serta GUI menulis petunjuk "start it with stacklyd" saat socket tidak ada (spec 0006, 0008, 0013, ditandai `provisional: F15`). Produk yang dijanjikan PRD (bagian 4.1, 25) adalah aplikasi `.app` yang membawa daemon dan CLI, dengan daemon berjalan di latar per user sejak login, persetujuan background lewat mekanisme macOS, dan state persetujuan yang terlihat. PRD menjadikan signing dan packaging release blocker (RISK-10) dan meminta bundling, signing development, serta registrasi helper dibuktikan di fase 02, bukan saat rilis (scope index, catatan urutan).

Aturan arsitektur yang membatasi desain: hanya daemon yang menjalankan proses (BR-011); CLI tidak pernah menjalankan proses dan `check-deps` menolak `std::process` di `apps/cli` (spec 0008 AC-11); GUI tidak menautkan Rust dan tidak menjalankan proses (spec 0013); `unsafe` hanya di `stackly-platform-macos`; port platform baru masuk lewat `with_<port>` (spec 0004); proses instance berjalan di session sendiri dan diadopsi setelah daemon restart (spec 0010); secret ada di login Keychain yang ACL nya mempercayai signature `stacklyd` (spec 0011). Daemon punya urutan berhenti yang teruji untuk SIGTERM (spec 0006) dan lock satu per root.

Faktor lingkungan: macOS 27 di Apple Silicon, Xcode 27, target app sudah `ENABLE_HARDENED_RUNTIME = YES` dan `CODE_SIGN_IDENTITY = "-"`, dan `cargo xtask build-app` hari ini hanya memanggil `xcodebuild`. Banyak perilaku Background Task Management (BTM, layanan macOS yang mencatat item latar dan persetujuannya) tidak terdokumentasi rinci; cek web 2026-10-19 (di bawah) mengonfirmasi sebagian dan meninggalkan sisanya untuk test native.

Kalau tidak diputuskan: fitur 16 (onboarding) tidak punya state izin untuk ditampilkan, fitur 17 tidak tahu di mana catalog bawaan berada, fitur 31 tidak punya mekanisme autostart daemon, dan masalah signing baru muncul di fitur 60.

## Options considered

Setiap keputusan di bawah menandai satu opsi sebagai rekomendasi. Program wave ini mengambil opsi yang direkomendasikan (log koordinasi `02-instalasi-pertama`, 2026-10-19).

### Keputusan 1: mekanisme helper (OQ-006)

#### Option 1: `SMAppService.agent` dengan LaunchAgent di dalam bundle (rekomendasi)

Plist di `Contents/Library/LaunchAgents/`, didaftarkan dengan `SMAppService.agent(plistName:)`, persetujuan lewat Login Items.

**Pros**:
- Mekanisme yang Apple sediakan untuk helper per user sejak macOS 13, dengan status dan halaman persetujuan yang bisa dibuka dari app (PRD bagian 25).
- Plist ikut bundle dan ikut ditandatangani; tidak ada file yang ditulis ke `~/Library/LaunchAgents`.
- `BundleProgram` membuat path helper relatif ke bundle, jadi app bisa dipindah.

**Cons**:
- Perilaku rinci (status setelah izin dimatikan, daftar ulang, syarat signing) kurang terdokumentasi dan harus dibuktikan native.
- Butuh identity signing nyata untuk bukti akhir.

#### Option 2: `SMAppService.loginItem` dengan helper app di `Contents/Library/LoginItems`

**Pros**:
- Login item adalah app biasa dengan Info.plist sendiri.

**Cons**:
- Daemon bukan app; membungkusnya sebagai app menambah bundle kedua, Info.plist, dan aturan activation policy tanpa manfaat.
- Tidak ada `KeepAlive` launchd untuk crash.

#### Option 3: plist LaunchAgent di `~/Library/LaunchAgents` dan `launchctl bootstrap`

**Pros**:
- Pola yang sudah dipakai spike spec 0003, perilaku launchd terdokumentasi.

**Cons**:
- Menulis file di luar bundle dan menjalankan `launchctl` dari produk, melanggar aturan proses CLI dan GUI.
- Tidak ada status persetujuan `SMAppService`; BTM tetap memberi notifikasi tanpa API status yang rapi.
- Plist tertinggal saat app dihapus.

#### Option 4: LaunchDaemon atau helper berhak root

**Pros**:
- Jalan sebelum login.

**Cons**:
- Di luar batas BR-012 (satu akun user) dan REQ-014 ("login bukan boot"), butuh admin.

### Keputusan 2: letak binary di bundle

#### Option 1: `Contents/MacOS` untuk `stacklyd` dan `stackly` (rekomendasi)

**Pros**:
- Contoh Apple untuk `BundleProgram` memakai `Contents/MacOS`.
- Untuk executable di `Contents/MacOS`, bundle utama proses kemungkinan besar adalah app itu sendiri, yang dibutuhkan `SMAppService` saat dipanggil CLI (belum terkonfirmasi sumber resmi; dibuktikan test native milestone 4, dengan jalur gagal `cli.agent_not_bundled`).
- Signing nested code paling sederhana.

**Cons**:
- `stacklyd` dan `stackly` terlihat di samping executable app; pengguna yang membuka bundle bisa menjalankannya langsung (tidak berbahaya, daemon tetap satu per root lewat lock).

#### Option 2: `Contents/Helpers`

**Pros**:
- Memisahkan helper dari executable utama.

**Cons**:
- Tidak terkonfirmasi untuk `BundleProgram`, dan bundle utama untuk executable di sana tidak pasti.

#### Option 3: helper dibungkus app di `Contents/Library/LoginItems`

**Cons**:
- Sama dengan Keputusan 1 Option 2.

### Keputusan 3: cara start, stop, dan restart daemon

#### Option 1: stop dan restart lewat IPC, `KeepAlive` yang hanya menyalakan ulang exit gagal, start lewat daftar ulang `SMAppService` (rekomendasi)

`daemon.shutdown stop` keluar 0 dan launchd membiarkannya mati; `restart` keluar 75 dan launchd menyalakannya lagi; start untuk job yang terdaftar tetapi tidak jalan memanggil `relaunch` di port, yang di macOS berarti `unregister` lalu `register`, sehingga `RunAtLoad` menyalakannya. Karena mekanismenya di belakang port, adapter bisa diganti kalau spike menunjukkan daftar ulang meminta persetujuan baru.

**Pros**:
- Tidak ada proses yang dijalankan CLI atau GUI; aturan spec 0001, 0008, dan 0013 utuh.
- Stop memakai urutan berhenti spec 0006 yang sudah teruji.
- Satu mekanisme start untuk CLI dan GUI.

**Cons**:
- Daftar ulang sebagai start belum terkonfirmasi (bisa memunculkan notifikasi lagi atau meminta persetujuan baru).
- Restart hanya mungkin untuk daemon di bawah launchd.

#### Option 2: `launchctl kickstart` dan `kill` dari adapter atau modul yang diizinkan

**Pros**:
- Pola launchd yang umum dan cepat.

**Cons**:
- Menjalankan proses dari CLI (butuh allowlist `check-deps`) dan dari GUI (butuh pengecualian aturan spec 0013).
- Apakah `kickstart` didukung untuk job `SMAppService` tidak terkonfirmasi.

#### Option 3: `MachServices` untuk menyalakan daemon sesuai permintaan

**Pros**:
- Cara launchd yang resmi untuk launch on demand, tanpa proses tambahan.

**Cons**:
- Daemon harus check in ke layanan Mach lewat XPC (FFI dengan block di Rust), CLI dan GUI butuh klien XPC, dan pesan yang tidak diterima membuat launchd menyalakan daemon lagi setelah stop.
- Jalur kode baru di tiga tempat hanya untuk menyalakan.

#### Option 4: `KeepAlive` true, stop berarti `unregister`

**Pros**:
- Paling sederhana.

**Cons**:
- Stop mengubah persetujuan dan pendaftaran, sesuatu yang PRD minta tidak terjadi tanpa sepengetahuan pengguna, dan menyalakan lagi bisa butuh persetujuan baru.

### Keputusan 4: siapa memanggil `SMAppService`

#### Option 1: GUI di Swift dan CLI lewat port `BackgroundAgent` dengan `objc2-service-management` (rekomendasi)

**Pros**:
- `stackly daemon install` dan `start` bekerja tanpa GUI, sesuai REQ-028.
- Port mengikuti pola spec 0004, dengan fake untuk test dan Windows nanti.

**Cons**:
- Dependency `objc2` dan blok `unsafe` baru.
- `SMAppService` dari CLI (terutama lewat symlink) belum terkonfirmasi; jalur gagalnya `cli.agent_not_bundled` dengan path yang benar.

#### Option 2: hanya GUI; CLI membuka app lewat URL

**Pros**:
- Satu tempat memanggil OS.

**Cons**:
- CLI butuh LaunchServices dan GUI yang terbuka; tidak jalan lewat SSH dan tidak bisa dipakai script.

#### Option 3: hanya CLI; GUI menjalankan CLI

**Cons**:
- GUI menjalankan proses, melanggar spec 0013.

### Keputusan 5: state `disabled`

#### Option 1: turunkan dari launch record yang ditulis daemon di bawah launchd (rekomendasi)

**Pros**:
- Bukti nyata bahwa izin pernah diberikan, tanpa API pribadi.
- Satu aturan untuk CLI dan GUI, dijaga fixture bersama.

**Cons**:
- Pendaftaran baru setelah pernah diizinkan bisa tampil `disabled`; langkah pemulihannya sama, jadi salah label tidak berbahaya.
- Satu file baru di root data.

#### Option 2: petakan semua `requiresApproval` ke `requires_approval`

**Pros**:
- Jujur pada status OS saja.

**Cons**:
- `disabled` yang diminta scope dan PRD tidak pernah terlihat.

#### Option 3: baca `launchctl print-disabled` atau database BTM

**Cons**:
- Menjalankan proses atau butuh admin, dan formatnya bukan API.

#### Option 4: simpan "pernah aktif" di `UserDefaults` GUI

**Cons**:
- CLI tidak bisa membacanya, jadi GUI dan CLI berbeda.

### Keputusan 6: assembly dan signing

#### Option 1: `cargo xtask build-app` merakit setelah `xcodebuild` tanpa signing, lalu menandatangani dari dalam ke luar (rekomendasi)

**Pros**:
- Satu pintu, urutan deterministik, identity dipilih di satu tempat.
- Script sandboxing Xcode tetap menyala, karena tidak ada Run Script phase.

**Cons**:
- Build dari Xcode IDE tidak punya helper; GUI menampilkannya sebagai `helper_missing`.

#### Option 2: Run Script phase di Xcode yang menyalin dan menandatangani helper

**Pros**:
- Build dari Xcode IDE lengkap.

**Cons**:
- Script sandboxing harus dimatikan atau input dideklarasikan satu per satu, dan cargo di dalam Xcode lambat.

#### Option 3: Copy Files phase dengan Code Sign On Copy

**Cons**:
- Identifier, hardened runtime, dan plist yang dirender tetap butuh langkah lain; binary harus sudah ada sebelum Xcode jalan.

### Keputusan 7: instalasi CLI

#### Option 1: symlink terkelola di `~/.local/bin` dan `~/.local/share/man/man1` (rekomendasi)

**Pros**:
- Tanpa admin, tanpa mengubah PATH atau file shell (PRD bagian 25).
- `man stackly` bekerja otomatis kalau `~/.local/bin` di PATH.

**Cons**:
- `~/.local/bin` tidak ada di PATH default macOS; pengguna menambahkan satu baris sendiri.

#### Option 2: symlink di `/usr/local/bin` dengan password admin (mockup)

**Pros**:
- `/usr/local/bin` sudah di PATH default.

**Cons**:
- Butuh hak admin dan perubahan global untuk semua user, dan GUI harus menjalankan proses berhak lebih tinggi.

#### Option 3: menambah baris PATH ke file shell dengan persetujuan

**Cons**:
- Mengubah file milik pengguna yang berbeda per shell, rapuh, dan mudah merusak setup orang.

#### Option 4: `/etc/paths.d/stackly`

**Cons**:
- Perubahan PATH global dengan admin, dan ikut membuka `stacklyd` di PATH.

### Keputusan 8: Keychain setelah signing (spec 0011)

#### Option 1: tetap login Keychain; persetujuan sekali per item saat identity berubah (rekomendasi)

**Pros**:
- Jalur kode spec 0011 tidak berubah.
- TN3137 menyebut program di luar konteks user seperti daemon launchd harus memakai keychain berbasis file.

**Cons**:
- Satu dialog per item di setiap perubahan identity (ad hoc ke Apple Development, lalu ke Developer ID).

#### Option 2: pindah ke data protection keychain

**Pros**:
- Akses lewat access group, tanpa dialog ACL.

**Cons**:
- Butuh entitlement access group dengan provisioning profile dan bungkus seperti app untuk tool tanpa bundle, Team ID berbayar untuk distribusi, dan migrasi item; ketersediaan untuk LaunchAgent belum pasti.

### Keputusan 9: verifikasi tanda tangan peer (spec 0006 dan 0013)

#### Option 1: tunda ke fitur 60 dengan review fitur 63 (rekomendasi)

**Pros**:
- Tanpa Team ID, requirement hanya cdhash yang berubah setiap build; verifikasi sekarang tidak melindungi apa pun.
- Batas BR-012 tetap batas yang diklaim.

**Cons**:
- Proses lain milik user yang sama bisa menyamar sebagai daemon sampai fitur 60.

#### Option 2: bangun sekarang dengan mode lunak untuk build ad hoc

**Cons**:
- Blok `unsafe` baru di Rust dan Swift untuk pemeriksaan yang hampir selalu dilewati, dan laporan error intermiten `SecCode` dari audit token di cek web.

### Keputusan 10: bentuk `daemon status --json` saat daemon tidak jalan

#### Option 1: `result` dengan `info`, `health`, `client` nullable plus `agent`, dan `CLI_SCHEMA_VERSION` 2 (rekomendasi)

**Pros**:
- State agent terlihat di `--json` tepat saat paling dibutuhkan (daemon mati), sesuai Done when.
- Mengikuti aturan stabilitas spec 0008 apa adanya.

**Cons**:
- Golden file semua perintah ikut berubah karena versi schema naik.

#### Option 2: hanya teks manusia yang menampilkan agent saat daemon mati

**Cons**:
- Script tidak bisa membaca state izin; REQ-028 meminta paritas.

#### Option 3: perintah terpisah `stackly daemon agent`

**Cons**:
- Di luar daftar perintah PRD bagian 12 dan memecah satu pertanyaan ("kenapa daemon tidak jalan") ke dua perintah.

### Keputusan 11: saat persetujuan belum ada

#### Option 1: tidak ada daemon cadangan; tampilkan langkah pemulihan (rekomendasi)

**Pros**:
- Jujur dan sesuai aturan GUI tidak menjalankan proses.

**Cons**:
- Tanpa persetujuan, Stackly tidak bisa mengelola instance sama sekali.

#### Option 2: GUI menjalankan daemon sebagai child selama app terbuka (teks mockup)

**Cons**:
- Melanggar spec 0013 dan BR-011, membuat supervisor kedua yang mati saat app ditutup, dan menyembunyikan state izin.

### Keputusan 12: completion dan man page

#### Option 1: `stackly completion` ke stdout, `stackly manpage` tersembunyi yang dipakai `build-app`, halaman di bundle dan ditautkan oleh instalasi CLI (rekomendasi)

**Pros**:
- CLI tetap bin saja (spec 0008) dan tidak menulis file.
- Halaman man selalu cocok dengan binary di bundle yang sama.

**Cons**:
- Satu perintah tersembunyi tambahan.

#### Option 2: `build.rs` dengan `clap_mangen`

**Cons**:
- Hasil di `OUT_DIR` sulit dicari xtask secara andal.

#### Option 3: crate lib untuk definisi clap

**Cons**:
- Mengubah keputusan "bin saja" spec 0008.

## Rationale

Inti keputusan adalah memisahkan dua hal yang mudah tercampur: persetujuan background (milik pengguna dan macOS) dan kendali harian daemon (milik Stackly). Persetujuan hanya disentuh lewat `SMAppService` saat pengguna bertindak, sehingga REQ-014 dan PRD bagian 25 ("tidak diubah tanpa sepengetahuan pengguna") terpenuhi dengan aturan yang bisa diuji: `register` dan `unregister` hanya dari dua aksi CLI dan dua tombol GUI. Kendali harian lewat IPC yang sudah ada di spec 0006, sehingga stop dan restart memakai urutan berhenti yang sudah teruji dan tidak ada proses yang dijalankan CLI atau GUI. `KeepAlive { SuccessfulExit: false }` adalah lem yang membuat keduanya cocok: exit 0 berarti tetap mati, exit 75 berarti nyalakan lagi, dan crash dinyalakan lagi otomatis.

Option yang ditolak di Keputusan 3 bukan karena salah secara teknis, tetapi karena biaya arsitekturnya: `launchctl` memaksa pengecualian di dua aturan "tidak menjalankan proses", dan `MachServices` menambah XPC di tiga tempat hanya untuk menyalakan. Risiko dari pilihan ini (daftar ulang belum terkonfirmasi sebagai start) ditangani dengan test native dan jalur kembali ke `/architect`, bukan dengan menebak. Hal yang sama berlaku untuk semua fakta OS yang belum terkonfirmasi: spec menyebutnya sebagai hipotesis yang dibuktikan AC native, dan AC ditulis supaya kegagalan OS tetap menghasilkan tampilan yang jujur.

Signing dikumpulkan di satu pintu (`cargo xtask build-app`) karena fitur 60 akan menambah Developer ID dan notarization di tempat yang sama; dengan begitu satu satunya hal yang menunggu akun adalah bukti, bukan kode. Untuk Keychain, tetap di login Keychain adalah pilihan yang paling sedikit berubah dan sesuai TN3137 untuk proses launchd; biaya satu dialog per item per perubahan identity diterima dan dicatat untuk fitur 60.

## Keputusan kecil

Rekomendasi saya, diambil tanpa ditanyakan karena tidak mengubah arah fitur; runner up dalam kurung.

- **Identifier sementara** turunan `com.ojiepermana.stackly`: label dan identifier `stacklyd` `com.ojiepermana.stackly.daemon`, CLI `com.ojiepermana.stackly.cli` (runner up: identifier signing sama dengan nama file binary).
- **`ProcessType` `Standard`**: proses instance (database) turun dari daemon dan tidak boleh dibatasi seperti tugas latar (runner up: tidak diset, yang sama dengan `Standard`, tetapi kurang eksplisit).
- **`ExitTimeOut` 30 detik**: lebih panjang dari urutan berhenti spec 0006 (runner up: default 20 detik).
- **Exit 75 untuk restart**: `EX_TEMPFAIL` dari `sysexits`, tidak bentrok dengan 0 sampai 3 (runner up: exit 1, yang tidak bisa dibedakan dari gagal start).
- **Batas tunggu CLI**: start dan restart 30 detik (dua `ThrottleInterval` ditambah start), stop 20 detik, panggilan OS 10 detik, polling 250 ms (runner up: 15 detik untuk start, yang bisa gagal saat daemon lama masih memegang lock).
- **GUI**: ambang 15 detik sebelum Start stacklyd muncul (di atas rebind socket 12 detik spec 0006 dan `ThrottleInterval`), polling status 5 detik selama tidak terjangkau (runner up: hanya saat app aktif kembali).
- **Folder instalasi development** `~/Applications` (runner up: `/Applications`, yang butuh admin di banyak Mac).
- **`MARKETING_VERSION` dari versi workspace** supaya versi app, daemon, dan CLI tidak pernah berbeda di satu build (runner up: diatur tangan di Xcode).
- **Konteks launchd** dari `XPC_SERVICE_NAME` yang sama dengan label plus parent PID 1, bukan salah satunya, karena proses Terminal mewarisi `XPC_SERVICE_NAME` milik app Terminal (runner up: parent PID 1 saja).
- **Launch record** ditulis setelah lock dan sebelum bind socket, gagal tidak fatal (runner up: setelah daemon `ready`, yang melewatkan crash saat start).
- **Kode error agent di area `cli`** (`cli.agent_*`), bukan area baru, supaya tabel area spec 0006 dan aturan spec 0008 "CLI hanya membuat error di area `cli`" tidak berubah; GUI memakai enum Swift `AgentActionError` karena ia tidak menampilkan kode (runner up: area `agent` baru yang dipakai CLI dan GUI).
- **`stop` menunggu EOF di koneksi yang mengirim `daemon.shutdown`**, karena daemon menutup koneksi tepat sebelum melepas lock; CLI tidak perlu membuka lock file daemon (runner up: membuka lock file read only dan mencoba `flock` shared).
- **Tunggu 15 detik sebelum `relaunch`** kalau launch record menunjukkan launch kurang dari 15 detik yang lalu, supaya daemon yang sedang start atau menunggu `ThrottleInterval` tidak dihentikan (runner up: selalu `relaunch` langsung).
- **Runtime tokio CLI dimatikan dengan `shutdown_timeout(Duration::ZERO)`**, karena panggilan OS yang macet di `spawn_blocking` tidak bisa dibatalkan (runner up: memanggil port di thread terpisah yang di detach).
- **CLI tidak pernah membuka System Settings**; hanya GUI yang membukanya, dan hanya setelah klik (runner up: flag `--open-settings`).
- **`daemon uninstall` tidak dibuat di fitur ini**; mematikan background service milik fitur 31 dan uninstall milik fitur 62 (runner up: `stackly daemon uninstall` sebagai cermin `install`).
- **Agent Skill dan MCP**: pencarian ditunda dan dicatat di Follow-up, karena sub sesi ini tidak boleh memasang skill di repo; tool baru fitur ini hanya crate kecil (`objc2*`, `clap_complete`, `clap_mangen`) dan skill `signing-entitlements` sudah terpasang (runner up: cari sekarang lewat lead).

## Cek web 2026-10-19

Satu pemeriksaan web dengan batas (6 pencarian, 9 halaman) dijalankan model murah untuk fakta macOS yang menentukan desain. Ringkasan dan tingkat keyakinan:

| Pertanyaan | Hasil | Keyakinan |
| --- | --- | --- |
| Arti `SMAppService.Status` | `notRegistered`, `enabled`, `requiresApproval`, `notFound` seperti di index; `notRegistered` juga mencakup percobaan daftar ulang setelah terdaftar | tinggi |
| Status setelah izin dimatikan, membedakan dari belum pernah diizinkan | tidak terkonfirmasi; tidak ada cara publik | rendah |
| `register` saat sudah terdaftar, efek `unregister` pada job yang jalan, persetujuan ulang setelah daftar ulang | tidak terkonfirmasi | rendah |
| `BundleProgram` relatif bundle, hanya untuk plist `SMAppService`; `AssociatedBundleIdentifiers` mengaitkan job dengan app di Login Items | terkonfirmasi | tinggi |
| Lokasi plist `Contents/Library/LaunchAgents/` | dari wrapper pihak ketiga, bukan Apple | sedang |
| Syarat signing (ad hoc, Team ID, lokasi app, hardened runtime) | tidak terkonfirmasi; satu laporan forum menyebut signing dibutuhkan | rendah |
| `SMAppService` dari CLI di `Contents/MacOS`, termasuk lewat symlink | tidak terkonfirmasi | rendah |
| `AbandonProcessGroup`, `KeepAlive.SuccessfulExit`, `ThrottleInterval` 10, nilai `ProcessType` | terkonfirmasi lewat man page `launchd.plist(5)` | tinggi |
| launchd membunuh coalition, bukan hanya process group | tidak terkonfirmasi | rendah |
| Keychain berbasis file untuk program di luar konteks user; data protection keychain butuh access group dan profile | terkonfirmasi (TN3137) | tinggi |
| Dialog ACL saat identity berubah | tidak terkonfirmasi | rendah |
| `SecCode` dari audit token | pola diketahui; ada laporan gagal intermiten | rendah |
| `sfltool resetbtm` dan `dumpbtm` | tidak terkonfirmasi; tidak dipakai spec ini | rendah |

Setiap baris berkeyakinan rendah yang dipakai desain punya AC native (AC-15 sampai AC-19) yang membuktikan atau membantahnya.

## Cross check

Spec dibaca model lain (Sonnet 5.5, baca saja, tanpa membuka link) pada 2026-10-19. Putusan: desain koheren dan aturan proyek tetap utuh (tidak ada proses yang dijalankan CLI atau GUI, urutan berhenti spec 0006, aturan stabilitas spec 0008), dengan 13 gap kelengkapan dan beberapa catatan soundness. Semua usulan perbaikan diterima lead atas nama user sesuai aturan program (opsi yang direkomendasikan diambil) dan diterapkan:

| No | Temuan | Perbaikan yang diterapkan |
| --- | --- | --- |
| 1 | `stop` selesai sebelum lock dilepas, jadi `start` langsung sesudahnya bentrok | `stop` menunggu EOF di koneksi shutdown lalu socket hilang (AC-10) |
| 2 | `daemon.shutting_down` untuk request kedua menjadi exit 6 | dianggap "sudah sedang berhenti" (AC-10) |
| 3 | kode `NSError` `SMAppService` tidak dipetakan | tabel *Hasil pendaftaran*, kode `cli.agent_invalid_signature` |
| 4 | AC-17 mengandaikan `register` selalu `requires_approval` | AC-17 bercabang A (langsung `enabled`) atau B |
| 5 | langkah pemulihan tidak ada di `daemon status` | `AgentReport.recovery`, baris `Next`, satu tabel *Langkah pemulihan CLI* |
| 6 | hasil `--json` saat gagal tidak terdefinisi | hasil parsial dengan `info` null untuk `install`, `start`, `stop`, `restart` |
| 7 | timeout Start di GUI tanpa kode | `AgentActionError` di GUI; kode agent CLI pindah ke area `cli`, tanpa area baru |
| 8 | plumbing exit 75 tidak bernama | `StartConfig`, `DaemonDeps.launch` dan `bundle`, `DaemonHandle::stop_requested()` |
| 9 | debug dylib Xcode di `Contents/MacOS`, path cargo, sisa staging | `ENABLE_DEBUG_DYLIB=NO`, artefak dari `--message-format=json`, hapus `.stage-*` |
| 10 | LaunchAgent test tidak membuktikan key plist produksi | plist test diturunkan dari golden file dan dibandingkan |
| 11 | perintah pembersihan agent test ambigu | langkah tambahan di `runtime spike --cleanup` lewat `launchctl list`, tanpa lock spike |
| 12 | bundle `NSBundle` bisa berbeda dari bundle CLI (symlink) | `Registration.bundle`; CLI menurunkannya ke `not_bundled` |
| 13 | area `agent` bentrok dengan spec 0006 dan 0008 | dihapus (lihat nomor 7) |
| S1 | fakta OS paling berisiko diuji paling akhir | spike AC-21 sebagai langkah 0; start di belakang `relaunch` |
| S2 | `start` bisa menghentikan daemon yang sedang start | tunggu 15 detik kalau launch record baru |
| S3 | panggilan OS macet menahan keluarnya CLI | `shutdown_timeout(Duration::ZERO)`; GUI membuang hasil terlambat |
| S5 | AC yang mencampur bagian bisa jalan dan `BLOCKED` | AC-2 ad hoc saja, AC-20 Team ID, AC-21 spike, AC-19 dengan reason dan owner akun kedua, AC-22 untuk `--install` |
| S6 | teks `disabled` bisa salah setelah reinstall | teks banner netral |
| S7 | `--install` tidak punya AC; nama milestone bentrok | AC-22; prasyarat fitur 13 ditulis dengan nama milestone nya |

Catatan lead yang ikut diterapkan: langkah yang mendaftarkan job atau agent di Mac ini, mengubah Login Items, atau memakai akun macOS kedua ditandai **setelah persetujuan** dan tidak dijalankan tanpa pengawasan; perubahan teks spec 0006 dan 0008 ditulis sebagai *Usulan update untuk spec lain*, bukan edit langsung; dan update spec 0004 tetap additive.

## Perubahan 2026-10-20

**Konsistensi lintas spec (keputusan lead).** Langkah catalog di `cargo xtask build-app` dimiliki milestone 4 langkah 11 spec 0017; spec ini hanya menyediakan slot nya, supaya milestone 1 spec ini tidak menunggu spec 0017. Bagian catalog AC-1 dibuktikan di sana. Baris `daemon.restart_unavailable` untuk tabel kode area `daemon` spec 0006 ditambahkan ke *Usulan update untuk spec lain*.

**Tambahan dari spec 0016 (onboarding).** Spec 0016 mengusulkan tambahan berikut di bagian *Usulan untuk spec lain* nya, dan semuanya diterapkan di sini secara additive tanpa mengubah ID AC:

- `BackgroundAgentModel.unavailableDetail: String?`, satu sumber `{detail}` untuk banner `unavailable` lain dan kartu Background Agent onboarding.
- `CommandLineToolModel` (`@MainActor @Observable`, dimiliki `ShellRuntime`, `refresh()` saat app aktif kembali), dipakai bersama `CommandLineToolSection` dan langkah Command Line onboarding.
- `CommandLineTool.bundledTool: URL?`; nil hanya menonaktifkan Install dan Repair, Remove tetap bisa.
- `UserHome.current()` di `StacklyUI/Agent/` sebagai satu helper home, dengan aturan batas komponen untuk `~`.
- `CommandLineToolPrompts` dengan teks spec 0016, dipakai Diagnostics dan onboarding.
- Penanda `CommandLineToolSection` menjadi `// provisional: F32` saja, karena layar onboarding sudah dirancang spec 0016.

Alasannya sama dengan spec 0016: dua layar (Diagnostics dan onboarding) menampilkan state yang sama, jadi state, teks, dan helper nya cukup satu, dan aturan AC-11 (pendaftaran hanya lewat aksi pengguna) tetap dijaga oleh model milik spec ini.

## References

**Project sources**:

- `prd.md`: bagian 4.1, 12, 25, 32.1, 32.2, 34 (G0, G5), 37 (RISK-10), 38 (OQ-001, OQ-006, OQ-012); REQ-001, REQ-013, REQ-014, REQ-016, REQ-027, REQ-028, REQ-047, BR-011, BR-012, NFR-010, NFR-015, AC-013, AC-014, AC-062.
- `docs/scope/02-instalasi-pertama.md` fitur 15, `docs/scope/index.md` (catatan urutan), `docs/scope/04-kerja-harian.md` fitur 30 sampai 32, `docs/scope/11-distribusi-aplikasi.md` fitur 60 sampai 62, `docs/scope/ui-macos.md`, dan mockup UI (langkah onboarding Background Agent dan Command Line).
- Spec 0001 (stack, bundle ID sementara, Batas keputusan), 0002 (evidence, test native, `BLOCKED`), 0003 (mekanisme LaunchAgent spike), 0004 (port `with_<port>`, capability, layout), 0006 (urutan start dan berhenti, lock, exit code, hint), 0008 (kontrak `--json`, AC-11, aturan stabilitas), 0009 (namespace `testing/`), 0010 (session sendiri, survival), 0011 (login Keychain, signing ad hoc), 0013 (presence, banner, Diagnostics, `ConnectionHint`), dan 0017 (catalog di bundle, lewat pesan antarsesi).
- Kode: `xtask/src/build_app.rs`, `apps/daemon/src/main.rs` dan `startup.rs`, `apps/cli/src/daemon_status.rs`, `crates/platform/src/capability.rs`, `crates/platform-macos/src/info.rs`, `apps/macos/Stackly.xcodeproj/project.pbxproj`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Diagnostics/ConnectionHint.swift`.
- `AGENTS.md` root dan `apps/macos/AGENTS.md`; skill terpasang `signing-entitlements`, `rust-best-practices`, `rust-async-patterns`, `swiftui-expert-skill`, `swift-concurrency`, `swift-testing-pro`.

**Practices & standards**:

- Signing nested code dari dalam ke luar, tanpa `--deep` saat menandatangani.
- Persetujuan pengguna untuk item latar dikelola OS; aplikasi hanya membaca status dan mengarahkan pengguna.
- `sysexits` `EX_TEMPFAIL` (75) untuk "coba lagi", dipakai sebagai tanda restart.
- Instalasi tool per user di `~/.local/bin` tanpa hak admin.

**Links** (terverifikasi saat cek web 2026-10-19):

- SMAppService.Status: https://developer.apple.com/documentation/servicemanagement/smappservice/status-swift.enum.md
- SMAppService: https://developer.apple.com/documentation/servicemanagement/smappservice.md
- TN3137, On Mac keychain APIs and implementations: https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains.md
- Apple Developer Forums, SMAppService dan signing: https://developer.apple.com/forums/thread/777520
- Apple Developer Forums, `BundleProgram`: https://developer.apple.com/forums/thread/779379
- `launchd.plist(5)` (mirror, bukan situs Apple): https://keith.github.io/xcode-man-pages/launchd.plist.5.html
