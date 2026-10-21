# 0069. Menu aplikasi, perintah, dan shortcut keyboard: rationale

[Kembali ke spec](index.md)

## Context

Fitur 69 di `docs/scope/ui-macos.md` meminta menu Stackly, File, Edit, View, Window, dan Help beserta peta shortcut, supaya setiap aksi utama di mockup UI bisa dijalankan tanpa mouse sesuai idiom macOS. Done when nya punya empat syarat: peta perintah ada sejak shell dan setiap fitur journey mendaftar di sana; shortcut tidak bentrok dengan shortcut sistem; perintah nonaktif tepat saat tombolnya nonaktif (daemon offline, operasi berjalan, capability unsupported); dan Help membuka dokumentasi pengguna fitur 66. Trace nya REQ-027, AC-027, dan NFR-012: alur membuat, menghentikan, dan memeriksa instance harus selesai dengan keyboard dan VoiceOver.

Saat spec ini ditulis (2026-10-21), `main` punya shell spec 0013 (`StacklyApp` dengan scene `Window` utama, `Settings` placeholder, dan Design Gallery Debug; `AppShell` dengan pilihan sidebar di `@SceneStorage("shell.section")`; `DaemonModel.reconnect()`), dan pola state spec 0012 (`MutationAvailability`, `.mutationGated()`, `.disabled(reason:)`, `.stacklyConfirmation`). Belum ada `.commands`, `CommandGroup`, atau shortcut selain `.defaultAction` dan `.cancelAction` di `SecretEntrySheet`. Enam spec yang sudah diterima menunda bagian menu nya ke fitur 69: 0013 (Reconnect, pindah bagian), 0016 (Welcome to Stackly, perintah shell nonaktif saat onboarding), 0017 (⌘F di Catalog), 0020 (New Instance ⌘N, dengan `CommandGroup` sementara), 0021 (detail dan copy connection), dan 0022 (`InstancesCommands` untuk urutan, inspector ⌥⌘I, dan ⌘F). Sebagian akan dibangun sebelum fitur ini, sebagian sesudahnya, jadi urutan merge tidak bisa diandaikan.

Ada tiga kekuatan teknis. Pertama, di SwiftUI menu didefinisikan di level `App` lewat `Commands`, jauh dari view yang memegang state halaman, jadi perintah harus mendapat aksi dan status nonaktifnya dari jendela yang aktif. Kedua, AppKit mencocokkan key equivalent menu sebelum field teks menerima tombol, jadi shortcut yang salah pilih mencuri pengeditan teks atau menimpa makna standar macOS. Ketiga, fitur 67 (berjalan paralel) membangun deskriptor aksi yang dipakai tombol, menu More, dan menu konteks, dan fitur 68 (juga paralel) membangun cek otomatis UI; keduanya harus bertemu dengan peta ini tanpa daftar kedua.

Konsekuensi tidak memutuskan: setiap fitur menambah `CommandGroup` sendiri dengan shortcut pilihannya, bentrok ketahuan pengguna, dan status nonaktif menu menyimpang dari tombol karena dihitung di dua tempat.

## Keputusan

### K1: mekanisme peta dan penyambungan

- **Opsi 1 (dipilih): `CommandMap` data murni plus `StacklyCommands`, dan layar menerbitkan `StacklyAction` milik fitur 67 lewat `focusedSceneValue`.** Pro: satu sumber judul dan shortcut yang bisa dites tanpa UI; menu dan tombol membaca action dan fungsi nonaktif yang sama; idiom SwiftUI untuk menu yang bergantung pada jendela aktif; baris planned mencadangkan shortcut sebelum fiturnya ada. Kontra: butuh fitur 67 milestone 1 lebih dulu; `focusedSceneValue` tidak membedakan sheet dari jendela induknya, sehingga perlu aturan key window tambahan (K2).
- **Opsi 2: registry global `@Observable` di `ShellRuntime`, layar mendaftarkan closure saat muncul.** Pro: tidak bergantung pada perilaku focused value. Kontra: pendaftaran dan pencabutan manual mudah bocor (closure basi setelah view hilang), tidak otomatis mengikuti jendela aktif, dan menjadi state global kedua di samping environment.
- **Opsi 3: responder chain AppKit (`NSApp.sendAction` dengan selector).** Pro: model asli AppKit untuk validasi menu. Kontra: melawan arsitektur SwiftUI proyek, butuh `NSResponder` per halaman, dan tidak bisa membaca environment `MutationAvailability`.

### K2: kapan perintah shell aktif

- **Opsi 1 (dipilih): perintah shell dan halaman aktif hanya saat jendela utama adalah key window dan `AppShell` menerbitkan perintah; perintah app selalu aktif.** Pro: onboarding, Settings, sheet, dan dialog mematikan perintah shell tanpa daftar pengecualian; sesuai kebiasaan macOS (perintah jendela mati saat jendela lain aktif); menjawab follow up spec 0016 lewat konstruksi. Kontra: ⌘N tidak bekerja saat jendela utama tertutup; pengguna menekan ⌘0 dulu.
- **Opsi 2: ⌘N dan pindah bagian membuka jendela utama dulu kalau tertutup.** Pro: satu tombol cukup dari mana saja. Kontra: perintah harus menunggu jendela muncul lalu meneruskan aksi ke view yang baru lahir, butuh antrean aksi lintas siklus hidup view, dan tetap harus menolak saat onboarding.

### K3: tempat aksi instance

- **Opsi 1 (dipilih): menu objek baru "Instance" di antara View dan Window.** Pro: tempat menu khusus app menurut HIG; aksi pada pilihan berkumpul seperti menu Message di Mail; VoiceOver dan Help search menemukannya dengan nama objeknya. Kontra: menambah satu menu di luar enam menu yang digambar mockup (dicatat sebagai selisih mockup).
- **Opsi 2: aksi instance di menu File.** Pro: tetap enam menu seperti mockup. Kontra: File menjadi campuran membuat sesuatu dan mengoperasikan pilihan, berlawanan dengan arti File di macOS.

### K4: pindah bagian

- **Opsi 1 (dipilih): View > Overview ⌘1 sampai Diagnostics ⌘7 dalam urutan sidebar, plus `SidebarCommands` (⌃⌘S).** Pro: pola ⌘angka untuk bagian dikenal dari Xcode, Safari, dan Mail; tujuh bagian pas di ⌘1 sampai ⌘7; tidak ada shortcut sistem di sana. Kontra: angka harus digeser kalau sidebar mendapat bagian baru di tengah.
- **Opsi 2: tanpa shortcut, hanya item menu.** Pro: tidak ada yang dicadangkan. Kontra: pindah bagian adalah aksi paling sering, dan keyboard harus lewat sidebar dengan Tab dan panah.
- **Opsi 3: ⇧⌘ dengan huruf seperti menu Go di Finder.** Pro: mnemonik. Kontra: beberapa huruf bentrok (⇧⌘C Show Colors, ⇧⌘D, ⇧⌘O) dan tidak ada menu Go di daftar menu scope.

### K5: shortcut aksi instance

- **Opsi 1 (dipilih): keluarga R. Start ⌘R, Stop ⇧⌘R, Restart ⌥⌘R, Reload tanpa shortcut; Copy Connection String ⇧⌘C; Open ⌘O; Back ⌘[; Duplicate… ⌘D; Remove… tanpa shortcut.** Pro: mengikuti idiom macOS bahwa ⇧ membalik (Undo dan Redo, Find Next dan Previous) dan ⌥ memberi varian (Hide Others, Close All); ⌘R akrab bagi developer sebagai "Run" di Xcode; Open, Back, dan Duplicate memakai makna standar. Kontra: ⌘R dan ⇧⌘C punya makna di menu Format HIG (Show Ruler, Show Colors), jadi butuh pengecualian tertulis; ⇧⌘R menghentikan instance tanpa konfirmasi.
- **Opsi 2: semua lifecycle tanpa shortcut, hanya lewat menu.** Pro: nol risiko salah tekan dan nol bentrok. Kontra: alur harian yang paling sering (start dan stop) jadi lambat bagi pengguna keyboard.
- **Ditolak: ⌘. untuk Stop** (gaya Xcode), karena ⌘. adalah Cancel di macOS (dialog, operasi, field), dan key equivalent menu akan menghentikan database saat pengguna bermaksud membatalkan sesuatu. **Ditolak: ⌘⌫ untuk Remove**, karena mencuri "hapus sampai awal baris" di field teks.

### K6: target perintah instance

- **Opsi 1 (dipilih): instance yang detailnya terbuka, selain itu baris terpilih di Instances; Overview tidak punya pilihan sehingga perintah instance nonaktif di sana; satu penerbit, halaman Instances.** Pro: target selalu jelas dan terlihat; satu penerbit menghindari dua nilai focused yang bersaing antara tabel dan detail di `NavigationStack`. Kontra: aksi baris Overview tetap hanya lewat tombol.
- **Opsi 2: Overview menerbitkan instance yang tombolnya terakhir difokus.** Pro: menu juga bekerja di Overview. Kontra: target tidak terlihat dan berubah karena fokus, berbahaya untuk Stop.

### K7: tempat Reconnect

- **Opsi 1 (dipilih): menu Stackly > "Reconnect to stacklyd", tanpa shortcut, aktif tepat saat tombol Reconnect di Diagnostics aktif.** Pro: daemon adalah bagian app, dan menu app adalah tempat aksi tingkat app; aturan aktif disalin dari satu tombol yang sudah ada. Kontra: menu app biasanya pendek dan jarang dibuka.
- **Opsi 2: menu View.** Pro: dekat dengan item pindah bagian (Open Diagnostics). Kontra: View adalah tampilan, bukan koneksi.

### K8: Find

- **Opsi 1 (dipilih): ganti grup text editing bawaan dengan Edit > Find > Find… ⌘F yang memfokuskan field pencarian halaman lewat `.searchFocused`.** Pro: ⌘F benar benar sampai ke field toolbar dan nonaktif di halaman tanpa pencarian; satu sumber ⌘F. Kontra: submenu Spelling, Substitutions, Transformations, dan Speech hilang untuk semua field (Stackly berisi nama, port, dan path, bukan teks panjang).
- **Opsi 2: biarkan Find bawaan.** Pro: tidak ada kode. Kontra: Find bawaan mengirim `performFindPanelAction:` ke first responder dan tidak menjangkau field `.searchable` di toolbar, jadi ⌘F tidak melakukan apa pun di Instances dan Catalog.

### K9: Help sebelum fitur 66

- **Opsi 1 (dipilih): ganti grup Help bawaan dengan Welcome to Stackly; "Stackly Help" ⌘? dicadangkan dan ditambah fitur 66.** Pro: tidak ada item yang hanya memunculkan "Help isn’t available"; konsisten dengan aturan scope bahwa fitur yang belum dibangun tidak tampil seolah bekerja. Kontra: sampai fitur 66, Help hanya berisi kolom pencarian sistem dan Welcome.
- **Opsi 2: sementara Stackly Help membuka README GitHub.** Pro: ada tujuan sekarang. Kontra: README adalah dokumen kontributor, bukan dokumentasi pengguna, dan butuh jaringan; fitur 66 harus menggantinya lagi.

### K10: membuka jendela utama

- **Opsi 1 (dipilih): Window > Stackly ⌘0 lewat `.keyboardShortcut` scene `Window`.** Pro: pola Mail (⌘0 untuk jendela utama); memberi jalan keyboard kembali setelah ⌘W, yang dibutuhkan K2. Kontra: satu shortcut lagi dicadangkan; bergantung pada probe P2.
- **Opsi 2: tanpa shortcut, buka lewat Dock atau menu Window.** Pro: tanpa probe. Kontra: setelah ⌘W tidak ada jalan keyboard cepat kembali ke shell.

### K11: alasan nonaktif di menu utama

- **Opsi 1 (dipilih): subtitle item menampilkan alasan bila action ada dan alasannya teks, mengikuti cek macOS 27 spec 0067.** Pro: sama dengan menu di dalam jendela (fitur 67); VoiceOver membaca alasan di tempat. Kontra: bergantung pada hasil cek platform; item yang nonaktif karena konteks tetap tanpa alasan.
- **Opsi 2: menu tanpa alasan.** Pro: menu lebih pendek. Kontra: pengguna menu tidak tahu kenapa New Instance mati; alasan hanya di banner halaman.

### K12: cek bentrok

- **Opsi 1 (dipilih): test unit peta (unik, tabel sistem, makna standar, pengecualian, aturan teks, ellipsis), source rule, dan probe accessibility di langkah 1 dan `/check verify`; XCUITest menu berulang milik fitur 68.** Pro: bentrok di peta gagal di `swift test` tanpa menjalankan app; probe AX menangkap item yang ditambah macOS sendiri; fitur 68 membaca peta sebagai ekspektasi, jadi tidak ada daftar kedua. Kontra: daftar sistem disusun dari pengetahuan dan diperbarui lewat probe, bukan dari API; probe AX butuh izin Accessibility dan dijalankan manual.
- **Opsi 2: hanya XCUITest.** Pro: menguji app nyata. Kontra: `XCUIElement` macOS tidak membuka key equivalent item menu (dicek arc-f68-spec di header SDK Xcode 27), jadi duplikat tidak terdeteksi.

### K13: urutan build dan pemindahan

- **Opsi 1 (dipilih): yang di merge belakangan memindahkan yang lain. Build ini memindahkan `CommandGroup` sementara (`// provisional: F69`) milik fitur yang sudah di merge; fitur yang dibangun sesudahnya langsung mengisi baris planned; milestone yang memakai `StacklyAction` menunggu fitur 67 milestone 1.** Pro: tidak ada fitur yang menunggu fitur lain hanya untuk satu item menu; penanda provisional membuat sisa pemindahan bisa dicari dengan grep. Kontra: ada periode `CommandGroup` sementara hidup dengan aturan aktif yang lebih kasar.
- **Opsi 2: semua menu menunggu fitur 69.** Pro: satu cara sejak awal. Kontra: ⌘N dan Welcome to Stackly tertahan oleh fitur yang tidak ada di journey mereka, berlawanan dengan pendekatan Journey.

## Rationale

Keputusan terbesar adalah K1 dan K2, karena keduanya menentukan apakah Done when "nonaktif tepat saat tombolnya nonaktif" bisa dijaga atau hanya diharapkan. Dengan menu membaca `StacklyAction` yang sama dan `availability(gate:)` yang sama dengan `ActionButton`, kesamaan itu menjadi sifat struktur: tidak ada kode aktif kedua yang bisa menyimpang. Registry global (opsi 2 K1) bisa mencapai hal yang sama, tetapi harus menebak jendela aktif dan merawat pendaftaran, sedangkan focused scene value sudah mengikuti scene yang aktif. Kekurangannya (sheet masih bagian scene yang sama) ditutup dengan satu fakta AppKit yang sederhana dan bisa dites: sheet, dialog, dan jendela Settings punya window sendiri yang menjadi key window.

Untuk shortcut (K4, K5), yang menentukan adalah kebiasaan macOS dan keselamatan data. ⌘angka untuk bagian dan makna standar untuk Open, Back, Duplicate, Find, dan New tidak butuh dihafal. Untuk lifecycle, keluarga R memberi satu huruf yang mudah diingat dengan aturan modifier yang sudah dipakai macOS, dan menghindari ⌘. yang di macOS berarti batal. Aksi destruktif tidak pernah mendapat shortcut karena dialog konfirmasi spec 0012 sengaja tidak menjalankan aksi dengan Return; shortcut untuk aksi itu hanya membuka dialog yang harus dijawab dengan Tab dan Space, tanpa menghemat apa pun.

Peta sebagai data murni dengan baris planned (K1, K12) menjawab syarat "tidak bentrok" untuk fitur yang belum ada: begitu baris dicadangkan, fitur lain tidak bisa memakai shortcut itu tanpa test gagal. Ini juga yang membuat fitur 68 bisa menguji menu nyata tanpa daftar kedua. Urutan build K13 mengikuti Journey: fitur 16 dan 20 tidak menunggu fitur 69 untuk satu item menu, dan penanda provisional membuat pemindahan nya tercatat.

## Keputusan kecil

Diputuskan saat menulis spec, dengan alasan dan runner up:

- **Modifier sendiri (`CommandModifiers`), bukan `EventModifiers` SwiftUI, di peta.** Peta tetap `nonisolated` dan `Sendable` dan bisa dibaca target UI test fitur 68 tanpa SwiftUI; konversi ke `KeyboardShortcut` hanya di `CommandItem`. Runner up: `EventModifiers` langsung, yang menyeret SwiftUI ke data murni.
- **`entry(for:)` sebagai `switch` lengkap.** Kompiler memastikan setiap `CommandID` punya baris, tanpa lookup yang bisa gagal. Runner up: kamus dengan `expect`, yang memindahkan kesalahan ke runtime.
- **Urutan instance sebagai tujuh `CommandID` dengan `checked`.** Generik untuk item bertanda centang mana pun, tanpa tipe khusus fitur 22 di `PublishedCommands`. Runner up: satu binding `InstanceSort` di `PublishedCommands`.
- **Judul generik ("Start", "Stop"), bukan "Start “pg17-dev”".** Mengikuti menu macOS yang bertindak pada pilihan (Finder, Mail); target terlihat di halaman. Runner up: judul bernama, yang lebih jelas di VoiceOver tetapi membuat menu berubah setiap pilihan berubah.
- **Show Inspector dan Hide Inspector sebagai judul yang berganti.** Pola HIG untuk tampilkan dan sembunyikan (sama dengan Show Sidebar). Runner up: satu judul dengan centang.
- **`SidebarCommands` dipakai, `ToolbarCommands` dan `InspectorCommands` tidak.** `InspectorCommands` membawa shortcut sendiri yang berbeda dengan ⌥⌘I yang sudah diputuskan spec 0022; toolbar Stackly tidak bisa diatur pengguna.
- **Requirements di peta deskriptif.** Aturan aktif hanya satu (`resolve` atas action); requirements menjadi ekspektasi test penerbit dan fitur 68. Runner up: `resolve` juga mengevaluasi requirements, yang berarti aturan kedua yang bisa menyimpang dari action.
- **Reconnect diterbitkan hanya tanpa koneksi.** Sama dengan tombol Diagnostics yang berganti menjadi Refresh saat terhubung; Refresh tidak mendapat perintah karena tidak diminta scope dan data sudah live.
- **Status awal baris milik fitur lain adalah planned.** Build yang menyambung (fitur 69 atau fitur pemilik) mengubahnya menjadi built di commit yang sama dengan `CommandItem` nya, dan source rule memastikan keduanya sejalan.
- **`MainWindowKey` per jendela, mendengar notifikasi key window jendela itu sendiri.** Murah, tanpa state global, dan bisa dites dengan `NSWindow` test; menu cukup membaca focused value (hasil cross check G2). Runner up: state global di `ShellRuntime` yang dibaca langsung `Commands.body`, yang pelacakan `@Observable` nya di `Commands` belum terbukti.

## Batas yang disepakati dengan sub sesi lain (2026-10-21)

- **arc-f67-spec (spec 0067):** `StacklyAction` (title, symbol, role, mutates, disabledReason, isBusy, confirmation `ActionConfirmation`, perform), `availability(gate:)`, dan `invoke(present:)` milik fitur 67 milestone 1; `ActionHost` dan `.actionHost()` sekali per jendela (dipasang di `AppShell` dan `StacklySheet`) juga milestone 1, jadi menu menjalankan `host.run(action)` dan dialog dari menu tidak pernah bertumpuk dengan dialog dari tombol. Shortcut hanya di `CommandMap`; `ActionMenu` dan menu konteks tidak memasang shortcut; tooltip dengan shortcut dipasang halaman lewat `CommandMap.helpText(for:)`. Subtitle alasan di menu utama mengikuti cek macOS 27 di langkah 1 milestone 1 spec 0067. `.stacklyInspector(isPresented:)` dan `InspectorToggle` di milestone 3 spec 0067; perintah ⌥⌘I milik spec ini memakai binding yang sama.
- **arc-f68-spec (spec 0068):** `XCUIElement` macOS tidak membuka key equivalent, jadi pemindaian duplikat seluruh menu bar tetap milik spec ini (test peta, source rule, probe AX sebagai evidence manual yang diulang `/check verify`). Fitur 68 memiliki target UI test, skenario fixture (`connected-empty`, `connected-mixed`, `daemon-offline`, `onboarding`), test bahwa setiap baris built ada di menu dengan judul benar dan aktif sesuai skenario, dan smoke `typeKey`. Untuk itu `CommandMap` dan barisnya public, read only, dengan scope dan requirements sebagai enum.

Keputusan lead atas delegasi user (2026-10-21), keputusan desain: semua rekomendasi K1 sampai K13, tanpa bagian References; daftar shortcut sistem dan bentrok menu standar (⇧⌘C Show Colors, ⌘R Show Ruler) ditulis eksplisit beserta sumbernya; usulan untuk spec 0013, 0016, 0017, 0020, 0021, dan 0022 ditulis per baris; XCUITest menu tetap milik fitur 68.

## Cross check

Dijalankan 2026-10-21 oleh model lain (Fable) sebagai pembaca read only atas draft spec, dengan fokus kelengkapan keputusan dan kesehatan desain. Hasilnya 2 blocker, 9 gap, dan 4 nit. Lead menerima semua perbaikan yang direkomendasikan atas delegasi user pada 2026-10-21 ("semua R + accept"), dan semuanya sudah diterapkan di `index.md`:

| ID | Temuan | Perbaikan |
| --- | --- | --- |
| G1 (blocker) | Item `.app` (Welcome, Stackly Help) tidak punya sumber aksi, dan `showMainWindow` bukan `CommandItem` | `CommandEntry.kind` (`button`, `toggle`, `system`); kind `system` dikecualikan dari aturan `CommandItem`; `StacklyCommands.appActions(runtime:)` dengan test; urutan Instances memakai `toggle` dari peta |
| G2 (blocker) | State menu bisa basi untuk key equivalent karena pembacaan `@Observable` di `Commands` belum terbukti | `MainWindowKey` per jendela diterbitkan lewat `PublishedCommands.isWindowKey`, menu hanya membaca focused value; penjaga `run` (`isKeyNow` dan `availability`) dan probe P7 |
| G3 | ⌘0 adalah "Actual Size" di HIG | ⌘0 masuk tabel B; `showMainWindow` dengan `exception`; pengecualian berlaku juga untuk tabel B dengan makna lain |
| G4 | ⌘. (Cancel) tidak ada di tabel A | ditambahkan |
| G5 | `List(selection:)` tidak memanggil setter untuk baris yang sudah terpilih | cabang pilih ulang `select(_:)` dengan hook `onReselect`, diisi spec 0021 (usulan update) |
| G6 | ⌘2 tidak memindah fokus ke tabel | alur AC-15 dan cek manual 5 menjadi "⌘2, Tab ke tabel, panah" |
| G7 | Bentuk hasil `availability(gate:)` belum dipatok | memakai `ActionAvailability` final spec 0067 (`.enabled`, `.busy`, `.disabled(reason:showsReason:)`), dengan aturan subtitle yang sama dengan menu spec 0067 |
| G8 | Premis K8 (Find bawaan tidak menjangkau `.searchable`) belum diprobe | probe P3b dan cadangannya |
| G9 | Konversi ⌘? ke `KeyboardShortcut` belum ditulis | aturan konversi dan normalisasi probe untuk tanda baca bershift |
| G10 | Penerbit di level `AppShell` bisa membaca gate default | urutan modifier dipatok (penerbit di dalam `.actionHost()` dan `.environment`) dan test struktur |
| G11 | Pemisah menu tidak bisa dites | `CommandEntry.group`, `submenu`, dan `CommandMap.layout(menu:submenu:)` dengan test |
| Nit | `find` saat detail terbuka; kalimat Settings key; popover; pemisah | `find` tidak diterbitkan saat detail terbuka; kalimat diselaraskan dengan P4; popover dicatat sebagai konsekuensi; pemisah lewat `layout` |

Setelah cross check, spec 0067 diterima dengan API final `invoke(gate:present:)`, `ActionRole.standard(confirmation:)` dan `.destructive(ActionConfirmation)`, `ActionAvailability`, dan `ActionHost` yang memegang `gate`; spec ini memakai nama itu apa adanya (dikirim arc-f67-spec 2026-10-21).
