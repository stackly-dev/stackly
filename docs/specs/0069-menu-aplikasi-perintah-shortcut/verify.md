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
- [ ] Ubah sementara satu shortcut di `CommandMap.swift` ke ⌘Q, jalankan `swift test ... --filter CommandMapTests` → gagal di `noRowBreaksAShortcutRule`; kembalikan → AC-2
- [ ] Tambah sementara `.keyboardShortcut("x")` di salah satu file `Components/` atau halaman, jalankan `--filter CommandSourceRuleTests` → gagal di `sourcesFollowTheRule`; kembalikan → AC-1
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus (test Swift integration berakhir `BLOCKED` kalau `stacklyd` dan `skeleton_seed` belum dibangun) dan `cargo xtask build-app` berhasil → AC-1, AC-2
- [x] `xcrun swift-format lint --recursive apps/macos/Packages/StacklyUI/Sources/StacklyUI/Commands apps/macos/Packages/StacklyUI/Tests/StacklyUITests/Commands` → tidak ada peringatan → AC-1

### Acceptance-criteria coverage

- AC-1 · bagian peta dan aturan sumber: `CommandMap`, `CommandEntry`, `layout`, `helpText`, dan aturan `.keyboardShortcut`, `CommandGroup`, `CommandMenu`, `.commands`, serta pemakaian `CommandID`; seluruh baris berstatus `planned` karena `CommandItem` dan `StacklyCommands` baru ada di milestone 2, yang juga mengubah status baris menjadi `built` dan menambah cek "tidak ada `provisional: F69`" (AC-16)
- AC-2 · bagian test unit atas `CommandMap` dan `SystemShortcuts` dan hasil probe P1 di app Stackly saat ini; probe atas menu bar yang sudah memakai peta (setiap baris built tampil dengan shortcut persis peta) menunggu milestone 2 langkah 5
- AC-3 sampai AC-16 · belum, milik Milestone 2 dan 3 (probe P2 sampai P7 sudah memberi dasarnya untuk AC-3, AC-4, AC-5, AC-10, AC-13)
