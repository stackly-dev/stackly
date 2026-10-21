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

**Perbaikan pasca penyelarasan (2026-10-20).** Dari `docs/scope/coordination/post-alignment/post-0015.md`: `classify` di GUI membaca `DaemonModel.rootOverrideActive` (keputusan K18: Bool baca saja dari `ShellBootstrap.assemble`, tanpa parameter baru, dan dibuat oleh langkah mana pun yang lebih dulu memakainya, langkah 10 spec ini atau langkah 2 spec 0016), dan AC-13 serta langkah 11 memakai cek teks besar spec 0012 (lebar sempit 640 pt) alih alih Dynamic Type, karena text style macOS 27 tidak ikut `dynamicTypeSize`.

## Hasil spike AC-21 (2026-10-20)

Langkah 0 dijalankan di Mac ini (macOS 27.0 build 26A428) setelah persetujuan kamu lewat lead. Bundle sementara dirakit tangan di folder scratch sesi, bukan hasil `xcodebuild`: `StacklyTestAgentSpike.app` (bundle id `dev.stackly.test.agent.spikeapp`), tool kedua `Contents/MacOS/spikectl` yang memanggil `SMAppService.agent(plistName:)`, agent test `Contents/MacOS/spike-agent`, dan `Contents/Library/LaunchAgents/dev.stackly.test.agent.spike.plist` dengan key plist produksi spec ini. Identifier test dipakai supaya pendaftaran `com.ojiepermana.stackly` tidak tersentuh. Semuanya ditandatangani ad hoc dengan hardened runtime, dari dalam ke luar. Record manual `native` nya adalah [`2026-10-20-spec-0015-spike-ad-hoc-smappservice-agent-on-thi.json`](../../../evidence/manual/2026-10-20-spec-0015-spike-ad-hoc-smappservice-agent-on-thi.json), berstatus `FAIL` karena empat asumsi dibantah, dengan log lengkap dan kode spike di `evidence/manual/files/01a15762-d108-760d-9ace-c2c5809a8ef8/`. Pendaftaran sudah dicabut, tidak ada job `dev.stackly.test.agent.*` tersisa, dan bundle sudah dihapus.

| Pertanyaan AC-21 | Hasil (ad hoc) | Sesuai spec |
| --- | --- | --- |
| Apakah build ad hoc bisa didaftarkan, dan statusnya | Bisa. `register` dari `Contents/MacOS/spikectl` berhasil dalam 141 ms, status langsung `enabled` tanpa prompt persetujuan, dan job start 0,5 detik kemudian. Sebelum pendaftaran pertama status nya `notFound` (3), bukan `notRegistered` | Tidak, lihat B1 |
| Status dari tool di `Contents/MacOS` | Sama dengan executable utama; `NSBundle.mainBundle` adalah bundle app dengan identifier nya | Ya |
| Status lewat symlink di luar bundle | `SMAppService` menjawab untuk bundle asli (`enabled`, dan `relaunch` lewat symlink menyalakan job dari bundle asli), tetapi `NSBundle.mainBundle` adalah folder symlink dan `bundleIdentifier` nil | Tidak, lihat B2 |
| `unregister` lalu `register` menyalakan job tanpa persetujuan baru | Ya, di setiap kasus yang dicoba (job jalan, sudah keluar 0, baru start kurang dari 10 detik, lewat symlink, di thread latar, dan saat switch Login Items mati): proses baru 0,08 sampai 1,0 detik kemudian, tanpa throttle | Ya |
| `XPC_SERVICE_NAME` sama dengan label | Ya, `dev.stackly.test.agent.spike`, dengan parent PID 1 | Ya |
| Status setelah izin dimatikan | Tidak berubah: tetap `enabled`, job tidak dihentikan, dan `relaunch` tetap menyalakannya | Tidak, lihat B3 |
| `unregister` menghentikan job yang jalan | Ya, `SIGTERM` selama panggilan, lalu status `notRegistered`; child di process group job ikut mati, child di session sendiri tetap hidup | Ya |

**Bantahan yang membawa bagian spec kembali ke `/architect` sebelum langkah 5 dan 6** (dijawab di *Update 2026-10-21* di bawah)**:**

- **B1. Bundle yang belum pernah didaftarkan membaca `notFound`.** `notRegistered` hanya muncul setelah `unregister`. Tabel *State background agent* memetakan `NotFound` ke `not_found` (langkah `move_or_reinstall_app`), sedangkan AC-8 dan Turn On hanya mendaftar dari `not_registered`, jadi instalasi baru tidak pernah bisa dinyalakan. Bagian terkait: AC-4, AC-8, AC-9, AC-13, AC-17, tabel *State background agent*, baris "berhasil, status sesudahnya `NotFound`" di *Hasil pendaftaran*, *Banner per state*, dan `protocols/fixtures/agent/states.json`. Yang perlu diputuskan: cara membedakan "belum pernah didaftarkan" dari "service tidak ditemukan", padahal plist dan `stacklyd` sudah dipastikan ada lewat cek `HelperMissing` sebelum `status` dibaca.
- **B2. Lewat symlink, gerbang `NSBundle.mainBundle` menolak kasus yang didukung OS.** `MacosBackgroundAgent` mengembalikan `NotBundled` kalau `bundleIdentifier` dari `NSBundle.mainBundle` bukan `com.ojiepermana.stackly`. Lewat `~/.local/bin/stackly` nilai itu nil, sehingga `daemon install`, `start`, dan `restart` dari CLI yang dipasang selalu gagal `cli.agent_not_bundled`, padahal `SMAppService` sendiri bekerja lewat symlink. `AgentProbe::Registration.bundle` yang diambil dari `NSBundle.mainBundle` juga menjadi folder symlink. Bagian terkait: langkah adapter di *Port `BackgroundAgent`*, AC-8 (hint `not_bundled`), dan penggantian `Registration` menjadi `NotBundled` di *State background agent*. Arah yang terlihat adalah gerbang dan path bundle dari `AppBundle::enclosing` atas `current_exe()` canonical, tetapi itu keputusan `/architect`.
- **B3. Di build ad hoc, switch Login Items tidak berpengaruh yang bisa dilihat.** Setelah kamu mematikan StacklyTestAgentSpike di Login Items dan UI tetap menunjukkan mati, status tetap `enabled` dari ketiga path, job yang jalan tidak dihentikan (ia keluar sendiri di batas 20 menit nya), `launchctl print-disabled` tetap `enabled`, `backgroundtaskmanagementd` tidak mencatat perubahan untuk item itu, dan `relaunch` dengan switch mati menyalakan job dalam 84 ms dengan status `enabled`. `requiresApproval`, dan dengan itu `disabled`, tidak bisa dihasilkan. Untuk build ad hoc, GUI dan CLI akan menampilkan `enabled` saat Login Items menunjukkan mati. Bagian terkait: Keputusan 5 (`disabled` dari launch record), cabang AC-17 (apakah `disabled` hanya diharapkan untuk build bertanda tangan), dan teks state `enabled`. Perilaku build bertanda tangan tetap dijawab AC-17. Percobaan pertama terjadi saat disk Mac ini penuh (117 MiB tersisa) dan mungkin tidak tersimpan; percobaan kedua dilakukan setelah ruang disk dibebaskan.
- **B4. Binary agent yang diganti merusak restart `KeepAlive` sampai `relaunch`.** Setelah `spike-agent` diganti build ad hoc baru (cdhash baru) saat terdaftar, exit 75 tidak memunculkan proses baru: launchd melaporkan `spawn failed`, last exit 78 (`EX_CONFIG`), dan properti `needs LWCR update`, sementara status tetap `enabled`. Hanya `relaunch` yang menyalakan build baru, tanpa persetujuan baru. Artinya, kalau daemon dari build lama masih jalan saat bundle diganti (`cargo xtask build-app --install` atau update app), `daemon restart` lewat IPC berakhir tanpa daemon sampai `cli.daemon_wait_timeout`, dan crash setelah penggantian juga tidak dinyalakan ulang. Bagian terkait: Keputusan 3 (restart lewat `KeepAlive`), AC-10 (`restart`), dan warning `cli.daemon_relaunched`. Apakah build bertanda tangan dengan Team ID terkena hal yang sama belum diketahui.

**Temuan lain yang tidak membantah spec:**

- `register` saat sudah `enabled` berhasil tanpa error dan tidak me-restart job, jadi baris `kSMErrorAlreadyRegistered` tidak muncul untuk kasus ini.
- Semua panggilan juga berjalan di thread latar, jadi cabang "harus di thread utama" di *Perintah CLI* tidak diperlukan.
- Exit 0 membuat job tetap mati dengan status `enabled`. Exit 75 memunculkan job 0,3 detik kemudian kalau job sudah jalan lebih dari 10 detik, dan sekitar 10 detik setelah launch nya kalau belum (`ThrottleInterval`), sesuai Keputusan 3 dan AC-15.
- Lingkungan job: parent PID 1, process group sendiri, cwd `/`, `PATH` `/usr/bin:/bin:/usr/sbin:/sbin`, dan tanpa `STACKLY_ROOT`, sehingga `LaunchContext::detect` AC-5 cukup.
- Salinan bundle yang identik di path lain juga membaca `enabled`: status mengikuti identitas bundle, bukan path nya. Selama daemon mati, `enabled` tidak membuktikan bahwa salinan inilah yang terdaftar; `cli.agent_bundle_mismatch` hanya bisa dicek saat daemon jalan.
- Salinan tool biasa di luar bundle membaca `notFound`; `register` gagal `SMAppServiceErrorDomain` 3 (`kSMErrorInvalidSignature`, codesign -67028) dan `unregister` gagal kode 22. Gerbang `NotBundled` sudah menolak kasus ini sebelum OS dipanggil.
- Setelah `unregister`, `launchctl print-disabled gui/<uid>` tetap menyimpan entri label test (`enabled`). Itu tabel override launchd, dan dibiarkan apa adanya.

**Batasan**: hanya build ad hoc, bundle di `/private/tmp`, dan identifier test. Sesi spike tidak melihat UI System Settings, jadi hasil switch Login Items bertumpu pada laporan kamu dan log OS.

## Update 2026-10-21 (hasil spike AC-21)

`/architect` update atas empat bantahan spike (B1 sampai B4) di atas. Aturan program masih berlaku: gap keputusan diisi dengan opsi rekomendasi dan dicatat di sini, sedangkan perubahan teks AC menunggu persetujuan kamu lewat lead. ID AC tidak berubah.

### Keputusan U1: arti `notFound` (B1)

#### Option 1: `notFound` di luar aksi pendaftaran menjadi `not_registered`; hanya `notFound` tepat setelah `register` atau `relaunch` di aksi yang sama menjadi `not_found` (rekomendasi)

**Pros**:
- Instalasi baru langsung menawarkan Turn On dan `stackly daemon install`, yang memang langkah yang benar.
- `HelperMissing` sudah memastikan plist dan `stacklyd` ada sebelum status dibaca, jadi `notFound` di luar aksi tidak punya arti lain yang bisa ditindaklanjuti selain "daftarkan".
- Memakai masukan `just_registered` yang sudah ada; tidak ada field atau state wire baru, dan `not_found` tetap punya arti jujur (OS menerima permintaan tetapi tetap tidak menemukan service).

**Cons**:
- Service yang hilang di luar aksi (misalnya database BTM di reset) tampil `not_registered` tanpa sebab nya.
- `just_registered` di GUI harus bertahan selama status tetap `notFound`, satu aturan lagi di model.

#### Option 2: `not_found` tetap, tetapi `install` dan Turn On juga mendaftar dari `not_found`

**Pros**:
- Tabel `classify` tidak berubah.

**Cons**:
- Setiap instalasi baru melihat banner critical "Stackly can’t find its background service" dengan langkah "pindahkan atau install ulang", yang salah dan menakutkan.
- AC-8, AC-13, dan spec 0016 harus menambah aksi untuk state yang artinya dua.

#### Option 3: bedakan dengan launch record (`notFound` tanpa record berarti belum didaftarkan, dengan record berarti hilang)

**Pros**:
- Bisa menyebut "hilang" untuk pendaftaran yang pernah jalan.

**Cons**:
- Launch record tertinggal setelah uninstall dan reinstall (fitur 62 menyimpan data), jadi reinstall tampil "hilang" dengan langkah "pindahkan atau install ulang", padahal langkah yang benar adalah daftar.
- Mencampur dua arti launch record (bukti izin pernah diberikan, dan bukti pernah terdaftar).

Runner up: Option 3. Option 1 dipilih karena satu satunya langkah pemulihan yang benar untuk `notFound` di luar aksi adalah `register`, apa pun sebab nya.

### Keputusan U2: cara CLI menemukan bundle (B2)

#### Option 1: `main` memberi adapter path executable canonical; adapter memakai `AppBundle::enclosing` dan membaca identifier dari `Info.plist` bundle itu (rekomendasi)

**Pros**:
- `~/.local/bin/stackly` (symlink terkelola dari AC-14) bekerja, karena `SMAppService` sendiri menjawab untuk bundle asli lewat symlink.
- Satu sumber bundle untuk gerbang, `Registration.bundle`, dan `AgentReport.bundlePath`, jadi langkah "ganti `Registration` yang bundle nya berbeda dengan `NotBundled`" hilang.
- Mengikuti aturan yang sudah ada: hanya bin yang membaca `current_exe`, library tidak.

**Cons**:
- `MacosBackgroundAgent` butuh konstruktor dengan path, dan `MacosPlatform` memasang adapter `unlocated()` yang hanya ada supaya capability jujur.
- Identifier dibaca lewat `NSBundle::bundleWithPath`, satu panggilan `objc2` lagi.

#### Option 2: tetap `NSBundle.mainBundle`, dan CLI meminta pengguna menjalankan `Stackly.app/Contents/MacOS/stackly` langsung

**Pros**:
- Adapter di patch yang ditahan tidak berubah.

**Cons**:
- `daemon install`, `start`, dan `restart` dari CLI yang dipasang selalu gagal `cli.agent_not_bundled`, jadi instalasi CLI AC-14 tidak berguna untuk perintah agent.

#### Option 3: adapter membaca `current_exe` sendiri dan membuatnya canonical

**Pros**:
- Tidak ada konstruktor baru.

**Cons**:
- Melanggar aturan spec ini bahwa library tidak memanggil `current_exe`, dan tes `locate` jadi butuh proses di dalam bundle.

Runner up: Option 3. Option 1 dipilih karena path canonical sudah dihitung `main` untuk `AgentReport.bundlePath`, dan adapter cukup menerimanya. GUI tetap memakai `Bundle.main`, karena app selalu dijalankan LaunchServices dari bundle nya sendiri; path nya lewat `realpath(3)` supaya sama dengan `std::fs::canonicalize` di CLI (`resolvingSymlinksInPath()` membuang awalan `/private`, jadi tidak dipakai).

### Keputusan U3: build ad hoc dan switch Login Items (B3)

#### Option 1: tetap tampilkan status OS apa adanya; `disabled` dari launch record tetap hipotesis untuk build bertanda tangan, dan AC-17 mendapat cabang C (rekomendasi)

**Pros**:
- Jujur pada OS, tanpa kode baru, dan tanpa `unsafe` baru.
- AC-17 tidak bisa berakhir ambigu: kalau build bertanda tangan juga mengabaikan switch, hasil nya tercatat dan Keputusan 5 kembali ke `/architect` dengan bukti.

**Cons**:
- Selama development tanpa identity, GUI menampilkan "Background service is on." saat System Settings menunjukkan mati.
- `disabled` dan `requires_approval` hanya bisa dicoba lewat fake sampai OQ-012 selesai.

#### Option 2: deteksi signature ad hoc saat runtime (`SecCodeCopySelf` dan flag ad hoc) dan tampilkan catatan di Diagnostics

**Pros**:
- Pengembang tahu kenapa switch tidak berpengaruh.

**Cons**:
- Blok `unsafe` dan panggilan Security framework baru di CLI dan GUI hanya untuk build development; produk yang dirilis selalu bertanda tangan.

#### Option 3: buang `disabled` sekarang

**Cons**:
- Bukti untuk build bertanda tangan belum ada, dan scope meminta `disabled` terlihat.

Runner up: Option 2. Option 1 dipilih karena masalah nya hanya ada di build development, dan cabang C membuat bukti build bertanda tangan menentukan nasib `disabled`, bukan tebakan.

### Keputusan U4: restart setelah binary diganti (B4)

#### Option 1: `restart` tetap lewat exit 75, dengan cadangan `relaunch` kalau launch record tidak menunjukkan respawn sampai `T_respawn` (rekomendasi)

**Pros**:
- Jalur biasa tidak menyentuh pendaftaran (respawn 0,3 detik di spike), jadi notifikasi "Background Items Added" tidak muncul di setiap restart.
- AC-6 dan kontrak `daemon.shutdown` tidak berubah, dan AC-15 tetap menguji jalur launchd biasa.
- Tanda respawn (`lastLaunchedAt` ditulis setelah lock, sebelum bind) sudah ada; tebakan yang salah hanya membuang beberapa detik.

**Cons**:
- Ada aturan waktu baru (`T_respawn`) yang harus diuji dengan jam yang dijeda.
- Setelah `build-app --install`, restart sekitar 5 detik lebih lama sebelum `relaunch`.
- `relaunch` sekarang juga dipanggil dari `restart` untuk daemon yang tadinya jalan, jadi teks AC-11 berubah.

#### Option 2: `restart` untuk daemon `launchd` selalu `stop` lalu `relaunch`

**Pros**:
- Paling sederhana dan deterministik, tanpa aturan waktu.

**Cons**:
- Setiap restart menyentuh pendaftaran, yang untuk build bertanda tangan mungkin memunculkan notifikasi atau meminta persetujuan (belum diketahui, AC-17).
- Mode `restart` dan exit 75 menjadi tidak terpakai oleh CLI, sehingga AC-6, AC-10, dan AC-15 berubah lebih banyak.

#### Option 3: CLI mendeteksi binary yang diganti sebelum restart (misalnya daemon melaporkan identitas file executable nya)

**Pros**:
- `relaunch` hanya saat memang perlu, tanpa menunggu.

**Cons**:
- Field kontrak baru di `DaemonInfo`, dan identitas file tidak menjamin launchd menolak respawn (pemicu nya cdhash dan LWCR, bukan path).

Runner up: Option 2. Option 1 dipilih karena ia menjaga jalur harian di IPC dan `KeepAlive` seperti Keputusan 3, dan hanya memakai `relaunch` saat launchd terbukti tidak menyalakan daemon.

### Keputusan U5: `cargo xtask build-app --install`

Tetap seperti AC-22: hanya mengganti bundle dan mencetak hint `stackly daemon restart` saat socket root default ada; *Penantian restart* yang menangani respawn yang gagal. Runner up: `--install` menjalankan sendiri `<bundle baru>/Contents/MacOS/stackly daemon restart`, yang ditolak karena perintah build tidak boleh mengubah daemon yang jalan tanpa perintah eksplisit dari kamu.

### Keputusan kecil update ini

- `just_registered` true setelah `register` atau `relaunch` yang berhasil atau gagal `NotFound`; error lain dibaca ulang dengan false, supaya `LaunchDenied` tetap bisa menjadi `disabled` (runner up: true setelah panggilan apa pun, yang membuat `disabled` mustahil setelah aksi).
- Di GUI, `justRegistered` bertahan selama status tetap `requiresApproval` atau `notFound`, dan model baru mulai dengan false (runner up: reset di refresh pertama, yang membuat banner `not_found` langsung berganti ke `not_registered`).
- `RESPAWN_GRACE` 5 detik di `commands/daemon/wait.rs` dan `LAUNCHD_THROTTLE_INTERVAL_SECS` 10 di `stackly-platform::bundle`, dipakai CLI dan plist (runner up: satu batas tetap 15 detik, yang menambah 10 detik ke setiap restart setelah `--install`).
- Dengan `STACKLY_ROOT`, `restart` tidak memakai cadangan `relaunch`, karena agent hanya melayani root default (runner up: gagal `cli.agent_root_override` di langkah 5, yang memberi exit 2 untuk daemon test yang sebenarnya hanya lambat).
- Script tahu apakah cadangan sudah dicoba dari warning `cli.agent_relaunched`, yang ada di envelope sukses maupun gagal (runner up: `details.relaunched` di `cli.daemon_wait_timeout`, yang awalnya dipilih lalu dihapus setelah cross check karena hanya ada di jalur gagal).
- Ctrl+C ditunda selama `register`, `unregister`, atau `relaunch` berjalan, paling lama 10 detik (runner up: keluar langsung seperti di penantian, yang bisa meninggalkan `not_registered`).
- `daemon start` dengan daemon yang sudah jalan memberi `agent` null supaya AC-9 ("tanpa memanggil OS") tetap benar (runner up: satu panggilan `status` dan ubah teks AC-9).
- `FakeBackgroundAgent` mulai di `NotFound` supaya test CLI mulai dari keadaan instalasi baru yang sebenarnya (runner up: tetap `NotRegistered`, yang menyembunyikan B1 dari test).
- Jalur "panggil port di thread utama" di *Perintah CLI* dihapus, karena spike menunjukkan semua panggilan bekerja di thread latar.

### Perubahan teks AC (menunggu persetujuan kamu lewat lead)

| AC | Perubahan | Sebab |
| --- | --- | --- |
| AC-4 | tambah aturan `notFound` menjadi `not_registered`, kecuali hasil pendaftaran di aksi yang sama (`not_found`) | B1 |
| AC-8 | bundle CLI dari path canonical, `~/.local/bin/stackly` bekerja seperti di dalam bundle; hint `not_bundled` menjadi `use_bundled_cli` tanpa path canonical | B2 |
| AC-10 | `restart` menunggu EOF lalu handshake, dengan cadangan `relaunch` sekali untuk state `enabled` dan warning `cli.agent_relaunched` | B4 |
| AC-11 | `relaunch` juga dari `restart` yang tidak melihat daemon baru sampai batas respawn | B4 |
| AC-17 | cabang C: status tetap `enabled` setelah izin dimatikan, dicatat, dan Keputusan 5 kembali ke `/architect` | B3 |
| AC-19 | upgrade juga mencatat apakah `restart` butuh `relaunch` | B4 |
| AC-11 (kalimat test) | `restart` untuk daemon yang jalan hanya memanggil `status` selama daemon baru muncul sebelum batas respawn; command lain "paling banyak" memanggil `status` | B4, cross check |
| AC-13 | `AgentActionError.timedOut` menjadi `.daemonDidNotStart(seconds: 30)`, menyamakan AC dengan bagian GUI (selisih lama, bukan dari spike) | cross check |

AC-1 sampai AC-3, AC-5 sampai AC-7, AC-9, AC-12, AC-14 sampai AC-16, AC-18, AC-20 sampai AC-22 tidak berubah teks nya.

### Cross check update ini

Spec yang sudah diperbarui dibaca model lain (Sonnet 5.5, baca saja, tanpa membuka link) pada 2026-10-21. Putusan: arah jawaban B1 sampai B4 benar dan B1 sampai B3 sudah opsi paling sederhana, tetapi cadangan `restart` (U4) kurang rinci dan daftar perubahan patch yang ditahan belum cukup untuk dipakai tanpa menebak. Semua gap keputusan diisi dengan rekomendasi saya sesuai aturan program, dan dua di antaranya menyentuh teks AC (AC-11 dan AC-13, tabel di atas):

| No | Temuan | Perbaikan yang diterapkan |
| --- | --- | --- |
| 1 | `T_respawn` mencampur wall clock (`L0`) dan jam test yang monoton; `L0` di masa depan bisa membuat cadangan tidak pernah jalan | `elapsed` dihitung sekali saat EOF dengan `Clock` dan dijepit 0 sampai 10 detik; batas di jam monoton, selalu 5 sampai 15 detik; record rusak berarti `None` |
| 2 | hasil setelah `relaunch` (selain `enabled`) dan isi `agent` JSON tidak ditentukan | `classify` dengan `just_registered` true; selain `enabled` gagal saat itu juga tanpa warning; `agent` dari probe hasil `relaunch`; baris *Hasil pendaftaran* untuk `RequiresApproval` dipisah |
| 3 | `AlreadyRegistered`, baca ulang yang gagal, dan `justRegistered` GUI saat aksi berturut turut | `AlreadyRegistered` dihitung berhasil; baca ulang yang gagal setelah `LaunchDenied` atau `MustBeEnabled` memberi `cli.agent_requires_approval`; setiap aksi GUI menimpa nilai nya |
| 4 | wiring CLI tidak bernama (`env::read`, `Environment`, OS lain, launch record saat `STACKLY_ROOT`) | `env::read(executable)`, field `agent`, `bundle`, `launch_record`; `UnsupportedBackgroundAgent` di OS lain; launch record root default, `None` saat override |
| 5 | `start` dengan daemon yang jalan harus mengisi `agent` tanpa memanggil OS | `agent` nullable, null hanya saat `alreadyRunning` true; `install` di state `enabled` mencoba satu handshake |
| 6 | penantian 250 ms bentrok dengan retry `daemon.starting` milik `Session`, dan error koneksi lain tidak ditentukan | satu percobaan per polling; daftar kode "belum naik"; error lain berhenti dan dilaporkan |
| 7 | bentuk `states.json` tidak ditentukan | skema baris, encoding `probe`, dan 29 baris wajib |
| 8 | daftar perubahan patch kurang: 0002 tidak bisa di `git apply`, test fake lain, `assert_not_bundled`, derive, method | daftar di langkah 2 dan 5 *Build plan* dilengkapi |
| S1 | respawn bisa terjadi selama `status` di langkah 5, lalu `relaunch` menghentikan daemon yang sehat | satu handshake dan baca record lagi tepat sebelum `relaunch` |
| S2 | Ctrl+C atau batas waktu di antara `unregister` dan `register` meninggalkan `not_registered` | Ctrl+C ditunda selama panggilan port yang mengubah pendaftaran; hint khusus untuk `relaunch` yang lewat batas |
| S3 | B3 dan B4 bertumpu pada satu run dan penggantian satu binary, bukan seluruh bundle | kata kata diperhalus, dan record tambahan ad hoc (setelah persetujuan) diusulkan di milestone 2 |
| S4 | `resolvingSymlinksInPath()` membuang `/private`, jadi path GUI bisa berbeda dari `canonicalize` Rust | `realpath(3)` lewat `CanonicalPath.resolve`, juga untuk status link CLI |
| S5 | B2 untuk build bertanda tangan tidak pernah diuji lewat symlink | record AC-17 (langkah 14) menjalankan CLI lewat kedua path |
| S6 | test urutan tulis launch record butuh hook filesystem; `NSBundle` menyimpan bundle per path | test memeriksa record sudah ada saat handshake pertama; path bundle palsu berbeda per kasus dan path harapan di `canonicalize` |
| W | teks lain: AC-11 kalimat test, Summary dan Decision soal restart, AC-13 `.timedOut`, timeout `status`, kolom "Kapan" `cli.agent_not_found` dan `cli.agent_not_registered`, `daemon.shutting_down` pada `restart`, `details.relaunched` | semua diselaraskan; `details.relaunched` dihapus karena warning `cli.agent_relaunched` sudah ada di envelope yang sama |

Yang tidak diambil: batas tetap `T_eof + 15 detik` (lebih sederhana, tetapi menambah 10 detik ke setiap restart setelah `--install`, justru jalur development yang paling sering), dan `DaemonInfo.startedAt` sebagai ganti `L0` (butuh `daemon.get_info` tambahan, sedangkan `L0` tetap diperlukan untuk tanda respawn).

### Dampak ke spec lain

- Spec 0013: tidak ada butir usulan yang berubah karena spike. Baris banner, `ConnectionHint`, seksi Diagnostics, dan aturan sumber tetap; yang berubah hanya state mana yang dilihat instalasi baru (`not_registered`, bukan `not_found`).
- Spec 0016: teks nya tidak perlu berubah, karena kartu Background Agent mengikuti state; instalasi baru sekarang melihat kartu `not_registered`.
- Spec 0008: warning `cli.agent_relaunched` ditambah ke usulan.
- Spec 0004: butir baru di usulan untuk `MacosBackgroundAgent::unlocated()` dan `new(executable)`.

## Update 2026-10-22 (Follow-up spec 0013)

Update gabungan spec 0013 (lead `ui-macos`) menemukan tiga gap milik spec ini dan mencatatnya sebagai Follow-up di spec 0013. Lead meneruskan nya dengan rekomendasi; aturan program berlaku (opsi rekomendasi diambil kecuali mengubah teks AC). Tidak ada teks AC yang berubah: AC-13 sudah meminta banner per state, ambang 15 detik, dan pengumuman lewat `announce(_:)`, dan AC-21 spec 0013 sudah meminta satu pengumuman per transisi.

- **G1, pengumuman transisi tanpa assessment.** Rekomendasi lead diambil: umumkan judul banner agent kalau assessment ada, selain itu tunggu assessment pertama paling lama 10 detik, dan jatuh ke "Can’t connect to stacklyd." kalau lewat; teks per state untuk state yang sama tidak diulang. Yang saya tambahkan supaya bisa dibangun tanpa menebak: "ada" berarti `lastCheckedAt` paling lama 5 detik (`pollInterval`) sebelum transisi, karena tanpa itu assessment dari launch bisa berumur berjam jam saat koneksi putus (runner up: selalu menunggu probe setelah transisi, yang lebih akurat tetapi menunda pengumuman walau assessment segar sudah ada); probe yang ditunggu adalah probe pertama yang selesai setelah transisi, jadi refresh yang sedang berjalan saat launch ikut dihitung; judul banner critical (`not_found`) tidak diumumkan `DaemonModel`, karena `StateBanner` sudah mengumumkan banner critical saat tampil; assessment pertama setelah launch tidak diumumkan sebagai perubahan state, supaya setiap launch tidak berbunyi "Background service is on."; dan teks per state diberi tabel lengkap, termasuk state yang tidak punya teks. Semua pengumuman lewat satu announcer `DaemonModel` (closure `didAssess` dari `BackgroundAgentModel`), supaya aturan "satu transisi, satu pengumuman" ada di satu tempat (runner up: `BackgroundAgentModel` mengumumkan sendiri, yang butuh koordinasi dua announcer).
- **G2, state agent dan jam ke banner.** Rekomendasi lead diambil: `AgentBanner.content` murni di `Agent/`, dievaluasi `DaemonBanner` di `TimelineView` 1 detik dengan `fixtureNow`, dari `BackgroundAgentModel` lewat `DaemonModel` di environment, dengan aksi ke `turnOn`, `openLoginItems`, `refresh`, dan `start`; `ShellPresentation` tetap murni. Tambahan saya: `DaemonModel` di environment dibaca sebagai opsional supaya preview `AppShell` tanpa model tetap jalan; `.id` pada `StateBanner` memakai judul yang tampil supaya banner critical agent diumumkan; empat aksi memakai `isBusy` dari `isWorking`; `ConnectionHint` memakai fungsi yang sama dan dibangun ulang setiap `lastCheckedAt` berubah; dan mode fixture memakai `FakeBackgroundAgentController` di `notFound` dan home sementara, dengan harapan test fixture yang diperbarui di langkah 11 (runner up: fixture tanpa controller sehingga banner lama tetap, yang menyembunyikan baris agent dari snapshot dan XCUITest).
- **G3, `CommandLineToolModel` di environment.** Rekomendasi lead diambil: `LiveShell` (nanti `MainWindowRoot`) memasang `.environment(runtime.commandLineTool)`. Nama properti `ShellRuntime.commandLineTool` dan `DaemonModel.agent` ditetapkan di sini.

Usulan untuk spec 0013 (nama, seam, dan environment), spec 0016 (`MainWindowRoot`, pemilik pengumuman, `isStarting`), dan spec 0068 (fixture agent) ditambahkan ke *Usulan update untuk spec lain*.

**Cross check (Sonnet 5.5, baca saja, 2026-10-22).** Putusan: arah nya benar (`AgentBanner` murni, satu announcer, `ShellPresentation` tetap murni), tetapi belum bisa dibangun tanpa menebak. Semua temuan diisi dengan rekomendasi saya:

| No | Temuan | Perbaikan |
| --- | --- | --- |
| 1 | seam test dan default tidak ada; test yang ada (`DaemonModelTests`, `ShellEndToEndTests`) mengharapkan "Can’t connect to stacklyd." segera | `DaemonModel.agent` opsional dengan default nil yang memberi perilaku spec 0013, parameter `sleep:`, parameter internal `assemble` dengan default nil, `live` tanpa parameter baru |
| 2 | baris agent hilang selama `refreshing(after: unreachable)` karena `lastError` dikosongkan saat tersambung | status dibuka dari `refreshing`, dan `DaemonModel.unreachableCode` menyimpan kode kegagalan |
| 3 | urutan `update()`, pengumuman tertunda saat presence pulih, satu pending, cek ulang kode, batas yang sama dengan timeout probe, urutan `didAssess` | status dihitung dulu; pending diselesaikan saat presence keluar (tetap satu pengumuman per transisi); paling banyak satu; judul jatuh ke teks umum kalau kode berubah; `announceWait` 12 detik; `didAssess` setelah field disimpan |
| 4 | konkurensi model, `isWorking`, polling, Check Again, app aktif | satu panggilan OS sekaligus, refresh bergabung atau dilewati, `isWorking` sinkron hanya untuk aksi, Check Again adalah `refresh()`, polling milik model, hook `didResignActive()` |
| 5 | nama field `ConnectionDiagnostics`, awalan pesan, petunjuk tertinggal 5 detik | `agentHint`, tanpa awalan, dihitung saat render di `TimelineView` 1 detik |
| 6 | fixture dengan home sementara melanggar AC-3 spec 0068 dan aturan sumber; balapan probe async; harapan `FixtureShellTests` | `FakeCommandLineTool` di memori, assessment awal sinkron, `bannerContent(now:)` sebagai harapan test, seksi Diagnostics opsional, snapshot direkam ulang |
| 7 | spec 0016 butuh baris `MainWindowRoot` dan pemilik pengumuman nya berubah | baris usulan spec 0016 |
| S3 | `.id` di setiap perubahan state menghilangkan fokus tombol | `.id` hanya untuk banner critical |
| S4 | `TimelineView` 1 detik terus menerus | 1 detik hanya selama `enabled` di bawah 15 detik |
| S6 | Turn On beberapa menit setelah launch langsung menawarkan Start stacklyd karena ambang dihitung dari `since` presence | `turnOn()` yang berhasil menunggu daemon seperti `start()`, dan baris `enabled` memakai `isStarting`; ambang AC-13 tetap dari `since` |

Yang tidak diambil: `DaemonBanner(contentAt:...)` yang diisi `LiveShell` (lebih mudah di preview, tetapi menyimpang dari rekomendasi lead yang memakai environment; preview dan snapshot baris agent tetap bisa lewat runtime fixture spec 0068), dan memindahkan ambang 15 detik dari `since` presence ke waktu aksi (mengubah arti AC-13). Teks AC tetap tidak berubah; yang bergeser hanya waktu: pengumuman transisi bisa tertunda sampai probe pertama (biasanya milidetik, paling lama 12 detik), dan baris `enabled` menunggu selama `isStarting`.

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
