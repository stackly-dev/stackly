# 0016. Rationale: onboarding

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: mockup onboarding ditulis untuk produk yang sudah lengkap. Ia menjanjikan hal yang di fase ini belum ada atau tidak akan dibangun: daemon cadangan "only while the app is open", CLI di `/usr/local/bin` dengan password, sumber catalog community dengan manifest bertanda tangan, environment `stackly.yaml`, dan perintah `env up` serta `doctor`. Kalau onboarding ditiru apa adanya, layar pertama yang dilihat pengguna justru layar yang paling tidak jujur. Spec ini memperlakukan mockup sebagai acuan tata letak dan alur, tetapi isinya mengikuti apa yang benar di build itu, dan tiap fitur yang nanti membawa kemampuan itu menambah barisnya sendiri.

PAGE-01 meminta halaman pertama yang menampilkan status daemon dan izin background, lokasi storage, model isolasi, trust catalog, dan pilihan memasang CLI, tanpa kewajiban akun cloud. JOURNEY-01 menaruh onboarding tepat setelah app dibuka dan sebelum izin background, catalog, dan install runtime. AC-061 meminta onboarding menjelaskan isolasi logis dan proses, dan tidak menyatakan provider native disandbox atau mustahil membaca file user yang sama; BR-012 (satu akun user sebagai batas), BR-020 (checksum bukan bukti bebas malware), dan REQ-032 (official dan community adalah label asal, bukan sandbox) adalah alasannya. Scope fitur 16 menambah syarat bahwa onboarding bisa dilewati dan dibuka lagi tanpa mengubah state daemon, dan bahwa tampilannya mengikuti lima langkah mockup dengan Not Now saat izin belum diberikan dan Get Started di langkah terakhir.

Bagian yang dipakai onboarding sudah diputuskan di spec lain. Spec 0015 memberi `BackgroundAgentModel` dengan enam state, langkah pemulihan, dan aksi Turn On, Open Login Items, Start stacklyd, serta `CommandLineTool` yang hanya membuat symlink terkelola di `~/.local/bin` setelah konfirmasi; spec itu juga menegaskan bahwa pendaftaran agent hanya berubah lewat tombol pengguna dan bahwa tidak ada daemon cadangan saat izin belum diberikan. Spec 0017 memberi `CatalogInfo` dengan sumber, trust, revision, tanggal terbit, dan flag outdated, dengan aturan bahwa catalog tidak pernah diklaim terbaru, hanya namespace `official` yang ada, dan sumber community atau lokal milik fitur 37 dan Deferred. Spec 0013 memberi satu jendela utama, `ShellRuntime`, `DaemonModel` dengan presence dan daemon terakhir yang diketahui (termasuk `DaemonInfo.roots`), dan aturan bahwa nilai `STACKLY_ROOT` tidak pernah ditampilkan. Spec 0012 memberi text style untuk judul onboarding, token, permukaan kartu, konfirmasi, dan `announce(_:)`.

Fakta yang membatasi desain: pada peluncuran pertama daemon hampir pasti belum berjalan, karena agent belum didaftarkan, jadi setiap langkah harus berguna tanpa daemon. GUI tidak menjalankan proses dan tidak link Rust. Mockup tidak punya tombol Skip dan tidak punya cara membuka onboarding lagi. Fitur 67 (komponen dasar), 68 (snapshot), dan 69 (peta perintah) belum didesain, dan program `01-foundation` masih membangun wiring fitur 13. Pendekatan proyek adalah Journey, jadi onboarding dibangun lengkap dengan semua statenya sebelum journey berikutnya.

## Options considered

### Keputusan 1: tempat onboarding tampil

#### Option 1: isi jendela utama yang menggantikan shell (dipilih)

`MainWindowRoot` menampilkan `OnboardingFlow` atau `AppShell` di jendela utama yang sama.

**Pros**:

- Sama dengan mockup: onboarding mengisi seluruh jendela 1280 × 800 dengan hanya tombol lampu lalu lintas.
- Satu jendela, satu notice tutup jendela, satu restorasi bagian sidebar; tidak ada urusan urutan dua jendela.
- Get Started cukup mengganti isi, dan bagian shell yang tersimpan tetap utuh.

**Cons**:

- Banner daemon dan sidebar tidak terlihat selama onboarding.
- Accessor jendela harus pindah dari `AppShell` ke akar baru.

#### Option 2: jendela Welcome terpisah

Scene `Window("Welcome to Stackly", id: "welcome")` berukuran tetap, ditampilkan di depan jendela utama.

**Pros**:

- Idiom yang dikenal di macOS (jendela Welcome Xcode), dan shell tetap terlihat di belakang.
- Tidak menyentuh struktur jendela utama spec 0013.

**Cons**:

- Berbeda dari mockup, yang menggambar onboarding di jendela utama.
- Dua jendela di peluncuran pertama: urutan fokus, notice tutup jendela, dan apa yang terjadi kalau salah satunya ditutup perlu aturan sendiri.

#### Option 3: sheet di atas shell

Onboarding tampil sebagai sheet modal di jendela utama.

**Pros**:

- Mudah dibuat dan modal secara alami.

**Cons**:

- Sheet macOS punya ukuran dan bingkai sendiri, jauh dari mockup.
- Shell di belakang sheet menampilkan banner daemon tidak terjangkau di peluncuran pertama, yang justru membuat cemas.

### Keputusan 2: cara mengingat onboarding sudah selesai

#### Option 1: satu Bool `onboarding.finished` di `UserDefaults` app (dipilih)

**Pros**:

- Preferensi GUI murni, sehingga melewati dan membuka lagi tidak mungkin mengubah state daemon.
- Bisa disetel lewat argumen launch untuk run manual dan disuntik di test.

**Cons**:

- Hilang kalau preferensi app dihapus, dan onboarding tampil lagi.
- Tidak dibagi antara GUI dan CLI (CLI memang tidak punya onboarding).

#### Option 2: setting di daemon atau registry

**Pros**:

- Satu sumber kebenaran untuk semua client.

**Cons**:

- Menulis ke daemon dari onboarding melanggar syarat scope "tanpa mengubah state daemon" dan butuh method IPC baru.
- Daemon belum berjalan di peluncuran pertama, jadi record tidak bisa ditulis saat paling dibutuhkan.

#### Option 3: Int revision onboarding

Simpan revision onboarding yang terakhir selesai supaya onboarding baru bisa tampil lagi setelah update.

**Pros**:

- Jalan untuk menampilkan langkah baru ke pengguna lama.

**Cons**:

- Menampilkan onboarding lagi setelah update biasanya mengganggu, dan belum ada kebutuhan nyata (YAGNI).

### Keputusan 3: cara melewati onboarding

#### Option 1: tanpa tombol baru; Escape, titik langkah, dan langkah yang tidak wajib diisi (dipilih)

Setiap langkah bisa diteruskan tanpa aksi (Continue atau Not Now), titik langkah pindah langsung ke langkah 5, dan Escape menutup onboarding dengan efek yang sama seperti Get Started.

**Pros**:

- Tata letak footer tetap sama dengan mockup.
- Escape adalah tombol batal yang wajar di macOS, dan VoiceOver punya aksi escape yang setara.

**Cons**:

- Pengguna mouse yang tidak tahu Escape butuh dua klik (titik terakhir, Get Started).

#### Option 2: tombol Skip terlihat di footer

**Pros**:

- Paling mudah ditemukan.

**Cons**:

- Menambah kontrol yang tidak ada di mockup, dan alur yang hanya lima langkah tidak membutuhkannya.

#### Option 3: menutup jendela dihitung sebagai melewati

**Pros**:

- Tanpa kontrol tambahan sama sekali.

**Cons**:

- Menutup jendela di Stackly berarti "app tetap jalan" (spec 0013), bukan "saya sudah paham"; menyamakan keduanya membuat onboarding hilang tanpa disengaja.

### Keputusan 4: cara membuka onboarding lagi

#### Option 1: item menu Help "Welcome to Stackly" sekarang, baris Settings di fitur 32 (dipilih)

**Pros**:

- Tersedia sejak fitur ini dibangun, tanpa menunggu Settings fitur 32.
- Menu Help adalah tempat macOS untuk bantuan memulai.

**Cons**:

- Item menu ditambah langsung di `StacklyApp` dan nanti dipindah ke peta perintah fitur 69.

#### Option 2: hanya dari Settings

**Pros**:

- Satu tempat untuk semua preferensi.

**Cons**:

- Settings masih placeholder sampai fitur 32, jadi selama itu onboarding tidak bisa dibuka lagi, dan scope meminta bisa dibuka lagi sekarang.

#### Option 3: tombol di Diagnostics

**Pros**:

- Diagnostics sudah ada.

**Cons**:

- Diagnostics untuk masalah, bukan untuk memulai; pengguna tidak akan mencarinya di sana.

### Keputusan 5: kontrol di langkah Command Line

#### Option 1: toggle native yang mencerminkan status nyata dan bekerja langsung lewat konfirmasi (dipilih)

**Pros**:

- Kontrol yang sama dengan mockup (toggle), dengan aturan spec 0015: instalasi hanya setelah konfirmasi eksplisit.
- Membuka onboarding lagi selalu menunjukkan keadaan sebenarnya, dan Get Started tidak pernah mengubah apa pun.

**Cons**:

- Toggle tidak langsung berubah saat diklik, baru setelah konfirmasi; status `stale` dan `broken` butuh tombol Repair, bukan toggle.

#### Option 2: toggle sebagai niat yang diterapkan saat Get Started

Seperti mockup: toggle menyala secara default dan CLI dipasang saat Get Started, dengan pesan sesudahnya.

**Pros**:

- Paling dekat dengan alur mockup.

**Cons**:

- Get Started menjadi aksi yang mengubah file di home, padahal scope meminta onboarding bisa diselesaikan tanpa efek; default menyala berarti pengguna yang hanya menekan Get Started memasang CLI tanpa sadar.

#### Option 3: tombol Install, Repair, dan Remove seperti Diagnostics

**Pros**:

- Kode dan perilaku sama persis dengan `CommandLineToolSection`.

**Cons**:

- Berbeda dari kontrol mockup tanpa alasan perilaku.

### Keputusan 6: isi langkah Catalog Trust

#### Option 1: satu baris untuk catalog yang dimuat, tanpa community (dipilih)

**Pros**:

- Jujur: build ini hanya punya catalog `official` yang ikut di app (spec 0017), dan Settings fitur 32 juga tidak boleh menampilkan setting fitur yang belum dibangun.
- Memakai `CatalogInfo` apa adanya, termasuk development catalog dan flag outdated.

**Cons**:

- Langkah ini lebih tipis dari mockup, dan judulnya berubah dari "Choose catalog sources" menjadi "Catalog sources" karena tidak ada pilihan.

#### Option 2: baris community nonaktif dengan catatan "not available yet"

**Pros**:

- Tata letak tetap dua baris seperti mockup.

**Cons**:

- Mengiklankan fitur yang ada di Deferred, dan membuat pengguna mencari cara menyalakannya.

#### Option 3: mockup apa adanya

**Pros**:

- Tidak ada selisih mockup yang perlu dicatat, dan langkah ini sudah siap untuk fitur 37.

**Cons**:

- Toggle community tidak punya perilaku, dan "Signed manifests" tidak benar di baseline (tidak ada kunci signing catalog, OQ-009).

### Keputusan 7: fitur dan perintah yang belum dibangun

#### Option 1: hanya tampilkan yang ada di build itu, fitur pemilik menambah barisnya (dipilih)

Welcome tanpa "Environments as files" sampai fitur 44, diganti "No account needed" (PAGE-01); baris "Verified runtimes" dan kalimat digest di Catalog Trust baru tampil saat install runtime lewat daemon (milestone 3 spec 0018) ada; contoh perintah awal hanya `stackly daemon status` dan `stackly provider list`, yang menjadi prasyarat milestone 2.

**Pros**:

- Setiap kalimat onboarding benar di build itu.
- Baris "No account needed" menjawab bagian PAGE-01 yang tidak ada di mockup.

**Cons**:

- Fitur 22, 34, 44, dan 46 harus ingat menambah barisnya (dicatat di Follow-up).

#### Option 2: mockup apa adanya

**Pros**:

- Tidak perlu koordinasi antar fitur.

**Cons**:

- Pengguna yang mencoba `stackly env up` di build ini mendapat error di langkah pertama mereka.

### Keputusan 8: tempat penjelasan isolasi (AC-061)

#### Option 1: baris Isolated instances di Welcome dan catatan trust di Catalog Trust (dipilih)

**Pros**:

- Penjelasan muncul tepat di tempat klaim isolasi dan trust dibuat, tanpa langkah tambahan.
- Dua kalimat pendek yang mudah diuji dan dibaca ulang.

**Cons**:

- Baris Welcome menjadi lebih panjang dari mockup.

#### Option 2: langkah keenam khusus keamanan

**Pros**:

- Ruang untuk menjelaskan isolasi, trust, dan batas satu akun user dengan lengkap.

**Cons**:

- Scope dan mockup mengunci lima langkah.

#### Option 3: catatan kaki kecil di Welcome

**Pros**:

- Tata letak baris Welcome tetap sama dengan mockup.

**Cons**:

- Mudah terlewat, dan klaim "never touching each other’s data" di atasnya tetap salah.

### Keputusan 9: sumber path di langkah Storage

#### Option 1: `DaemonInfo.roots` kalau diketahui, selain itu default spec 0004 (dipilih)

**Pros**:

- Daemon tetap sumber kebenaran untuk path, dan langkah ini tetap berguna di peluncuran pertama saat daemon belum berjalan.

**Cons**:

- Aturan default spec 0004 ditulis lagi di Swift tanpa fixture bersama.

#### Option 2: hanya dari daemon

**Pros**:

- Tidak ada aturan path di GUI.

**Cons**:

- Pada peluncuran pertama langkah Storage kosong, padahal itu saat path paling perlu dijelaskan.

#### Option 3: hanya dihitung GUI

**Pros**:

- Satu jalur kode, sama di peluncuran pertama dan setelahnya.

**Cons**:

- Bisa berbeda dari daemon tanpa ada yang tahu; GUI tidak boleh menjadi sumber path.

### Keputusan 10: ikon di langkah Welcome

#### Option 1: ikon app (dipilih)

**Pros**:

- Idiom macOS, dan spec 0012 membatasi accent hanya untuk kontrol native, tint seleksi, dan link.

**Cons**:

- Berbeda dari tile accent di mockup.

#### Option 2: tile accent dengan simbol, seperti mockup

**Pros**:

- Sama persis dengan mockup, dan tidak bergantung pada aset ikon app.

**Cons**:

- Memakai accent sebagai isian kustom, yang dilarang aturan warna spec 0012, dan kontras simbol putih di atas accent bergantung pada pilihan accent pengguna.

## Rationale

Keputusan utama spec ini adalah bahwa onboarding hanya membaca. Scope meminta onboarding bisa dilewati dan dibuka lagi tanpa mengubah state daemon, spec 0015 meminta izin background tidak pernah diubah tanpa tindakan pengguna, dan pada peluncuran pertama daemon belum berjalan. Ketiganya mengarah ke satu bentuk: onboarding menampilkan state dari model yang sudah ada, setiap aksi adalah tombol yang sama dengan Diagnostics, dan satu satunya yang ditulis onboarding sendiri adalah preferensi GUI. Dari situ keputusan lain mengikuti: record di `UserDefaults` (bukan daemon), Get Started yang tidak memasang apa pun, toggle CLI yang bekerja lewat konfirmasi saat diklik, dan Escape sebagai cara melewati yang tidak butuh efek apa pun.

Keputusan kedua adalah kejujuran copy. AC-061, BR-012, BR-020, dan REQ-032 semuanya berkata hal yang sama: Stackly memisahkan instance secara logis, tetapi semua proses berjalan sebagai user yang sama tanpa sandbox, dan trust adalah label asal. Mockup melanggar ini di dua tempat ("never touching each other’s data", "Signed manifests") dan menjanjikan fitur yang belum ada di tiga tempat. Karena onboarding adalah layar pertama, spec ini memilih teks yang benar di build itu dan menjaganya dengan test copy dan pindaian frasa terlarang, lalu menyerahkan baris tambahan ke fitur yang membawa kemampuannya.

Bentuk tampilan mengikuti mockup sejauh tidak bertentangan dengan dua hal di atas: onboarding di jendela utama, lima langkah, footer dengan titik, Back, dan tombol utama berlabel Continue, Not Now, atau Get Started, dan text style dari spec 0012. Selisih yang tersisa (ikon app, judul Catalog Trust, baris Welcome, perilaku toggle) dicatat sebagai usulan selisih mockup untuk `ui-macos.md`, supaya pembaca mockup tahu mana yang sudah diputuskan lain.

## Cross check

Cross check `Another model` (Fable 5.1, baca saja) pada draft 2026-10-20 menemukan satu blocker, 19 gap kelengkapan keputusan, dan 7 nit. Nit wording langsung diperbaiki (`76a45cf`). Lead menyetujui semua 20 rekomendasi atas nama user pada hari yang sama, dan semuanya diterapkan di spec ini:

- Blocker: di bawah `STACKLY_ROOT`, secret ada di keychain file `R/data/keychain/` (spec 0011 AC-2), jadi Storage memakai subjudul dan nilai Secrets varian override.
- Kartu Background Agent: `{detail}` dari `BackgroundAgentModel.unavailableDetail` (usulan ke spec 0015), `since` dari `ShellPresentation.status`, Start stacklyd hanya untuk `ipc.endpoint_missing` dan `ipc.connection_refused`, baris untuk presence lain dan `connecting`, `TimelineView` untuk ambang 15 detik, dan pengumuman state tetap milik `BackgroundAgentModel`.
- Command Line: `CommandLineToolModel` bersama Diagnostics (usulan ke spec 0015), status menentukan baris dan CLI yang hilang dari bundle hanya menonaktifkan Install dan Repair.
- Catalog Trust: label "Catalog" sebelum ada `CatalogInfo`, toggle hanya saat `info` ada, baris `info` menang atas error refresh, dan label untuk sumber `unrecognized`.
- Fitur yang belum ada: "Verified runtimes" dan kalimat digest menunggu milestone 3 spec 0018; milestone 2 butuh langkah build 7 spec 0017.
- Lainnya: `OnboardingFooter.primaryLabel`, injeksi `onboardingStore` lewat `ShellRuntime` dan `ShellBootstrap` (usulan ke spec 0013), frame minimum di `MainWindowRoot`, tile Welcome netral, warna dan area klik titik langkah, `UserHome.current()` dengan batas komponen path, `@AccessibilityFocusState` dan catatan Keyboard navigation, dan Escape yang tidak ditahan aksi yang sedang berjalan.

## Penyelarasan 2026-10-20 (keputusan K foundation)

Spec ini diselaraskan dengan keputusan K18 program foundation dan dengan update spec 0012, 0015, dan 0017 di `main`. AC ID dan status `Proposed` tidak berubah.

- **K18, isi jendela utama: diganti.** Branch f13-wiring membangun `LiveShell(runtime:windowWillClose:)` dan `ShellBootstrap.assemble`. Di langkah 2, `MainWindowRoot(runtime:windowWillClose:)` menggantikan `LiveShell` sebagai isi `Window`, mengambil alih wiring nya, dan `LiveShell` dihapus (pilihan yang direkomendasikan; membuatnya internal hanya menyisakan dua jalur ke `AppShell`). Init `AppShell` tidak lagi menerima `windowOpener` dan `windowWillClose`; `AppShell.minimumSize` tetap ada supaya `ShellLayoutTests` (`35f6bf6`) tidak perlu di host ulang.
- **K18, teks spec 0013.** Pemindahan accessor, `openWindow`, dan frame, pengecualian `onboarding.finished`, dan signature `live(...onboardingStore:)` ditulis di commit langkah 2; update 0013 lead setelah f13-wiring hanya menulis `LiveShell` dan `assemble` seperti yang dibangun, ditambah baris batas `MainWindowRoot` milik spec ini.
- **K18, Bool `STACKLY_ROOT`.** Tidak ada parameter baru di `live` atau `assemble`: `assemble` menurunkan `DaemonModel.rootOverrideActive` dari `rootOverride != nil`, dan langkah Storage membacanya lewat `ShellRuntime.model`. Bool itu masuk di milestone 3 fitur 15 atau langkah 2 spec ini, mana yang lebih dulu memakainya. Nama `rootOverrideActive` dipilih di sini karena K18 tidak memberi nama.
- **K18, urutan.** Langkah 2 mengubah file yang sama dengan langkah 16 fitur 13 (`AppShell.swift`, `LiveShell.swift`, `StacklyApp.swift`, `ShellRuntime.swift`), jadi baru dimulai setelah langkah 16 di-merge; lewat langkah 16, milestone 1 juga menunggu milestone 2 fitur 10. Aturan observasi untuk revision yang sama milik store fase 02 pertama yang menerbitkannya; langkah Catalog Trust hanya membaca `CatalogStore.info` dan tidak bergantung padanya.
- **Spec 0012: `toneSurface` butuh `strength`.** Tile Welcome memakai `toneSurface(.neutral, on: .window, strength: .badge, in: RoundedRectangle(cornerRadius: Radius.small.value))`, fill 16% yang sama dengan mockup.
- **Spec 0015 di `main`.** Semua usulan spec ini untuk 0015 sudah diterapkan di sana (`unavailableDetail`, `CommandLineToolModel`, `CommandLineToolPrompts`, `bundledTool`, `UserHome.current()` dan `UserHome.tildePath(_:home:)`, penanda `// provisional: F32`), jadi milestone 2 memakai nya dari langkah 10 sampai 12 spec 0015 dan tidak membangunnya sendiri. `UserHome` dibuat di langkah 3 spec ini hanya kalau milestone 3 fitur 15 belum masuk.
- **Spec 0017 di `main`: nomor langkah bergeser.** `CatalogStore` kini langkah build 9 (langkah 8 menjadi `CatalogAppearance` dan `CatalogCopy`), jadi milestone 3 spec ini butuh langkah 9; `stackly provider list` tetap langkah 7.

## Perbaikan pasca penyelarasan 2026-10-20

Dari daftar `docs/scope/coordination/post-alignment/post-0016.md` (tidak ada keputusan lead yang menyentuh spec ini): milestone 1 kini juga butuh spec 0015 langkah 10 karena `OnboardingFooter.primaryLabel` memakai `BackgroundAgentState` (tanpa stub lokal); langkah 2 dan 3 tidak berjalan bersamaan dengan milestone 3 fitur 15 karena berbagi `ShellRuntime.swift`, `ShellBootstrap.assemble`, dan `DaemonModel.swift`; `ShellLayoutTests` hanya kehilangan dua argumen `AppShell(...)` di langkah 2; dan tabel batas menaruh tipe `CommandLineToolPrompts` di spec 0015 dan teks prompt nya di spec ini.

## Update 2026-10-21 dari spec 0067, 0068, dan 0069

- **AC-11 diubah (disetujui user 2026-10-21 lewat lead).** Dengan `FeedbackCenter` spec 0067, kartu umpan balik sudah mengumumkan pesannya sekali. Panggilan `announce("Setup complete.")` yang kedua membuat VoiceOver mengucapkan kalimat yang sama dua kali, jadi AC-11 kini meminta `FeedbackCenter.show(.success("Setup complete."))` saja. Kalimat kedua AC-11 (Escape tidak mengumumkan apa pun selain perpindahan ke shell) dan trace PAGE-01 tetap, karena usulan hanya mengganti bagian pengumuman.
- **`FeedbackCenter` di `MainWindowRoot`.** Spec 0067 menaruh `FeedbackCenter` di view akar jendela. Karena `MainWindowRoot` menggantikan `LiveShell`, ia juga mengambil alih `@State FeedbackCenter`, dan `OnboardingFlow` bisa memanggil `show` sebelum shell tampil.
- **Teks AC-8 tidak diubah.** Usulan spec 0067 menyebut AC-8 untuk `CopyButton` dan `ConsoleBlock`, tetapi teks AC-8 ("tombol Copy", "Blok contoh") sudah cocok dengan kedua komponen itu. Pemakaian komponennya ditulis di *Kontrol Command Line*, jadi tidak ada teks AC lain yang berubah.
- **Label baru.** "Copy PATH line" dan "Example commands" ditambah ke tabel *Copy* karena `CopyButton` dan `ConsoleBlock` butuh label VoiceOver dari layar.
- **Item menu Help dan snapshot punya dua jalur.** Spec 0069 milestone 2 dan milestone 1 spec 0068 belum di `main`, jadi langkah 2 dan 7 menulis jalur untuk kedua urutan merge, seperti usulan spec 0069 dan aturan *Key invariants* spec 0068.
