# Verify: Komponen dasar Stackly · spec 0067 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0067. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Langkah yang butuh setting sistem atau VoiceOver sungguhan ditandai `NOT_RUN` dan menjadi langkah manual user; Claude tidak mengubah System Settings dan tidak menyalakan VoiceOver._

## Milestone 1: Base components, actions, and feedback

_Dibangun 2026-10-21 oleh `dev-f67-m1` di branch sendiri (langkah Build plan 1 sampai 8). Hasil yang sudah dijalankan dicentang; `NOT_RUN` menunggu user atau `/check verify`. Catatan: app diperiksa lewat pohon accessibility (API AX) dan tangkapan jendela per ID jendela, tanpa VoiceOver._

### UI / manual

- [x] Langkah 1 · di macOS 27.0.1 buka app probe dengan `Button { Text(judul); Text(alasan) }` nonaktif di `Menu` toolbar, `.contextMenu`, dan `CommandMenu` → subtitle tampil di bawah judul di ketiganya, tetapi pohon AX item menu hanya memuat `AXTitle`; jalur yang dipakai adalah item nonaktif tambahan (`MenuReasonPath.extraItem`) → AC-8 (catatan di `rationale.md` *Temuan langkah 1* dan `design.md` Menus)
- [ ] `NOT_RUN` · nyalakan VoiceOver, buka menu More di halaman Components lalu ubah `MenuReasonPath.current` ke `.subtitle` → VoiceOver membacakan alasan sebagai subtitle item menu toolbar, menu konteks, dan `CommandMenu`; kalau ya, `.subtitle` boleh dipakai dan menu menjadi lebih pendek → AC-8 (langkah manual user)
- [x] Jalankan app Debug (`STACKLY_ROOT` sementara), Window › Design Gallery › Components → halaman memuat ProviderTile (enam tile di 18, 26, 40, 52 pt, mark nil dengan nama, tile nil, unrecognized), CardSection dan key value (baris biasa, nilai monospace, nilai panjang yang turun di bawah label, baris dengan CopyButton di luar `LabeledContent`, footer teks, footer `DisabledReasonText`), CopyButton, ConsoleBlock, ActionButton, ActionMenu, dan FeedbackCard → AC-20
- [x] Di halaman Components baca pohon AX (axwin) → tile tidak ada di pohon (tersembunyi); setiap baris key value satu elemen `AXStaticText` "Software, PostgreSQL 17.4"; tombol Copy `AXButton` "Copy address" dengan help sama dan identifier `copy.button.gallery`; ConsoleBlock `AXDescription` "Equivalent command" dengan `AXValueDescription` berisi perintah tanpa "$ "; tombol aksi memakai judulnya dengan identifier `action.button.<id>`, `AXValue` "In progress" saat sibuk, dan help berisi alasan saat nonaktif; menu More `AXMenuButton` "More" dengan identifier `action.more`; setiap kartu umpan balik `AXGroup` `feedback.card` berisi satu elemen teks dan tombol "Dismiss" `feedback.dismiss` (dan `feedback.action`) → AC-2, AC-3, AC-4, AC-5, AC-7, AC-9, AC-19, AC-20
- [x] Tekan More di halaman Components → menu menampilkan Copy Connection String dan Start, pemisah, Restart nonaktif dengan baris nonaktif "In progress" di bawahnya, Back Up… nonaktif dengan baris nonaktif berisi alasan, pemisah, lalu Remove… terakhir; tanpa shortcut → AC-8
- [x] Tekan Remove… di bagian ActionButton → alert native "Remove “pg17-dev”?" dengan tombol Remove dan Cancel muncul di atas jendela (satu host), Cancel menutupnya tanpa menjalankan apa pun → AC-6, AC-7
- [x] Di demo umpan balik (app aktif): tekan Success → kartu `positive` muncul dan hilang sendiri setelah sekitar 4 detik; saat app lain aktif, hitungan tertahan → AC-9
- [x] Di demo umpan balik: tekan Not Sent → kartu `caution` "stacklyd is unavailable. Nothing was sent." tetap tampil lebih dari 7 detik; tekan Success saat itu → kartu `caution` tidak diganti; tekan Dismiss → kartu hilang → AC-9, AC-10
- [ ] `NOT_RUN` · di demo umpan balik: tahan pointer di atas kartu `positive`, lalu fokuskan tombol Dismiss dengan Tab (Keyboard navigation menyala) → kartu tidak hilang selama pointer di atasnya dan selama fokus di tombolnya, dan hitungan 4 detik dimulai dari awal setelah keduanya pergi → AC-9 (butuh pointer dan setting keyboard dari user)
- [ ] `NOT_RUN` · matikan daemon (`stackly daemon stop`) di app dengan daemon development lalu tekan sebuah aksi mutasi → kartu `caution` "stacklyd is unavailable. Nothing was sent." tampil di bawah halaman, diumumkan sekali, dan tetap sampai Dismiss; kontrol yang difokus di bawah halaman tidak tertutup kartu → AC-9, AC-11 (butuh layar yang memakai `Feedback.outcome`, yang baru dibangun fitur fase 02)
- [ ] `NOT_RUN` · nyalakan Keyboard navigation di System Settings, tekan Tab dan Space di setiap tombol komponen (Copy, aksi, More dan itemnya, Dismiss dan aksi kartu) → semuanya bisa difokus dan dijalankan → AC-19
- [ ] `NOT_RUN` · nyalakan VoiceOver di halaman Components → baris key value dibaca "{label}, {value}", tombol Copy "Copy {what}" dan pengumuman "{What} copied." terdengar sekali setelah ditekan, blok konsol dibaca label lalu perintah, tombol sibuk dibaca dengan nilai "In progress", umpan balik diumumkan sekali (pesan lalu detail) → AC-4, AC-5, AC-7, AC-9, AC-19
- [ ] `NOT_RUN` · Increase Contrast, Reduce Transparency, Reduce Motion, Differentiate Without Color, light dan dark pada halaman Components → kartu, banner, dan baris tint mendapat border 1 pt saat Increase Contrast; tidak ada yang berubah saat Reduce Transparency; kartu umpan balik hanya crossfade; tone selalu berpasangan dengan simbol atau kata → AC-19
- [ ] `NOT_RUN` · aktifkan Narrow Width (640 pt) di jendela galeri → tidak ada label atau judul yang terpotong dan teks membungkus; nilai panjang turun di bawah labelnya → AC-3, AC-19
- [ ] `NOT_RUN` · app dengan `LiveShell` (`cargo xtask build-app`, jalankan, buka jendela utama) → kolom detail punya `.feedbackPresenter()` di inset bawah dan akar shell punya `.actionHost()` (juga dicek test pemindai sumber) → AC-11

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI` (dengan `STACKLY_DAEMON_BIN` dan `STACKLY_SEED_BIN` menunjuk `target/debug/stacklyd` dan `target/debug/examples/skeleton_seed` dari checkout utama) → 376 test di 45 suite lulus, termasuk test komponen di bawah → AC-1 sampai AC-21
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ProviderTileTests` → mark default ("PostgreSQL" "Po", "9front" "9f", "Á" "?", kosong dan nil "?"), tabel warna tile (dua belas baris), ukuran 18, 26, 40, 52, dan fallback mark → AC-2
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter KeyValueLayoutTests` → nilai pendek satu baris, nilai panjang turun dan membungkus, tinggi baris minimal `rowMinHeight`, `ForEach` satu baris per elemen, judul dan footer menambah tinggi, elemen VoiceOver dibangun dari label dan nilai → AC-3
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CopyConfirmationTests|ConsoleBlockTests"` → satu tulis dan satu pengumuman per tekanan, tekanan kedua mulai ulang 2 detik, wait yang dibatalkan tidak mengubah state, pengumuman default "{What} copied.", label VoiceOver, tombol 24 × 24 pt, permukaan konsol dari token saja → AC-4, AC-5, AC-19
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter StacklyActionTests` → `availability(gate:)` untuk setiap kombinasi alasan sendiri, gate, dan sibuk; `invoke` tidak berbuat apa pun saat tidak `.enabled`, `present` dengan judul, pesan, label, dan role yang benar untuk standard dan destructive, `perform` langsung tanpa konfirmasi; `ActionHost.run` memakai gate host; tanpa shortcut dan tanpa tap gesture; tombol destruktif tidak pernah prominent → AC-6, AC-7, AC-8, AC-19
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "FeedbackTests|FeedbackCenterTests"` → `Feedback.outcome` untuk keempat case dan `Feedback.failure` (hint nil, pesan bertitik, `showOperations` nil), `visibleDuration` (4, 5, 7, 10 detik), `isPersistent` per tone dan dengan aksi, `show` mengganti dan mengumumkan sekali, `show` sementara di atas umpan balik tetap hanya mengumumkan, `countdown` mengosongkan setelah durasi, tidak mengosongkan kalau id berganti, tidak saat dibatalkan, dan langsung kembali untuk umpan balik tetap, `FeedbackHold.countsDown` untuk delapan kombinasi, `DaemonText.sentence` → AC-9, AC-10, AC-11
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter SourceRuleTests` → enam aturan lulus, termasuk dua aturan baru (pasteboard di luar `PlainPasteboard.swift` dan `ConcealedPasteboard.swift`, `.inspector(` di luar `Components/Inspector/`) dengan contoh terlarang dan contoh yang boleh; aturan warna sekunder dipersempit agar `prominence: .secondary` milik `ActionProminence` tidak terhitung (contoh terlarang `.foregroundStyle(.secondary)`, `Color.secondary`, dan ternary tetap tertangkap) → AC-21
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ShellStructureTests` → `AppShell.swift` memasang `.feedbackPresenter()` di kolom detail dan `.actionHost()` sebelum `.environment(\.mutationAvailability`, `LiveShell.swift` membuat tepat satu `FeedbackCenter` dan menaruhnya di environment → AC-11
- [x] `swift test --package-path clients/swift` → 48 test `StacklyTestSupportTests` (satu known issue lama) dan 154 test `StacklyClientTests` lulus → tidak ada regresi
- [x] `cargo xtask build-app` → `BUILD SUCCEEDED`, `target/xcode/Build/Products/Debug/Stackly.app` → AC-20
- [x] `xcrun swift-format lint --recursive apps/macos/Packages/StacklyUI` → tanpa peringatan
- [ ] `NOT_RUN` · `cargo xtask evidence check` → dijalankan lead setelah merge

### Acceptance-criteria coverage

- AC-1 (pemakaian ulang dan lokasi) · folder `Components/` dan bagian "Components" di `design.md` (Tile, Key value, Copy and console, Actions, Menus, Feedback); *Status checklist* dan *State tahap* menunggu milestone 2 · test `SourceRuleTests`, `StacklyActionTests`
- AC-2 · `ProviderTileTests`, galeri · AC-3 · `KeyValueLayoutTests`, cek AX, galeri · AC-4 · `CopyConfirmationTests`, `ConsoleBlockTests` · AC-5 · `ConsoleBlockTests`, cek AX
- AC-6 · `StacklyActionTests`, alert host di galeri · AC-7 · `StacklyActionTests`, galeri · AC-8 · langkah 1 plus menu di galeri; subtitle dibacakan VoiceOver `NOT_RUN`
- AC-9 · `FeedbackCenterTests`, demo di galeri (hold hover dan fokus `NOT_RUN`) · AC-10 · `FeedbackTests` · AC-11 · `ShellStructureTests`; perilaku di app dengan layar sungguhan menunggu fitur fase 02
- AC-12 sampai AC-17 · menunggu milestone 2 (sheet, checklist, progress) dan milestone 4 (log, diff) · AC-18 · menunggu milestone 3 (inspector); hanya aturan sumbernya yang ada di milestone 1 (AC-21)
- AC-19 · dicek untuk komponen milestone 1 lewat test sumber dan pohon AX; Tab, Space, VoiceOver, dan setting sistem `NOT_RUN` · AC-20 · galeri Components (bagian milestone 1) dan tiga `#Preview` per komponen; bagian milestone 2 sampai 4 menunggu · AC-21 · `SourceRuleTests`

## Milestone 3: Inspector

_Dibangun 2026-10-21 oleh `dev-f67-m3` di branch sendiri (langkah Build plan 14). Hasil yang sudah dijalankan dicentang; `NOT_RUN` menunggu user atau `/check verify`. Catatan: app diperiksa lewat pohon accessibility (API AX atas proses probe dan app) dan tangkapan jendela per ID jendela, tanpa VoiceOver dan tanpa mengubah System Settings._

### UI / manual

- [x] Langkah 14 · di macOS 27.0.1 buka app probe dengan `NavigationSplitView` dan `.stacklyInspector` di kolom halaman, jendela 900 pt, terang dan gelap → latar `surfaceWindow` mengisi seluruh kolom inspector tanpa sambungan; inspector 280 pt dibuka, kolom halaman 384 pt, tidak ada teks terpotong; pembagi inspector dijepit 240 dan 360 pt lewat AX; toggle `AXButton` "Hide Inspector" dengan help sama dan identifier `inspector.toggle`, menjadi "Show Inspector" setelah `AXPress` → AC-18, AC-19 (catatan di `rationale.md` *Temuan langkah 14* dan `design.md` Inspector)
- [x] Jalankan app Debug (`STACKLY_ROOT` sementara), Window › Design Gallery › Components → halaman memuat InspectorHeader (kolom 280 pt dengan tile, status, dua aksi, dan satu CardSection; kolom 280 pt tanpa aksi; kolom 240 pt dengan judul dan subjudul panjang yang membungkus dan dua aksi yang bertumpuk), InspectorPlaceholder (judul bawaan dengan pesan, "Nothing to Show"), dan bagian `.stacklyInspector` dengan tombol "Open Inspector Demo"; pohon AX memuat `inspector.placeholder` ("No Selection, Select an instance to see its details.") → AC-20
- [x] Di bagian `.stacklyInspector` tekan "Open Inspector Demo" → sheet dengan halaman dan inspector asli (tile, "pg17-dev", "Status: Running" dan CardSection) terbuka; tekan toggle `inspector.toggle` ("Hide Inspector") → inspector menutup dan label menjadi "Show Inspector" → AC-18
- [ ] `NOT_RUN` · nyalakan Keyboard navigation di System Settings, tekan Tab dan Space di setiap tombol komponen (Copy, aksi, More dan itemnya, Dismiss dan aksi kartu) → semuanya bisa difokus dan dijalankan → AC-19
- [ ] `NOT_RUN` · nyalakan VoiceOver di halaman Components → baris key value dibaca "{label}, {value}", tombol Copy "Copy {what}" dan pengumuman "{What} copied." terdengar sekali setelah ditekan, blok konsol dibaca label lalu perintah, tombol sibuk dibaca dengan nilai "In progress", umpan balik diumumkan sekali (pesan lalu detail) → AC-4, AC-5, AC-7, AC-9, AC-19
- [ ] `NOT_RUN` · Increase Contrast, Reduce Transparency, Reduce Motion, Differentiate Without Color, light dan dark pada halaman Components → kartu, banner, dan baris tint mendapat border 1 pt saat Increase Contrast; tidak ada yang berubah saat Reduce Transparency; kartu umpan balik hanya crossfade; tone selalu berpasangan dengan simbol atau kata → AC-19
- [ ] `NOT_RUN` · aktifkan Narrow Width (640 pt) di jendela galeri → tidak ada label atau judul yang terpotong dan teks membungkus; nilai panjang turun di bawah labelnya → AC-3, AC-19
- [ ] `NOT_RUN` · app dengan `LiveShell` (`cargo xtask build-app`, jalankan, buka jendela utama) → kolom detail punya `.feedbackPresenter()` di inset bawah dan akar shell punya `.actionHost()` (juga dicek test pemindai sumber) → AC-11

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI` (dengan `STACKLY_DAEMON_BIN` dan `STACKLY_SEED_BIN` menunjuk `target/debug/stacklyd` dan `target/debug/examples/skeleton_seed` dari checkout utama) → 376 test di 45 suite lulus, termasuk test komponen di bawah → AC-1 sampai AC-21
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ProviderTileTests` → mark default ("PostgreSQL" "Po", "9front" "9f", "Á" "?", kosong dan nil "?"), tabel warna tile (dua belas baris), ukuran 18, 26, 40, 52, dan fallback mark → AC-2
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter KeyValueLayoutTests` → nilai pendek satu baris, nilai panjang turun dan membungkus, tinggi baris minimal `rowMinHeight`, `ForEach` satu baris per elemen, judul dan footer menambah tinggi, elemen VoiceOver dibangun dari label dan nilai → AC-3
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CopyConfirmationTests|ConsoleBlockTests"` → satu tulis dan satu pengumuman per tekanan, tekanan kedua mulai ulang 2 detik, wait yang dibatalkan tidak mengubah state, pengumuman default "{What} copied.", label VoiceOver, tombol 24 × 24 pt, permukaan konsol dari token saja → AC-4, AC-5, AC-19
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter StacklyActionTests` → `availability(gate:)` untuk setiap kombinasi alasan sendiri, gate, dan sibuk; `invoke` tidak berbuat apa pun saat tidak `.enabled`, `present` dengan judul, pesan, label, dan role yang benar untuk standard dan destructive, `perform` langsung tanpa konfirmasi; `ActionHost.run` memakai gate host; tanpa shortcut dan tanpa tap gesture; tombol destruktif tidak pernah prominent → AC-6, AC-7, AC-8, AC-19
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "FeedbackTests|FeedbackCenterTests"` → `Feedback.outcome` untuk keempat case dan `Feedback.failure` (hint nil, pesan bertitik, `showOperations` nil), `visibleDuration` (4, 5, 7, 10 detik), `isPersistent` per tone dan dengan aksi, `show` mengganti dan mengumumkan sekali, `show` sementara di atas umpan balik tetap hanya mengumumkan, `countdown` mengosongkan setelah durasi, tidak mengosongkan kalau id berganti, tidak saat dibatalkan, dan langsung kembali untuk umpan balik tetap, `FeedbackHold.countsDown` untuk delapan kombinasi, `DaemonText.sentence` → AC-9, AC-10, AC-11
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter SourceRuleTests` → enam aturan lulus, termasuk dua aturan baru (pasteboard di luar `PlainPasteboard.swift` dan `ConcealedPasteboard.swift`, `.inspector(` di luar `Components/Inspector/`) dengan contoh terlarang dan contoh yang boleh; aturan warna sekunder dipersempit agar `prominence: .secondary` milik `ActionProminence` tidak terhitung (contoh terlarang `.foregroundStyle(.secondary)`, `Color.secondary`, dan ternary tetap tertangkap) → AC-21
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ShellStructureTests` → `AppShell.swift` memasang `.feedbackPresenter()` di kolom detail dan `.actionHost()` sebelum `.environment(\.mutationAvailability`, `LiveShell.swift` membuat tepat satu `FeedbackCenter` dan menaruhnya di environment → AC-11
- [x] `swift test --package-path clients/swift` → 48 test `StacklyTestSupportTests` (satu known issue lama) dan 154 test `StacklyClientTests` lulus → tidak ada regresi
- [x] `cargo xtask build-app` → `BUILD SUCCEEDED`, `target/xcode/Build/Products/Debug/Stackly.app` → AC-20
- [x] `xcrun swift-format lint --recursive apps/macos/Packages/StacklyUI` → tanpa peringatan
- [ ] `NOT_RUN` · `cargo xtask evidence check` → dijalankan lead setelah merge

### Acceptance-criteria coverage

- AC-1 (pemakaian ulang dan lokasi) · folder `Components/` dan bagian "Components" di `design.md` (Tile, Key value, Copy and console, Actions, Menus, Feedback); *Status checklist* dan *State tahap* menunggu milestone 2 · test `SourceRuleTests`, `StacklyActionTests`
- AC-2 · `ProviderTileTests`, galeri · AC-3 · `KeyValueLayoutTests`, cek AX, galeri · AC-4 · `CopyConfirmationTests`, `ConsoleBlockTests` · AC-5 · `ConsoleBlockTests`, cek AX
- AC-6 · `StacklyActionTests`, alert host di galeri · AC-7 · `StacklyActionTests`, galeri · AC-8 · langkah 1 plus menu di galeri; subtitle dibacakan VoiceOver `NOT_RUN`
- AC-9 · `FeedbackCenterTests`, demo di galeri (hold hover dan fokus `NOT_RUN`) · AC-10 · `FeedbackTests` · AC-11 · `ShellStructureTests`; perilaku di app dengan layar sungguhan menunggu fitur fase 02
- AC-12 sampai AC-17 · menunggu milestone 2 (sheet, checklist, progress) dan milestone 4 (log, diff) · AC-18 · menunggu milestone 3 (inspector); hanya aturan sumbernya yang ada di milestone 1 (AC-21)
- AC-19 · dicek untuk komponen milestone 1 lewat test sumber dan pohon AX; Tab, Space, VoiceOver, dan setting sistem `NOT_RUN` · AC-20 · galeri Components (bagian milestone 1) dan tiga `#Preview` per komponen; bagian milestone 2 sampai 4 menunggu · AC-21 · `SourceRuleTests`

## Milestone 3: Inspector

_Dibangun 2026-10-21 oleh `dev-f67-m3` di branch sendiri (langkah Build plan 14). Hasil yang sudah dijalankan dicentang; `NOT_RUN` menunggu user atau `/check verify`. Catatan: app diperiksa lewat pohon accessibility (API AX atas proses probe dan app) dan tangkapan jendela per ID jendela, tanpa VoiceOver dan tanpa mengubah System Settings._

### UI / manual

- [x] Langkah 14 · di macOS 27.0.1 buka app probe dengan `NavigationSplitView` dan `.stacklyInspector` di kolom halaman, jendela 900 pt, terang dan gelap → latar `surfaceWindow` mengisi seluruh kolom inspector tanpa sambungan; inspector 280 pt dibuka, kolom halaman 384 pt, tidak ada teks terpotong; pembagi inspector dijepit 240 dan 360 pt lewat AX; toggle `AXButton` "Hide Inspector" dengan help sama dan identifier `inspector.toggle`, menjadi "Show Inspector" setelah `AXPress` → AC-18, AC-19 (catatan di `rationale.md` *Temuan langkah 14* dan `design.md` Inspector)
- [ ] Jalankan app Debug (`STACKLY_ROOT` sementara), Window › Design Gallery › Components → halaman memuat InspectorHeader (dengan tile, status, dan aksi; tanpa aksi; judul panjang di kolom 240 pt), InspectorPlaceholder (judul bawaan dengan pesan, judul sendiri), dan bagian `.stacklyInspector` dengan toggle → AC-20
- [ ] Di bagian `.stacklyInspector` halaman Components tekan toggle di bagian itu dan toggle di toolbar → inspector asli terbuka di jendela galeri dan tertutup, label toggle berganti "Show Inspector" dan "Hide Inspector" → AC-18
- [ ] `NOT_RUN` · nyalakan Keyboard navigation di System Settings, fokuskan toggle inspector dengan Tab dan tekan Space → inspector terbuka dan tertutup → AC-19 (butuh setting sistem dari user)
- [ ] `NOT_RUN` · nyalakan VoiceOver di halaman Components → toggle dibacakan "Hide Inspector" atau "Show Inspector"; placeholder dibaca satu kelompok, judul lalu pesan; header dibaca judul sebagai heading, status "Status: Running", lalu tombol aksi satu per satu → AC-18, AC-19
- [ ] `NOT_RUN` · Increase Contrast, Reduce Transparency, Reduce Motion, light dan dark pada inspector asli → latar tetap opak dan tidak berubah saat Reduce Transparency, tidak ada gerak selain crossfade bawaan sistem → AC-19
- [ ] `NOT_RUN` · di jendela 900 pt dengan sidebar dan inspector terbuka di layar fitur 22 atau 18 (setelah milestone fitur itu dibangun) → tidak ada teks terpotong di kolom halaman 384 pt maupun di inspector → AC-18

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter InspectorTests` → lebar kolom 240, 280, 360 dan padding 16; label toggle mengikuti binding; help bawaan label dan help dari layar; simbol `sidebar.trailing` ada; identifier `inspector.toggle` dan `inspector.placeholder`; judul dan subjudul panjang membungkus di kolom 240 pt; tile menambah tinggi header; aksi lebar bertumpuk di 240 pt dan berjajar di 480 pt; header tanpa aksi lebih pendek; placeholder bawaan "No Selection" tanpa pesan dan pesan panjang membungkus → AC-18, AC-19
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter SourceRuleTests` → aturan `.inspector(` di luar `Components/Inspector/` lulus untuk seluruh sumber, termasuk `StacklyInspector.swift` → AC-21
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` (dengan `STACKLY_DAEMON_BIN` dan `STACKLY_SEED_BIN` menunjuk `target/debug/stacklyd` dan `target/debug/examples/skeleton_seed` dari checkout utama) → 421 test di 48 suite lulus
- [x] `cargo xtask build-app` → `BUILD SUCCEEDED`
- [x] `xcrun swift-format lint --strict` atas file Swift milestone ini → tanpa peringatan
- [x] `cargo xtask evidence check` → 39 catatan manual, baseline valid, 577 file Rust dan 237 file Swift mengikuti aturan; lead menjalankannya lagi setelah merge

### Acceptance-criteria coverage

- AC-18 · `InspectorTests`, langkah 14 (probe), galeri; kolom 900 pt di layar sungguhan menunggu fitur 22, 18, dan 21; VoiceOver `NOT_RUN`
- AC-19 · target toggle 44 × 52 pt dari toolbar dan label di pohon AX (langkah 14); Tab, Space, VoiceOver, dan setting sistem `NOT_RUN`
- AC-20 · bagian Inspector di galeri Components dan tiga `#Preview` per komponen (`InspectorHeader`, `InspectorPlaceholder`, `InspectorToggle` dengan `.stacklyInspector`); dicocokkan `/check verify` dengan tabel *State di galeri*
- AC-21 · aturan `.inspector(` di `SourceRuleTests` (dibangun di milestone 1) lulus untuk kode milestone ini

