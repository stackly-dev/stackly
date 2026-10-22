# 0067. Rationale: komponen dasar Stackly

[Kembali ke spec](index.md)

## Context

Fondasi UI sudah ada di `main`: spec 0012 membangun token, palet status, `StatusLabel`, `ToneBadge`, pola state (`ViewState`, `StateBanner`, `StatePlaceholder`), gate mutasi, `.disabled(reason:)`, dan satu jalur konfirmasi; spec 0013 membangun shell, jalur mutasi (`MutationPath`, `MutationOutcome`, copy `MutationFeedback`), dan `OperationCenter`; spec 0011 membangun komponen secret. Yang belum ada adalah bagian yang muncul berulang di layar mockup tapi bukan status atau state: tile provider, baris key value di kartu dan inspector, sheet dengan tombol di footer, toast setelah aksi, checklist preflight dan doctor, progress bertahap untuk install dan create, view log dan diff, menu More dan menu konteks, inspector, tombol Copy, dan blok perintah monospace.

Spec fase 02 yang ditulis 2026-10-19 sampai 2026-10-20 (0016, 0017, 0018, 0020, 0021, 0022) masing masing butuh sebagian bagian itu sebelum fitur 67 punya spec. Akibatnya mereka merencanakan versi sementara di folder layarnya: `ProviderTile` di `Catalog/`, `PreflightSection`, footer sheet, dan `CreateProgressSection` di `CreateInstance/`, `ActionFeedbackBanner` di `Instances/`, ditambah kalimat "pakai komponen fitur 67 kalau sudah ada". Pemeriksaan 2026-10-21 menunjukkan belum ada satu pun versi sementara itu di `main` atau branch mana pun, karena milestone GUI fase 02 belum dibangun. Tanpa keputusan siapa membangun lebih dulu, build fase 02 dan build fitur 67 bisa menulis dua versi komponen yang sama dengan API berbeda, lalu satu harus diadopsi dan yang lain dihapus.

Kekuatan yang membatasi: tidak ada glass kustom di konten dan semua permukaan kustom opak (spec 0012 AC-6), teks kustom hanya di pasangan kontras yang sudah dites (spec 0012 *Pasangan kontras*), status tidak pernah hanya warna (NFR-012), aksi destruktif selalu dikonfirmasi dan bukan tombol default (spec 0012 AC-15), tombol capability unsupported tidak boleh tampil seolah bekerja dan alasannya terlihat tanpa hover (REQ-026, spec 0012 AC-13), dan alur membuat, menghentikan, dan memeriksa instance harus selesai dengan keyboard dan VoiceOver (PRD AC-027). Text style macOS 27 tidak ikut `dynamicTypeSize`, jadi cek teks besar adalah cek lebar sempit 640 pt (spec 0012). Mockup memakai toast glass yang hilang setelah 2,8 detik untuk semua pesan, termasuk "stacklyd is unavailable. Nothing was sent."; pesan yang hilang sendiri sebelum terbaca, tanpa cara menahan atau mengulang, menyulitkan pengguna yang lambat membaca atau memakai pembesar layar (WCAG 2.2.1) dan tidak sampai ke pengguna VoiceOver kalau tidak diumumkan (WCAG 4.1.3).

Fitur 68 (snapshot dan XCUITest) dan fitur 69 (menu dan shortcut) ditulis paralel di wave yang sama dan bergantung pada bentuk komponen: fitur 68 butuh daftar state per komponen dan identifier yang stabil, fitur 69 butuh aturan nonaktif yang sama persis dengan tombol (scope fitur 69 *Done when*).

Keputusan berikut diambil lead atas delegasi user (2026-10-21), selalu dengan opsi yang direkomendasikan.

## Options considered

### Keputusan 1: inventaris komponen

**Opsi A, sepuluh komponen scope plus tiga yang berulang (dipilih).** Tile provider, key value, sheet dengan footer, umpan balik, progress bertahap, checklist, view log, view diff, menu aksi, inspector, ditambah `CardSection`, `CopyButton`, dan `ConsoleBlock`, karena ketiganya muncul di banyak layar mockup (kartu di Overview, Summary, Diagnostics, inspector; tombol Copy di Summary, inspector, sheet create, onboarding; blok monospace di sheet create, onboarding, banner gagal).

- Pros: setiap layar fase 02 mendapat semua bagian berulang dari satu tempat; copy dan pengumuman Copy seragam.
- Cons: fitur 67 lebih besar; tiga komponen tambahan belum disebut scope.

**Opsi B, hanya sepuluh komponen scope.**

- Pros: cakupan persis seperti scope.
- Cons: tombol Copy, kartu, dan blok monospace dibuat ulang di setiap layar dengan perilaku pengumuman yang bisa berbeda.

### Keputusan 2: siapa membangun versi pertama

**Opsi A, fitur 67 satu satunya pembangun (dipilih).** Komponen hanya ada di `Components/` dengan API dari spec 0067. Milestone fase 02 yang terpaksa mulai lebih dulu membangun komponen itu di path dan API spec 0067, dan milestone fitur 67 tinggal mencentang.

- Pros: tidak ada kode sementara, tidak ada adopsi atau pemindahan kode, dan test serta galeri ditulis sekali.
- Cons: milestone 1 fitur 67 menjadi prasyarat lunak layar fase 02 pertama; kalau fase 02 lebih cepat, milestone fase 02 menanggung pekerjaan komponen.

**Opsi B, fase 02 membangun versi sementara, fitur 67 mengadopsi.**

- Pros: fase 02 tidak menunggu siapa pun.
- Cons: setiap komponen ditulis dua kali atau dipindah dengan rename; API sementara mengikat fitur 67; penanda `// provisional: F67` tersebar di empat folder.

### Keputusan 3: bentuk umpan balik setelah aksi

**Opsi A, kartu opak per jendela di tengah bawah (dipilih).** Satu `FeedbackCenter` per jendela, kartu di posisi toast mockup lewat `.safeAreaInset(edge: .bottom)`, tint tone, crossfade, yang terbaru menggantikan (kecuali kabar baik di atas pesan yang harus dibaca), kabar baik tanpa aksi hilang sendiri dengan waktu yang bisa ditahan, peringatan, error, dan pesan beraksi tetap sampai ditutup, semuanya diumumkan.

- Pros: posisi dan rasa toast mockup tetap, bagian atas konten tidak bergeser, kontrol yang difokus tidak tertutup, aman untuk WCAG 2.2.1, 2.4.11, dan 4.1.3, satu tempat untuk semua layar.
- Cons: area konten di bawah menyempit selama kartu tampil; butuh pemasangan di shell.

**Opsi B, banner di atas halaman (seperti `ActionFeedbackBanner` spec 0022).**

- Pros: memakai `StateBanner` yang sudah ada; tidak menutupi konten.
- Cons: konten bergeser setiap ada umpan balik; jauh dari posisi mockup; setiap halaman memasangnya sendiri.

**Opsi C, hanya pengumuman VoiceOver.**

- Pros: paling sederhana.
- Cons: pengguna yang melihat layar tidak mendapat apa pun untuk `notSent` dan `outcomeUnknown`.

### Keputusan 4: konfirmasi copy

**Opsi A, konfirmasi di tombol itu sendiri (dipilih).** Label "Copied" dengan centang selama 2 detik dan pengumuman.

- Pros: berjalan di sheet, popover, dan inspector, di mana kartu jendela tidak terlihat atau berada di belakang sheet; tidak menggantikan umpan balik lain yang lebih penting.
- Cons: berbeda dari toast "Copied …" mockup.

**Opsi B, copy mengirim ke `FeedbackCenter`.**

- Pros: sama dengan mockup.
- Cons: tidak jalan di sheet; copy berulang menggantikan error yang sedang tampil.

### Keputusan 5: pemetaan `MutationOutcome`

**Opsi A, `notSent` dan `outcomeUnknown` bertone `caution`, `rejected` bertone `critical` (dipilih).** Copy dari `MutationFeedback`; `outcomeUnknown` punya aksi Show Operations.

- Pros: `notSent` tidak mengubah apa pun dan sudah dijelaskan banner jendela, jadi bukan kegagalan; `outcomeUnknown` mengarahkan ke tempat memeriksa.
- Cons: pengguna yang tidak membaca bisa menganggap `caution` kurang serius.

**Opsi B, `notSent` bertone `critical`.**

- Pros: lebih mencolok.
- Cons: dua tanda merah (banner jendela dan kartu) untuk satu sebab.

### Keputusan 6: sheet dengan footer

**Opsi A, footer eksplisit dengan `.defaultAction` dan `.cancelAction`, dicek dulu di macOS 27 (dipilih).**

- Pros: ruang untuk `DisabledReasonText` dan state sibuk di footer, perilaku Return untuk aksi destruktif bisa diatur, sama dengan mockup.
- Cons: kode footer sendiri yang harus dicek ulang tiap versi macOS.

**Opsi B, placement toolbar `.confirmationAction` dan `.cancellationAction`.**

- Pros: paling native, macOS mengatur posisi dan shortcut.
- Cons: tidak ada tempat untuk alasan nonaktif di dekat tombol; perilaku di macOS 27 dengan Liquid Glass belum dicek; menjadi cadangan kalau opsi A gagal.

### Keputusan 7: progress bertahap

**Opsi A, satu komponen dengan dua tata letak plus fungsi murni dari `OperationSummary` (dipilih).**

- Pros: rumus persen spec 0018 ditulis dan dites sekali; Runtimes, Catalog, Overview, Operations, dan sheet create sepakat.
- Cons: definisi tahap tetap datang dari setiap spec pemilik operasi.

**Opsi B, setiap fitur membuat sendiri.**

- Pros: bebas per layar.
- Cons: rumus dan teks VoiceOver berbeda per layar.

### Keputusan 8: checklist

**Opsi A, `CheckList` generik dengan detail membungkus penuh (dipilih).**

- Pros: pesan error preflight dan doctor terbaca utuh; satu tabel status untuk preflight, doctor, dan cek environment.
- Cons: baris lebih tinggi daripada mockup (yang memotong detail dengan elipsis).

**Opsi B, detail satu baris terpotong, teks penuh di help dan VoiceOver.**

- Pros: sama dengan mockup.
- Cons: pengguna yang melihat layar tidak bisa membaca error panjang tanpa hover; melanggar aturan "tidak hanya lewat hover".

### Keputusan 9: log

**Opsi A, `ConsoleBlock` statis dan `LogView` lazy di fitur 67 (dipilih).**

- Pros: scope fitur 67 menyebut view log; satu tampilan untuk Logs, Diagnostics, dan cuplikan.
- Cons: `LogView` dibangun sebelum fitur 27 menulis kebutuhannya.

**Opsi B, hanya `ConsoleBlock`, `LogView` ke fitur 27.**

- Pros: API log ditulis bersama datanya.
- Cons: tidak memenuhi *Done when* fitur 67.

### Keputusan 10: permukaan diff

**Opsi A, diff di atas kartu biasa dengan tint tone (dipilih).**

- Pros: pasangan kontras P3 dan P4 spec 0012 sudah menutupinya; tidak ada token atau test baru.
- Cons: di dark mode lebih terang daripada latar konsol mockup.

**Opsi B, base `.console` baru di `SurfaceToken` dan test kontras diperluas.**

- Pros: setia pada mockup.
- Cons: token dan baris tabel kontras baru di spec 0012 untuk satu komponen.

### Keputusan 11: alasan nonaktif di menu

**Opsi A, subtitle item menu, dicek dulu di macOS 27, cadangan item nonaktif tambahan (dipilih).**

- Pros: alasan terlihat tanpa hover di menu, sesuai AC-027 dan spec 0012 AC-13.
- Cons: bergantung pada perilaku menu macOS 27 yang belum dicek.

**Opsi B, alasan hanya di help.**

- Pros: paling sederhana.
- Cons: help item menu tidak tampil dan alasannya hanya di tooltip, yang dilarang spec 0012.

### Keputusan 12: deskriptor aksi bersama

**Opsi A, `StacklyAction` milik fitur 67 dipakai tombol, menu, dan perintah fitur 69 (dipilih, disepakati arc-f69-spec).**

- Pros: satu fungsi availability, jadi perintah nonaktif tepat saat tombolnya nonaktif; satu host konfirmasi per jendela.
- Cons: fitur 69 bergantung pada milestone 1 fitur 67.

**Opsi B, fitur 69 memiliki deskriptor, fitur 67 hanya view.**

- Pros: peta perintah memegang semuanya.
- Cons: tombol di dalam jendela (yang ada lebih dulu) bergantung pada fitur 69.

### Keputusan 13: inspector

**Opsi A, `.inspector` native dibungkus modifier dan view header (dipilih).**

- Pros: lebar, toggle, dan header seragam di Instances dan Runtimes; glass kolom diurus macOS.
- Cons: mockup menggambar panel glass melayang yang tidak ditiru.

**Opsi B, panel kanan kustom seperti mockup.**

- Pros: setia pada mockup.
- Cons: glass kustom dilarang spec 0012.

### Keputusan 14: pemasangan di shell

**Opsi A, build fitur 67 memasang center, presenter, dan host di shell, dicatat sebagai usulan untuk spec 0013 (dipilih).**

- Pros: setiap halaman otomatis punya umpan balik dan konfirmasi; satu titik pasang.
- Cons: build fitur 67 menyentuh file milik spec 0013 (`LiveShell`, `AppShell`).

**Opsi B, setiap halaman memasang sendiri.**

- Pros: tidak menyentuh shell.
- Cons: dua halaman bisa punya dua kartu atau dua konfirmasi di satu jendela.

### Keputusan 15: preview dan galeri

**Opsi A, preview per komponen dan satu halaman galeri, snapshot milik fitur 68 (dipilih, disepakati arc-f68-spec).**

- Pros: satu sumber nilai contoh (`ComponentSamples`) untuk preview, galeri, dan snapshot.
- Cons: halaman galeri panjang.

**Opsi B, fitur 67 juga menulis snapshot.**

- Pros: komponen langsung terlindungi regresi visual.
- Cons: dua pemilik helper snapshot.

### Keputusan 16: urutan build

**Opsi A, empat milestone menurut kebutuhan fase 02 (dipilih).** Milestone 1 untuk Catalog, Onboarding, Overview, Instances, dan fitur 69; milestone 2 untuk sheet create dan Runtimes; milestone 3 inspector; milestone 4 log dan diff.

- Pros: milestone 1 membuka layar fase 02 paling awal; milestone 2 sampai 4 paralel.
- Cons: milestone 1 paling besar.

**Opsi B, satu milestone per komponen.**

- Pros: langkah kecil.
- Cons: sepuluh lebih milestone; prasyarat fase 02 sulit ditulis.

## Rationale

Kekuatan terbesar di Context adalah tabrakan antara build fase 02 dan build fitur 67. Karena belum ada kode sementara sama sekali, biaya menghindarinya sekarang hampir nol: cukup menetapkan bahwa komponen hanya punya satu rumah dan satu API, dan bahwa siapa pun yang sampai lebih dulu membangunnya di rumah itu (Keputusan 2). Pola ini sudah dipakai proyek untuk `ShellNavigator` dan `newInstanceAction` (spec 0022 dan 0020), jadi lead dan sesi build sudah mengenalnya. Inventaris diperluas dengan tiga komponen kecil (Keputusan 1) karena bagian itulah yang paling sering muncul dan paling mudah berbeda diam diam, misalnya pengumuman Copy.

Umpan balik (Keputusan 3, 4, 5) dibentuk oleh dua batas spec 0012 (tanpa glass, permukaan opak) dan oleh aksesibilitas. Kartu di posisi toast mockup menjaga niat desain, dan karena dipasang lewat `.safeAreaInset(edge: .bottom)`, konten bergulir di atasnya sehingga kontrol yang difokus tidak pernah tertutup (WCAG 2.4.11, Focus Not Obscured). Pembagian tone menjawab WCAG 2.2.1 (Timing Adjustable): hanya pesan yang mengabarkan hasil baik, tanpa aksi, dan yang hasilnya juga terlihat di layar yang boleh hilang sendiri. Hitungannya hanya berjalan selama kartu tampil, jendela aktif, pointer tidak di atas kartu, dan fokus keyboard tidak di dalamnya, lalu dimulai dari awal setiap kali syarat itu kembali; durasinya memanjang untuk teks panjang (kira kira 15 karakter per detik, minimum 4 detik, lebih panjang dari 2,8 detik mockup). Pesan yang perlu ditindaklanjuti (peringatan, error, termasuk `notSent` dan `outcomeUnknown`, dan setiap pesan yang punya tombol aksi) tidak pernah hilang sendiri, sehingga tidak ada batas waktu untuk membaca informasi penting atau mencapai tombolnya dengan keyboard. Kabar baik yang datang saat pesan semacam itu tampil hanya diumumkan dan tidak menggantikannya. WCAG 4.1.3 (Status Messages) dipenuhi lewat pengumuman VoiceOver sekali per umpan balik tanpa memindah fokus. Karena kartu baru tetap bisa menggantikan kartu lama, aturan design.md mewajibkan kegagalan juga tercatat di tempatnya (Needs Attention, banner item, Operations), jadi kartu tidak pernah satu satunya jejak. Copy dikonfirmasi di tombolnya sendiri supaya berjalan di sheet dan popover, dan supaya copy berulang tidak menggantikan error yang sedang tampil.

Aksi dan menu (Keputusan 11, 12) dibentuk oleh aturan spec 0012 bahwa alasan nonaktif tidak boleh hanya di tooltip, dan oleh *Done when* fitur 69 bahwa perintah nonaktif tepat saat tombolnya nonaktif. Satu deskriptor dengan satu fungsi availability adalah cara paling murah untuk menjamin itu, dan satu host konfirmasi per jendela mencegah dua dialog bertumpuk dari tombol dan menu. Karena subtitle item menu dan Return di footer sheet bergantung pada perilaku macOS 27 yang belum dicek, keduanya menjadi langkah build pertama di milestone masing masing dengan cadangan yang sudah ditulis (Keputusan 6, 11), mengikuti preseden langkah 17 spec 0012 yang menemukan `confirmationDialog` gagal.

Diff, inspector, dan sheet (Keputusan 6, 10, 13) memilih permukaan yang kontrasnya sudah dibuktikan daripada meniru latar mockup atau mengandalkan material sistem, karena setiap pasangan warna baru butuh baris tabel dan test baru di spec 0012. Progress, checklist, dan log (Keputusan 7, 8, 9) dibuat generik dengan fungsi murni supaya rumus dan teks VoiceOver dites sekali, dan detail checklist dibiarkan membungkus karena pesan error preflight adalah informasi yang dibutuhkan pengguna untuk memperbaiki masalah. Urutan build (Keputusan 16) mengikuti layar fase 02 yang paling awal, dan milestone dipotong supaya tiga di antaranya bisa jalan paralel tanpa menyentuh file yang sama.

### Keputusan kecil yang diambil di spec ini

- Isi inspector di atas latar opak `surfaceWindow`, bukan langsung di atas material kolom inspector, supaya `textSecondary`, teks tone, dan `StatusLabel` konteks `.standard` di header tetap di pasangan kontras P1 dan P2 spec 0012. Alternatif: memakai konteks `.row` dan warna teks utama untuk semua teks di inspector, yang menghilangkan teks sekunder.
- Isi sheet `StacklySheet` di atas latar opak `surfaceWindow` (`.presentationBackground`), alasan yang sama dengan inspector; mockup juga menggambar sheet opak. Alternatif: latar material sheet bawaan, yang membuat subjudul dan teks alasan di footer berada di luar tabel kontras.
- Konfirmasi dibawa `ActionRole` (`.standard(confirmation:)` atau `.destructive(ActionConfirmation)`), jadi aksi destruktif tanpa konfirmasi tidak bisa ditulis bahkan di build Release, dan `invoke` menyusun `ConfirmationRequest` dengan `perform` action. Alternatif: field `confirmation` opsional yang dijaga `assert`, yang hilang di build Release, atau menyimpan `ConfirmationRequest` lengkap, yang membawa `perform` kedua.
- `invoke(gate:present:)` menerima gate, dan `.actionHost()` membaca `mutationAvailability` dari environment tempat ia dipasang, jadi `ActionHost.run` dan perintah fitur 69 memakai gate yang sama dengan tombol. Alternatif: `invoke` tanpa gate, yang membiarkan perintah menu berjalan saat daemon tidak terjangkau.
- Komponen yang menjalankan action dengan konfirmasi tanpa host memanggil `assertionFailure` dan tidak menjalankan apa pun. Alternatif: menjalankan tanpa konfirmasi, yang bisa menghapus data tanpa dialog.
- `FeedbackCenter` dimiliki view akar jendela, bukan `AppShell`, supaya onboarding (yang tampil sebelum `AppShell`) bisa mengirim "Setup complete.". Hitungan berjalan di `FeedbackCard` lewat `.task(id:)` dengan state hover, fokus, dan jendela aktif yang terpisah, sementara `FeedbackCenter.countdown(_:)` memegang tidur yang bisa diganti untuk test. Alternatif: satu Bool tahanan di center, yang tersangkut kalau kartu hilang di bawah pointer.
- Persen progress dihitung dengan bilangan bulat dari `done` dan `total`, karena perkalian `Double` memberi 28% untuk 29 dari 100. Alternatif: `floor(fraction × 100)`.
- Simbol tahap pending memakai `textSecondary`, bukan `StacklyColor.decorative`, karena simbol itu membawa arti dan butuh kontras 3:1 (WCAG 1.4.11). Alternatif: warna dekoratif mockup, yang terlalu samar.
- Kolom grid checklist minimum 200 pt dengan jarak 16 pt, supaya sheet create 560 pt (lebar isi kartu 488 pt) memberi dua kolom seperti mockup. Alternatif: 240 pt, yang hanya muat dua kolom dengan jarak 8 pt atau kurang.
- Durasi `min(10, max(4, ceil(karakter / 15)))` detik. Alternatif: 4 detik tetap, yang terlalu cepat untuk pesan install yang panjang.
- `CopyButton` mencadangkan lebar label terpanjang supaya "Copy" ke "Copied" tidak menggeser baris. Alternatif: lebar mengikuti label, yang membuat baris key value melompat.
- `ProviderTile` menerima `name` untuk mark default (tambahan pada signature usulan spec 0022), karena aturan default spec 0017 butuh nama. Alternatif: "?" untuk semua instance tanpa entri catalog.
- Ukuran font mark lewat helper di `Typography.swift`, file yang sudah diizinkan aturan ukuran font, sehingga aturan sumber spec 0012 tidak mendapat pengecualian baru. Alternatif: menambah `ProviderTile.swift` ke daftar pengecualian.
- Judul `CardSection` `.headline` (mockup 13, 700, sama dengan judul seksi spec 0012). Alternatif: `.callout.weight(.semibold)` `textSecondary` seperti label kecil "Preflight" di mockup, yang terlalu samar untuk judul kartu.
- Persen progress dibulatkan ke bawah (pembagian bilangan bulat) dan dibatasi 99 sebelum `succeeded`, supaya "100%" tidak pernah tampil untuk operasi yang belum selesai. Alternatif: pembulatan biasa, yang bisa menampilkan 100% saat Commit masih berjalan.
- Status `pending` checklist memakai `DelayedVisibility` 300 ms yang sama dengan loading spec 0012, supaya preflight cepat tidak berkedip.
- Dua aturan sumber baru (pasteboard dan inspector) supaya pemakaian ulang dijaga test, bukan ingatan reviewer. Alternatif: aturan di design.md saja.

### Koordinasi wave 1

- **arc-f69-spec (spec 0069), 2026-10-21:** setuju `StacklyAction`, `availability(gate:)`, `invoke` (sebelum cross check tanpa gate, lihat butir berikutnya), `ActionHost`, dan `.actionHost()` milik spec 0067 di milestone 1; `CommandID` dan `CommandMap` (shortcut hanya di sana), `.publishesCommands(_:)` yang membaca `actionHost` dan `mutationAvailability` dari environment, dan `PublishedCommands { actions, host, gate }` milik spec 0069; menu dan menu konteks fitur 67 tanpa shortcut; tooltip dengan shortcut dipasang halaman lewat `CommandMap.helpText(for:)`; `.keyboardShortcut` di luar `Commands/` hanya `.defaultAction` dan `.cancelAction`; subtitle alasan di menu utama memakai hasil cek langkah 1 spec ini.
- **arc-f68-spec (spec 0068), 2026-10-21:** setuju data contoh komponen di `ComponentSamples` (bukan fixture kontrak), halaman galeri "Components" sebagai daftar state yang dirender snapshot, konvensi `<scope>.<part>[.<key>]` di `AccessibilityID.swift` (F67 membuat file nya kalau membangun lebih dulu), dan scope komponen checklist bernama `checklist`.
- **arc-f69-spec, setelah cross check:** signature final `StacklyAction`, `ActionRole`, `ActionAvailability`, `invoke(gate:present:)`, dan `ActionHost` dengan `gate` sendiri dikirim persis seperti bagian *Signature Swift* di index.md.

### Temuan langkah 1

Dicek 2026-10-21 di macOS 27.0.1 (26A434) dengan Xcode 27.0 (SDK 27.0), memakai app probe kecil di luar repo: `Button { Text(judul); Text(alasan) }` yang nonaktif di tiga tempat, `Menu` di toolbar jendela, `.contextMenu`, dan `CommandMenu` di menu utama. Menu dibuka lewat accessibility API dan tangkapan jendela menu (tanpa mengubah System Settings dan tanpa VoiceOver).

- **Tampil:** di ketiganya SwiftUI mengubah `Text` kedua menjadi subtitle `NSMenuItem` (`item.subtitle` terisi, dibaca langsung dari `NSApp.mainMenu`). Judul tampil redup karena item nonaktif, subtitle tampil di bawahnya dengan huruf lebih kecil dan juga redup. Tampil benar di mode gelap, tanpa `CommandMenu` yang membuangnya.
- **Dibacakan:** tidak terbukti. Pohon AX item (`AXMenuItem`) hanya memuat `AXTitle` (judul tanpa alasan); tidak ada `AXDescription`, `AXHelp`, atau `AXValue`, baik saat menu tertutup maupun saat terbuka, dan `.accessibilityHint`, `.accessibilityLabel`, `.accessibilityValue`, dan `.help` di tombolnya tidak mengubah pohon AX item menu. Jadi tidak ada cara dari SwiftUI untuk menaruh alasan ke jalur yang dibaca VoiceOver, dan VoiceOver sungguhan belum diuji (langkah manual user, `NOT_RUN`).
- **Keputusan jalur:** karena AC-8 mensyaratkan alasan terlihat dan dibacakan, dan bukti AX menunjukkan subtitle tidak ada di pohon AX, build memakai jalur cadangan spec: item nonaktif tambahan berisi teks alasan tepat di bawah item (judul `AXMenuItem` itu sendiri adalah alasannya, jadi dibacakan). Pilihan ini satu konstanta (`MenuReasonPath.current` di `Components/Actions/`); kalau user mengecek dengan VoiceOver dan subtitle ternyata dibacakan, ganti ke `.subtitle` dan menu menjadi lebih pendek, tanpa mengubah API. Spec 0069 memakai jalur yang sama untuk menu utama.

### Temuan langkah 9

Dicek 2026-10-21 di macOS 27.0.1 (26A434) dengan Xcode 27.0, memakai app probe kecil di luar repo: satu jendela hijau dengan satu sheet berisi header, `TextField`, `Toggle`, dan footer eksplisit (alasan, Cancel dengan `.keyboardShortcut(.cancelAction)`, aksi utama dengan `.keyboardShortcut(.defaultAction)`), latar sheet `.presentationBackground` merah pekat. Tombol ditekan lewat CGEvent ke proses probe dan hasilnya dibaca dari log aksi dan pohon accessibility (API AX); tanpa mengubah System Settings dan tanpa VoiceOver. Keyboard navigation mati di Mac ini (`AppleKeyboardUIMode` 0), jadi Tab dan Space ke tombol tidak bisa dicek.

- **Return** menjalankan aksi utama saat fokus di `TextField` dan saat fokus di `Toggle`. Aksi utama destruktif (`.bordered`, `role: .destructive`, tanpa shortcut) tidak menjalankan apa pun saat Return, dan aksi utama yang `.disabled(true)` juga tidak. Jalur footer eksplisit lolos, jadi placement toolbar tidak dipakai.
- **Escape menutup sheet sendiri tanpa menjalankan aksi Cancel.** Dengan `.keyboardShortcut(.cancelAction)` saja, Escape membuat sheet hilang (`dismiss` bawaan jendela) dan aksi Cancel tidak berjalan (log tetap kosong), padahal Cancel yang menekan dengan pointer atau AX menjalankannya. Dengan `.interactiveDismissDisabled()` di sheet yang sama, Escape menjalankan aksi Cancel persis satu kali, dan sheet tetap terbuka kalau aksi itu tidak menutupnya (diperiksa dengan Cancel yang hanya mencatat). Jadi `StacklySheet` selalu memasang `.interactiveDismissDisabled()`, karena Cancel bisa berarti lain dari menutup (spec 0020: `operation.cancel` dengan konfirmasi di mode progress). Klik di luar sheet tidak menutupnya di macOS, jadi tidak ada jalur tutup yang hilang.
- **Konfirmasi dari dalam sheet** (`confirmationDialog` di probe, `.stacklyConfirmation` memakai `NSAlert` yang sama) tampil sebagai sheet kedua di atas sheet pertama (pohon AX: `AXSheet` bersarang di `AXSheet` dengan tombol Cancel dan Remove). Escape di dalamnya hanya membatalkan konfirmasi dan sheet pertama tetap terbuka.
- **Latar opak**: `.presentationBackground` merah pekat memberi piksel seragam di seluruh sheet tanpa campuran hijau jendela di belakangnya; hanya jendela di belakang yang digelapkan. `surfaceWindow` memakai jalur yang sama.
- **Belum dicek (manual user, `NOT_RUN`)**: Tab ke Cancel dan aksi utama dan Space menjalankannya dengan Keyboard navigation menyala (Space pada `Toggle` tidak mengubah nilai saat setting mati, seperti perilaku macOS), dan VoiceOver membaca footer.

Keputusan: footer eksplisit (bukan placement toolbar), dengan `.interactiveDismissDisabled()` supaya Escape menjalankan aksi Cancel layar.

## Cross check

Dijalankan 2026-10-21 oleh model lain (Sonnet 5.5) sebagai pembacaan read only atas index.md dan rationale.md, dengan rujukan ke spec 0012, 0013, 0016 sampai 0022, kode `StacklyUI`, dan tipe hasil generate `StacklyClient`. Semua rujukan baris di *Usulan update untuk spec lain* tepat, kecuali dua klaim spec 0018 yang dibetulkan. Lead menerima cross check ini dan semua perbaikan yang direkomendasikan atas delegasi user (2026-10-21, "apply + accept"). Yang diterapkan:

- Gap kelengkapan: `invoke(gate:present:)` dan gate di `ActionHost` (G1); hitungan umpan balik di kartu dengan tahanan hover, fokus, dan jendela aktif yang terpisah (G2); baris tabel progress untuk batal tanpa tahap, daftar tahap kosong, `needsRecovery`, `cancelRequested`, `queued`, stage tidak dikenal, nomor tahap mulai 1, dan aturan pengumuman tahap (G3); susunan elemen VoiceOver dengan tombol di luar elemen teks untuk key value, konsol, checklist, dan kartu (G4); alasan gate di footer sheet dan `showsReason: false` untuk tombol toolbar (G5); grid checklist 200 pt dengan jarak 16 pt (G6); bagian *Signature Swift* lengkap dengan `TileSpec`, `.consoleSurface()`, initializer `CardSection` dan `InspectorHeader`, `.neutral`, dan ukuran tile mockup (G7).
- Soundness: kartu lewat `.safeAreaInset`, pesan beraksi tetap, hitungan hanya saat jendela aktif, dan kabar baik tidak menggantikan pesan yang harus dibaca (S1); `ActionRole` yang membawa konfirmasi dan cek konfirmasi dari dalam sheet di langkah 9 (S2); persen bilangan bulat (S3); latar opak sheet, simbol pending `textSecondary`, dan pengecualian warna dekoratif di invariant (S5).
- Minor: gaya key value hanya untuk baris read only dengan dua alternatif `ViewThatFits` yang tegas dan aturan satu baris satu view untuk `Group(subviews:)`; `LogView` dengan `onScrollPhaseChange` dan toleransi 24 pt, sumber panjang tampil utuh, dan teks VoiceOver tanpa bagian kosong; `DiffView` lazy; `Feedback.action` bertipe `StacklyAction`; `PlainPasteboard` dan aturan inspector dibuat di langkah 2; `SecretEntryModel` ikut pindah ke `DaemonText.sentence`; penanda stub `// provisional: F67 M2`; tinggi isi sheet dibatasi tinggi layar; cek inspector macOS 27 di langkah 14; test struktur shell lewat pemindai sumber; AC-20 dicek di /check verify.

### Temuan langkah 14

Dicek 2026-10-21 di macOS 27.0.1 (26A434) dengan Xcode 27.0 (SDK 27.0), memakai app probe kecil di luar repo (`NavigationSplitView` dengan sidebar 200 sampai 260 pt dan kolom halaman minimal 640 pt, di jendela yang minimalnya 900 pt, lalu `.stacklyInspector` memakai komponen asli milestone ini di kolom halaman). Probe diperiksa dalam mode terang dan gelap lewat accessibility API (`AXUIElement` atas prosesnya sendiri, tanpa mengubah System Settings dan tanpa VoiceOver) dan tangkapan jendela per ID jendela.

- **Sambungan latar opak.** `.background(StacklyColor.surfaceWindow)` pada `ScrollView` mengisi seluruh kolom: ke atas sampai tepi atas jendela di bawah toolbar, ke bawah sampai sudut membulat jendela, dan ke samping sampai pembagi. Tidak ada glass yang tembus dan tidak ada sambungan di dalam kolom, terang maupun gelap. Pita toolbar di atas kolom halaman adalah milik sistem dan nadanya tidak sama dengan halaman (putih di mode terang), seperti sebelum ada inspector, jadi bukan temuan inspector. Teks di atas `surfaceWindow` tetap di pasangan P1 dan P2 spec 0012; tidak ada pasangan baru.
- **Lebar kolom.** Mengatur nilai pembagi inspector lewat accessibility ke nilai ekstrem menjepit kolom di 240 pt (pembagi di 659 pada jendela 900 pt) dan 360 pt (pembagi di 539). Saat dibuka di 900 pt lebarnya 280 pt. `.inspectorColumnWidth(min: 240, ideal: 280, max: 360)` bekerja seperti di spec 0022.
- **Jendela 900 pt, sidebar dan inspector terbuka.** Sidebar 236 pt, inspector 280 pt, dan kolom halaman **384 pt**, bukan 640. Sebabnya: `navigationSplitViewColumnWidth(min: 640)` dipenuhi kolom detail secara keseluruhan, yang memuat inspector (900 − 236 = 664 pt). Pada lebar inspector 240 pt halaman mendapat 424 pt. Di inspector tidak ada teks terpotong pada 240 maupun 280 pt: judul dan subjudul header membungkus, dua aksi bertumpuk, dan jalur panjang di baris kartu turun di bawah labelnya lalu membungkus (tangkapan di 240 pt tanpa potongan). Jendela tidak tumbuh dan tidak melempar pengecualian dengan `.frame(minWidth: 900, minHeight: 600)` di split view, seperti `AppShell` (diperiksa).
- **Toggle.** Pohon AX: `AXButton` dengan `AXDescription` dan `AXHelp` "Hide Inspector" dan `AXIdentifier` `inspector.toggle`, target 44 × 52 pt dari toolbar; setelah `AXPress` menjadi "Show Inspector". Header berupa `AXHeading`, status terbaca "Status: Running", dan tombol aksinya elemen terpisah; baris kartu terbaca "Software, PostgreSQL 17.4".
- **Pagar yang ditemukan.** Memasang `.inspector` di akar `NSHostingView` (di luar `NavigationSplitView`, dengan `.frame(minWidth: 900)` pada split view) membuat jendela tumbuh sendiri ke 1180 pt lalu AppKit melempar `NSGenericException` ("more Update Constraints in Window passes than there are views"). Memasangnya di isi kolom detail, seperti yang dilakukan `.stacklyInspector`, tidak. Karena itu dokumen aturan pakai (design.md) menyebut kolom detail. Hal serupa terlihat di galeri desain: `.stacklyInspector` yang dipasang pada satu bagian di dalam `ScrollView` halaman tidak punya tinggi sendiri dan isinya tergambar di atas bagian tetangganya (bukan dalam aliran gulir), jadi bagian demo asli di galeri membuka halaman sendiri di sheet, dan itu bekerja (inspector terbuka di sheet, toggle di halaman menutupnya, label berganti).

Dibangun menurut itu, dengan penyimpangan kecil dari spec yang tidak mengubah API: tinggi isi kolom dibuat minimal sama dengan tinggi kolom (lewat `onGeometryChange`) supaya `InspectorPlaceholder`, yang mengisi tinggi, berada di tengah kolom, dan toggle memakai `.labelStyle(.iconOnly)` supaya di toolbar dan di galeri hanya simbol yang tampil sementara VoiceOver membaca labelnya. Alternatif yang ditolak: `containerRelativeFrame(.vertical)` di placeholder, yang membuat isi sedikit melebihi kolom dan menampilkan scroller tanpa perlu.

**Keputusan user (2026-10-21).** Spec 0013 menjamin kolom isi minimal 640 pt, tetapi dengan inspector terbuka di jendela terkecil 900 pt halaman hanya mendapat 384 pt (424 pt dengan inspector tersempit). Dari tiga jalan, (a) menerima, (b) menaikkan lebar minimum jendela saat inspector terbuka ke 640 + 236 + 240 = 1116 pt, dan (c) menutup inspector otomatis di bawah ambang lebar (state `@SceneStorage` tidak lagi sama dengan yang tampil), user memilih (a): setiap halaman dengan inspector bekerja di kolom halaman 384 pt (tabel dan kartu membungkus atau bergulir, tanpa teks terpotong), dan jaminan 640 pt spec 0013 hanya berlaku saat inspector tertutup (update spec 0013 menyusul). Karena inspector yang dilebarkan ke 360 pt di jendela 900 pt memberi kolom halaman 900 − 236 − 360 = 304 pt, user juga memilih membatasi lebar maksimum inspector menjadi 280 pt di jendela di bawah 1000 pt, sehingga 384 pt tetap batas bawah yang sebenarnya; di jendela 1000 pt atau lebih tetap 240, 280, dan 360 pt. AC-18 dan bagian *Inspector* index.md mencatatnya, dan *Tambahan langkah 14* membangunnya. Spec 0018, 0021, dan 0022 menulis aturan 384 pt untuk halamannya.

## Perubahan 2026-10-22 (environment fixture di bawah inspector)

Update `/architect` sesi `arc-f126768-fix`, bersama spec 0068 K15. Sesi F17 menemukan bahwa varian HC snapshot halaman Catalog identik dengan varian standard, dan probe spec 0068 menunjukkan sebabnya: `.inspector` menampung ulang isi utama dan isi kolom, dan hanya `colorSchemeContrast` yang kembali ke nilai sistem di sana. Spec ini hanya mendapat catatan di *Inspector* dan *Key invariants*: `StacklyInspectorModifier` memanggil relay spec 0068 di `#if DEBUG` di dua posisi yang dibuktikan probe. Keputusan, opsi, dan bukti nya ada di rationale spec 0068 (*Keputusan K15*). AC-18 tidak berubah, karena perilaku Release `.stacklyInspector` sama persis; yang berubah hanya jalur Debug saat `fixtureAppearance` mengisi kunci `fixtureAccessibility` (snapshot dan preview).

## Perubahan 2026-10-22 (inspector seperti yang dibangun, wave 7)

Update `/architect` sesi `arc-ui-sync`, bersama spec 0012, 0013, 0068, dan `apps/macos/design.md`. Spec ini diselaraskan dengan tiga perbaikan sesi `dbg-f67-inspector` yang sudah di `main` (F67 bagian 1 `61d4e0ac`, bagian 2 `ace13361`). Tidak ada ID atau teks AC yang berubah: AC-18 sudah menuntut isi kolom di atas latar opak `surfaceWindow` dan tidak menyebut cara halaman diukur, dan perbaikan ini membuat kode memenuhi kalimat itu.

| Butir | Perubahan | Sumber |
| --- | --- | --- |
| *Inspector*, catatan baru *Kolom tidak vibrant* | `backgroundMaterial` nil di isi kolom, di dalam `ScrollView` | `6aaea490` |
| *Inspector*, catatan baru *Ukuran halaman* | frame 0 sampai tak hingga pada isi utama; halaman tanpa `minWidth` sendiri | `5d12cb22`, `e99c6061` |
| *Inspector*, catatan *Environment fixture* | relay isi utama di dalam `ZStack` sendiri | `e191d756`; detail di spec 0068 |
| Key invariants | dua butir: kolom tanpa material kaca, halaman mengambil ukuran yang ditawarkan | sama |
| Critical test scenarios | `columnContentDrawsWithoutTheGlassMaterial`, `pageTakesTheSizeTheColumnOffers` | `6aaea490`, `5d12cb22` |
| Build plan | catatan *Perbaikan langkah 14* (tanpa langkah baru) | merge `61d4e0ac`, `ace13361` |
| Consequences | dua butir negatif | update ini |

### Kolom tidak vibrant

**Temuan (macOS 27.0.1, 2026-10-22).** Di snapshot, ikon `ActionButton` di kolom inspector tergambar putih di atas putih. Pohon view kolom: `NSGlassEffectView` dengan appearance `NSAppearanceNameVibrantLight`, dan environment isi kolom membawa `backgroundMaterial` kaca (`DesignLibrary.GlassMaterialProvider`), sementara isi utama membawa nil. SwiftUI lalu menggambar bezel tombol, lapisan simbol SF, dan teks tanpa gaya eksplisit dengan filter `vibrantColorMatrix`. `cacheDisplay` tidak menjalankan filter Core Animation, jadi snapshot menunjukkan warna mentah. Di app nyata ikonnya tampil, tetapi filter itu menggeser semua warna kolom, padahal spec ini dan design.md menyatakan kolom opak `surfaceWindow` dan tidak pernah kaca.

- **Option 1: `.environment(\.backgroundMaterial, nil)` di komponen (dipilih).** Isi kolom tergambar seperti di permukaan opak lain, dengan warna token, di app nyata dan di snapshot. Kekurangan: bergantung pada kunci environment yang diamati, bukan aturan Apple; dan kolom sedikit berbeda dari inspector app lain.
- **Option 2: perbaikan di harness snapshot.** Ditolak: masalahnya juga ada di app nyata (warna kolom bergeser), jadi harness hanya akan menyembunyikannya.
- **Option 3: menerima gaya vibrant.** Ditolak: bertentangan dengan latar opak dan pasangan kontras P1 dan P2 spec 0012 yang dijanjikan AC-18 dan AC-19.

Posisinya sama dengan relay kolom (pada `VStack` di dalam `ScrollView`); dipasang pada `ScrollView` nilai itu tidak sampai ke isi. Sesudah perbaikan, di app nyata hanya kolom inspector yang berubah (paling banyak 71 level per kanal di light dan 27 di dark), dan halaman serta sidebar sama piksel demi piksel.

### Ukuran halaman

**Temuan (2026-10-22, dari F18 milestone 4).** Probe `Layout` yang mencatat usulan ukuran menunjukkan split view dari `.inspector` mengusulkan `(0, 0)` dan `(inf, inf)` ke halaman, terbuka atau tertutup. Di `(0, 0)`, halaman dengan catatan yang membungkus menjawab 32 × 5228 pt, lalu `NSSplitView` dan halaman ditata 1044 × 5228 pt di jendela 748 pt. Halaman Catalog dengan panel NATS yang panjang kena akibat yang sama: split view 1492 pt, kolom inspector yang tidak bisa digulir, dan baris kartu Capabilities 35 pt (bukan 39 pt). F18 menutupinya dengan `.frame(minWidth: 384)` di halaman Runtimes.

- **Option 1: frame 0 sampai tak hingga di `StacklyInspectorModifier` (dipilih).** Dengan kedua batas terpasang, frame mengambil ukuran yang diusulkan, jadi halaman menjawab 0 × 0 di `(0, 0)` dan split view tetap setinggi jendela. Satu tempat untuk semua halaman. Kekurangan: halaman tidak bisa lagi memaksa ukuran minimum lewat view akar nya.
- **Option 2: setiap halaman memasang `minWidth` sendiri.** Ditolak: menyembunyikan gejala, harus diingat setiap halaman baru, dan angka 384 pt adalah urusan jendela dan inspector, bukan halaman.
