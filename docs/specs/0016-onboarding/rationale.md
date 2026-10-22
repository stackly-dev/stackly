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

- Idiom macOS, dan spec 0012 membatasi accent hanya untuk kontrol native, tint seleksi, dan link (sejak 2026-10-22 juga `StacklyColor.selectionRing`, tetapi hanya untuk cincin pilihan kartu kustom, tidak pernah untuk isian, jadi pilihan ini tetap berlaku).

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
- Kartu Background Agent: `{detail}` dari `BackgroundAgentModel.unavailableDetail` (usulan ke spec 0015), `since` dari `ShellPresentation.status`, Start stacklyd hanya untuk `ipc.endpoint_missing` dan `ipc.connection_refused`, baris untuk presence lain dan `connecting`, `TimelineView` untuk ambang 15 detik, dan pengumuman state tetap milik `BackgroundAgentModel` (diganti 2026-10-22: milik announcer `DaemonModel`, lihat *Update 2026-10-22 dari spec 0015*).
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
- **Cross check (2026-10-21).** Cross check `Another model` (Sonnet 5.5) atas update ini dan perbaikannya diterima lead atas delegasi user (2026-10-21): `FeedbackCenter` dibuat `MainWindowRoot` kalau belum ada, teks `CopyButton` dari `OnboardingCopy` dengan gaya `.text`, `CardSection` tanpa judul untuk baris Storage dan Catalog Trust (D15), dan ukuran snapshot `.fitting(width: 1280, narrowWidth: 900)` serta identifier dan assertion XCUITest onboarding di langkah 7 (D10).

## Update 2026-10-22 dari spec 0015

Sumber nya dua baris spec 0016 di *Usulan update untuk spec lain* spec 0015 (`main`, merge `c27cca1`), ditambah hasil spike AC-21 di update spec 0015 tanggal 2026-10-21 (B1) dan update tanggal 2026-10-22 (nama file CLI di bundle, Follow-up spec 0013, dan update spec 0013 dan 0068). Spec ini diselaraskan dengan spec 0015, 0013, dan 0068 di `main` (`c27cca1`, `36bce94`). ID AC, ID keputusan, dan status `Proposed` tidak berubah. Tidak ada perubahan protokol atau Rust.

| No | Sumber | Perubahan di spec ini |
| --- | --- | --- |
| 1 | nama file CLI di bundle, opsi B (dipilih user 2026-10-22) | setiap `Contents/MacOS/stackly` menjadi `Contents/MacOS/stackly-cli` (AC-8, *Kontrol Command Line*, *Value sourcing*, cek manual 3); perintah, nama link, dan teks yang menyebut `~/.local/bin/stackly` tetap |
| 2 | G3 spec 0015 | `MainWindowRoot` memasang `.environment(runtime.commandLineTool)` di samping `.environment(runtime.model)`, untuk kedua urutan merge dengan langkah 12 spec 0015 |
| 3 | G1 spec 0015 | pengumuman state agent milik announcer `DaemonModel` (AC-10 dan butir pengumuman di *Kartu Background Agent*) |
| 4 | G2, G7, G9 spec 0015 (`isStarting`, `sawHandshake`, state menjadi `enabled` di luar aksi) | baris `enabled` kartu memakai `isStarting`; progress tampil selama penantian daemon (AC-5, tabel kartu, butir `isWorking`) |
| 5 | B1 spec 0015 | `not_registered` untuk bundle yang belum pernah didaftarkan dan `not_found` hanya tepat setelah Turn On atau Start stacklyd, di tabel kartu, *Critical test scenarios*, dan cek manual 1; teks kartu tidak berubah, karena kartu mengikuti state |
| 6 | pemilik model di spec 0013 dan 0015 | `BackgroundAgentModel` dimiliki `DaemonModel` (`DaemonModel.agent`), `CommandLineToolModel` dimiliki `ShellRuntime` (`ShellRuntime.commandLineTool`), keduanya opsional; `ShellRuntime.init(model:closeNotice:commandLineTool:onboardingStore:)` |

### Keputusan kecil update ini

Opsi rekomendasi diambil menurut aturan user; yang mengubah teks AC dikirim ke lead lebih dulu.

- **Input kartu sama dengan banner.** `BackgroundAgentStepContent.make` menerima `AgentBanner.Input` dari `DaemonModel.agentBannerInput`, bukan enam parameter terpisah. Alasan: spec 0015 membuat satu properti itu supaya banner, `ConnectionHint`, dan predikat jadwal tidak pernah berbeda, dan kartu adalah tempat keempat yang membaca hal yang sama. Kode koneksi ikut pindah dari `DaemonModel.lastError?.code` ke `unreachableCode`, yang memang dibuat untuk itu. Runner up: hanya menambah parameter `isStarting`, yang lebih kecil tetapi membiarkan dua sumber kode koneksi.
- **Jam kartu sama dengan banner.** `now` = `fixtureNow ?? context.date` dan jadwal `DaemonBanner.ticksEverySecond(_:now:)` spec 0013, menggantikan `TimelineView(.explicit([since + startGrace]))` dengan "jam yang disuntik" yang tidak punya nama. Alasan: runtime fixture spec 0068 memakai `FixtureClock` tetap, dan tanpa `fixtureNow` snapshot kartu di detik ke 14 akan memakai jam nyata. Runner up: `.explicit` dengan `fixtureNow`, yang lebih hemat render tetapi menjadi mekanisme kedua untuk ambang yang sama.
- **`refreshing` tetap baris tersambung di kartu.** Banner membuka `.refreshing(after:)` supaya baris agent tetap tampil selama snapshot (aturan spec 0013 untuk banner). Kartu tidak, karena `refreshing` berarti daemon sudah menjawab handshake, dan progress penantian tetap tampil lewat `isWorking` sampai presence keluar dari `refreshing`. Runner up: membuka `refreshing` seperti banner, yang membuat kartu menulis "stacklyd is starting…" untuk daemon yang sudah menjawab.
- **Kartu `not_found` mengumumkan dirinya.** Tabel *Pengumuman* spec 0015 tidak memberi `not_found` teks per state karena banner critical nya yang mengumumkan, tetapi banner jendela tidak tampil selama onboarding (AC-1). Tanpa pengecualian, Turn On yang berakhir `not_found` di onboarding tidak terdengar, dan fokus VoiceOver hilang karena tombol Turn On ikut hilang. Kartu critical mengumumkan judulnya sekali, mengikuti aturan banner critical spec 0012. Runner up: usul ke spec 0015 supaya `DaemonModel` mengumumkan nya, ditolak karena `DaemonModel` tidak tahu onboarding sedang menutupi banner. *(Dipersempit 2026-10-22, lihat bagian Update 2026-10-22 dari langkah 12a spec 0015: sejak langkah 12a `DaemonModel` mengumumkan `not_found` saat baris agent banner tidak tampil, dan onboarding hanya menutup kasus banner yang tersembunyi, dari `OnboardingFlow`.)*
- **Kegagalan aksi agent diusulkan ke spec 0015, bukan diumumkan onboarding.** `lastError` setelah Turn On atau Start stacklyd tidak terdengar di mana pun hari ini, termasuk banner dan Diagnostics. Pemilik nya announcer `DaemonModel`, jadi spec ini mengusulkannya ke spec 0015 dan tidak menambah announcer kedua. Runner up: kartu onboarding mengumumkan `lastError` sendiri, yang akan terdengar dua kali begitu spec 0015 menambahkannya. *(Diterapkan di spec 0015 2026-10-22 sebagai closure `didFail`, langkah 12a.)*
- **Model dari environment sebagai opsional.** View onboarding membaca `DaemonModel` dan `CommandLineToolModel` dari environment seperti seksi Diagnostics spec 0015, dan tanpa model kartu atau baris CLI tidak tampil. Runner up: memberi `runtime` ke `OnboardingFlow` lewat init, yang memaksa preview dan test membuat runtime lengkap.
- **"Or run it directly" dari `bundledTool`.** Path baris itu diambil dari `CommandLineToolModel.bundledTool`, bukan dari `Bundle.main.bundleURL` ditambah akhiran, dan baris itu hilang kalau `bundledTool` nil. Alasan: spec 0015 menjaga model di atas fake tidak pernah membaca `Bundle.main`, dan path yang ditampilkan harus file yang benar ada. Runner up: `Bundle.main` dengan akhiran `stackly-cli`, yang di runtime fixture menampilkan bundle Xcode yang sedang berjalan.
- **Urutan parameter `ShellRuntime.init`.** `commandLineTool:` sebelum `onboardingStore:`, mana pun yang masuk lebih dulu, supaya pemanggil di kedua urutan merge tidak berubah dua kali.
- **Larangan berjalan bersamaan diperluas.** Langkah 2 dan 3 tidak berjalan bersamaan dengan langkah 10 sampai 12 spec 0015 (sebelumnya 10 dan 12), karena langkah 11 juga mengubah `ShellBootstrap.fixture` dan `DaemonModel.swift`, dan langkah 12 menulis `LiveShell.swift`, yang dihapus langkah 2.

### Cross check update ini

Cross check `Another model` (Sonnet 5.5, baca saja, 2026-10-22). Putusan: kedua baris spec 0016 di tabel usulan spec 0015 sudah diterapkan penuh, dan tidak ada teks lama yang tertinggal di `index.md`. Tiga belas temuan, semuanya diisi dengan rekomendasi saya (aturan opsi rekomendasi; yang mengubah teks AC hanya penajaman pemicu pengumuman di AC-10, yang ikut dikirim ke lead):

| No | Temuan | Yang diterapkan |
| --- | --- | --- |
| 1 | `make` tidak punya sumber tone footer dan `StatusLabel`; label critical "stacklyd unavailable" di bawah baris "stacklyd is starting…" | parameter `footer: StatusAppearance` dari `DaemonModel.presentation.footer`; label itu diterima apa adanya karena jujur |
| 2 | mekanisme pengumuman kartu `not_found` tidak disebut, dan bisa memotong pengumuman langkah | `onChange(of: content.row)` tanpa `initial`, hanya saat berubah menjadi `not_found`; tidak saat langkah dibuka (diganti 2026-10-22: `onChange(of: assessment.state)` di `OnboardingFlow`, lihat *Update 2026-10-22 dari langkah 12a spec 0015*) |
| 3 | `lastError` tertinggal di samping daemon yang sudah tersambung | baris `enabled` tersambung tidak menampilkan `lastError`; Diagnostics tetap |
| 4 | fokus keyboard hilang saat tombol aksi kartu hilang | `@FocusState` `cardAction` dan `primary` di `OnboardingFlow` |
| 5 | langkah Command Line tanpa `isWorking`, modifier prompt, dan init `OnboardingFlow` | `make(...isWorking:)` dengan "Working…", `.stacklyConfirmation(item:)`, `OnboardingFlow(onboarding:)` |
| 6 | snapshot per baris tabel tidak punya cara | view isi yang menerima nilai `make(...)` dan pembungkus tipis; snapshot per langkah lewat skenario `onboarding` |
| 7 | nama presence di tabel kartu tidak dipetakan ke `ShellStatus`; `failureCode` nil; kunci `fixtureNow` | pemetaan case `ShellStatus`, nil masuk "presence lain", `@Environment(\.fixtureNow)` |
| 8 | nilai fixture `bundledTool` berbeda dengan spec 0068 di `main` | Follow-up yang menunjuk baris 0068 di usulan spec 0015 |
| 9 | Storage masih membaca `runtime.model` | `model?.lastDaemon?.info?.roots` dan `model?.rootOverrideActive ?? false` dari environment |
| 10 | spec 0015 langkah 12 hanya menyebut `LiveShell` | butir (d) di baris usulan spec 0015 |
| 11 | urutan baris selama penantian dan arti "tersambung" | butir `isWorking` mengikuti presence dan menyebut arti "sampai daemon tersambung" di AC-5 |
| 12 | pengumuman `DaemonModel` saat banner tersembunyi; pergantian di detik ke 15 | diterima dan ditulis; pergantian di detik ke 15 tidak diumumkan |
| 13 | catatan lama di rationale bagian *Cross check* | ditandai diganti |

Yang tidak diambil: meminta spec 0015 memakai pemetaan `refreshing` milik kartu untuk banner (aturan banner milik spec 0013, dan risiko sisanya sudah dicatat di spec 0015), dan predikat "daemon tidak ada" bersama di `AgentBanner` (dua baris logika yang sama dengan banner, dijaga test per baris; kalau banner berubah, test kartu ikut gagal). Satu catatan soundness dijadikan usulan untuk spec 0015, butir (e): penantian daemon yang tetap berjalan saat assessment berubah dari `enabled` di tengah nya.

### Perubahan teks AC (menunggu persetujuan user lewat lead)

- **AC-5**: `isStarting` masuk daftar kombinasi, dan kalimat "Kartu berganti tepat di detik ke 15 tanpa menunggu polling" menjadi berlaku selama tidak `isStarting`, dengan kalimat baru untuk penantian daemon (G2, G7, G9).
- **AC-8**: `Contents/MacOS/stackly` menjadi `Contents/MacOS/stackly-cli`, dan "path lengkap `stackly` di bundle" menjadi path lengkap CLI di bundle (opsi B nama file CLI).
- **AC-10**: pengumuman state agent milik announcer `DaemonModel` (G1), dengan satu pengecualian untuk kartu `not_found`.

AC lain tidak berubah. AC-2 tetap benar: label tombol utama tetap dari state assessment, hanya jalurnya sekarang `DaemonModel.agent`.

## Update 2026-10-22 dari langkah 12a spec 0015

Sumber nya satu baris spec 0016 di *Usulan update untuk spec lain* spec 0015 (`main`, merge `dcfc2446`), balasan spec 0015 atas usulan (a) sampai (e) spec ini, yang dibangun langkah susulan 12a fitur 15. ID AC, ID keputusan, dan status `Proposed` tidak berubah. Tidak ada perubahan protokol, Rust, atau model fitur 15: semua perilaku baru ada di `BackgroundAgentModel` dan announcer `DaemonModel` (langkah 12a), dan spec ini hanya membaca nya.

| No | Butir usulan spec 0015 | Perubahan di spec ini |
| --- | --- | --- |
| 1 | kartu `not_found` mengumumkan judul nya hanya saat `AgentBanner.content(agentBannerInput, now:)` tidak nil | butir pengumuman di *Kartu Background Agent*, AC-10, *Copy*, *API surface* (`OnboardingAnnouncements.agentNotFound`), *Value sourcing*, test AC-10 |
| 2 | Last error diumumkan announcer `DaemonModel` lewat `didFail`, bukan kartu | butir `lastError`, AC-10, *Value sourcing* |
| 3 | penantian daemon berakhir tanpa error saat state menjadi `requires_approval`, `disabled`, `not_registered`, atau `not_found` (tidak `unavailable`) | butir `isWorking`, butir baru akhir penantian per state, AC-5, *Value sourcing*, test alur AC-5 |
| 4 | langkah 12a spec 0015 prasyarat langkah 4; test alur langkah 4 memakai butir 1 sampai 3 | pengantar *Build plan*, langkah 4, *Critical test scenarios* |

Usulan untuk spec 0013 milik lead ui-macos (`didFail` di `attach`, dan kalimat "bukan `DaemonModel`" yang menjadi bersyarat) ditulis sebagai baris 0013 di *Usulan untuk spec lain*; spec 0013 tidak diedit. Penanda Diterapkan di baris spec 0015 ditulis di commit yang sama, tanpa perubahan lain di spec 0015.

### Keputusan kecil update ini

Opsi rekomendasi diambil menurut aturan delegasi; yang mengubah teks AC dikirim ke lead lebih dulu.

- **Pemicu pengumuman `not_found` di `OnboardingFlow`, bukan view kartu.** Sebelum nya kartu mengumumkan judul nya saat baris nya berubah di langkah yang sedang tampil, jadi Turn On yang hasil nya datang setelah pengguna pindah ke Storage (panggilan OS bisa sampai 10 detik) tidak terdengar di mana pun: banner tersembunyi, kartu tidak dirender, dan sejak langkah 12a `DaemonModel` diam karena baris agent banner "tampil". Banner yang digantikan tersembunyi di kelima langkah, jadi pengganti nya ada di view yang juga tampil di kelima langkah. Pemicu nya perubahan `assessment.state`, bukan baris kartu, karena baris `not_found` hanya bergantung pada state itu, dan `OnboardingFlow` tidak punya `TimelineView`. Runner up: tetap di view kartu dengan celah pindah langkah dicatat sebagai diterima, yang lebih kecil tetapi meninggalkan satu jalur tanpa pengumuman.
- **Fungsi murni `OnboardingAnnouncements.agentNotFound(old:new:input:now:)`.** Syarat nya dihitung dengan `AgentBanner.content`, fungsi yang sama dengan announcer `DaemonModel`, dan dibalik, supaya di setiap presence tepat satu dari keduanya berbicara, dan supaya pasangan itu bisa diuji per presence tanpa meng host view. Runner up: syarat ditulis langsung di `onChange` view, yang hanya bisa diuji lewat snapshot atau XCUITest. Sambungan `onChange` nya tidak di host di test, sama dengan pengumuman tahap spec 0067 (`stageAnnouncement`, nilai murni), dan pindaian sumber membatasi `announce(` di `Onboarding/` ke `OnboardingFlow.swift` (cross check 1).
- **Teks yang diumumkan adalah judul banner, bukan judul kartu** *(cross check 2)*. `agentNotFound` mengembalikan `title` dari `AgentBanner.content` ("Stackly can’t find its background service.", dengan titik), karena pengumuman ini menggantikan banner yang tersembunyi, dan spec 0015 menjamin teks `not_found` tidak pernah berbeda antara `StateBanner` dan `DaemonModel`. Judul kartu di `OnboardingCopy` tetap tanpa titik dan tidak lagi diumumkan, jadi entri pengumuman nya hilang dari tabel *Copy*. AC-9 tidak berubah: teks itu milik spec 0015, dan tabel *Copy* serta *Value sourcing* menyebut pengecualian nya. Runner up: satu konstanta bersama untuk judul kartu dan pengumuman, yang membuat kartu berbeda dari gaya judul kartu lain atau pengumuman berbeda dari banner.
- **Nilai lama nil selalu diam** *(cross check 3)*, sama dengan `DaemonModel` yang mencatat assessment pertama tanpa teks. Dalam praktik probe pertama tidak pernah `not_found`, jadi aturan ini hanya menutup tabel test.
- **Fokus setelah aksi kartu lewat flag** *(cross check 8)*. `.disabled(reason:)` memasang `.disabled(true)`, jadi fokus keyboard bisa lepas dari tombol yang ditekan selama `isWorking`, dan aturan lama ("selagi fokus di `cardAction`") tidak pernah terpicu. Flag `focusAfterCardAction` yang dipasang tombol aksi dan dilepas setelah fokus dipindah sekali membuat hasil nya tetap: aksi kartu kalau baris saat itu punya aksi aktif, selain itu tombol utama. Runner up: membuat tombol tetap bisa difokus selama `isWorking`, yang menyimpang dari `.disabled(reason:)` spec 0012.
- **Celah waktu antara dua pemeriksaan diterima.** `DaemonModel` memeriksa saat `didAssess`, onboarding pada render berikut nya. Transisi presence tepat di antara keduanya bisa memberi nol atau dua pengumuman. Menutup nya butuh `DaemonModel` menerbitkan keputusan nya (misalnya flag "sudah diumumkan"), yang mengubah spec 0015 untuk kasus yang butuh dua kejadian main actor dalam satu siklus render. Dicatat di *Consequences*.
- **State `unavailable` selama penantian.** Spec 0015 membiarkan `unavailable` tidak mengakhiri penantian. Kartu lalu menampilkan baris `unavailable` nya (Check Again) dengan aksi nonaktif dan progress sampai penantian berakhir menurut aturan spec 0015 (presence bukan `connecting`, `unreachable`, atau `refreshing`, presence kembali `unreachable` sesudah handshake, salah satu dari empat state, atau 30 detik sebelum handshake; handshake sendiri hanya mencabut batas 30 detik), dan `unavailable` yang menjadi `enabled` tidak memulai penantian baru (cross check 6). Ini ditulis eksplisit supaya builder tidak mengira baris itu salah. Runner up: kartu menyembunyikan baris `unavailable` selama `isStarting`, yang membuat kartu berbeda dari banner untuk keadaan yang sama.

### Cross check update ini (langkah 12a)

Cross check `Another model` (Sonnet, baca saja, 2026-10-22). Putusan: keempat butir baris usulan spec 0015 diterapkan penuh, tidak ada teks lama `content.row` atau "di langkah yang sedang tampil" yang tertinggal di `index.md`, dan pemindahan pemicu ke `OnboardingFlow` sound (XOR per presence tepat, `agent` nil, menu Help, dan buka ulang jendela diam; Start stacklyd yang berakhir `not_found` tertutup). Dua belas temuan, semuanya diisi dengan rekomendasi saya (aturan delegasi):

| No | Temuan | Yang diterapkan |
| --- | --- | --- |
| 1 | sambungan `onChange` tanpa test; larangan pengumuman dari `Onboarding/` tanpa test | pola spec 0067 (nilai murni diuji, sambungan direview), pindaian sumber `announce(` hanya di `OnboardingFlow.swift` |
| 2 | teks pengumuman tanpa titik berbeda dengan banner dan `DaemonModel` | `agentNotFound` mengembalikan `title` banner; entri *Copy* diganti (keputusan kecil kedua) |
| 3 | nil ke `not_found` tanpa hasil yang disebut | nil selalu diam |
| 4 | setup test samar (`.os`, launch record, `setStatus`) | `.invalidSignature` dengan teks nya, `FakeLaunchRecordReader.set(_:)` yang disimpan rig, `setStatus` |
| 5 | kalimat test "tidak pernah oleh folder `Onboarding/`" bertentangan dengan pengecualian | "kecuali `not_found` menurut tabel pasangan" |
| 6 | handshake ditulis mengakhiri penantian; kalimat `enabled` lagi salah tempat | aturan akhir spec 0015 disalin; `unavailable` ke `enabled` tidak memulai apa pun |
| 7 | jendela ditutup selagi Turn On berjalan | dicatat di *Consequences* sebagai diterima, klaim "presence mana pun" dibatasi "selagi jendela utama terbuka" |
| 8 | fokus lepas dari tombol nonaktif selama `isWorking` | flag `focusAfterCardAction` (keputusan kecil keempat) |
| 9 | `not_found` di himpunan akhir penantian tidak tercapai | ditulis, tanpa test kartu untuk itu |
| 10 | `AgentBanner.content` dengan input opsional tidak terkompilasi; `fixtureNow` tidak perlu | `flatMap`, `now` = `Date.now` |
| 11 | teks lama di rationale; kata kata kartu di tabel *Pengumuman* spec 0015; tense Follow-up; `OnboardingFlow.swift` di sesi paralel; 12a bersama milestone 1; tabel batas | semua diperbaiki; kata kata spec 0015 menjadi baris usulan baru (spec 0015 tidak diedit selain penanda) |
| 12 | cakupan tabel test `refreshing(after:)` | ditambah sesudah `connected` dan sesudah `unreachable` dengan `ipc.call_timeout` |

Alternatif yang lebih sederhana dari cross check, tidak diambil di update ini dan dikirim ke lead sebagai pilihan: flag di `DaemonModel` (misalnya `windowBannerHidden`, diisi `ShellRuntime` dari `onboarding.isPresented`), sehingga `DaemonModel` mengumumkan `not_found` saat baris agent banner nil atau banner tersembunyi, dan onboarding tidak mengumumkan state agent sama sekali. Itu menghapus `OnboardingAnnouncements`, celah waktu render, test XOR, dan pengecualian AC-10, dan murah selama langkah 12a belum dibangun. Tidak diambil karena mengubah lagi spec 0015 dan 0013 (di luar izin brief update ini) dan membalik runner up lama ("`DaemonModel` tidak tahu onboarding menutupi banner"); lead dan pemilik spec 0015 yang memutuskan.

### Perubahan teks AC (disetujui user 2026-10-22)

Kedua perubahan di bawah disetujui user langsung di sesi update ini ("lakukan", 2026-10-22), setelah lead menerima spec nya atas aturan delegasi.

- **AC-5**: progress penantian juga berakhir saat state yang dibaca menjadi `requires_approval`, `disabled`, `not_registered`, atau `not_found`, tanpa Last error, dan `unavailable` tidak mengakhiri nya (butir 3).
- **AC-10**: kegagalan aksi agent (teks Last error) masuk daftar yang diumumkan announcer `DaemonModel` (butir 2), dan pengecualian `not_found` menjadi bersyarat: onboarding mengumumkan judul baris banner agent nya ("Stackly can’t find its background service.") di langkah mana pun selagi onboarding tampil, hanya saat banner jendela yang tersembunyi akan menampilkan baris agent; di presence lain `DaemonModel` yang mengumumkan nya (butir 1 dan keputusan kecil pertama dan kedua).

AC lain tidak berubah. Teks lama dan baru lengkap ada di catatan lead `arc-f16-update-2-ac-and-scope.md`.
