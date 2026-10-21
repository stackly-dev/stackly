# Verify: Menu aplikasi, perintah & shortcut keyboard · spec 0069 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0069. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27. Probe memakai accessibility API dan event tombol ke proses app, jadi proses pemeriksa butuh izin Accessibility; jangan mengubah System Settings._

## Milestone 1: Command map and probe

### UI / manual

- [x] Buka `apps/macos/design.md` bagian "Commands and shortcuts" → tabel hasil probe P1 sampai P7 dan P3b memuat versi macOS 27.0.1 (26A434) dan Xcode 27.0 (27A266a), dan setiap baris punya hasil dan akibatnya untuk spec → AC-2, AC-3
- [x] Bangun app Debug (`cargo xtask build-app`), jalankan dengan `STACKLY_ROOT` ke folder sementara, lalu dump semua menu bar lewat accessibility API (`AXMenuItemCmdChar`, `AXMenuItemCmdModifiers`) → tidak ada dua item dengan key equivalent yang sama; selain tabel A hanya ⌥⌘Q (Quit and Keep Windows) dan keluarga 🌐 (🌐D, 🌐E, 🌐F, 🌐⌃F, 🌐⌃C, 🌐⌃R) yang tampil, dan ⌥⌘Q ada di `SystemShortcuts.blocked` → AC-2
- [ ] Ulangi probe P2 sampai P7 dan P3b di app stand in atau di app Stackly setelah milestone 2 → semuanya sama dengan hasil di `design.md`; kalau macOS atau Xcode naik versi, catat versi baru → AC-2, AC-3, AC-10

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter CommandMapTests` → semua test peta lulus: satu baris untuk setiap `CommandID` dengan raw value yang tetap, shortcut persis tabel *Peta perintah*, tidak ada dua baris dengan shortcut yang sama (built dan planned), tidak ada baris di tabel A atau aturan D, baris di tabel B hanya dengan makna sama atau `exception`, baris di tabel C hanya dengan `exception`, akhiran "…" sama dengan `needsInput`, tabel A sampai C tidak bentrok satu sama lain, tampilan "⇧⌘R" dan "⌘?", `helpText` dengan dan tanpa shortcut, dan `layout(menu:submenu:)` untuk status awal (kosong) dan untuk peta dengan semua baris built (kelompok berurut, tanpa kelompok kosong) → AC-1, AC-2
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter CommandSourceRuleTests` → aturan sumber lulus: `.keyboardShortcut(` dengan argumen selain `.defaultAction`, `.cancelAction`, atau `CommandMap`, dan `CommandGroup`, `CommandMenu`, `.commands` tidak muncul di luar `Commands/` dan `StacklyApp.swift`; setiap contoh terlarang tertangkap; `CommandID` built dipakai tepat satu kali oleh `CommandItem` (system oleh scene), planned tidak dipakai → AC-1
- [x] Ubah sementara satu shortcut di `CommandMap.swift` ke ⌘Q, jalankan `swift test ... --filter CommandMapTests` → gagal di `noRowBreaksAShortcutRule`; kembalikan → AC-2
- [x] Tambah sementara `.keyboardShortcut("x")` di salah satu file `Components/` atau halaman, jalankan `--filter CommandSourceRuleTests` → gagal di `sourcesFollowTheRule`; kembalikan → AC-1
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus (test Swift integration berakhir `BLOCKED` kalau `stacklyd` dan `skeleton_seed` belum dibangun) dan `cargo xtask build-app` berhasil → AC-1, AC-2
- [x] `xcrun swift-format lint --recursive apps/macos/Packages/StacklyUI/Sources/StacklyUI/Commands apps/macos/Packages/StacklyUI/Tests/StacklyUITests/Commands` → tidak ada peringatan → AC-1

### Hasil /check verify milestone 1 · 2026-10-21 · PASS (satu langkah belum dijalankan)

Diperiksa di macOS 27.0.1 (26A434), Xcode 27.0 (27A266a), dari merge `524ec3f`. Semua langkah yang bertanda `[x]` dijalankan ulang oleh checker, bukan disalin dari laporan developer.

- Langkah 1: `design.md` memuat versi 27.0.1 (26A434) dan 27.0 (27A266a), dan tiap baris P1 sampai P7 dan P3b punya kolom hasil dan akibat.
- Langkah 2: app Debug dari `cargo xtask build-app` dijalankan langsung dengan `STACKLY_ROOT` ke folder sementara (dihapus sesudahnya), menu bar didump lewat AX (`AXMenuItemCmdChar`, `AXMenuItemCmdModifiers`). 22 item ber-key equivalent di luar menu Apple, tidak ada pasangan yang sama. Di luar tabel A hanya ⌥⌘Q (Quit and Keep Windows, ada di `SystemShortcuts.blocked`), keluarga 🌐 (bit 16 pada `AXMenuItemCmdModifiers`: 🌐D, 🌐E, 🌐F, 🌐⌃F Fill, 🌐⌃C Center, 🌐⌃R, dan juga 🌐⌃ dengan panah serta kombinasi Arrange di Window > Move & Resize), dan item Help bawaan "Stackly Help ⌘?" yang akan diganti `CommandGroup(replacing: .help)`. Dump: scratchpad sesi `ax/p1-dump.txt`.
- Langkah 3 (ulang P2 sampai P7 dan P3b) tetap `[ ]`: checker mengulang P1, P2, P3, dan P3b di app Debug sungguhan dengan perubahan sementara di `StacklyApp.swift` (`.commands` dengan `replacing: .newItem`, `.textEditing`, `.help`, `CommandMenu("Instance")`, dan `.keyboardShortcut("0")` pada `Window`; dikembalikan lewat `git checkout`, working tree bersih). Hasilnya sama dengan `design.md`: File berisi New Instance… ⌘N, Close ⌘W, Close All tanpa "New Window"; Edit berisi Find ▸ Find… ⌘F dan item sistem; Help berisi kolom pencarian sistem dan Welcome to Stackly; menu Instance duduk di antara View dan Window; Window > Stackly ⌘0 membuka jendela lagi sesudah ⌘W (lewat tombol dan lewat menu) dan mengembalikannya dari minimize. P3b diulang tanpa `replacing: .textEditing`: menu Edit tanpa item Find, jadi rencana `replacing: .textEditing` benar. P4 sampai P7 (focused value, sheet, `.searchFocused`, shortcut di field pencarian, state basi) tidak bisa diulang di app sungguhan karena shell belum punya `.searchable`, `focusedSceneValue`, atau `StacklyCommands`; hasil developer di app stand in diterima sebagai dasar milestone 1 dan langkah ini diulang di milestone 2 langkah 5.
- Langkah 4: `CommandMapTests` 25 test lulus (53 kasus argumen pada `aRowOnABlockedShortcutIsRefused`). Langkah 5: `CommandSourceRuleTests` 9 test lulus.
- Langkah 6: shortcut Overview diubah ke ⌘Q → `noRowBreaksAShortcutRule` dan `rowsCarryTheShortcutsOfTheSpecTable` gagal; dikembalikan.
- Langkah 7: `.keyboardShortcut("x")` ditambah di `Shell/UnbuiltPage.swift` → `sourcesFollowTheRule` gagal pada aturan "keyboard shortcut outside the command map"; dikembalikan. (Folder `Components/` tidak ada; `Shell/` dipakai.)
- Langkah 8: `swift test --package-path apps/macos/Packages/StacklyUI` → 349 test di 40 suite lulus tanpa `BLOCKED`; `cargo xtask build-app` → BUILD SUCCEEDED. Langkah 9: `swift-format lint` bersih.
- Catatan, bukan kegagalan: daftar 🌐 di `design.md` baris P1 tidak menyebut item panah 🌐⌃ dan kombinasi Arrange di Window > Move & Resize; aturan E sudah mencakupnya. Semua 30 baris peta `planned` seperti yang direncanakan (`everyRowStartsPlannedUntilItsMenuItemExists`).

### Acceptance-criteria coverage

- AC-1 · bagian peta dan aturan sumber: `CommandMap`, `CommandEntry`, `layout`, `helpText`, dan aturan `.keyboardShortcut`, `CommandGroup`, `CommandMenu`, `.commands`, serta pemakaian `CommandID`; seluruh baris berstatus `planned` karena `CommandItem` dan `StacklyCommands` baru ada di milestone 2, yang juga mengubah status baris menjadi `built` dan menambah cek "tidak ada `provisional: F69`" (AC-16)
- AC-2 · bagian test unit atas `CommandMap` dan `SystemShortcuts` dan hasil probe P1 di app Stackly saat ini; probe atas menu bar yang sudah memakai peta (setiap baris built tampil dengan shortcut persis peta) menunggu milestone 2 langkah 5
- AC-3 sampai AC-16 · belum, milik Milestone 2 dan 3 (probe P2 sampai P7 sudah memberi dasarnya untuk AC-3, AC-4, AC-5, AC-10, AC-13)
